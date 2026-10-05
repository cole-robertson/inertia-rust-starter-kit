//! Per-request CSP nonce + security response headers.
//!
//! The nonce is inserted as the `CspNonce` request extension; the HTML
//! document builder puts it on its inline `<script>` tags. Headers a handler
//! already set are left alone.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::Rng;

use super::config::Settings;

/// Request extension: this request's CSP nonce (base64, 128 bits).
#[derive(Clone, Debug)]
pub struct CspNonce(pub String);

pub const HSTS_VALUE: &str = "max-age=63072000; includeSubDomains";

pub fn layer(router: Router, settings: Arc<Settings>) -> Router {
    router.layer(axum::middleware::from_fn_with_state(settings, middleware))
}

pub fn generate_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    STANDARD.encode(bytes)
}

/// The Content-Security-Policy value for `nonce`.
pub fn content_security_policy(settings: &Settings, nonce: &str) -> String {
    let mut script = format!("'self' 'nonce-{nonce}'");
    let mut style = "'self' 'unsafe-inline'".to_owned();
    let mut img = "'self' data:".to_owned();
    let mut font = "'self' data:".to_owned();
    let mut connect = "'self'".to_owned();
    if settings.vite.dev_server {
        if let Ok(vite) = url::Url::parse(&settings.vite.dev_server_url) {
            let origin = vite.origin().ascii_serialization();
            for directive in [&mut script, &mut style, &mut img, &mut font, &mut connect] {
                directive.push(' ');
                directive.push_str(&origin);
            }
            let ws_scheme = if vite.scheme() == "https" {
                "wss"
            } else {
                "ws"
            };
            let ws = origin.replacen(vite.scheme(), ws_scheme, 1);
            connect.push(' ');
            connect.push_str(&ws);
        } else {
            tracing::warn!(url = %settings.vite.dev_server_url, "invalid vite.dev_server_url; CSP not widened");
        }
    }
    format!(
        "default-src 'self'; script-src {script}; style-src {style}; img-src {img}; \
         font-src {font}; connect-src {connect}; object-src 'none'; base-uri 'self'; \
         form-action 'self'; frame-ancestors 'none'"
    )
}

fn set_default(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if !headers.contains_key(&name) {
        headers.insert(
            name,
            HeaderValue::from_str(value).expect("static header values are valid"),
        );
    }
}

async fn middleware(
    State(settings): State<Arc<Settings>>,
    mut req: Request,
    next: Next,
) -> Response {
    let nonce = generate_nonce();
    req.extensions_mut().insert(CspNonce(nonce.clone()));
    let mut res = next.run(req).await;
    // Rack's defaults when the app set none: `no-cache` on redirects, and on rendered pages
    // `max-age=0, private, must-revalidate` (Rack::ETag). Public files and errors set theirs.
    let cache = match res.status() {
        s if s.is_redirection() => Some("no-cache"),
        s if s.is_success() => Some("max-age=0, private, must-revalidate"),
        _ => None,
    };
    let h = res.headers_mut();
    if let Some(cache) = cache {
        set_default(h, header::CACHE_CONTROL, cache);
    }
    set_default(
        h,
        header::CONTENT_SECURITY_POLICY,
        &content_security_policy(&settings, &nonce),
    );
    set_default(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set_default(
        h,
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin",
    );
    set_default(h, header::X_FRAME_OPTIONS, "DENY");
    // Rails' remaining `default_headers`.
    set_default(h, HeaderName::from_static("x-xss-protection"), "0");
    set_default(
        h,
        HeaderName::from_static("x-permitted-cross-domain-policies"),
        "none",
    );
    set_default(
        h,
        HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=()",
    );
    set_default(
        h,
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    // The resolved AppContext environment (see `Settings::production`), not
    // process env: `--environment production` must get HSTS too.
    if settings.production {
        set_default(h, header::STRICT_TRANSPORT_SECURITY, HSTS_VALUE);
    }
    res
}
