//! Flash, validation errors and history flags that survive one redirect, kept in the encrypted
//! `_flash` cookie: inertia-omega's session, implemented over a cookie instead of a server-side
//! store.
//!
//! - Request: the cookie is decrypted into a [`CookieSession`] request extension (empty when
//!   absent or tampered), which the Inertia layer (`render::layer`) hands to omega.
//! - omega reads it on every page render and removes what the page delivers, and writes what
//!   the request queued for the next page: a [`Redirect`](super::redirect::Redirect)'s
//!   `notice`/`alert`/`errors`/history flags ([`FlashState`], replayed by `render::layer`).
//! - Response: when the session changed, the cookie is rewritten, or deleted once empty. A
//!   response that rendered nothing (a redirect, a 409, a JSON endpoint) leaves it as it was, so
//!   the flash waits for the next page.
//! - A prefetch (`Purpose: prefetch`, see [`super::redirect::is_prefetch`]) leaves the cookie
//!   alone: omega gets an empty session, so the page shows no flash, and nothing it consumes or
//!   queues is written back. The page it builds may be shown later or never, so it must not take
//!   the flash from the visit that follows.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
    Router,
};
use omega::session::key;
use serde_json::{Map, Value};

use super::config::Settings;
use super::cookies;
use super::redirect::is_prefetch;

/// What a [`Redirect`](super::redirect::Redirect) carries to the next page (Rails' `flash`
/// and inertia-rails' session options).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlashState {
    pub notice: Option<String>,
    pub alert: Option<String>,
    pub errors: Option<Value>, // {"field": ["msg", ...]}
    pub error_bag: Option<String>,
    pub clear_history: bool,
    pub preserve_fragment: bool,
}

impl FlashState {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Queue this flash on the request's Inertia handle, for the next page render: `notice` and
    /// `alert` in the page's `flash`, `errors` in its `errors` prop (under `error_bag` when set).
    pub fn queue(self, inertia: &omega::Inertia) {
        if let Some(notice) = self.notice {
            inertia.flash("notice", notice);
        }
        if let Some(alert) = self.alert {
            inertia.flash("alert", alert);
        }
        if let Some(errors) = self.errors {
            let bag = self.error_bag.unwrap_or_else(|| "default".to_owned());
            inertia.with_errors_in(bag, validation_errors(&errors));
        }
        if self.clear_history {
            inertia.clear_history();
        }
        if self.preserve_fragment {
            inertia.preserve_fragment();
        }
    }
}

/// `{"field": ["msg", ...]}` (or `{"field": "msg"}`) as omega's errors.
fn validation_errors(errors: &Value) -> omega::ValidationErrors {
    let mut out = omega::ValidationErrors::new();
    for (field, messages) in errors.as_object().into_iter().flatten() {
        match messages {
            Value::Array(messages) => {
                for message in messages {
                    out.add(field.clone(), text(message));
                }
            }
            message => {
                out.add(field.clone(), text(message));
            }
        }
    }
    out
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Response extension: flash to carry to the next request.
#[derive(Clone)]
pub struct OutgoingFlash(pub FlashState);

/// The keys the cookie may hold: Inertia's own. Anything else (a cookie from an older version
/// of the app) is dropped.
const KEYS: [&str; 4] = [
    key::FLASH,
    key::ERRORS,
    key::CLEAR_HISTORY,
    key::PRESERVE_FRAGMENT,
];

/// The request's `_flash` cookie as omega's session. Clones share their data.
#[derive(Clone, Debug, Default)]
pub struct CookieSession(Arc<Mutex<Stored>>);

#[derive(Debug, Default)]
struct Stored {
    values: Map<String, Value>,
    modified: bool,
}

impl CookieSession {
    fn new(mut values: Map<String, Value>) -> Self {
        let before = values.len();
        values.retain(|k, _| KEYS.contains(&k.as_str()));
        Self(Arc::new(Mutex::new(Stored {
            modified: values.len() != before,
            values,
        })))
    }

    fn stored(&self) -> MutexGuard<'_, Stored> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Forget what an earlier response left for the next page, so a redirect's flash replaces
    /// it rather than adding to it (omega's `commit` merges into what is stored).
    pub fn clear(&self) {
        let mut stored = self.stored();
        stored.modified |= !stored.values.is_empty();
        stored.values.clear();
    }
}

impl omega::Session for CookieSession {
    async fn get(&self, key: &str) -> Option<Value> {
        self.stored().values.get(key).cloned()
    }

    async fn put(&self, key: &str, value: Value) {
        let mut stored = self.stored();
        stored.values.insert(key.to_owned(), value);
        stored.modified = true;
    }

    async fn pull(&self, key: &str) -> Option<Value> {
        let mut stored = self.stored();
        let value = stored.values.remove(key);
        stored.modified |= value.is_some();
        value
    }
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
        let session = CookieSession::default();
        req.extensions_mut().insert(session.clone());
        let res = next.run(req).await;
        if session.stored().modified {
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
        .and_then(|json| serde_json::from_str::<Map<String, Value>>(&json).ok());
    let tampered = had_cookie && decoded.is_none();
    if tampered {
        tracing::warn!("discarding undecryptable _flash cookie");
    }
    let session = CookieSession::new(decoded.unwrap_or_default());
    req.extensions_mut().insert(session.clone());

    let mut res = next.run(req).await;

    let stored = std::mem::take(&mut *session.stored());
    if stored.modified && !stored.values.is_empty() {
        let json = serde_json::to_string(&stored.values).expect("a JSON map serializes");
        let mut cookie = cookies::base_cookie(&settings, cookies::FLASH_COOKIE, json);
        cookie.set_http_only(true);
        let cookie = cookies::encrypt(&key, cookie);
        cookies::append_set_cookie(res.headers_mut(), &cookie);
    } else if had_cookie && (tampered || stored.modified) {
        let removal = cookies::removal_cookie(&settings, cookies::FLASH_COOKIE);
        cookies::append_set_cookie(res.headers_mut(), &removal);
    }
    res
}
