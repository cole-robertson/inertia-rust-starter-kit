//! Page props: inertia-omega's `Props` and `Prop`, with lazy constructors whose closures return
//! Loco's `Result`, as every handler does.
//!
//! ```ignore
//! Props::new()
//!     .with("user", json!({"name": "Ada"}))                      // plain value
//!     .with("account", account.to_props())                       // a typed props struct
//!     .with("stats", lazy(|| async { Ok(stats().await?) }))      // only evaluated when kept
//!     .with("permissions", defer(|| async { .. }).group("sidebar"))
//!     .with("auth.user", json!(..))                              // dot notation nests
//! ```
//!
//! A closure's error fails the render the way a handler's error does (a `NotFound` is still a
//! 404), unless the prop is `.rescue()`d. Merge, deep merge, once and scroll behaviour are
//! omega's builder methods (`.merge()`, `.match_on("id")`, `.once_as(key)`, ...).

use std::future::Future;

pub use omega::{
    always, deep_merge, merge, scroll, scroll_with, IntoProp, IntoProps, Paginator, Prop, Props,
    ProvidesScrollMetadata, ScrollMetadata,
};

use loco_rs::Result;
use serde::Serialize;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A value computed only when the prop is kept (a Ruby `-> { }` prop).
pub fn lazy<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    omega::try_lazy(f)
}

/// `InertiaRails.optional`: never on the first visit, only on partial reloads that ask.
pub fn optional<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).optional()
}

/// `InertiaRails.defer`: loaded by the client after the first render, in the
/// `default` group unless `.group(..)` is set.
pub fn defer<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).deferred()
}

/// `InertiaRails.once`: the client caches it and sends its key back in
/// `X-Inertia-Except-Once-Props`, after which it is not resent.
pub fn once<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).once()
}

/// Builds the shared props for a request (inertia-rails' `inertia_share`). It runs when a page
/// renders, not on every request, so redirects and JSON endpoints never pay for its queries.
pub type SharedPropsFn = std::sync::Arc<
    dyn Fn(
            &axum::http::request::Parts,
            &loco_rs::app::AppContext,
        ) -> BoxFuture<'static, Result<Props>>
        + Send
        + Sync,
>;

/// `ctx.shared_store` entry holding the app's [`SharedPropsFn`].
#[derive(Clone)]
pub struct SharedProps(pub SharedPropsFn);
