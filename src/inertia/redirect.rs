//! Redirects for Inertia: the `Redirect` builder (flash-carrying 302s), and inertia-rails'
//! `convert_external_redirects`, which the Inertia layer (`render::layer`) applies to every
//! Inertia visit: one redirected (301/302/303) to another origin becomes 409 +
//! `X-Inertia-Location`. XHR can't follow cross-origin redirects, so the client does a full
//! `window.location` visit instead. Headers (notably Set-Cookie) are kept; the body is dropped.
//!
//! inertia-omega applies the rest of the protocol's redirect rules: a 302 after an Inertia
//! PUT/PATCH/DELETE becomes 303, and a redirect to a URL with a `#fragment` becomes 409 +
//! `X-Inertia-Redirect` (not for prefetches, see [`is_prefetch`]).

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use url::Url;

use super::flash::{FlashState, OutgoingFlash};

pub const X_INERTIA: HeaderName = HeaderName::from_static("x-inertia");
pub const X_INERTIA_LOCATION: HeaderName = HeaderName::from_static("x-inertia-location");

pub fn is_inertia(headers: &HeaderMap) -> bool {
    headers.get(X_INERTIA).is_some_and(|v| v == "true")
}

/// The client is prefetching the page, not visiting it: `Purpose` (sent by
/// Inertia's prefetch), `Sec-Purpose` or `X-Moz` is `prefetch`, in any case.
/// Laravel's `Request::prefetch()`, inertia-omega's `Request::is_prefetch`.
pub fn is_prefetch(headers: &HeaderMap) -> bool {
    ["purpose", "sec-purpose", "x-moz"].iter().any(|name| {
        headers
            .get(*name)
            .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"prefetch"))
    })
}

/// A 302 redirect that carries flash state to the next request.
#[must_use]
#[derive(Clone, Debug)]
pub struct Redirect {
    to: String,
    flash: FlashState,
}

impl Redirect {
    pub fn to(url: impl Into<String>) -> Self {
        Self {
            to: url.into(),
            flash: FlashState::default(),
        }
    }

    /// Redirect to the Referer when it is same-origin (same host:port as the
    /// request's Host header), otherwise to `fallback`. Only the Referer's
    /// path and query are used, and a path a browser would read as a
    /// network-path reference (`//host`, `/\host`) falls back too, so the
    /// result is always a local path.
    pub fn back(headers: &HeaderMap, fallback: impl Into<String>) -> Self {
        let back = headers
            .get(header::REFERER)
            .and_then(|v| v.to_str().ok())
            .and_then(|r| Url::parse(r).ok())
            .filter(|referer| {
                let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
                host.is_some_and(|host| same_authority(referer, host))
            })
            .map(|referer| match referer.query() {
                Some(q) => format!("{}?{q}", referer.path()),
                None => referer.path().to_owned(),
            })
            .filter(|path| is_local_path(path));
        Self::to(back.unwrap_or_else(|| fallback.into()))
    }

    pub fn notice(mut self, message: impl Into<String>) -> Self {
        self.flash.notice = Some(message.into());
        self
    }

    pub fn alert(mut self, message: impl Into<String>) -> Self {
        self.flash.alert = Some(message.into());
        self
    }

    /// Validation errors, `{"field": ["msg", ...]}`. Accepts a
    /// `serde_json::Value` or anything `Serialize`.
    ///
    /// # Panics
    /// If `errors` fails to serialize (e.g. a map with non-string keys).
    pub fn errors(mut self, errors: impl Serialize) -> Self {
        let value = serde_json::to_value(errors).expect("redirect errors must serialize to JSON");
        self.flash.errors = Some(value);
        self
    }

    pub fn error_bag(mut self, bag: impl Into<String>) -> Self {
        self.flash.error_bag = Some(bag.into());
        self
    }

    pub fn clear_history(mut self) -> Self {
        self.flash.clear_history = true;
        self
    }

    pub fn preserve_fragment(mut self) -> Self {
        self.flash.preserve_fragment = true;
        self
    }

    pub fn target(&self) -> &str {
        &self.to
    }

    pub fn flash(&self) -> &FlashState {
        &self.flash
    }
}

impl IntoResponse for Redirect {
    fn into_response(self) -> Response {
        let mut res = StatusCode::FOUND.into_response();
        // Rails' `redirect_to` answers `text/html` with an empty body.
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        match HeaderValue::from_str(&self.to) {
            Ok(value) => {
                res.headers_mut().insert(header::LOCATION, value);
            }
            Err(_) => {
                tracing::error!(to = %self.to, "redirect target is not a valid header value");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
        if !self.flash.is_empty() {
            res.extensions_mut().insert(OutgoingFlash(self.flash));
        }
        res
    }
}

/// An Inertia visit's response, with a redirect (301/302/303) to another origin turned into
/// 409 + `X-Inertia-Location`; any other response as it was. `request_host` is the request's
/// `Host` header.
#[must_use]
pub fn convert_external(res: Response, app_url: &str, request_host: Option<&str>) -> Response {
    if !matches!(res.status().as_u16(), 301..=303) {
        return res;
    }
    let external = res
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|location| is_external(location, app_url, request_host));
    if !external {
        return res;
    }
    let (mut parts, _body) = res.into_parts();
    let location = parts
        .headers
        .remove(header::LOCATION)
        .expect("checked above");
    parts.status = StatusCode::CONFLICT;
    parts.headers.insert(X_INERTIA_LOCATION, location);
    parts.headers.remove(header::CONTENT_TYPE);
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::empty())
}

/// A path-absolute reference a browser resolves on this origin: starts with
/// one `/` and is not a network-path reference. Browsers treat `\` like `/`
/// in http(s) URLs, so `//x`, `/\x`, `\\x` and `\/x` all name another host.
#[must_use]
pub fn is_local_path(path: &str) -> bool {
    let mut chars = path.chars();
    chars.next() == Some('/') && !matches!(chars.next(), Some('/' | '\\'))
}

/// True when `location`, resolved against `app_url` the way a browser
/// resolves a Location header, names a scheme/host/port that matches
/// neither `app_url` nor the request's Host. So scheme-relative `//evil.com`
/// is external, while `/path` and `path` are internal. Unparseable
/// locations are treated as internal.
pub fn is_external(location: &str, app_url: &str, request_host: Option<&str>) -> bool {
    let app = Url::parse(app_url).ok();
    let target = match &app {
        Some(app) => app.join(location),
        None => Url::parse(location),
    };
    let Ok(target) = target else {
        return false; // garbage the browser cannot follow either
    };
    if target.host_str().is_none() {
        return false;
    }
    if app
        .as_ref()
        .is_some_and(|app| app.origin() == target.origin())
    {
        return false;
    }
    // Same host:port as the request, under the app's scheme.
    let scheme_ok = app
        .as_ref()
        .is_none_or(|app| app.scheme() == target.scheme());
    let host_ok = request_host.is_some_and(|host| same_authority(&target, host));
    !(scheme_ok && host_ok)
}

/// `url`'s host and (default-resolved) port equal the `host[:port]` header.
fn same_authority(url: &Url, host_header: &str) -> bool {
    let Ok(request) = Url::parse(&format!("{}://{host_header}", url.scheme())) else {
        return false;
    };
    request
        .host_str()
        .zip(url.host_str())
        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        && request.port_or_known_default() == url.port_or_known_default()
}
