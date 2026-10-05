//! CSRF protection for Inertia's cookie-to-header scheme.
//!
//! - Each browser gets a random 32-byte secret in the signed, HttpOnly `_csrf`
//!   cookie.
//! - Tokens are `base64url(nonce || HMAC-SHA256(secret, nonce))`. A fresh one
//!   is published in the readable `XSRF-TOKEN` cookie on GET/HEAD responses
//!   when the browser's current one is missing or doesn't verify; Inertia's
//!   client echoes it in `X-XSRF-TOKEN`.
//! - Non-safe methods must carry `X-XSRF-TOKEN` (or `X-CSRF-TOKEN`) that
//!   verifies against the secret, must not be `Sec-Fetch-Site: cross-site`,
//!   and any `Origin` must equal `app_url`'s origin; or, for a request to one of
//!   `settings.extra_hosts`, that same host's origin (never another listed host's, and never
//!   `app_url`'s: each host has its own cookies, so its own session). Outside production, a
//!   loopback `app_url` (`localhost`, `127.0.0.1`, `[::1]`) also accepts the other loopback
//!   spellings on the same scheme and port: they are the same dev server.
//! - Failure: 422 (JSON for `X-Inertia` requests, plain text otherwise).
//! - Handlers rotate the secret (sign-in / sign-out) by inserting the
//!   `RotateCsrf` response extension.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cookie::Key;
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use sha2::Sha256;

use super::config::Settings;
use super::cookies;
use super::redirect::is_inertia;

pub const X_XSRF_TOKEN: HeaderName = HeaderName::from_static("x-xsrf-token");
pub const X_CSRF_TOKEN: HeaderName = HeaderName::from_static("x-csrf-token");
const SECRET_LEN: usize = 32;
const NONCE_LEN: usize = 16;
const MAC_LEN: usize = 32;
pub const REJECTION_MESSAGE: &str = "Can't verify CSRF token authenticity.";

/// Response extension: replace this browser's CSRF secret (and token).
/// Insert it on sign-in and sign-out responses to defeat token fixation.
#[derive(Clone, Copy, Debug)]
pub struct RotateCsrf;

/// Why a request was rejected (logged; not sent to the client).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    CrossSite,
    BadOrigin,
    MissingToken,
    InvalidToken,
}

pub fn layer(router: Router, settings: Arc<Settings>) -> Router {
    router.layer(axum::middleware::from_fn_with_state(settings, middleware))
}

fn new_secret() -> [u8; SECRET_LEN] {
    let mut secret = [0u8; SECRET_LEN];
    rand::rng().fill_bytes(&mut secret);
    secret
}

fn mac(secret: &[u8]) -> Hmac<Sha256> {
    Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts keys of any length")
}

/// A fresh masked token for `secret`: `base64url(nonce || HMAC(secret, nonce))`.
pub fn generate_token(secret: &[u8]) -> String {
    let mut buf = [0u8; NONCE_LEN + MAC_LEN];
    rand::rng().fill_bytes(&mut buf[..NONCE_LEN]);
    let mut m = mac(secret);
    m.update(&buf[..NONCE_LEN]);
    buf[NONCE_LEN..].copy_from_slice(&m.finalize().into_bytes());
    URL_SAFE_NO_PAD.encode(buf)
}

/// Constant-time check that `token` was generated for `secret`.
pub fn verify_token(secret: &[u8], token: &str) -> bool {
    let Ok(raw) = URL_SAFE_NO_PAD.decode(token.trim()) else {
        return false;
    };
    if raw.len() != NONCE_LEN + MAC_LEN {
        return false;
    }
    let mut m = mac(secret);
    m.update(&raw[..NONCE_LEN]);
    m.verify_slice(&raw[NONCE_LEN..]).is_ok()
}

/// The browser's verified CSRF secret, if its `_csrf` cookie is intact.
pub fn read_secret(headers: &HeaderMap, key: &Key) -> Option<Vec<u8>> {
    let encoded = cookies::read_signed(headers, key, cookies::CSRF_SECRET_COOKIE)?;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .filter(|s| s.len() == SECRET_LEN)
}

/// Origin / Sec-Fetch-Site / token checks for a non-safe request.
pub fn check(
    headers: &HeaderMap,
    settings: &Settings,
    secret: Option<&[u8]>,
) -> Result<(), Rejection> {
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"cross-site"))
    {
        return Err(Rejection::CrossSite);
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        // A request to a listed extra host must come from that host; anything else (the
        // primary host, an unlisted one) from `app_url`, as with no extra hosts at all.
        let to_extra = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| settings.extra_origin_for(h));
        let expected = to_extra.or_else(|| {
            url::Url::parse(&settings.app_url)
                .ok()
                .map(|u| u.origin().ascii_serialization())
        });
        let matches = origin
            .to_str()
            .ok()
            .zip(expected.as_deref())
            .is_some_and(|(got, want)| {
                got.eq_ignore_ascii_case(want)
                    || (!settings.production && same_loopback_server(got, want))
            });
        if !matches {
            return Err(Rejection::BadOrigin);
        }
    }
    let token = headers
        .get(X_XSRF_TOKEN)
        .or_else(|| headers.get(X_CSRF_TOKEN))
        .and_then(|v| v.to_str().ok())
        .filter(|t| !t.is_empty());
    let Some(token) = token else {
        return Err(Rejection::MissingToken);
    };
    match secret {
        Some(secret) if verify_token(secret, token) => Ok(()),
        _ => Err(Rejection::InvalidToken),
    }
}

/// `http://127.0.0.1:5150` and `http://localhost:5150` (or `http://[::1]:5150`): both loopback,
/// same scheme and port, so the same local server. `bin/dev` answers on either, and a browser
/// sends whichever was typed as the `Origin`.
fn same_loopback_server(origin: &str, app_origin: &str) -> bool {
    let loopback = |url: &url::Url| match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    match (url::Url::parse(origin), url::Url::parse(app_origin)) {
        (Ok(a), Ok(b)) => {
            loopback(&a)
                && loopback(&b)
                && a.scheme() == b.scheme()
                && a.port_or_known_default() == b.port_or_known_default()
        }
        _ => false,
    }
}

/// 422: JSON `{"message": …}` for Inertia requests, plain text otherwise.
pub fn rejection_response(headers: &HeaderMap) -> Response {
    if is_inertia(headers) {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "message": REJECTION_MESSAGE })),
        )
            .into_response()
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, REJECTION_MESSAGE).into_response()
    }
}

fn is_safe(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

/// Benchmark builds only: the `/bench/` endpoints skip the token check (Rails:
/// `skip_forgery_protection`), but still get the CSRF cookies like any other response.
#[cfg(feature = "bench")]
fn bench_exempt(path: &str) -> bool {
    path.starts_with("/bench/")
}

#[cfg(not(feature = "bench"))]
const fn bench_exempt(_: &str) -> bool {
    false
}

async fn middleware(State(settings): State<Arc<Settings>>, req: Request, next: Next) -> Response {
    if !settings.forgery_protection {
        return next.run(req).await;
    }
    let key = cookies::csrf_key(&settings);
    let existing = read_secret(req.headers(), &key);
    let presented = cookies::request_jar(req.headers())
        .get(cookies::XSRF_COOKIE)
        .map(|c| c.value().to_owned());
    let method = req.method().clone();

    let mut res = if is_safe(&method) || bench_exempt(req.uri().path()) {
        next.run(req).await
    } else {
        match check(req.headers(), &settings, existing.as_deref()) {
            Ok(()) => next.run(req).await,
            Err(reason) => {
                tracing::warn!(?reason, %method, path = %req.uri().path(), "CSRF check failed");
                rejection_response(req.headers())
            }
        }
    };

    let rotate = res.extensions_mut().remove::<RotateCsrf>().is_some();
    let (secret, secret_is_new) = match existing {
        Some(secret) if !rotate => (secret, false),
        _ => (new_secret().to_vec(), true),
    };
    if secret_is_new {
        let value = URL_SAFE_NO_PAD.encode(&secret);
        let mut cookie = cookies::base_cookie(&settings, cookies::CSRF_SECRET_COOKIE, value);
        cookie.set_http_only(true);
        cookie.set_max_age(cookies::PERMANENT_MAX_AGE);
        cookies::append_set_cookie(res.headers_mut(), &cookies::sign(&key, cookie));
    }

    let token_stale = presented.is_none_or(|t| !verify_token(&secret, &t));
    let is_read = matches!(method, Method::GET | Method::HEAD);
    if secret_is_new || (is_read && token_stale) {
        let token = generate_token(&secret);
        let cookie = cookies::base_cookie(&settings, cookies::XSRF_COOKIE, token);
        cookies::append_set_cookie(res.headers_mut(), &cookie);
    }
    res
}
