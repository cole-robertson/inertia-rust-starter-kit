//! Inertia.js v3 for the kit: inertia-omega (the protocol: props, page object, render,
//! versioning, redirect rules) plus the kit's own layers around it. See docs/INERTIA.md.

pub mod config;
pub mod cookies;
pub mod csrf;
pub mod document;
pub mod exceptions;
pub mod flash;
pub mod headers;
pub mod meta;
pub mod precognition;
pub mod props;
pub mod public;
pub mod redirect;
pub mod render;
pub mod request_log;
pub mod ssr;
pub mod timing;
pub mod vite;

pub use config::Settings;
pub use meta::{InertiaMeta, MetaTag, MetaTitleTemplate};
pub use props::{
    always, deep_merge, defer, lazy, merge, once, optional, scroll, scroll_with, Paginator, Prop,
    Props, ScrollMetadata, SharedProps, SharedPropsFn,
};
pub use render::Inertia;

use std::sync::Arc;

use axum::Router;
use loco_rs::{
    app::AppContext,
    controller::middleware::{self, MiddlewareLayer, MiddlewareStackExt},
    environment::Environment,
    Result,
};

/// Boot-time setup for `Hooks::after_context`: validates `settings:`, loads the
/// Vite manifest once (required in production) and stores `Arc<Settings>` in
/// `ctx.shared_store`.
///
/// # Errors
/// Invalid settings, or (production) a short secret, empty `app_url` or a
/// missing Vite manifest.
#[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
pub fn install(ctx: &AppContext) -> Result<Arc<Settings>> {
    let settings = Settings::from_ctx(ctx)?;
    vite::init(&settings.vite, ctx.environment == Environment::Production)?;
    ctx.shared_store.insert(settings.clone());
    Ok(settings)
}

/// For `Hooks::before_routes`: the base router, whose fallback serves
/// `public/` for GET/HEAD requests no route matched (see [`public`]).
pub fn base_router() -> Router<AppContext> {
    Router::new().fallback(public::fallback)
}

/// The service `Hooks::serve` runs: the app router behind Rails-style trailing-slash
/// handling (`/sign_in/` routes like `/sign_in`), which has to happen before routing.
pub fn service(router: Router) -> tower_http::normalize_path::NormalizePath<Router> {
    use tower::Layer;
    tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash().layer(router)
}

/// For `Hooks::middlewares`: Loco's default stack with its `logger` replaced
/// by [`request_log::Middleware`], which redacts sensitive query params.
#[must_use]
pub fn middlewares(ctx: &AppContext) -> Vec<Box<dyn MiddlewareLayer>> {
    let mut stack = middleware::default_middleware_stack(ctx);
    stack.replace("logger", Box::new(request_log::Middleware::from_ctx(ctx)));
    stack
}
