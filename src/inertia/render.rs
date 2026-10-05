//! The `Inertia` extractor and `Inertia::render` (port of inertia-rails'
//! Renderer): JSON for Inertia visits, the HTML document otherwise.

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts, OriginalUri},
    http::{header, request::Parts, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use loco_rs::{app::AppContext, Error, Result};
use serde_json::{Map, Value};

use super::{
    config::Settings,
    document::Document,
    flash::{FlashConsumed, FlashState, IncomingFlash},
    headers::CspNonce,
    meta::{self, InertiaMeta, MetaTitleTemplate},
    page::Page,
    props::{always, Prop, Props, SharedProps},
    resolver::{resolve, Visit},
    ssr::SsrClient,
    vite,
};

pub const X_INERTIA: &str = "x-inertia";
pub const ERROR_BAG: &str = "x-inertia-error-bag";

/// Whether the request is an Inertia visit (`X-Inertia: true`).
#[must_use]
pub fn is_inertia_request(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(X_INERTIA)
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"true"))
}

/// Per-request Inertia renderer. Extract it in a handler and call
/// [`Inertia::render`].
pub struct Inertia {
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
        // Tags set earlier in the request (e.g. by a layer) via
        // `parts.extensions.insert(InertiaMeta)`.
        let meta = parts
            .extensions
            .get::<InertiaMeta>()
            .cloned()
            .unwrap_or_default();
        Ok(Self {
            parts: parts.clone(),
            ctx,
            settings,
            meta,
        })
    }
}

/// The SSR client, built once per app and kept in `shared_store`.
#[derive(Clone)]
struct SsrSlot(Option<SsrClient>);

impl Inertia {
    /// Builds the renderer from request parts directly (outside an extractor).
    #[must_use]
    pub fn new(parts: Parts, ctx: AppContext, settings: Arc<Settings>) -> Self {
        Self {
            parts,
            ctx,
            settings,
            meta: InertiaMeta::new(),
        }
    }

    /// Adds head tags for this render (inertia-rails' `inertia_meta.add` /
    /// `render inertia:, meta:`); later tags replace earlier ones with the
    /// same head key.
    #[must_use]
    pub fn meta(mut self, meta: InertiaMeta) -> Self {
        self.meta = std::mem::take(&mut self.meta).merged_with(meta);
        self
    }

    #[must_use]
    pub fn is_inertia_request(&self) -> bool {
        is_inertia_request(&self.parts.headers)
    }

    /// Renders `component` with `props` merged over the shared props.
    ///
    /// # Errors
    /// When the shared-props function or a (non-rescued) lazy prop fails, or
    /// (with `server_head`) a prop uses the reserved meta prop name.
    /// SSR failures are not errors: they fall back to client rendering.
    pub async fn render(self, component: &str, props: Props) -> Result<Response> {
        let page = self.page(component, props).await?;
        let mut res = if self.is_inertia_request() {
            let mut res = (
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json; charset=utf-8"),
                )],
                page.to_json(),
            )
                .into_response();
            res.headers_mut()
                .insert(X_INERTIA, HeaderValue::from_static("true"));
            res
        } else {
            self.html(&page).await
        };
        *res.status_mut() = StatusCode::OK;
        res.headers_mut()
            .append(header::VARY, HeaderValue::from_static("X-Inertia"));
        res.extensions_mut().insert(FlashConsumed);
        Ok(res)
    }

    /// Builds the page object without rendering it.
    ///
    /// # Errors
    /// See [`Inertia::render`].
    pub async fn page(&self, component: &str, props: Props) -> Result<Page> {
        let flash = self
            .parts
            .extensions
            .get::<IncomingFlash>()
            .map(|f| f.0.clone())
            .unwrap_or_default();

        // inertia_shared_data: the errors hash (always_include_errors_hash),
        // then the app's shared props; sharedProps lists all of their keys.
        let shared = Props::new().prop("errors", self.errors_prop(&flash));
        let shared = match self.ctx.shared_store.get::<SharedProps>() {
            Some(SharedProps(f)) => shared.merged_with(f(&self.parts, &self.ctx).await?),
            None => shared,
        };
        let mut shared_keys: Vec<String> = Vec::new();
        for key in shared.keys() {
            let top = key.split('.').next().unwrap_or(key).to_owned();
            if !shared_keys.contains(&top) {
                shared_keys.push(top);
            }
        }

        let mut all = shared.merged_with(props);
        self.validate_meta_prop(&all)?;
        // A user-supplied `errors` prop is always included, like inertia-rails.
        if let Some(errors) = all.get_mut("errors") {
            if !errors.is_modified() {
                let taken = std::mem::replace(errors, Prop::value(Value::Null));
                *errors = taken.always();
            }
        }

        let visit = Visit::from_headers(&self.parts.headers, component);
        let (mut resolved, metadata) = resolve(all, &visit).await?;
        self.merge_meta_tags(&mut resolved);

        let mut flash_map = Map::new();
        if let Some(notice) = &flash.notice {
            flash_map.insert("notice".into(), Value::String(notice.clone()));
        }
        if let Some(alert) = &flash.alert {
            flash_map.insert("alert".into(), Value::String(alert.clone()));
        }

        Ok(Page {
            component: component.to_owned(),
            props: resolved,
            url: self.original_url(),
            version: vite::shared(&self.settings.vite).version().to_owned(),
            encrypt_history: self.settings.encrypt_history,
            clear_history: flash.clear_history,
            flash: (!flash_map.is_empty()).then_some(flash_map),
            shared_props: (!shared_keys.is_empty()).then_some(shared_keys),
            preserve_fragment: flash.preserve_fragment,
            metadata,
        })
    }

    /// renderer.rb#validate_meta_prop!: with `server_head`, the meta prop name
    /// is reserved.
    #[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
    fn validate_meta_prop(&self, props: &Props) -> Result<()> {
        if !self.settings.server_head_enabled() {
            return Ok(());
        }
        let prop = self.settings.meta_prop();
        if props
            .keys()
            .iter()
            .any(|k| k.split('.').next() == Some(prop))
        {
            return Err(Error::Message(format!(
                "The `{prop}` prop is reserved by `settings.server_head`. Rename the conflicting prop, or set `server_head` to a custom prop name."
            )));
        }
        Ok(())
    }

    /// renderer.rb#merge_meta_tags!: applies the title template, then puts
    /// the tags under the meta prop (never filtered by partial reloads).
    fn merge_meta_tags(&self, props: &mut Map<String, Value>) {
        let mut tags = self.meta.clone();
        if let Some(MetaTitleTemplate(template)) = self.ctx.shared_store.get::<MetaTitleTemplate>()
        {
            tags.apply_title_template(&*template);
        }
        if tags.is_empty() {
            return;
        }
        props.insert(
            self.settings.meta_prop().to_owned(),
            tags.serialize(
                self.settings.server_head_enabled(),
                self.settings.head_attribute(),
            ),
        );
    }

    /// `{}` by default; flash errors nested under `X-Inertia-Error-Bag`
    /// (or the bag stored with the flash) when there is one.
    fn errors_prop(&self, flash: &FlashState) -> Prop {
        let errors = flash
            .errors
            .clone()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let bag = self
            .parts
            .headers
            .get(ERROR_BAG)
            .and_then(|v| v.to_str().ok())
            .filter(|b| !b.is_empty())
            .map(str::to_owned)
            .or_else(|| flash.error_bag.clone());
        let value = match bag {
            Some(bag) if flash.errors.is_some() => {
                let mut m = Map::new();
                m.insert(bag, errors);
                Value::Object(m)
            }
            _ => errors,
        };
        always(move || async move { Ok(value) })
    }

    /// Original path + query (before any nesting strips a prefix).
    fn original_url(&self) -> String {
        let uri = self
            .parts
            .extensions
            .get::<OriginalUri>()
            .map_or(&self.parts.uri, |o| &o.0);
        uri.path_and_query()
            .map_or_else(|| uri.path().to_owned(), ToString::to_string)
    }

    fn ssr_client(&self) -> Option<SsrClient> {
        if let Some(SsrSlot(client)) = self.ctx.shared_store.get::<SsrSlot>() {
            return client;
        }
        let client = SsrClient::from_settings(&self.settings);
        self.ctx.shared_store.insert(SsrSlot(client.clone()));
        client
    }

    async fn html(&self, page: &Page) -> Response {
        let nonce = self.parts.extensions.get::<CspNonce>().map(|n| n.0.clone());
        let ssr = match self.ssr_client() {
            Some(client) => client.render(page).await,
            None => None,
        };
        let vite = vite::shared(&self.settings.vite);
        let tags = vite.tags(nonce.as_deref());
        // inertia_meta_tags: with `serverHead` on the client, SSR output
        // carries the tags in its own head; write only the ones it lacks.
        let meta_tags = meta::head_html_missing_from(
            page.props.get(self.settings.meta_prop()),
            self.settings.head_attribute(),
            ssr.as_ref().map_or(&[][..], |s| &s.head[..]),
        );
        let html = Document {
            meta_tags: &meta_tags,
            app_name: &self.settings.app_name,
            page,
            vite_tags: &tags,
            nonce: nonce.as_deref(),
            ssr,
        }
        .render();
        (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )],
            html,
        )
            .into_response()
    }
}
