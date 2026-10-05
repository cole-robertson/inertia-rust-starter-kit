//! Server-managed `<head>` tags: a port of inertia-rails' `MetaTag`,
//! `MetaTagBuilder`, the renderer's meta handling, and the
//! `inertia_meta_tags` helper.
//!
//! ```ignore
//! let meta = InertiaMeta::new()
//!     .title("Dashboard")
//!     .tag(MetaTag::new().attr("name", "description").attr("content", "…"));
//! inertia.meta(meta).render("dashboard/index", props).await
//! ```
//!
//! The tags go into the page props under `settings.meta_prop()`: as objects
//! (`_inertia_meta`, the default) or, with `server_head`, as HTML strings under
//! `head` for Inertia v3's `serverHead` client option. On client-rendered
//! HTML responses they are also emitted in the document `<head>`; SSR
//! responses carry them in the SSR head instead.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{document::script_safe_json, vite::escape_attr};

/// Void elements (Rails' tag helper `VOID_ELEMENTS` subset used by MetaTag).
pub const UNARY_TAGS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "meta", "source",
    "track", "wbr",
];
pub const LD_JSON_TYPE: &str = "application/ld+json";
pub const DEFAULT_SCRIPT_TYPE: &str = "text/plain";
const GENERATABLE_HEAD_KEY_PROPERTIES: [&str; 3] = ["name", "property", "http_equiv"];

/// Builds one tag, like `InertiaRails::MetaTag.new(**data)`. Attribute keys
/// are snake_case (`http_equiv`); they become `httpEquiv` in JSON and
/// `http-equiv` in HTML.
#[derive(Debug, Clone, Default)]
pub struct MetaTag {
    tag_name: Option<String>,
    head_key: Option<String>,
    allow_duplicates: bool,
    tag_type: Option<String>,
    data: BTreeMap<String, Value>,
    /// Insertion order of `data` keys (JSON/HTML attribute order).
    order: Vec<String>,
}

impl MetaTag {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `<title>` shorthand (`{ title: "…" }` in Ruby).
    #[must_use]
    pub fn title(title: impl Into<String>) -> Self {
        Self::new().tag_name("title").inner_content(title.into())
    }

    /// Element name (lowercased, as HTML treats it); `meta` when unset.
    #[must_use]
    pub fn tag_name(mut self, name: impl Into<String>) -> Self {
        self.tag_name = Some(name.into().to_ascii_lowercase());
        self
    }

    #[must_use]
    pub fn head_key(mut self, key: impl Into<String>) -> Self {
        self.head_key = Some(key.into());
        self
    }

    /// Keep several tags with the same name/property (suffixes the head key
    /// with a digest of the attributes).
    #[must_use]
    pub fn allow_duplicates(mut self) -> Self {
        self.allow_duplicates = true;
        self
    }

    /// The `type` attribute. For `script` tags anything but
    /// `application/ld+json` becomes `text/plain`, so no script executes.
    #[must_use]
    pub fn tag_type(mut self, t: impl Into<String>) -> Self {
        self.tag_type = Some(t.into());
        self
    }

    /// Any attribute (snake_case key). The structural keys Ruby takes as
    /// keyword arguments are not attributes: `type`, `tag_name`, `head_key`
    /// and `allow_duplicates` (in any case or camelCase spelling) go to
    /// [`Self::tag_type`], [`Self::tag_name`], [`Self::head_key`] and
    /// [`Self::allow_duplicates`], so each is serialized exactly once.
    #[must_use]
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        let key = key.into();
        let value = value.into();
        match Reserved::of(&key) {
            Some(Reserved::Type) => return self.tag_type(text(&value)),
            Some(Reserved::TagName) => return self.tag_name(text(&value)),
            Some(Reserved::HeadKey) => return self.head_key(text(&value)),
            Some(Reserved::AllowDuplicates) => {
                self.allow_duplicates = !is_blank(&value);
                return self;
            }
            Some(Reserved::InnerContent) => {
                return self.set("inner_content".to_owned(), value);
            }
            None => {}
        }
        self.set(key, value)
    }

    fn set(mut self, key: String, value: Value) -> Self {
        if !self.data.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.data.insert(key, value);
        self
    }

    /// Element content: escaped text, or JSON for an ld+json `script`.
    #[must_use]
    pub fn inner_content(self, content: impl Into<Value>) -> Self {
        self.attr("inner_content", content)
    }

    fn name(&self) -> &str {
        self.tag_name.as_deref().unwrap_or("meta")
    }

    /// The one place `type` is normalized; both serializers emit only this.
    fn resolved_type(&self) -> Option<String> {
        if self.name() == "script" {
            let ld_json = self
                .tag_type
                .as_deref()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case(LD_JSON_TYPE));
            Some(if ld_json {
                LD_JSON_TYPE.to_owned()
            } else {
                DEFAULT_SCRIPT_TYPE.to_owned()
            })
        } else {
            self.tag_type.clone()
        }
    }

    /// The tag's identity in the head (`title`, `meta-name-description`, …).
    #[must_use]
    pub fn resolved_head_key(&self) -> String {
        if self.name() == "title" {
            return "title".to_owned();
        }
        if let Some(key) = &self.head_key {
            return key.clone();
        }
        self.generate_meta_head_key()
            .unwrap_or_else(|| format!("{}-{}", self.name(), self.digest()))
    }

    fn generate_meta_head_key(&self) -> Option<String> {
        if self.name() != "meta" {
            return None;
        }
        if self.data.contains_key("charset") {
            return Some("meta-charset".to_owned());
        }
        GENERATABLE_HEAD_KEY_PROPERTIES.iter().find_map(|key| {
            let value = self.data.get(*key)?;
            let mut parts = vec![
                "meta".to_owned(),
                (*key).to_owned(),
                parameterize(&text(value)),
            ];
            if self.allow_duplicates {
                parts.push(self.digest());
            }
            Some(parts.join("-"))
        })
    }

    /// First 8 hex chars of SHA-256 over `k=v&…` sorted by key (tag_digest).
    fn digest(&self) -> String {
        let signature = self
            .data
            .iter()
            .map(|(k, v)| format!("{k}={}", text(v)))
            .collect::<Vec<_>>()
            .join("&");
        let hex = hex::encode(Sha256::digest(signature.as_bytes()));
        hex[..8].to_owned()
    }

    fn inner(&self) -> Option<&Value> {
        self.data.get("inner_content")
    }

    /// `MetaTag#as_json`: `tagName`, `headKey`, `type`, then camelCased
    /// attributes; blank values dropped.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("tagName".into(), Value::String(self.name().to_owned()));
        m.insert("headKey".into(), Value::String(self.resolved_head_key()));
        if let Some(t) = self.resolved_type() {
            m.insert("type".into(), Value::String(t));
        }
        for key in &self.order {
            let value = &self.data[key];
            if !is_blank(value) {
                m.insert(camelize_lower(key), value.clone());
            }
        }
        m.retain(|_, v| !is_blank(v));
        Value::Object(m)
    }

    /// `MetaTag#to_tag`: the HTML element, marked with `attribute="headKey"`.
    #[must_use]
    pub fn to_html(&self, attribute: &str) -> String {
        let name = self.name();
        let mut attrs = String::new();
        for key in &self.order {
            let html_name = key.replace('_', "-");
            if key == "inner_content" || html_name.eq_ignore_ascii_case(attribute) {
                continue;
            }
            push_attr(&mut attrs, &html_name, &self.data[key]);
        }
        if let Some(t) = self.resolved_type() {
            push_attr(&mut attrs, "type", &Value::String(t));
        }
        push_attr(
            &mut attrs,
            attribute,
            &Value::String(self.resolved_head_key()),
        );
        if UNARY_TAGS.contains(&name) {
            return format!("<{name}{attrs}>");
        }
        let inner = match (name, self.inner()) {
            (_, None | Some(Value::Null)) => String::new(),
            ("script", Some(v @ (Value::Object(_) | Value::Array(_)))) => {
                script_safe_json(&v.to_string())
            }
            (_, Some(v)) => escape_attr(&text(v)),
        };
        format!("<{name}{attrs}>{inner}</{name}>")
    }
}

/// Keys with a structural meaning (Ruby's `MetaTag.new` keyword arguments,
/// plus `inner_content`), never serialized as generic attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reserved {
    Type,
    TagName,
    HeadKey,
    AllowDuplicates,
    InnerContent,
}

impl Reserved {
    /// `key` in snake_case, camelCase or any letter case.
    fn of(key: &str) -> Option<Self> {
        let folded: String = key
            .chars()
            .filter(|c| !matches!(c, '_' | '-'))
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match folded.as_str() {
            "type" => Some(Self::Type),
            "tagname" => Some(Self::TagName),
            "headkey" => Some(Self::HeadKey),
            "allowduplicates" => Some(Self::AllowDuplicates),
            "innercontent" => Some(Self::InnerContent),
            _ => None,
        }
    }
}

fn push_attr(out: &mut String, name: &str, value: &Value) {
    match value {
        Value::Null | Value::Bool(false) => {}
        Value::Bool(true) => {
            out.push(' ');
            out.push_str(name);
            out.push_str(&format!("=\"{name}\""));
        }
        v => {
            out.push_str(&format!(" {name}=\"{}\"", escape_attr(&text(v))));
        }
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Number(_) | Value::Bool(true) => false,
    }
}

/// ActiveSupport `camelize(:lower)` for snake_case keys.
fn camelize_lower(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// ActiveSupport `parameterize`: lowercase, runs of non-alphanumerics become
/// one `-`, trimmed.
fn parameterize(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    out
}

/// A request's head tags, keyed by head key (`MetaTagBuilder`). Adding a tag
/// with an existing head key replaces it.
#[derive(Debug, Clone, Default)]
pub struct InertiaMeta {
    tags: Vec<(String, MetaTag)>,
}

impl InertiaMeta {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces, by head key) a tag.
    #[must_use]
    pub fn tag(mut self, tag: MetaTag) -> Self {
        self.add(tag);
        self
    }

    /// `<title>` shorthand.
    #[must_use]
    pub fn title(self, title: impl Into<String>) -> Self {
        self.tag(MetaTag::title(title))
    }

    pub fn add(&mut self, tag: MetaTag) {
        let key = tag.resolved_head_key();
        if let Some(slot) = self.tags.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = tag;
        } else {
            self.tags.push((key, tag));
        }
    }

    /// Removes the tag with `head_key`.
    pub fn remove(&mut self, head_key: &str) {
        self.tags.retain(|(k, _)| k != head_key);
    }

    /// Removes the tags matching `f`.
    pub fn remove_if(&mut self, f: impl Fn(&MetaTag) -> bool) {
        self.tags.retain(|(_, t)| !f(t));
    }

    pub fn clear(&mut self) {
        self.tags.clear();
    }

    /// Merges `other` over `self` (later tags win per head key).
    #[must_use]
    pub fn merged_with(mut self, other: Self) -> Self {
        for (_, tag) in other.tags {
            self.add(tag);
        }
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// The current `<title>` text, if any.
    #[must_use]
    pub fn current_title(&self) -> Option<String> {
        self.tags
            .iter()
            .find(|(k, _)| k == "title")
            .and_then(|(_, t)| t.inner())
            .map(text)
    }

    pub fn tags(&self) -> impl Iterator<Item = &MetaTag> {
        self.tags.iter().map(|(_, t)| t)
    }

    /// Applies a title template (`meta_title_template`) to the current title;
    /// a blank result leaves the title alone.
    pub fn apply_title_template(&mut self, template: &TitleTemplate) {
        let title = template(self.current_title());
        if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
            self.add(MetaTag::title(title));
        }
    }

    /// The prop value: tag objects, or HTML strings with `server_head`.
    #[must_use]
    pub fn serialize(&self, server_head: bool, attribute: &str) -> Value {
        Value::Array(
            self.tags()
                .map(|t| {
                    if server_head {
                        Value::String(t.to_html(attribute))
                    } else {
                        t.to_json()
                    }
                })
                .collect(),
        )
    }
}

/// `meta_title_template`: receives the page's title (if any), returns the
/// final one (or `None` to leave it). Store it in `ctx.shared_store` as
/// [`MetaTitleTemplate`].
pub type TitleTemplate = dyn Fn(Option<String>) -> Option<String> + Send + Sync;

/// `ctx.shared_store` entry holding the app's title template.
#[derive(Clone)]
pub struct MetaTitleTemplate(pub std::sync::Arc<TitleTemplate>);

/// `inertia_meta_tags`: the head HTML for tags already in the page props
/// (objects or server_head strings), joined by newlines.
#[must_use]
pub fn head_html(value: Option<&Value>, attribute: &str) -> String {
    head_html_missing_from(value, attribute, &[])
}

/// [`head_html`] for an SSR response: only the tags `ssr_head` lacks.
///
/// Inertia's `serverHead` client option makes SSR return the tags itself,
/// marked `data-inertia="<head key>"`; those are skipped so nothing repeats.
/// Anything SSR did not render (object mode, which the React adapter does
/// not consume, or a client built without `serverHead`) is still written,
/// except a `<title>` when SSR already produced one.
#[must_use]
pub fn head_html_missing_from(
    value: Option<&Value>,
    attribute: &str,
    ssr_head: &[String],
) -> String {
    let Some(Value::Array(items)) = value else {
        return String::new();
    };
    let ssr_has_title = ssr_head
        .iter()
        .any(|h| h.trim_start().starts_with("<title"));
    let in_ssr = |key: &str| {
        let marker = format!(" {attribute}=\"{}\"", escape_attr(key));
        ssr_head.iter().any(|h| h.contains(&marker))
    };
    items
        .iter()
        .filter_map(|item| {
            let (html, key) = match item {
                Value::String(html) => (html.clone(), marker_value(html, attribute)),
                Value::Object(obj) => {
                    let tag = from_json(obj);
                    (tag.to_html(attribute), Some(tag.resolved_head_key()))
                }
                _ => return None,
            };
            let skip = key.is_some_and(|k| in_ssr(&k) || (k == "title" && ssr_has_title));
            (!skip).then_some(html)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The unescaped value of ` attribute="…"` in a tag produced by
/// [`MetaTag::to_html`] (the marker is its last attribute).
fn marker_value(html: &str, attribute: &str) -> Option<String> {
    let start = html.rfind(&format!(" {attribute}=\""))? + attribute.len() + 3;
    let len = html[start..].find('"')?;
    Some(
        html[start..start + len]
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

/// Rebuilds a tag from its `to_json` form (keys back to snake_case).
fn from_json(obj: &Map<String, Value>) -> MetaTag {
    let mut tag = MetaTag::new();
    for (k, v) in obj {
        match k.as_str() {
            "tagName" => tag = tag.tag_name(text(v)),
            "headKey" => tag = tag.head_key(text(v)),
            "type" => tag = tag.tag_type(text(v)),
            _ => tag = tag.attr(snake(k), v.clone()),
        }
    }
    tag
}

fn snake(key: &str) -> String {
    let mut out = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn description() -> MetaTag {
        MetaTag::new()
            .head_key("meta-12345678")
            .attr("name", "description")
            .attr("content", "Inertia rules")
    }

    // Ported from spec/inertia/meta_tag_spec.rb.

    #[test]
    fn json_shape() {
        assert_eq!(
            description().to_json().to_string(),
            r#"{"tagName":"meta","headKey":"meta-12345678","name":"description","content":"Inertia rules"}"#
        );
        let t = MetaTag::new()
            .head_key("k")
            .attr("http_equiv", "content-security-policy")
            .attr("content", "default-src 'self'");
        assert_eq!(t.to_json()["httpEquiv"], json!("content-security-policy"));
        let t = MetaTag::new()
            .tag_name("script")
            .head_key("k")
            .tag_type("application/javascript")
            .inner_content("<script>alert(\"XSS\")</script>");
        assert_eq!(
            t.to_json().to_string(),
            r#"{"tagName":"script","headKey":"k","type":"text/plain","innerContent":"<script>alert(\"XSS\")</script>"}"#
        );
    }

    #[test]
    fn generated_head_keys() {
        let t = MetaTag::new()
            .attr("some_name", "description")
            .attr("content", "Inertia rules");
        let expected = format!(
            "meta-{}",
            &hex::encode(Sha256::digest(
                b"content=Inertia rules&some_name=description"
            ))[..8]
        );
        assert_eq!(t.resolved_head_key(), expected);
        let swapped = MetaTag::new()
            .attr("content", "Inertia rules")
            .attr("some_name", "description");
        assert_eq!(swapped.resolved_head_key(), expected, "order independent");

        let named = || {
            MetaTag::new()
                .attr("name", "description")
                .attr("content", "Inertia rules")
        };
        assert_eq!(named().resolved_head_key(), "meta-name-description");
        assert_eq!(
            MetaTag::new()
                .attr("http_equiv", "content-security-policy")
                .resolved_head_key(),
            "meta-http_equiv-content-security-policy"
        );
        assert_eq!(
            MetaTag::new()
                .attr("property", "og:title")
                .resolved_head_key(),
            "meta-property-og-title"
        );
        let hash = &hex::encode(Sha256::digest(b"content=Inertia rules&name=description"))[..8];
        assert_eq!(
            named().allow_duplicates().resolved_head_key(),
            format!("meta-name-description-{hash}")
        );
        assert_eq!(
            MetaTag::new().attr("charset", "utf-8").resolved_head_key(),
            "meta-charset"
        );
    }

    #[test]
    fn html_rendering() {
        assert_eq!(
            description().to_html("inertia"),
            r#"<meta name="description" content="Inertia rules" inertia="meta-12345678">"#
        );
        assert_eq!(
            description().to_html("data-inertia"),
            r#"<meta name="description" content="Inertia rules" data-inertia="meta-12345678">"#
        );
        let ld = MetaTag::new()
            .tag_name("script")
            .head_key("meta-12345678")
            .tag_type(LD_JSON_TYPE)
            .inner_content(json!({"@context": "https://schema.org"}));
        assert_eq!(
            ld.to_html("inertia"),
            r#"<script type="application/ld+json" inertia="meta-12345678">{"@context":"https://schema.org"}</script>"#
        );
        let js = MetaTag::new()
            .tag_name("script")
            .head_key("meta-12345678")
            .tag_type("application/javascript")
            .inner_content("alert(\"XSS\")");
        assert_eq!(
            js.to_html("inertia"),
            r#"<script type="text/plain" inertia="meta-12345678">alert(&quot;XSS&quot;)</script>"#
        );
        let div = MetaTag::new()
            .tag_name("div")
            .head_key("meta-12345678")
            .inner_content("<script>alert(\"XSS\")</script>");
        assert_eq!(
            div.to_html("inertia"),
            r#"<div inertia="meta-12345678">&lt;script&gt;alert(&quot;XSS&quot;)&lt;/script&gt;</div>"#
        );
        for name in UNARY_TAGS {
            let t = MetaTag::new()
                .tag_name(name)
                .head_key("meta-12345678")
                .attr("content", "Inertia rules");
            assert_eq!(
                t.to_html("inertia"),
                format!(r#"<{name} content="Inertia rules" inertia="meta-12345678">"#)
            );
        }
        let title = MetaTag::title("Inertia Is Great").head_key("ignored");
        assert_eq!(
            title.to_json().to_string(),
            r#"{"tagName":"title","headKey":"title","innerContent":"Inertia Is Great"}"#
        );
        assert_eq!(
            title.to_html("inertia"),
            r#"<title inertia="title">Inertia Is Great</title>"#
        );
    }

    #[test]
    fn script_type_is_normalized_once_however_it_is_given() {
        let expected_html = r#"<script type="text/plain" inertia="k">alert(1)</script>"#;
        let expected_json = json!({"tagName": "script", "headKey": "k",
                                   "type": "text/plain", "innerContent": "alert(1)"});
        let variants = [
            MetaTag::new()
                .tag_name("script")
                .tag_type("application/javascript"),
            MetaTag::new()
                .tag_name("script")
                .attr("type", "application/javascript"),
            MetaTag::new().tag_name("script").attr("TYPE", "module"),
            MetaTag::new()
                .tag_name("SCRIPT")
                .attr("Type", "text/javascript"),
            MetaTag::new()
                .attr("tag_name", "script")
                .attr("type", "module"),
            MetaTag::new()
                .attr("tagName", "script")
                .attr("type", "module"),
            // `.attr("type")` and `.tag_type` set the same field: last wins.
            MetaTag::new()
                .tag_name("script")
                .tag_type(LD_JSON_TYPE)
                .attr("type", "module"),
        ];
        for tag in variants {
            let tag = tag.head_key("k").inner_content("alert(1)");
            let html = tag.to_html("inertia");
            assert_eq!(html, expected_html, "{tag:?}");
            assert_eq!(html.matches("type=").count(), 1);
            assert_eq!(tag.to_json(), expected_json, "{tag:?}");
        }

        for tag in [
            MetaTag::new().tag_name("script").attr("type", LD_JSON_TYPE),
            MetaTag::new()
                .tag_name("script")
                .tag_type(" Application/LD+JSON "),
        ] {
            let tag = tag.head_key("k").inner_content(json!({"a": 1}));
            assert_eq!(
                tag.to_html("inertia"),
                r#"<script type="application/ld+json" inertia="k">{"a":1}</script>"#
            );
            assert_eq!(tag.to_json()["type"], json!(LD_JSON_TYPE));
        }

        // Non-script tags keep their type, still exactly once.
        let link = MetaTag::new()
            .tag_name("link")
            .head_key("k")
            .attr("type", "image/png")
            .attr("rel", "icon");
        assert_eq!(
            link.to_html("inertia"),
            r#"<link rel="icon" type="image/png" inertia="k">"#
        );
        assert_eq!(
            link.to_json(),
            json!({"tagName": "link", "headKey": "k", "type": "image/png", "rel": "icon"})
        );
    }

    #[test]
    fn structural_keys_are_not_generic_attributes() {
        let tag = MetaTag::new()
            .attr("name", "description")
            .attr("head_key", "custom")
            .attr("headKey", "custom")
            .attr("inertia", "spoofed")
            .attr("innerContent", "ignored for meta");
        assert_eq!(tag.resolved_head_key(), "custom");
        let html = tag.to_html("inertia");
        assert_eq!(html, r#"<meta name="description" inertia="custom">"#);
        let json = tag.to_json();
        assert_eq!(json["headKey"], json!("custom"));
        assert_eq!(json["tagName"], json!("meta"));
        assert!(json.get("head_key").is_none());

        let dup = MetaTag::new()
            .attr("name", "a")
            .attr("allow_duplicates", true);
        assert!(dup.resolved_head_key().starts_with("meta-name-a-"));
        assert!(!dup.to_html("inertia").contains("allow"));
        assert!(dup.to_json().get("allowDuplicates").is_none());
    }

    #[test]
    fn ld_json_cannot_close_its_script() {
        let ld = MetaTag::new()
            .tag_name("script")
            .tag_type(LD_JSON_TYPE)
            .inner_content(json!({"x": "</script><script>alert(1)</script>"}));
        assert!(!ld.to_html("inertia").contains("</script><script>"));
    }

    #[test]
    fn builder_replaces_by_head_key_and_applies_the_title_template() {
        let mut meta = InertiaMeta::new()
            .title("One")
            .tag(
                MetaTag::new()
                    .attr("name", "description")
                    .attr("content", "a"),
            )
            .tag(
                MetaTag::new()
                    .attr("name", "description")
                    .attr("content", "b"),
            );
        assert_eq!(meta.tags().count(), 2);
        assert_eq!(meta.current_title().as_deref(), Some("One"));
        let template: Box<TitleTemplate> =
            Box::new(|t| Some(format!("{} | Kit", t.unwrap_or_default())));
        meta.apply_title_template(&template);
        assert_eq!(meta.current_title().as_deref(), Some("One | Kit"));
        let blank: Box<TitleTemplate> = Box::new(|_| Some(String::new()));
        meta.apply_title_template(&blank);
        assert_eq!(meta.current_title().as_deref(), Some("One | Kit"));
        meta.remove("title");
        assert_eq!(meta.current_title(), None);
        assert_eq!(
            meta.serialize(false, "inertia"),
            json!([{"tagName": "meta", "headKey": "meta-name-description",
                    "name": "description", "content": "b"}])
        );
    }

    #[test]
    fn ssr_head_suppresses_only_the_tags_it_rendered() {
        let meta = InertiaMeta::new().title("T").tag(
            MetaTag::new()
                .attr("name", "description")
                .attr("content", "d"),
        );
        let strings = meta.serialize(true, "data-inertia");
        let description =
            r#"<meta name="description" content="d" data-inertia="meta-name-description">"#;

        // serverHead on the client: SSR returned both tags, nothing repeats.
        let ssr = vec![
            r#"<title data-inertia="">T - App</title>"#.to_owned(),
            description.to_owned(),
        ];
        assert_eq!(
            head_html_missing_from(Some(&strings), "data-inertia", &ssr),
            ""
        );

        // A client built without serverHead: SSR rendered only the page's own
        // title, so the description is still written (and the title is not doubled).
        let ssr = vec![r#"<title data-inertia="">Page</title>"#.to_owned()];
        assert_eq!(
            head_html_missing_from(Some(&strings), "data-inertia", &ssr),
            description
        );
        // Object mode is never rendered by the React adapter.
        let objects = meta.serialize(false, "inertia");
        assert_eq!(
            head_html_missing_from(Some(&objects), "inertia", &ssr),
            r#"<meta name="description" content="d" inertia="meta-name-description">"#
        );
        // SSR without any title keeps the server title.
        assert_eq!(
            head_html_missing_from(Some(&objects), "inertia", &[]),
            head_html(Some(&objects), "inertia")
        );
    }

    #[test]
    fn head_html_accepts_objects_and_strings() {
        let meta = InertiaMeta::new().title("T");
        let objects = meta.serialize(false, "inertia");
        assert_eq!(
            head_html(Some(&objects), "inertia"),
            r#"<title inertia="title">T</title>"#
        );
        let strings = meta.serialize(true, "data-inertia");
        assert_eq!(
            head_html(Some(&strings), "ignored"),
            r#"<title data-inertia="title">T</title>"#
        );
    }
}
