//! Files under `public/` (Rails' `public_file_server`), served only when no
//! route matched, so a file can never shadow an application route.
//!
//! Installed as the router's fallback in `App::before_routes`, so Loco's
//! middleware stack and the Inertia layers wrap it like any route.
//!
//! - Only GET and HEAD; any other unmatched request is a plain 404.
//! - `/vite/…` (fingerprinted Vite output) gets `Cache-Control: public,
//!   max-age=31536000, immutable`; other files get [`CACHE_CONTROL`].
//! - A missing file is a real 404 with `public/404.html` as the body and
//!   `Cache-Control: no-cache`, never the immutable header.
//! - Dotfiles/dot-directories (e.g. `/vite/.vite/manifest.json`) are not served.
//!   The check runs on the percent-decoded segments, as `ServeDir` decodes
//!   them too: `%2e`-prefixed segments, encoded separators (`%2f`, `%5c`),
//!   backslashes, NUL and `..` are all 404s.

use std::path::Path;

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use tower::ServiceExt;
use tower_http::services::ServeDir;

/// Where public files live, relative to the working directory.
pub const PUBLIC_DIR: &str = "public";
/// URL prefix of the Vite build output (`public/vite`).
pub const VITE_PREFIX: &str = "/vite/";
/// Fingerprinted assets never change.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// Icons, robots.txt, error pages: cacheable, but revalidated hourly.
pub const CACHE_CONTROL: &str = "public, max-age=3600";
/// 404s must not be cached as if they were the asset.
pub const NOT_FOUND_CACHE_CONTROL: &str = "no-cache";

/// The router fallback: serves [`PUBLIC_DIR`].
pub async fn fallback(req: Request) -> Response {
    serve(Path::new(PUBLIC_DIR), req).await
}

/// Serves `req` from `dir` (see the module docs).
pub async fn serve(dir: &Path, req: Request) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(path) = servable_path(req.uri().path()) else {
        return not_found(dir, req.method()).await;
    };
    let cache = if path.starts_with(VITE_PREFIX) {
        IMMUTABLE
    } else {
        CACHE_CONTROL
    };
    let method = req.method().clone();
    let service = ServeDir::new(dir).append_index_html_on_directories(false);
    let res = match service.oneshot(req).await {
        Ok(res) => res.map(Body::new),
        Err(e) => match e {},
    };
    if !res.status().is_success() && res.status() != StatusCode::NOT_MODIFIED {
        if res.status() == StatusCode::NOT_FOUND {
            return not_found(dir, &method).await;
        }
        return res;
    }
    let mut res = res;
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}

/// The decoded request path, or `None` when any segment is hidden (`.`
/// prefix, which covers `.` and `..`), contains a separator once decoded
/// (`%2f`, `\`, `%5c`) or a NUL, or does not decode to UTF-8.
fn servable_path(raw: &str) -> Option<String> {
    let mut decoded = String::with_capacity(raw.len());
    for (i, seg) in raw.split('/').enumerate() {
        let seg = percent_decode(seg)?;
        if seg.starts_with('.') || seg.contains(['/', '\\', '\0']) {
            return None;
        }
        if i > 0 {
            decoded.push('/');
        }
        decoded.push_str(&seg);
    }
    Some(decoded)
}

/// Strict percent-decoding: a malformed escape or non-UTF-8 result is `None`.
fn percent_decode(seg: &str) -> Option<String> {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

async fn not_found(dir: &Path, method: &Method) -> Response {
    let body = if *method == Method::HEAD {
        String::new()
    } else {
        tokio::fs::read_to_string(dir.join("404.html"))
            .await
            .unwrap_or_else(|_| "Not Found".to_owned())
    };
    (
        StatusCode::NOT_FOUND,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(NOT_FOUND_CACHE_CONTROL),
            ),
        ],
        body,
    )
        .into_response()
}
