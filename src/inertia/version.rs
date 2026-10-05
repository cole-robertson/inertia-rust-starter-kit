//! Asset-version check: an Inertia GET carrying a stale `X-Inertia-Version`
//! gets `409 Conflict` + `X-Inertia-Location`, so the client does a full page
//! load. The flash middleware keeps the flash on 409.

use std::sync::Arc;

use axum::{
    extract::{OriginalUri, Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};

use super::{config::Settings, render::is_inertia_request, vite};

pub const X_INERTIA_VERSION: &str = "x-inertia-version";
pub const X_INERTIA_LOCATION: &str = "x-inertia-location";

pub fn layer(router: Router, settings: Arc<Settings>) -> Router {
    router.layer(axum::middleware::from_fn_with_state(settings, middleware))
}

async fn middleware(State(settings): State<Arc<Settings>>, req: Request, next: Next) -> Response {
    if req.method() == Method::GET && is_inertia_request(req.headers()) {
        let server = vite::shared(&settings.vite);
        let client = req
            .headers()
            .get(X_INERTIA_VERSION)
            .and_then(|v| v.to_str().ok());
        if client != Some(server.version()) {
            let uri = req
                .extensions()
                .get::<OriginalUri>()
                .map_or_else(|| req.uri().clone(), |o| o.0.clone());
            let path = uri
                .path_and_query()
                .map_or_else(|| uri.path().to_owned(), ToString::to_string);
            // A tab on one of the extra hosts reloads on that host (its own session).
            let base = req
                .headers()
                .get(axum::http::header::HOST)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| settings.extra_origin_for(h))
                .unwrap_or_else(|| settings.base_url().to_owned());
            let location = format!("{base}{path}");
            let mut res = StatusCode::CONFLICT.into_response();
            match HeaderValue::from_str(&location) {
                Ok(v) => {
                    res.headers_mut().insert(X_INERTIA_LOCATION, v);
                }
                Err(e) => tracing::error!(%location, error = %e, "invalid X-Inertia-Location"),
            }
            return res;
        }
    }
    next.run(req).await
}
