//! Current-session resolution: the Rails kit's `ApplicationController#authenticate` /
//! `perform_authentication` / `require_no_authentication` and `Current`.
//!
//! [`layer`] reads the signed `session_token` cookie on every request, loads the session and
//! its user, and inserts [`CurrentSession`] into the request extensions (absent when signed
//! out). Handlers then pick an extractor:
//!
//! - [`Authenticated`] — signed-in only; otherwise a redirect to `/sign_in`.
//! - [`MaybeAuthenticated`] — either (the home page).
//! - [`RequireGuest`] — signed-out only; otherwise a redirect to `/` with
//!   "You are already signed in".
//! - [`Details`] — the user agent and client IP that new sessions record.
//! - [`CurrentAccount`] — signed in *and* a member of the account in the URL
//!   (`/{account_slug}/…`): `Current.account` and `Current.membership`. Anyone else gets a 404.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use loco_rs::{app::AppContext, controller::middleware::remote_ip::RemoteIP, model::ModelError};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use ts_rs::TS;

use crate::{
    db::First,
    inertia::{config::Settings, cookies, redirect::Redirect},
    models::{
        accounts, memberships,
        sessions::{self, RequestDetails},
        users,
    },
    route_table,
};

/// The signed-in browser: `Current.session` and `Current.user`.
#[derive(Debug, Clone)]
pub struct CurrentSession {
    pub session: sessions::Model,
    pub user: users::Model,
}

/// Only what the middleware reads. axum clones layer state on every request, and a whole
/// `AppContext` clone costs ~0.9 µs and 42 allocations (its `Config` holds the raw `settings:`
/// JSON), which was a third of the sampled CPU time of `GET /up` (docs/PROFILING.md).
#[derive(Clone)]
struct AuthState {
    db: DatabaseConnection,
    settings: Arc<Settings>,
}

/// Apply the current-session middleware. Must sit inside the flash/redirect layers so a
/// rejection redirect from an extractor still gets its flash cookie written.
pub fn layer(router: Router, ctx: &AppContext, settings: Arc<Settings>) -> Router {
    router.layer(axum::middleware::from_fn_with_state(
        AuthState {
            db: ctx.db.clone(),
            settings,
        },
        middleware,
    ))
}

async fn middleware(State(state): State<AuthState>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    if let Some(token) = cookies::read_session_token(&parts.headers, &state.settings) {
        match sessions::Model::find_by_token_with_user(&state.db, &token).await {
            Ok((session, user)) => {
                parts.extensions.insert(CurrentSession { session, user });
            }
            // A deleted session (signed out elsewhere, password changed): treat as a guest.
            Err(ModelError::EntityNotFound) => {}
            Err(err) => {
                tracing::error!(%err, "could not load the current session");
                return loco_rs::Error::from(err).into_response();
            }
        }
    }
    next.run(Request::from_parts(parts, body)).await
}

async fn request_details(parts: &mut Parts) -> RequestDetails {
    let user_agent = parts
        .headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    RequestDetails {
        user_agent,
        ip_address: client_ip(parts).await.map(|ip| ip.to_string()),
    }
}

/// `request.ip`: loco's `RemoteIP` when the remote_ip middleware is enabled (proxy headers),
/// otherwise the socket peer.
pub async fn client_ip(parts: &mut Parts) -> Option<IpAddr> {
    let Ok(remote) = RemoteIP::from_request_parts(parts, &()).await;
    match remote {
        RemoteIP::Forwarded(ip) | RemoteIP::Socket(ip) => Some(ip),
        RemoteIP::None => parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip()),
    }
}

/// The `auth` shared prop: the signed-in user and session, both `null` for a guest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Auth {
    pub user: Option<AuthUser>,
    pub session: Option<AuthSession>,
}

/// The signed-in user, as every page sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct AuthUser {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub verified: bool,
    // ISO 8601, UTC.
    pub created_at: String,
    // ISO 8601, UTC.
    pub updated_at: String,
}

/// The browser's session. `id` is its random token (what `DELETE /sessions/{id}` takes), never
/// the integer id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct AuthSession {
    pub id: String,
}

/// The `auth` shared prop for `current`.
#[must_use]
pub fn auth_prop(current: Option<&CurrentSession>) -> Auth {
    match current {
        Some(CurrentSession { session, user }) => Auth {
            user: Some(AuthUser {
                id: user.id,
                name: user.name.clone(),
                email: user.email.clone(),
                verified: user.verified,
                created_at: crate::models::as_json_time(&user.created_at),
                updated_at: crate::models::as_json_time(&user.updated_at),
            }),
            session: Some(AuthSession {
                id: session.token.clone(),
            }),
        },
        None => Auth {
            user: None,
            session: None,
        },
    }
}

/// Route layer for controllers that skip authentication entirely (Rails'
/// `skip_before_action :authenticate` without `perform_authentication`): the handler and the
/// shared `auth` prop see a guest, whatever the cookie says.
pub async fn without_session(mut req: Request, next: Next) -> Response {
    req.extensions_mut().remove::<CurrentSession>();
    next.run(req).await
}

/// Signed-in only. Rejects with a redirect to `/sign_in`.
pub struct Authenticated(pub CurrentSession);

impl<S: Send + Sync> FromRequestParts<S> for Authenticated {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentSession>()
            .cloned()
            .map(Self)
            .ok_or_else(|| Redirect::to(route_table::SIGN_IN).into_response())
    }
}

/// Signed in or not.
pub struct MaybeAuthenticated(pub Option<CurrentSession>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeAuthenticated {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<CurrentSession>().cloned()))
    }
}

/// Signed-out only (`require_no_authentication`).
pub struct RequireGuest;

impl<S: Send + Sync> FromRequestParts<S> for RequireGuest {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        if parts.extensions.get::<CurrentSession>().is_some() {
            return Err(Redirect::to(route_table::ROOT)
                .notice("You are already signed in")
                .into_response());
        }
        Ok(Self)
    }
}

/// The user agent and IP of this request (`Current.user_agent` / `Current.ip_address`).
/// Resolved at the handler, inside loco's `remote_ip` middleware, so proxy headers count
/// when that middleware is enabled.
pub struct Details(pub RequestDetails);

impl<S: Send + Sync> FromRequestParts<S> for Details {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(request_details(parts).await))
    }
}

/// The flash a forbidden action redirects back with.
pub const PERMISSION_DENIED: &str = "You don't have permission to do that";

/// The alert an Inertia visit to an account the user can't open gets (missing, or not a
/// member: the same words either way).
pub const ACCOUNT_UNAVAILABLE: &str = "That account isn't available";

/// Signed in and a member of the account named by the `{account_slug}` path segment
/// (the Rails app's `AccountScoped` concern). A non-member gets the same 404 as a missing
/// account, so the response never tells whether the account exists. An Inertia visit (a page
/// already open, e.g. after the user was removed from the account) is redirected to the user's
/// home with the same neutral alert instead, so Inertia doesn't show the raw 404 page in its
/// error overlay. Each visit remembers the account as the user's last one
/// (`users.last_account_id`).
#[derive(Debug, Clone)]
pub struct CurrentAccount {
    pub session: CurrentSession,
    pub account: accounts::Model,
    pub membership: memberships::Model,
}

impl CurrentAccount {
    /// Owners and admins manage members and invitations.
    #[must_use]
    pub fn is_manager(&self) -> bool {
        self.membership.is_manager()
    }

    /// `redirect_back_or_to account_path, alert: "You don't have permission to do that"`.
    #[must_use]
    pub fn forbidden(&self, headers: &axum::http::HeaderMap) -> Response {
        Redirect::back(headers, route_table::account_path(&self.account.slug))
            .alert(PERMISSION_DENIED)
            .into_response()
    }
}

impl FromRequestParts<AppContext> for CurrentAccount {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        ctx: &AppContext,
    ) -> Result<Self, Self::Rejection> {
        use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

        let Authenticated(session) = Authenticated::from_request_parts(parts, ctx).await?;
        let not_found = || loco_rs::Error::NotFound.into_response();
        let params = axum::extract::RawPathParams::from_request_parts(parts, ctx)
            .await
            .map_err(|_| not_found())?;
        let slug = params
            .iter()
            .find(|(key, _)| *key == "account_slug")
            .map(|(_, value)| value.to_owned())
            .ok_or_else(not_found)?;
        let found = memberships::Entity::find()
            .find_also_related(accounts::Entity)
            .filter(memberships::Column::UserId.eq(session.user.id))
            .filter(accounts::Column::Slug.eq(slug))
            .first(&ctx.db)
            .await
            .map_err(|err| loco_rs::Error::from(err).into_response())?;
        let Some((membership, Some(account))) = found else {
            if !crate::inertia::redirect::is_inertia(&parts.headers) {
                return Err(not_found());
            }
            let home = crate::controllers::members::home_path(
                ctx,
                session.user.id,
                session.user.last_account_id,
            )
            .await
            .map_err(IntoResponse::into_response)?;
            return Err(Redirect::to(home)
                .alert(ACCOUNT_UNAVAILABLE)
                .into_response());
        };
        session
            .user
            .remember_account(&ctx.db, account.id)
            .await
            .map_err(|err| loco_rs::Error::from(err).into_response())?;
        Ok(Self {
            session,
            account,
            membership,
        })
    }
}

/// Rails' `constraints: { account_slug: SLUG_FORMAT }` on the `/{account_slug}/…` routes. axum
/// matches `/robots.txt` to `/{account_slug}`; a first segment that can't be a slug did not
/// match in Rails, so it is served from `public/` like any unmatched path. Runs after routing
/// (it reads `MatchedPath`) and before the browser check, which only applies to routes.
pub fn slug_constraint(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(
        |req: Request, next: Next| async move {
            let not_a_slug = req
                .extensions()
                .get::<axum::extract::MatchedPath>()
                .is_some_and(|p| p.as_str().starts_with("/{account_slug}"))
                && !req
                    .uri()
                    .path()
                    .split('/')
                    .nth(1)
                    .is_some_and(accounts::is_slug);
            if not_a_slug {
                crate::inertia::public::fallback(req).await
            } else {
                next.run(req).await
            }
        },
    ))
}

/// The account switcher: `[{name, slug}]` of the user's accounts, ordered by name, as a
/// **once** prop. The client keeps it across visits and asks the server to skip it; the key
/// is a digest of the user's account ids and their `updated_at`, so joining, leaving,
/// creating or renaming an account changes the key and the list is sent again.
async fn account_switcher(
    db: &DatabaseConnection,
    user_id: i64,
) -> loco_rs::Result<crate::inertia::props::Prop> {
    use sha2::{Digest, Sha256};

    let mut accounts = accounts::Model::list_for_user(db, user_id).await?;
    let list: Vec<accounts::AccountSummary> =
        accounts.iter().map(accounts::Model::to_summary).collect();
    accounts.sort_by_key(|a| a.id);
    let mut digest = Sha256::new();
    for a in &accounts {
        digest.update(format!("{}:{};", a.id, a.updated_at.to_rfc3339()));
    }
    let key = format!("accounts:{}", &hex::encode(digest.finalize())[..16]);
    Ok(crate::inertia::Prop::value(list).once_as(key))
}

/// Register the shared props (the Rails kit's `inertia_share`): `auth`, and for a signed-in
/// user the `accounts` switcher list (see [`account_switcher`]).
pub fn register_shared_props(ctx: &AppContext) {
    let shared: crate::inertia::props::SharedPropsFn = Arc::new(|parts, ctx| {
        let current = parts.extensions.get::<CurrentSession>().cloned();
        let auth = auth_prop(current.as_ref());
        let db = ctx.db.clone();
        Box::pin(async move {
            let props = crate::inertia::Props::new().with("auth", auth);
            Ok(match current {
                Some(current) => {
                    props.with("accounts", account_switcher(&db, current.user.id).await?)
                }
                None => props,
            })
        })
    });
    ctx.shared_store
        .insert(crate::inertia::props::SharedProps(shared));
}
