//! Flash + errors + history flags that survive one redirect, stored in the
//! encrypted `_flash` cookie.
//!
//! - Request: the cookie is decrypted into the `IncomingFlash` extension
//!   (empty `FlashState` when absent or tampered).
//! - Response carrying `OutgoingFlash`: the cookie is (re)written.
//! - Response carrying `FlashConsumed` (a render): the cookie is deleted,
//!   unless the status is a redirect (301/302/303/307/308) or 409, where it
//!   must survive until the next render.
//! - A prefetch (`Purpose: prefetch`, see [`super::redirect::is_prefetch`])
//!   leaves the cookie alone: it reads an empty flash, never deletes the
//!   cookie and never writes one. The page it builds may be shown later or
//!   never, so it must not take the flash from the visit that follows.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Router,
};

use super::config::Settings;
use super::cookies;
use super::redirect::is_prefetch;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlashState {
    pub notice: Option<String>,
    pub alert: Option<String>,
    pub errors: Option<serde_json::Value>, // {"field": ["msg", ...]}
    pub error_bag: Option<String>,
    pub clear_history: bool,
    pub preserve_fragment: bool,
}

impl FlashState {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Request extension: the flash carried over from the previous response.
#[derive(Clone)]
pub struct IncomingFlash(pub Arc<FlashState>);

/// Response extension inserted by a render: the incoming flash was shown.
#[derive(Clone, Copy)]
pub struct FlashConsumed;

/// Response extension: flash to carry to the next request.
#[derive(Clone)]
pub struct OutgoingFlash(pub FlashState);

/// Statuses on which an unconsumed flash must be kept (redirects, and the 409
/// asset-version / external-location responses that trigger a fresh visit).
pub fn keeps_flash(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308 | 409)
}

pub fn layer(router: Router, settings: Arc<Settings>) -> Router {
    router.layer(axum::middleware::from_fn_with_state(settings, middleware))
}

async fn middleware(
    State(settings): State<Arc<Settings>>,
    mut req: Request,
    next: Next,
) -> Response {
    if is_prefetch(req.headers()) {
        req.extensions_mut()
            .insert(IncomingFlash(Arc::new(FlashState::default())));
        let res = next.run(req).await;
        if res.extensions().get::<OutgoingFlash>().is_some() {
            tracing::info!(
                "flash on a prefetch response dropped; prefetches never touch the flash"
            );
        }
        return res;
    }
    let key = cookies::flash_key(&settings);
    let had_cookie = cookies::request_jar(req.headers())
        .get(cookies::FLASH_COOKIE)
        .is_some();
    let decoded = cookies::read_private(req.headers(), &key, cookies::FLASH_COOKIE)
        .and_then(|json| serde_json::from_str::<FlashState>(&json).ok());
    let tampered = had_cookie && decoded.is_none();
    if tampered {
        tracing::warn!("discarding undecryptable _flash cookie");
    }
    req.extensions_mut()
        .insert(IncomingFlash(Arc::new(decoded.unwrap_or_default())));

    let mut res = next.run(req).await;

    if let Some(OutgoingFlash(flash)) = res.extensions_mut().remove::<OutgoingFlash>() {
        let json = serde_json::to_string(&flash).expect("FlashState serializes");
        let mut cookie = cookies::base_cookie(&settings, cookies::FLASH_COOKIE, json);
        cookie.set_http_only(true);
        let cookie = cookies::encrypt(&key, cookie);
        cookies::append_set_cookie(res.headers_mut(), &cookie);
    } else if had_cookie
        && (tampered
            || (res.extensions().get::<FlashConsumed>().is_some() && !keeps_flash(res.status())))
    {
        let removal = cookies::removal_cookie(&settings, cookies::FLASH_COOKIE);
        cookies::append_set_cookie(res.headers_mut(), &removal);
    }
    res
}
