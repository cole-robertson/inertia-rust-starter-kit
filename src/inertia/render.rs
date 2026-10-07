//! The `Inertia` extractor and `Inertia::render` (inertia-omega's render: JSON for Inertia
//! visits, the HTML document otherwise), and the layer that runs omega around every request:
//! it reloads clients on outdated assets (409), applies the protocol's redirect rules and keeps
//! the flash session. The kit adds its shared props, `<head>` tags, Rails-style flash and the
//! external-redirect rule (`redirect.rs`) on top.

use std::{convert::Infallible, sync::Arc};

use axum::{
    body::Body,
    extract::{FromRef, FromRequestParts, OriginalUri, Request, State},
    http::{header, request::Parts, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
    Router,
};
use loco_rs::{app::AppContext, Error, Result};

use super::{
    config::Settings,
    document,
    flash::{CookieSession, OutgoingFlash},
    headers::CspNonce,
    meta::{InertiaMeta, MetaTitleTemplate},
    props::{Props, SharedProps},
    redirect,
    ssr::SsrClient,
    vite,
};

/// Per-request Inertia renderer. Extract it in a handler and call [`Inertia::render`].
pub struct Inertia {
    handle: omega::Inertia,
    parts: Parts,
    ctx: AppContext,
    settings: Arc<Settings>,
    meta: InertiaMeta,
}

impl<S> FromRequestParts<S> for Inertia
where
    AppContext: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        let ctx = AppContext::from_ref(state);
        let settings = ctx
            .shared_store
            .get::<Arc<Settings>>()
            .ok_or_else(|| Error::Message("inertia settings missing from shared_store".into()))?;
        let handle = parts
            .extensions
            .get::<omega::Inertia>()
            .cloned()
            .ok_or_else(|| Error::Message("the Inertia extractor needs `render::layer`".into()))?;
        // Tags set earlier in the request (e.g. by a layer) via
        // `parts.extensions.insert(InertiaMeta)`.
        let meta = parts
            .extensions
            .get::<InertiaMeta>()
            .cloned()
            .unwrap_or_default();
        Ok(Self {
            handle,
            parts: parts.clone(),
            ctx,
            settings,
            meta,
        })
    }
}

impl Inertia {
    /// Adds head tags for this render (inertia-rails' `inertia_meta.add` /
    /// `render inertia:, meta:`); later tags replace earlier ones with the
    /// same head key.
    #[must_use]
    pub fn meta(mut self, meta: InertiaMeta) -> Self {
        self.meta = std::mem::take(&mut self.meta).merged_with(meta);
        self
    }

    /// Renders `component` with `props` merged over the shared props.
    ///
    /// The props are resolved here, in the handler, rather than by the Inertia layer once the
    /// handler returns (omega's default): Loco's own middlewares (compression, ETag) sit
    /// between the two, and would otherwise see an empty body.
    ///
    /// # Errors
    /// When the shared-props function or a (non-rescued) lazy prop fails (a lazy prop's
    /// `NotFound` is a 404, as from the handler), or (with `server_head`) a prop uses the
    /// reserved meta prop name. SSR failures are not errors: they fall back to client rendering.
    pub async fn render(self, component: &str, props: Props) -> Result<Response> {
        let shared = match self.ctx.shared_store.get::<SharedProps>() {
            Some(SharedProps(f)) => f(&self.parts, &self.ctx).await?,
            None => Props::new(),
        };
        self.validate_meta_prop(shared.keys().chain(props.keys()))?;
        for (key, prop) in shared {
            self.handle.share(key, prop);
        }

        let mut response = self.handle.render(component, props);
        if let Some(tags) = self.meta_tags() {
            response = response.with(self.settings.meta_prop(), omega::always(tags));
        }
        if let Some(CspNonce(nonce)) = self.parts.extensions.get::<CspNonce>() {
            response = response.with_view_data(document::NONCE, nonce);
        }
        match response.try_into_http().await {
            Ok(res) => Ok(res.map(Body::from)),
            Err(error) => Err(prop_error(error)),
        }
    }

    /// renderer.rb#validate_meta_prop!: with `server_head`, the meta prop name
    /// is reserved.
    #[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
    fn validate_meta_prop<'k>(&self, mut keys: impl Iterator<Item = &'k str>) -> Result<()> {
        if !self.settings.server_head_enabled() {
            return Ok(());
        }
        let prop = self.settings.meta_prop();
        if keys.any(|k| k.split('.').next() == Some(prop)) {
            return Err(Error::Message(format!(
                "The `{prop}` prop is reserved by `settings.server_head`. Rename the conflicting prop, or set `server_head` to a custom prop name."
            )));
        }
        Ok(())
    }

    /// renderer.rb#merge_meta_tags!: the tags with the title template applied, for the meta
    /// prop (always sent, never filtered by partial reloads); `None` without tags.
    fn meta_tags(&self) -> Option<serde_json::Value> {
        let mut tags = self.meta.clone();
        if let Some(MetaTitleTemplate(template)) = self.ctx.shared_store.get::<MetaTitleTemplate>()
        {
            tags.apply_title_template(&*template);
        }
        (!tags.is_empty()).then(|| {
            tags.serialize(
                self.settings.server_head_enabled(),
                self.settings.head_attribute(),
            )
        })
    }
}

/// inertia-omega's configuration for these settings: the asset version, the HTML document
/// (`document.rs`), SSR, history encryption, and every validation message per field (the
/// frontend reads `errors.name` as a list, like inertia-rails sends it).
#[must_use]
pub fn config(settings: &Arc<Settings>) -> omega::Config {
    let version = settings.clone();
    let root = settings.clone();
    let config = omega::Config::new()
        .version_with(move || vite::shared(&version.vite).version().to_owned())
        .root_id(document::ROOT_ID)
        .root_view(move |view: &omega::View<'_>| document::render(&root, view))
        .encrypt_history(settings.encrypt_history)
        .with_all_errors(true);
    match SsrClient::from_settings(settings) {
        Some(client) => config.ssr(client),
        None => config,
    }
}

/// The Inertia layer, around every route: install it inside the flash layer (it reads and
/// writes the request's [`CookieSession`]) and outside anything that renders.
pub fn layer(router: Router, settings: Arc<Settings>) -> Router {
    let inertia = omega::axum::InertiaLayer::new(config(&settings))
        .session(|parts| parts.extensions.get::<CookieSession>().cloned());
    router.layer(axum::middleware::from_fn_with_state(
        Arc::new((inertia, settings)),
        middleware,
    ))
}

async fn middleware(
    State(state): State<Arc<(omega::axum::InertiaLayer, Arc<Settings>)>>,
    req: Request,
    next: Next,
) -> Response {
    let (inertia, settings) = &*state;
    let reload_location = reload_location(settings, &req);
    let handled = inertia
        .handle(req, |req| async move {
            let handle = req.extensions().get::<omega::Inertia>().cloned();
            let session = req.extensions().get::<CookieSession>().cloned();
            let host = req
                .headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut res = next.run(req).await;
            let Some(handle) = handle else {
                return Ok::<_, Infallible>(res);
            };
            if let Some(OutgoingFlash(flash)) = res.extensions_mut().remove::<OutgoingFlash>() {
                // A redirect's flash is the next page's whole flash, as the `_flash` cookie
                // held only the newest redirect's before.
                if let Some(session) = &session {
                    session.clear();
                }
                flash.queue(&handle);
            }
            if handle.request().is_inertia() {
                res = redirect::convert_external(res, &settings.app_url, host.as_deref());
            }
            Ok(res)
        })
        .await;
    let mut res = match handled {
        Ok(res) => res,
        Err(never) => match never {},
    };
    // omega's asset-version 409 (the only 409 that carries `X-Inertia-Version`) reloads the
    // request's own `Host`; the kit's reload stays on `app_url`, or the extra host it came in on.
    if res.status() == StatusCode::CONFLICT && res.headers().contains_key(omega::header::VERSION) {
        match HeaderValue::from_str(&reload_location) {
            Ok(location) => {
                res.headers_mut().insert(omega::header::LOCATION, location);
            }
            Err(e) => tracing::error!(%reload_location, error = %e, "invalid X-Inertia-Location"),
        }
    }
    if res.headers().get(omega::header::INERTIA).is_some() {
        res.headers_mut()
            .insert(header::CONTENT_TYPE, JSON_CONTENT_TYPE);
    }
    res
}

/// Where a client running outdated assets reloads: the request's path and query on `app_url`,
/// or on the extra host the request came in on (`settings.extra_hosts`), whatever `Host` says.
fn reload_location(settings: &Settings, req: &Request) -> String {
    let uri = req
        .extensions()
        .get::<OriginalUri>()
        .map_or(req.uri(), |o| &o.0);
    let path = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), ToString::to_string);
    let base = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| settings.extra_origin_for(h))
        .unwrap_or_else(|| settings.base_url().to_owned());
    format!("{base}{path}")
}

/// A lazy prop's error as the handler's: the kit's prop closures return `loco_rs::Error`.
fn prop_error(error: omega::PropError) -> Error {
    match error.into_inner().downcast::<Error>() {
        Ok(error) => *error,
        Err(error) => Error::Message(format!("failed to resolve Inertia props: {error}")),
    }
}

/// The page JSON's `Content-Type`: `application/json; charset=utf-8`, as Rails answers it.
const JSON_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("application/json; charset=utf-8");
