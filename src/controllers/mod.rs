//! HTTP controllers: one module per Rails kit controller, same routes, components, props and
//! flash texts. Shared helpers (settings, clock, params, page rendering) live here.

use std::sync::Arc;

use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use loco_rs::{app::AppContext, Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{
    inertia::{config::Settings, precognition, props::Props, render::Inertia},
    models::{
        tokens::{Clock, SystemClock},
        users::Errors,
    },
};

pub mod accounts;
#[cfg(feature = "bench")]
pub mod bench;
pub mod browser;
pub mod dashboard;
pub mod health;
pub mod home;
pub mod identity;
pub mod rate_limit;
pub mod sessions;
pub mod settings;
pub mod users;

/// The app settings stored in `ctx.shared_store` by `Hooks::after_context`.
///
/// # Errors
/// When they were never stored (a boot-order bug).
// `loco_rs::Error` is loco's (>128 bytes); every handler already returns it.
#[allow(clippy::result_large_err)]
pub fn settings(ctx: &AppContext) -> Result<Arc<Settings>> {
    ctx.shared_store
        .get::<Arc<Settings>>()
        .ok_or_else(|| Error::Message("inertia settings missing from shared_store".into()))
}

/// The clock used for token generation and verification. Tests replace it via
/// [`set_clock`]; everything else gets the system clock.
#[derive(Clone)]
pub struct AppClock(pub Arc<dyn Clock>);

#[must_use]
pub fn clock(ctx: &AppContext) -> Arc<dyn Clock> {
    ctx.shared_store
        .get::<AppClock>()
        .map_or_else(|| Arc::new(SystemClock) as Arc<dyn Clock>, |c| c.0)
}

/// Replace the app's clock (tests travel in time with a `FixedClock`).
pub fn set_clock(ctx: &AppContext, clock: Arc<dyn Clock>) {
    ctx.shared_store.insert(AppClock(clock));
}

/// Rails-style `params`: the query string merged with a JSON or form-urlencoded body (body
/// wins), deserialized into `T`. Inertia's client posts JSON; plain HTML forms and curl post
/// forms. Unknown keys are ignored like `params.permit`.
pub struct Params<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Params<T> {
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> std::result::Result<Self, Self::Rejection> {
        let mut map: Map<String, Value> = req
            .uri()
            .query()
            .map(|q| serde_urlencoded::from_str::<Vec<(String, String)>>(q).unwrap_or_default())
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        let is_json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("application/json"));
        let bytes = axum::body::Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        if !bytes.is_empty() {
            let bad = |e: String| Error::BadRequest(e).into_response();
            if is_json {
                match serde_json::from_slice::<Value>(&bytes).map_err(|e| bad(e.to_string()))? {
                    Value::Object(body) => map.extend(body),
                    _ => return Err(bad("expected a JSON object".into())),
                }
            } else {
                let body = serde_urlencoded::from_bytes::<Vec<(String, String)>>(&bytes)
                    .map_err(|e| bad(e.to_string()))?;
                map.extend(body.into_iter().map(|(k, v)| (k, Value::String(v))));
            }
        }
        serde_json::from_value(Value::Object(map))
            .map(Self)
            .map_err(|e| Error::BadRequest(e.to_string()).into_response())
    }
}

/// Deserializer for an optional permitted attribute that must tell a missing key from an
/// explicit `null`, like Rails' `params.permit` does. Use as
/// `#[serde(default, deserialize_with = "nullable")] field: Option<Option<String>>`:
/// a missing key is `None` (serde's `default`; the attribute keeps its value), `null` is
/// `Some(None)` (assigns nil) and a string is `Some(Some(value))`.
///
/// # Errors
/// When the value is neither `null` nor a string.
pub fn nullable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

/// Deserializer for a permitted string param where an explicit `null` means the same as a
/// missing key: the type's default (`""`). Rails' controllers here read these with
/// `params[:x] || ""` or `with_defaults(x: "")`, and a nil email/password/token finds no
/// record exactly like `""` does. Use as
/// `#[serde(default, deserialize_with = "null_default")] field: String`.
///
/// # Errors
/// When the value is neither `null` nor a `T`.
pub fn null_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<T, D::Error> {
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

/// Answer a `Precognition: true` request with `errors`; `None` for a normal request.
#[must_use]
pub fn precognitive(headers: &HeaderMap, errors: &Errors) -> Option<Response> {
    precognition::is_precognition(headers).then(|| {
        precognition::respond_for(
            headers,
            &serde_json::to_value(errors).unwrap_or(Value::Null),
        )
    })
}

/// For mutating endpoints without a validation-only mode: a `Precognition: true` request is
/// answered `400 Precognition not supported` before any other extractor or the handler runs,
/// so it can never write, send mail, spend a rate-limit token or touch the session. List it
/// first among the handler's extractors.
pub struct NoPrecognition;

impl<S: Send + Sync> FromRequestParts<S> for NoPrecognition {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> std::result::Result<Self, Response> {
        if precognition::is_precognition(&parts.headers) {
            return Err((StatusCode::BAD_REQUEST, "Precognition not supported").into_response());
        }
        Ok(Self)
    }
}

/// `path?invitation=<token>`, or `path` when there is no token: sign-in and sign-up carry an
/// invitation token through a failed attempt (`sign_in_path(invitation: params[:invitation])`).
#[must_use]
pub fn with_invitation(path: &str, token: &str) -> String {
    if token.is_empty() {
        return path.to_owned();
    }
    let query = serde_urlencoded::to_string([("invitation", token)]).unwrap_or_default();
    format!("{path}?{query}")
}

/// Render an Inertia page whose props are plain values: a `json!({..})` object, or a
/// `Serialize` struct (each field a prop). For lazy, deferred or once props build
/// [`Props`] and call `inertia.render` instead.
///
/// # Errors
/// When the props don't serialize to JSON, and rendering failures (SSR is non-fatal; see
/// `inertia::render`).
pub async fn render(inertia: Inertia, component: &str, props: impl Serialize) -> Result<Response> {
    inertia
        .render(component, Props::from_json(serde_json::to_value(props)?))
        .await
}

/// Sign this browser in to the freshly created `session`: set the permanent signed
/// `session_token` cookie and rotate the CSRF secret, then redirect with `redirect`.
///
/// # Errors
/// Missing settings.
// `loco_rs::Error` is loco's (>128 bytes); every handler already returns it.
#[allow(clippy::result_large_err)]
pub fn start_session(
    ctx: &AppContext,
    session: &crate::models::sessions::Model,
    redirect: crate::inertia::redirect::Redirect,
) -> Result<Response> {
    let settings = settings(ctx)?;
    let mut res = redirect.into_response();
    crate::inertia::cookies::set_session_token(res.headers_mut(), &settings, &session.token);
    res.extensions_mut()
        .insert(crate::inertia::csrf::RotateCsrf);
    Ok(res)
}

pub mod members;

pub mod invitations;

pub mod account_invitations;
