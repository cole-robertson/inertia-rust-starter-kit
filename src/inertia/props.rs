//! Prop kinds and the `Props` builder (port of inertia-rails' BaseProp family).
//!
//! ```ignore
//! Props::new()
//!     .prop("user", json!({"name": "Ada"}))                      // plain value
//!     .prop("account", Prop::serialize(&account.to_props())?)    // a typed props struct
//!     .prop("stats", lazy(|| async { Ok(stats().await?) }))      // only evaluated when kept
//!     .prop("permissions", defer(|| async { .. }).group("sidebar"))
//!     .prop("posts", scroll(ScrollMetadata::new("page", None, Some(2), 1), || async { .. }))
//!     .prop("auth.user", json!(..))                             // dot notation nests
//! ```

use std::{
    future::Future,
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use loco_rs::Result;
use serde::Serialize;
use serde_json::Value;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A lazy prop: runs at most once, and only when the resolver keeps the prop.
/// It yields a [`Prop`], so a closure can return plain JSON, nested [`Props`],
/// an array of props, or another prop wrapper (see [`lazy_prop`]).
pub type LazyFn = Box<dyn FnOnce() -> BoxFuture<'static, Result<Prop>> + Send>;

pub const DEFAULT_GROUP: &str = "default";

pub(crate) enum Source {
    Value(Value),
    Lazy(LazyFn),
    Nested(Props),
    /// An array whose items may be prop wrappers or nested props.
    Array(Vec<Prop>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Plain,
    /// Always included, even when a partial reload does not ask for it.
    Always,
    /// Excluded on the first visit; only sent when a partial reload asks for it.
    Optional,
    /// Like `Optional`, but listed in `deferredProps` so the client fetches it.
    Defer {
        group: String,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MergeSpec {
    pub deep: bool,
    /// Root-level append (true) or prepend (false); ignored when paths are set.
    pub append: bool,
    pub appends_at: Vec<String>,
    pub prepends_at: Vec<String>,
    pub match_on: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OnceSpec {
    pub key: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub expires_at: Option<i64>,
    pub fresh: bool,
}

/// Pagination metadata for `scroll` props (`scrollProps` in the page).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollMetadata {
    pub page_name: String,
    pub previous_page: Option<Value>,
    pub next_page: Option<Value>,
    pub current_page: Value,
}

impl ScrollMetadata {
    #[must_use]
    pub fn new(
        page_name: impl Into<String>,
        previous_page: Option<i64>,
        next_page: Option<i64>,
        current_page: i64,
    ) -> Self {
        Self {
            page_name: page_name.into(),
            previous_page: previous_page.map(Value::from),
            next_page: next_page.map(Value::from),
            current_page: Value::from(current_page),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScrollSpec {
    pub metadata: ScrollMetadata,
    /// Key inside the value that holds the items (`data` for a paginated
    /// envelope); `None` merges at the prop root.
    pub wrapper: Option<String>,
}

/// Builds the shared props for a request (inertia-rails' `inertia_share`).
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

/// One prop: a value source plus its kind and modifiers.
pub struct Prop {
    pub(crate) source: Source,
    pub(crate) kind: Kind,
    pub(crate) merge: Option<MergeSpec>,
    pub(crate) once: Option<OnceSpec>,
    pub(crate) scroll: Option<ScrollSpec>,
    /// A failure is logged and reported in `rescuedProps` instead of failing the render.
    pub(crate) rescue: bool,
}

/// A value computed only when the prop is kept (a Ruby `-> { }` prop).
pub fn lazy<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    Prop::with_source(Source::Lazy(Box::new(move || {
        Box::pin(async move {
            let v = f().await?;
            Ok(Prop::value(serde_json::to_value(v)?))
        })
    })))
}

/// A lazy prop whose closure returns a prop tree rather than JSON: nested
/// [`Props`] (which may hold `defer`/`optional`/`once`/... wrappers), an array
/// of props, or a single prop wrapper. Like a Ruby `-> { }` returning
/// `InertiaRails.defer { .. }` or a hash of prop types: the wrapper's metadata
/// is collected and its inclusion rules apply at this prop's path.
pub fn lazy_prop<F, Fut, P>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<P>> + Send + 'static,
    P: Into<Prop>,
{
    Prop::with_source(Source::Lazy(Box::new(move || {
        Box::pin(async move { f().await.map(Into::into) })
    })))
}

/// `InertiaRails.always`: included even when a partial reload excludes it.
pub fn always<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).always()
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
    lazy(f).defer()
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

/// `InertiaRails.merge`: appended to the client's existing value on reload.
pub fn merge<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).merge()
}

/// `InertiaRails.deep_merge`.
pub fn deep_merge<F, Fut, V>(f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).deep_merge()
}

/// `InertiaRails.scroll`: an infinite-scroll prop. Appends (or prepends when
/// the client sends `X-Inertia-Infinite-Scroll-Merge-Intent: prepend`).
pub fn scroll<F, Fut, V>(metadata: ScrollMetadata, f: F) -> Prop
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<V>> + Send + 'static,
    V: Serialize,
{
    lazy(f).scroll(metadata)
}

impl Prop {
    fn with_source(source: Source) -> Self {
        Self {
            source,
            kind: Kind::Plain,
            merge: None,
            once: None,
            scroll: None,
            rescue: false,
        }
    }

    /// A plain, already-computed value.
    pub fn value(v: impl Into<Value>) -> Self {
        Self::with_source(Source::Value(v.into()))
    }

    /// A plain prop from any `Serialize` value, e.g. a typed props struct whose TypeScript
    /// type is generated (`src/page_types.rs`). Modifiers chain as usual:
    /// `Prop::serialize(&list)?.once_key(key)`.
    ///
    /// # Errors
    /// When serde can't turn the value into JSON (a map with non-string keys).
    pub fn serialize(v: &impl Serialize) -> Result<Self> {
        Ok(Self::value(serde_json::to_value(v)?))
    }

    /// An array whose items may be prop wrappers or nested props (a Ruby
    /// array holding `InertiaRails.optional { .. }` etc.). Hash items are
    /// resolved at `path.<index>`; see the resolver.
    pub fn array<I, P>(items: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Prop>,
    {
        Self::with_source(Source::Array(items.into_iter().map(Into::into).collect()))
    }

    #[must_use]
    pub fn always(mut self) -> Self {
        self.kind = Kind::Always;
        self
    }

    #[must_use]
    pub fn optional(mut self) -> Self {
        self.kind = Kind::Optional;
        self
    }

    #[must_use]
    pub fn defer(mut self) -> Self {
        self.kind = Kind::Defer {
            group: DEFAULT_GROUP.to_owned(),
        };
        self
    }

    /// Deferred group (implies `defer`).
    #[must_use]
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.kind = Kind::Defer {
            group: group.into(),
        };
        self
    }

    fn merge_spec(&mut self) -> &mut MergeSpec {
        self.merge.get_or_insert_with(|| MergeSpec {
            append: true,
            ..MergeSpec::default()
        })
    }

    /// Append at the root on reload.
    #[must_use]
    pub fn merge(mut self) -> Self {
        self.merge_spec();
        self
    }

    /// Prepend at the root on reload.
    #[must_use]
    pub fn prepend(mut self) -> Self {
        self.merge_spec().append = false;
        self
    }

    #[must_use]
    pub fn deep_merge(mut self) -> Self {
        self.merge_spec().deep = true;
        self
    }

    /// Append at a nested path (e.g. `data` in a paginated envelope).
    #[must_use]
    pub fn append_at(mut self, path: impl Into<String>) -> Self {
        self.merge_spec().appends_at.push(path.into());
        self
    }

    #[must_use]
    pub fn prepend_at(mut self, path: impl Into<String>) -> Self {
        self.merge_spec().prepends_at.push(path.into());
        self
    }

    /// Match merged items on this field (relative to the prop path). Implies merge.
    #[must_use]
    pub fn match_on(mut self, field: impl Into<String>) -> Self {
        self.merge_spec().match_on.push(field.into());
        self
    }

    #[must_use]
    pub fn once(mut self) -> Self {
        self.once.get_or_insert_with(OnceSpec::default);
        self
    }

    /// Cache key shared across pages (implies `once`).
    #[must_use]
    pub fn once_key(mut self, key: impl Into<String>) -> Self {
        self.once.get_or_insert_with(OnceSpec::default).key = Some(key.into());
        self
    }

    /// Absolute expiry in ms since the epoch (implies `once`).
    #[must_use]
    pub fn expires_at(mut self, epoch_ms: i64) -> Self {
        self.once.get_or_insert_with(OnceSpec::default).expires_at = Some(epoch_ms);
        self
    }

    /// Expiry relative to now (implies `once`).
    #[must_use]
    pub fn expires_in(self, duration: Duration) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let ms = i64::try_from((now + duration).as_millis()).unwrap_or(i64::MAX);
        self.expires_at(ms)
    }

    /// Resend even when the client says it has it cached (implies `once`).
    #[must_use]
    pub fn fresh(mut self) -> Self {
        self.once.get_or_insert_with(OnceSpec::default).fresh = true;
        self
    }

    /// Make this an infinite-scroll prop with the given pagination metadata.
    /// It merges at the root (`ScrollProp`'s `@merge = true`) until it is
    /// resolved, when the client's merge intent moves the merge to the
    /// wrapper key.
    #[must_use]
    pub fn scroll(mut self, metadata: ScrollMetadata) -> Self {
        self.merge_spec();
        self.scroll = Some(ScrollSpec {
            metadata,
            wrapper: None,
        });
        self
    }

    /// Scroll items live under this key of the value (e.g. `data`).
    /// No effect unless `scroll` was set first.
    #[must_use]
    pub fn wrapper(mut self, key: impl Into<String>) -> Self {
        if let Some(s) = self.scroll.as_mut() {
            s.wrapper = Some(key.into());
        }
        self
    }

    /// Log a failure and list the prop in `rescuedProps` instead of failing
    /// the whole render (inertia-rails' `rescue: true`).
    #[must_use]
    pub fn rescue(mut self) -> Self {
        self.rescue = true;
        self
    }

    pub(crate) fn is_modified(&self) -> bool {
        self.kind != Kind::Plain
            || self.merge.is_some()
            || self.once.is_some()
            || self.scroll.is_some()
            || self.rescue
    }
}

impl From<Value> for Prop {
    fn from(v: Value) -> Self {
        Self::value(v)
    }
}

impl From<Props> for Prop {
    fn from(p: Props) -> Self {
        Self::with_source(Source::Nested(p))
    }
}

impl From<Vec<Prop>> for Prop {
    fn from(items: Vec<Prop>) -> Self {
        Self::array(items)
    }
}

impl From<Vec<Props>> for Prop {
    fn from(items: Vec<Props>) -> Self {
        Self::array(items)
    }
}

impl From<&str> for Prop {
    fn from(v: &str) -> Self {
        Self::value(v)
    }
}

impl From<String> for Prop {
    fn from(v: String) -> Self {
        Self::value(v)
    }
}

impl From<bool> for Prop {
    fn from(v: bool) -> Self {
        Self::value(v)
    }
}

impl From<i64> for Prop {
    fn from(v: i64) -> Self {
        Self::value(v)
    }
}

/// An ordered set of props. Keys may use dot notation (`"auth.user"`).
#[derive(Default)]
pub struct Props {
    pub(crate) entries: Vec<(String, Prop)>,
}

impl Props {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) a prop.
    #[must_use]
    pub fn prop(mut self, key: impl Into<String>, prop: impl Into<Prop>) -> Self {
        self.insert(key, prop);
        self
    }

    /// Add (or replace) a prop in place.
    pub fn insert(&mut self, key: impl Into<String>, prop: impl Into<Prop>) {
        let key = key.into();
        let prop = prop.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = prop;
        } else {
            self.entries.push((key, prop));
        }
    }

    /// Every top-level key of a JSON object as a plain prop (non-objects give
    /// empty props).
    #[must_use]
    pub fn from_json(value: Value) -> Self {
        let mut props = Self::new();
        if let Value::Object(map) = value {
            for (k, v) in map {
                props.insert(k, v);
            }
        }
        props
    }

    /// Shallow merge: `other`'s keys replace ours (page props over shared ones).
    #[must_use]
    pub fn merged_with(mut self, other: Self) -> Self {
        for (k, v) in other.entries {
            self.insert(k, v);
        }
        self
    }

    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.entries.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut Prop> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, p)| p)
    }

    pub(crate) fn remove(&mut self, key: &str) -> Option<Prop> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(i).1)
    }

    /// Expand `"a.b"` keys into nested props (props_resolver.rb#expand_dot_notation):
    ///
    /// - a non-dotted key whose value and existing value are both plain
    ///   objects (JSON objects or nested `Props`) is shallow-merged, so
    ///   `"user.name"` then `"user": {..}` keeps both, as in Ruby;
    /// - walking a dotted key, a missing (or null/false) parent becomes an
    ///   empty map, a plain object parent is extended, and a plain lazy
    ///   parent (a Ruby `-> { }`) is evaluated now and extended;
    /// - any other parent (a scalar, an array, a prop wrapper) is an error,
    ///   where Ruby would raise on `[]=`.
    ///
    /// # Errors
    /// A lazy parent that fails, or a parent that cannot hold children.
    pub(crate) async fn expand_dot_notation(self) -> Result<Self> {
        let mut result = Self::new();
        for (key, prop) in self.entries {
            if key.contains('.') {
                let parts: Vec<&str> = key.split('.').collect();
                let (last, parents) = parts.split_last().expect("split yields one part");
                let mut current = &mut result;
                for part in parents {
                    current = descend(current, part, &key).await?;
                }
                current.insert(*last, prop);
                continue;
            }
            let existing_is_map = result.get_mut(&key).is_some_and(|p| p.is_plain_map());
            if existing_is_map && prop.is_plain_map() {
                let existing = descend(&mut result, &key, &key).await?;
                for (k, v) in prop.into_plain_map().entries {
                    existing.insert(k, v);
                }
            } else {
                result.insert(key, prop);
            }
        }
        Ok(result)
    }
}

impl Prop {
    /// An unmodified JSON object or nested `Props` (a Ruby `Hash`).
    fn is_plain_map(&self) -> bool {
        !self.is_modified()
            && matches!(
                self.source,
                Source::Nested(_) | Source::Value(Value::Object(_))
            )
    }

    /// The map of a [`Prop::is_plain_map`] prop.
    fn into_plain_map(self) -> Props {
        match self.source {
            Source::Nested(p) => p,
            Source::Value(obj @ Value::Object(_)) => Props::from_json(obj),
            _ => Props::new(),
        }
    }
}

/// The nested props under `key` (resolve_value in props_resolver.rb),
/// creating, converting or evaluating the parent as needed.
async fn descend<'a>(props: &'a mut Props, key: &str, full_key: &str) -> Result<&'a mut Props> {
    let parent = props.remove(key);
    let nested = match parent {
        None => Props::new(),
        Some(p) if p.is_plain_map() => p.into_plain_map(),
        Some(Prop {
            source: Source::Value(Value::Null | Value::Bool(false)),
            ..
        }) => Props::new(),
        Some(p) if !p.is_modified() && matches!(p.source, Source::Lazy(_)) => {
            let Source::Lazy(f) = p.source else {
                unreachable!("checked above")
            };
            let value = f().await?;
            match value.source {
                _ if value.is_plain_map() => value.into_plain_map(),
                Source::Value(Value::Null | Value::Bool(false)) if !value.is_modified() => {
                    Props::new()
                }
                _ => return Err(not_a_map(key, full_key)),
            }
        }
        Some(_) => return Err(not_a_map(key, full_key)),
    };
    props.insert(key, nested);
    match props.get_mut(key).map(|p| &mut p.source) {
        Some(Source::Nested(nested)) => Ok(nested),
        _ => unreachable!("nested props were just inserted"),
    }
}

fn not_a_map(key: &str, full_key: &str) -> loco_rs::Error {
    loco_rs::Error::string(&format!(
        "inertia: cannot nest `{full_key}` under `{key}`: the parent is not an object or a plain lazy prop"
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn nested<'a>(p: &'a Props, key: &str) -> &'a Props {
        match p
            .entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| &v.source)
        {
            Some(Source::Nested(n)) => n,
            _ => panic!("{key} should be nested"),
        }
    }

    #[test]
    fn insert_replaces_in_place_and_keeps_order() {
        let p = Props::new().prop("a", 1i64).prop("b", 2i64).prop("a", 3i64);
        assert_eq!(p.keys(), vec!["a", "b"]);
    }

    #[tokio::test]
    async fn dot_notation_nests_into_plain_objects() {
        let p = Props::new()
            .prop("auth", json!({"user": 1}))
            .prop("auth.session", json!(2))
            .prop("x.y.z", json!(3))
            .expand_dot_notation()
            .await
            .unwrap();
        assert_eq!(p.keys(), vec!["auth", "x"]);
        assert_eq!(nested(&p, "auth").keys(), vec!["user", "session"]);
        assert_eq!(nested(nested(&p, "x"), "y").keys(), vec!["z"]);
    }

    #[test]
    fn modifiers_imply_their_family() {
        let p = Prop::value(1).group("sidebar");
        assert_eq!(
            p.kind,
            Kind::Defer {
                group: "sidebar".into()
            }
        );
        assert_eq!(
            Prop::value(1).expires_at(5).once.unwrap().expires_at,
            Some(5)
        );
        assert!(Prop::value(1).match_on("id").merge.unwrap().append);
        assert!(!Prop::value(1).prepend().merge.unwrap().append);
    }
}
