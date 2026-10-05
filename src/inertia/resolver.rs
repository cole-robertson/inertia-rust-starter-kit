//! Partial-reload filtering and page metadata (port of inertia-rails'
//! props_resolver.rb). Lazy props are evaluated only when kept.

use axum::http::HeaderMap;
use loco_rs::Result;
use serde::Serialize;
use serde_json::{Map, Value};

use super::props::{BoxFuture, Kind, MergeSpec, Prop, Props, Source};

/// What the client asked for, read from the `X-Inertia-*` request headers.
#[derive(Debug, Clone, Default)]
pub struct Visit {
    /// `X-Inertia-Partial-Component` equals the rendered component.
    pub partial: bool,
    /// `X-Inertia-Partial-Data`.
    pub only: Vec<String>,
    /// `X-Inertia-Partial-Except`.
    pub except: Vec<String>,
    /// `X-Inertia-Reset`.
    pub reset: Vec<String>,
    /// `X-Inertia-Except-Once-Props`.
    pub except_once: Vec<String>,
    /// `X-Inertia-Infinite-Scroll-Merge-Intent` (`append` | `prepend`).
    pub scroll_intent: Option<String>,
}

pub const PARTIAL_COMPONENT: &str = "x-inertia-partial-component";
pub const PARTIAL_DATA: &str = "x-inertia-partial-data";
pub const PARTIAL_EXCEPT: &str = "x-inertia-partial-except";
pub const RESET: &str = "x-inertia-reset";
pub const EXCEPT_ONCE_PROPS: &str = "x-inertia-except-once-props";
pub const SCROLL_MERGE_INTENT: &str = "x-inertia-infinite-scroll-merge-intent";

fn header_list(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Visit {
    /// Reads the visit for rendering `component`.
    #[must_use]
    pub fn from_headers(headers: &HeaderMap, component: &str) -> Self {
        Self {
            partial: headers.get(PARTIAL_COMPONENT).and_then(|v| v.to_str().ok())
                == Some(component),
            only: header_list(headers, PARTIAL_DATA),
            except: header_list(headers, PARTIAL_EXCEPT),
            reset: header_list(headers, RESET),
            except_once: header_list(headers, EXCEPT_ONCE_PROPS),
            scroll_intent: headers
                .get(SCROLL_MERGE_INTENT)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
        }
    }
}

/// Page metadata keys; empty ones are omitted from the page.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub deferred_props: Map<String, Value>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub scroll_props: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub merge_props: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prepend_props: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deep_merge_props: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub match_props_on: Vec<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub once_props: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rescued_props: Vec<String>,
}

/// Resolves `props` for `visit`: `(props object, metadata)`.
///
/// # Errors
/// The first lazy prop that fails (unless it was marked `rescue`).
pub async fn resolve(props: Props, visit: &Visit) -> Result<(Map<String, Value>, Metadata)> {
    let mut r = Resolver {
        visit,
        meta: Metadata::default(),
    };
    let props = props.expand_dot_notation().await?;
    let resolved = r.transform(props, String::new(), false).await?;
    Ok((resolved, r.meta))
}

struct Resolver<'v> {
    visit: &'v Visit,
    meta: Metadata,
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

impl Resolver<'_> {
    /// deep_transform_props: resolves every entry of `props` under `prefix`.
    fn transform(
        &mut self,
        props: Props,
        prefix: String,
        parent_resolved: bool,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            let mut out = Map::new();
            for (key, prop) in props.entries {
                let path = join(&prefix, &key);
                if let Some(value) = self.entry(prop, path, parent_resolved).await? {
                    out.insert(key, value);
                }
            }
            Ok(out)
        })
    }

    /// One prop at `path`; `None` omits it.
    fn entry(
        &mut self,
        mut prop: Prop,
        path: String,
        parent_resolved: bool,
    ) -> BoxFuture<'_, Result<Option<Value>>> {
        Box::pin(async move {
            // Unmodified containers: recurse (filtering only).
            if !prop.is_modified() {
                match prop.source {
                    Source::Nested(nested) if !nested.is_empty() => {
                        if !parent_resolved && self.excluded_by_partial(&path) {
                            return Ok(None);
                        }
                        let map = self.transform(nested, path, parent_resolved).await?;
                        return Ok((!map.is_empty()).then_some(Value::Object(map)));
                    }
                    Source::Value(Value::Object(map)) if !map.is_empty() => {
                        if !parent_resolved && self.excluded_by_partial(&path) {
                            return Ok(None);
                        }
                        let map = self.filter_json(map, &path, parent_resolved);
                        return Ok((!map.is_empty()).then_some(Value::Object(map)));
                    }
                    Source::Value(Value::Array(items)) => {
                        if !parent_resolved && self.excluded_by_partial(&path) {
                            return Ok(None);
                        }
                        return Ok(Some(Value::Array(items)));
                    }
                    Source::Array(items) => {
                        if !parent_resolved && self.excluded_by_partial(&path) {
                            return Ok(None);
                        }
                        return self
                            .transform_array(items, path, parent_resolved)
                            .await
                            .map(Some);
                    }
                    source => prop.source = source,
                }
            }

            self.apply_scroll_intent(&mut prop);
            self.collect_metadata(&prop, &path);
            if !self.keep(&prop, &path, parent_resolved) {
                return Ok(None);
            }

            let rescue = prop.rescue;
            // A plain closure (a Ruby Proc, as opposed to a BaseProp) may
            // return a prop wrapper or a tree of them.
            let closure = !prop.is_modified();
            match self.evaluate(prop, &path, parent_resolved, closure).await {
                Ok(v) => Ok(v),
                Err(e) if rescue => {
                    tracing::error!(prop = %path, error = %e, "inertia: rescued prop error");
                    self.meta.rescued_props.push(path);
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        })
    }

    /// Evaluates a kept prop. A closure's result is unwrapped once when it is
    /// a prop wrapper (its metadata collected and inclusion rules applied at
    /// `path`), and nested props or arrays it returns are resolved as a
    /// parent that was already resolved (no further partial filtering).
    fn evaluate<'a>(
        &'a mut self,
        prop: Prop,
        path: &'a str,
        parent_resolved: bool,
        closure: bool,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move {
            match prop.source {
                Source::Value(v) => Ok(Some(v)),
                Source::Nested(nested) => {
                    let was_empty = nested.is_empty();
                    let map = self.transform(nested, path.to_owned(), true).await?;
                    Ok((was_empty || !map.is_empty()).then_some(Value::Object(map)))
                }
                Source::Array(items) => self
                    .transform_array(items, path.to_owned(), true)
                    .await
                    .map(Some),
                Source::Lazy(f) => {
                    let mut value = f().await?;
                    if closure && value.is_modified() {
                        self.apply_scroll_intent(&mut value);
                        self.collect_metadata(&value, path);
                        if !self.keep(&value, path, parent_resolved) {
                            return Ok(None);
                        }
                    }
                    self.evaluate(value, path, true, false).await
                }
            }
        })
    }

    /// transform_array: an array that holds no prop wrapper or closure
    /// (Rails' `needs_transform?`) is returned intact, exactly as the same
    /// data would be through [`Prop::value`]. Otherwise items that are maps
    /// resolve at `path.<index>` (and are dropped when that leaves them
    /// empty); other items are evaluated.
    fn transform_array(
        &mut self,
        items: Vec<Prop>,
        path: String,
        parent_resolved: bool,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            if !items.iter().any(needs_transform) {
                return Ok(Value::Array(items.into_iter().map(into_json).collect()));
            }
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                let item_path = join(&path, &i.to_string());
                let map = match item.source {
                    Source::Nested(nested) if !item.is_modified() => Some(nested),
                    Source::Value(obj @ Value::Object(_)) if !item.is_modified() => {
                        Some(Props::from_json(obj))
                    }
                    source => {
                        let item = Prop { source, ..item };
                        if let Some(v) = self.evaluate(item, &item_path, true, false).await? {
                            out.push(v);
                        }
                        None
                    }
                };
                if let Some(map) = map {
                    let map = self.transform(map, item_path, parent_resolved).await?;
                    if !map.is_empty() {
                        out.push(Value::Object(map));
                    }
                }
            }
            Ok(Value::Array(out))
        })
    }

    /// Partial-reload filtering inside a plain JSON object.
    fn filter_json(
        &self,
        map: Map<String, Value>,
        prefix: &str,
        parent_resolved: bool,
    ) -> Map<String, Value> {
        let mut out = Map::new();
        for (key, value) in map {
            let path = join(prefix, &key);
            if !parent_resolved && self.excluded_by_partial(&path) {
                continue;
            }
            match value {
                Value::Object(inner) if !inner.is_empty() => {
                    let inner = self.filter_json(inner, &path, parent_resolved);
                    if !inner.is_empty() {
                        out.insert(key, Value::Object(inner));
                    }
                }
                v => {
                    out.insert(key, v);
                }
            }
        }
        out
    }

    /// A scroll prop appends (or prepends, per the client's intent) at its
    /// wrapper key, or at the root without one.
    fn apply_scroll_intent(&self, prop: &mut Prop) {
        let Some(scroll) = prop.scroll.as_ref() else {
            return;
        };
        let prepend = self.visit.scroll_intent.as_deref() == Some("prepend");
        let spec = prop.merge.get_or_insert_with(MergeSpec::default);
        spec.append = !prepend;
        spec.appends_at.clear();
        spec.prepends_at.clear();
        if let Some(wrapper) = &scroll.wrapper {
            if prepend {
                spec.prepends_at.push(wrapper.clone());
            } else {
                spec.appends_at.push(wrapper.clone());
            }
        }
    }

    fn collect_metadata(&mut self, prop: &Prop, path: &str) {
        // Deferred
        if let Kind::Defer { group } = &prop.kind {
            if !self.visit.partial && !self.excluded_by_once_cache(prop, path) {
                let list = self
                    .meta
                    .deferred_props
                    .entry(group.clone())
                    .or_insert_with(|| Value::Array(vec![]));
                if let Value::Array(items) = list {
                    items.push(Value::String(path.to_owned()));
                }
            }
        }

        // Merge / scroll
        if let Some(spec) = &prop.merge {
            if !(self.visit.partial && self.excluded_by_partial(path)) {
                let resetting = self.visit.reset.iter().any(|k| k == path);
                if let Some(scroll) = &prop.scroll {
                    if self.visit.partial || !matches!(prop.kind, Kind::Defer { .. }) {
                        let mut meta = serde_json::to_value(&scroll.metadata)
                            .unwrap_or_else(|_| Value::Object(Map::new()));
                        if let Value::Object(m) = &mut meta {
                            m.insert("reset".into(), Value::Bool(resetting));
                        }
                        self.meta.scroll_props.insert(path.to_owned(), meta);
                    }
                }
                if !resetting {
                    let at_root = spec.appends_at.is_empty() && spec.prepends_at.is_empty();
                    if spec.deep {
                        self.meta.deep_merge_props.push(path.to_owned());
                    } else if at_root && spec.append {
                        self.meta.merge_props.push(path.to_owned());
                    } else if at_root {
                        self.meta.prepend_props.push(path.to_owned());
                    } else {
                        for p in &spec.appends_at {
                            self.meta.merge_props.push(join(path, p));
                        }
                        for p in &spec.prepends_at {
                            self.meta.prepend_props.push(join(path, p));
                        }
                    }
                    for m in &spec.match_on {
                        self.meta.match_props_on.push(join(path, m));
                    }
                }
            }
        }

        // Once
        if let Some(once) = &prop.once {
            if !self.excluded_by_partial(path) {
                let key = once.key.clone().unwrap_or_else(|| path.to_owned());
                let mut entry = Map::new();
                entry.insert("prop".into(), Value::String(path.to_owned()));
                if let Some(at) = once.expires_at {
                    entry.insert("expiresAt".into(), Value::from(at));
                }
                self.meta.once_props.insert(key, Value::Object(entry));
            }
        }
    }

    fn keep(&self, prop: &Prop, path: &str, parent_resolved: bool) -> bool {
        if prop.kind == Kind::Always {
            return true;
        }
        if self.excluded_by_once_cache(prop, path) {
            return false;
        }
        if !parent_resolved && self.excluded_by_partial(path) {
            return false;
        }
        // Optional/deferred props are only sent on partial reloads.
        self.visit.partial || !matches!(prop.kind, Kind::Optional | Kind::Defer { .. })
    }

    fn excluded_by_once_cache(&self, prop: &Prop, path: &str) -> bool {
        let Some(once) = &prop.once else {
            return false;
        };
        if once.fresh || self.explicitly_requested(path) {
            return false;
        }
        let key = once.key.as_deref().unwrap_or(path);
        self.visit.except_once.iter().any(|k| k == key)
    }

    fn explicitly_requested(&self, path: &str) -> bool {
        self.visit.partial
            && !self.visit.only.is_empty()
            && self.visit.only.iter().any(|k| related(k, path))
    }

    fn excluded_by_partial(&self, path: &str) -> bool {
        if !self.visit.partial || (self.visit.only.is_empty() && self.visit.except.is_empty()) {
            return false;
        }
        let by_only =
            !self.visit.only.is_empty() && !self.visit.only.iter().any(|k| related(k, path));
        let by_except = self
            .visit
            .except
            .iter()
            .any(|k| k == path || path.starts_with(&format!("{k}.")));
        by_only || by_except
    }
}

/// `key` names `path`, one of its ancestors, or one of its descendants.
/// Rails' `needs_transform?`: whether `prop` holds a prop wrapper or a
/// closure anywhere (plain JSON and plain nested props/arrays do not).
fn needs_transform(prop: &Prop) -> bool {
    if prop.is_modified() {
        return true;
    }
    match &prop.source {
        Source::Value(_) => false,
        Source::Lazy(_) => true,
        Source::Nested(nested) => nested.entries.iter().any(|(_, p)| needs_transform(p)),
        Source::Array(items) => items.iter().any(needs_transform),
    }
}

/// The JSON of a prop for which [`needs_transform`] is false.
fn into_json(prop: Prop) -> Value {
    match prop.source {
        Source::Value(v) => v,
        Source::Nested(nested) => Value::Object(
            nested
                .entries
                .into_iter()
                .map(|(k, p)| (k, into_json(p)))
                .collect(),
        ),
        Source::Array(items) => Value::Array(items.into_iter().map(into_json).collect()),
        Source::Lazy(_) => unreachable!("needs_transform is true for lazy props"),
    }
}

fn related(key: &str, path: &str) -> bool {
    key == path || path.starts_with(&format!("{key}.")) || key.starts_with(&format!("{path}."))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use serde_json::json;

    use super::*;
    use crate::inertia::props::{
        defer, lazy, lazy_prop, merge, once, optional, Prop, ScrollMetadata,
    };

    fn partial(only: &[&str], except: &[&str]) -> Visit {
        Visit {
            partial: true,
            only: only.iter().map(|s| (*s).to_owned()).collect(),
            except: except.iter().map(|s| (*s).to_owned()).collect(),
            ..Visit::default()
        }
    }

    async fn run(props: Props, visit: &Visit) -> (Value, Value) {
        let (p, m) = resolve(props, visit).await.unwrap();
        (Value::Object(p), serde_json::to_value(m).unwrap())
    }

    #[tokio::test]
    async fn lazy_props_run_only_when_kept() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let props = Props::new().prop("a", 1i64).prop(
            "b",
            lazy(move || async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(2)
            }),
        );
        let (p, _) = run(props, &partial(&["a"], &[])).await;
        assert_eq!(p, json!({"a": 1}));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dot_paths_select_inside_plain_objects() {
        let props = Props::new()
            .prop(
                "auth",
                json!({"user": {"id": 1, "name": "A"}, "session": 9}),
            )
            .prop("other", 3i64);
        let (p, _) = run(props, &partial(&["auth.user.name"], &[])).await;
        assert_eq!(p, json!({"auth": {"user": {"name": "A"}}}));
        let props = Props::new().prop("auth", json!({"user": 1, "session": 9}));
        let (p, _) = run(props, &partial(&[], &["auth.session"])).await;
        assert_eq!(p, json!({"auth": {"user": 1}}));
    }

    #[tokio::test]
    async fn optional_and_deferred_skip_first_load() {
        let props = Props::new()
            .prop("o", optional(|| async { Ok(1) }))
            .prop("d", defer(|| async { Ok(2) }).group("g"));
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({}));
        assert_eq!(m, json!({"deferredProps": {"g": ["d"]}}));
    }

    #[tokio::test]
    async fn scroll_prepend_intent_uses_wrapper() {
        let visit = Visit {
            scroll_intent: Some("prepend".into()),
            ..Visit::default()
        };
        let props = Props::new().prop(
            "posts",
            lazy(|| async { Ok(json!({"data": [1]})) })
                .scroll(ScrollMetadata::new("page", None, Some(2), 1))
                .wrapper("data"),
        );
        let (_, m) = run(props, &visit).await;
        assert_eq!(m["prependProps"], json!(["posts.data"]));
        assert_eq!(m["scrollProps"]["posts"]["reset"], json!(false));
    }

    #[tokio::test]
    async fn rescued_prop_is_reported_not_fatal() {
        let props = Props::new().prop(
            "bad",
            lazy(|| async { Err::<i64, _>(loco_rs::Error::string("boom")) }).rescue(),
        );
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({}));
        assert_eq!(m, json!({"rescuedProps": ["bad"]}));
        let failing = Props::new().prop(
            "bad",
            lazy(|| async { Err::<i64, _>(loco_rs::Error::string("boom")) }),
        );
        assert!(resolve(failing, &Visit::default()).await.is_err());
    }

    // ---- #13: dot notation, ported from props_resolver_spec.rb

    #[tokio::test]
    async fn dot_key_then_plain_object_merges_both() {
        let props = Props::new()
            .prop("user.name", "Ada")
            .prop("user", json!({"id": 1}));
        let (p, _) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"user": {"name": "Ada", "id": 1}}));
    }

    #[tokio::test]
    async fn plain_object_then_dot_key_merges_both() {
        let props = Props::new()
            .prop("user", json!({"id": 1}))
            .prop("user.name", "Ada");
        let (p, _) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"user": {"id": 1, "name": "Ada"}}));
        // A nested Props parent is extended the same way.
        let props = Props::new()
            .prop("user.name", "Ada")
            .prop("user", Props::new().prop("id", 1i64));
        let (p, _) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"user": {"name": "Ada", "id": 1}}));
    }

    #[tokio::test]
    async fn dot_key_merges_into_a_lazy_parent() {
        // 'dot-notation prop merges when parent is a closure'
        let props = Props::new()
            .prop(
                "auth",
                lazy(|| async {
                    Ok(json!({"user": {"name": "Jonathan", "email": "jonathan@example.com"}}))
                }),
            )
            .prop(
                "auth.user.permissions",
                lazy(|| async { Ok(json!(["edit-posts", "delete-posts"])) }),
            );
        let (p, _) = run(props, &Visit::default()).await;
        assert_eq!(
            p,
            json!({"auth": {"user": {"name": "Jonathan", "email": "jonathan@example.com",
                                     "permissions": ["edit-posts", "delete-posts"]}}})
        );
    }

    #[tokio::test]
    async fn dot_key_with_prop_type_is_deferred_under_its_parent() {
        let props = Props::new()
            .prop("auth.notifications", defer(|| async { Ok(json!(["msg"])) }))
            .prop("auth.user", "Jonathan");
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"auth": {"user": "Jonathan"}}));
        assert_eq!(
            m["deferredProps"],
            json!({"default": ["auth.notifications"]})
        );
    }

    #[tokio::test]
    async fn dot_key_under_a_scalar_is_an_error() {
        let props = Props::new().prop("a", 1i64).prop("a.b", 2i64);
        assert!(resolve(props, &Visit::default()).await.is_err());
    }

    // ---- #14: closures returning prop types, arrays holding prop types

    #[tokio::test]
    async fn closure_returning_defer_prop_is_excluded_with_metadata() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let props = Props::new().prop(
            "notifications",
            lazy_prop(move || async move {
                Ok(defer(move || async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(json!([]))
                })
                .group("alerts"))
            }),
        );
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({}));
        assert_eq!(m, json!({"deferredProps": {"alerts": ["notifications"]}}));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn closure_returning_merge_or_once_prop_resolves_with_metadata() {
        let props = Props::new()
            .prop(
                "posts",
                lazy_prop(|| async { Ok(merge(|| async { Ok(json!([{"id": 1}])) })) }),
            )
            .prop(
                "locale",
                lazy_prop(|| async { Ok(once(|| async { Ok("en") })) }),
            );
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"posts": [{"id": 1}], "locale": "en"}));
        assert_eq!(m["mergeProps"], json!(["posts"]));
        assert_eq!(m["onceProps"], json!({"locale": {"prop": "locale"}}));
    }

    #[tokio::test]
    async fn closure_returning_props_with_deferred_children() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let tree = move || {
            let c = c.clone();
            lazy_prop(move || async move {
                Ok(Props::new()
                    .prop("user", "Jonathan")
                    .prop(
                        "notifications",
                        defer(move || async move {
                            c.fetch_add(1, Ordering::SeqCst);
                            Ok(json!(["msg"]))
                        }),
                    )
                    .prop("roles", defer(|| async { Ok(json!(["admin"])) })))
            })
        };
        let (p, m) = run(Props::new().prop("auth", tree()), &Visit::default()).await;
        assert_eq!(p, json!({"auth": {"user": "Jonathan"}}));
        assert_eq!(
            m["deferredProps"],
            json!({"default": ["auth.notifications", "auth.roles"]})
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let visit = Visit {
            partial: true,
            only: vec!["auth.notifications".into(), "auth.roles".into()],
            ..Visit::default()
        };
        let (p, _) = run(Props::new().prop("auth", tree()), &visit).await;
        assert_eq!(p["auth"]["notifications"], json!(["msg"]));
        assert_eq!(p["auth"]["roles"], json!(["admin"]));
    }

    fn foos() -> Prop {
        Prop::array(vec![
            Props::new()
                .prop("name", "First")
                .prop("bar", optional(|| async { Ok("expensive-1") }))
                .prop("notifications", defer(|| async { Ok(json!(["msg"])) })),
            Props::new()
                .prop("name", "Second")
                .prop("bar", optional(|| async { Ok("expensive-2") })),
        ])
    }

    #[tokio::test]
    async fn array_items_hold_prop_wrappers_at_indexed_paths() {
        let (p, m) = run(Props::new().prop("foos", foos()), &Visit::default()).await;
        assert_eq!(p, json!({"foos": [{"name": "First"}, {"name": "Second"}]}));
        assert_eq!(
            m["deferredProps"],
            json!({"default": ["foos.0.notifications"]})
        );

        let (p, _) = run(Props::new().prop("foos", foos()), &partial(&["foos"], &[])).await;
        assert_eq!(p["foos"][0]["bar"], json!("expensive-1"));
        assert_eq!(p["foos"][0]["notifications"], json!(["msg"]));
        assert_eq!(p["foos"][1]["bar"], json!("expensive-2"));

        let (p, _) = run(
            Props::new().prop("foos", foos()),
            &partial(&["foos.0.bar"], &[]),
        )
        .await;
        assert_eq!(p, json!({"foos": [{"bar": "expensive-1"}]}));

        let (p, _) = run(
            Props::new().prop("foos", foos()),
            &partial(&["foos.bar"], &[]),
        )
        .await;
        assert_eq!(p, json!({"foos": []}), "non-indexed path does not match");
    }

    /// The same plain data as `Prop::value` JSON and as `Prop::array` of
    /// nested props/values.
    fn plain_rows() -> (Value, Prop) {
        let json = json!([{}, {"id": 1, "tags": {}}, {"id": 2, "name": "b"}, 3]);
        let array = Prop::array(vec![
            Prop::from(Props::new()),
            Prop::from(Props::new().prop("id", 1).prop("tags", Props::new())),
            Prop::from(json!({"id": 2, "name": "b"})),
            Prop::from(3),
        ]);
        (json, array)
    }

    #[tokio::test]
    async fn an_array_needing_no_transform_resolves_like_the_same_json_value() {
        let visits = [
            Visit::default(),
            partial(&["rows"], &[]),
            partial(&["rows.1.id"], &[]),
            partial(&["rows.0"], &[]),
            partial(&[], &["rows.1.tags"]),
        ];
        for visit in &visits {
            let (json, array) = plain_rows();
            let (from_value, mv) = run(Props::new().prop("rows", Prop::value(json)), visit).await;
            let (from_array, ma) = run(Props::new().prop("rows", array), visit).await;
            assert_eq!(from_array, from_value, "{visit:?}");
            assert_eq!(ma, mv, "{visit:?}");
        }
        let (json, array) = plain_rows();
        let (p, _) = run(Props::new().prop("rows", array), &Visit::default()).await;
        assert_eq!(p["rows"], json, "empty objects are kept");
    }

    #[tokio::test]
    async fn an_array_with_a_prop_wrapper_is_still_transformed() {
        let rows = || {
            Prop::array(vec![
                Prop::from(Props::new()),
                Prop::from(
                    Props::new()
                        .prop("id", 1)
                        .prop("x", optional(|| async { Ok(json!(1)) })),
                ),
            ])
        };
        let (p, _) = run(Props::new().prop("rows", rows()), &Visit::default()).await;
        assert_eq!(
            p,
            json!({"rows": [{"id": 1}]}),
            "Rails drops empty maps here"
        );
        let (p, _) = run(
            Props::new().prop("rows", rows()),
            &partial(&["rows.1.x"], &[]),
        )
        .await;
        assert_eq!(p, json!({"rows": [{"x": 1}]}));
    }

    #[tokio::test]
    async fn merge_prop_inside_array_uses_indexed_metadata() {
        let props = Props::new().prop(
            "foos",
            Prop::array(vec![Props::new()
                .prop("name", "First")
                .prop("posts", merge(|| async { Ok(json!([{"id": 1}])) }))]),
        );
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(
            p,
            json!({"foos": [{"name": "First", "posts": [{"id": 1}]}]})
        );
        assert_eq!(m["mergeProps"], json!(["foos.0.posts"]));
    }

    #[tokio::test]
    async fn closure_returning_array_with_prop_wrappers() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let props = Props::new().prop(
            "foos",
            lazy_prop(move || async move {
                Ok(vec![Props::new()
                    .prop("name", "First")
                    .prop(
                        "bar",
                        optional(move || async move {
                            c.fetch_add(1, Ordering::SeqCst);
                            Ok("expensive")
                        }),
                    )
                    .prop(
                        "notifications",
                        defer(|| async { Ok(json!(["msg"])) }),
                    )])
            }),
        );
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"foos": [{"name": "First"}]}));
        assert_eq!(
            m["deferredProps"],
            json!({"default": ["foos.0.notifications"]})
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dot_key_with_array_of_prop_wrappers() {
        let array = || {
            Prop::array(vec![Props::new()
                .prop("name", "First")
                .prop("bar", optional(|| async { Ok("expensive") }))])
        };
        let (p, _) = run(Props::new().prop("foos.items", array()), &Visit::default()).await;
        assert_eq!(p, json!({"foos": {"items": [{"name": "First"}]}}));
        let (p, _) = run(
            Props::new().prop("foos.items", array()),
            &partial(&["foos.items"], &[]),
        )
        .await;
        assert_eq!(p["foos"]["items"][0]["bar"], json!("expensive"));
    }

    #[tokio::test]
    async fn rescued_prop_inside_a_closure_tree_uses_its_dot_path() {
        let props = Props::new().prop(
            "auth",
            lazy_prop(|| async {
                Ok(Props::new().prop("user", "J").prop(
                    "bad",
                    lazy(|| async { Err::<i64, _>(loco_rs::Error::string("boom")) }).rescue(),
                ))
            }),
        );
        let (p, m) = run(props, &Visit::default()).await;
        assert_eq!(p, json!({"auth": {"user": "J"}}));
        assert_eq!(m["rescuedProps"], json!(["auth.bad"]));
    }
}
