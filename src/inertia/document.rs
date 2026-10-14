//! The HTML root document (mirrors the Rails kit's layouts/application.html.erb): inertia-omega's
//! root view.

use std::sync::Arc;

use super::{
    config::Settings,
    meta,
    vite::{self, escape_attr, nonce_attr},
};

/// Root element id (`<div id="app">` / `<script data-page="app">`).
pub const ROOT_ID: &str = "app";

/// The view data key the request's CSP nonce travels under (`Inertia::render` sets it).
pub const NONCE: &str = "nonce";

/// Escapes JSON for an HTML `<script>` body: no `</script>`, `<!--`, or
/// line separators that older parsers treat as newlines.
#[must_use]
pub fn script_safe_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// `<script data-page="app" type="application/json">` + `<div id="app">`
/// (inertia-rails' `inertia_root` with `use_script_element_for_initial_page`).
#[must_use]
pub fn root(page: &omega::Page, nonce: Option<&str>) -> String {
    let json = serde_json::to_string(page).expect("a page serializes");
    format!(
        "<script data-page=\"{ROOT_ID}\" type=\"application/json\"{}>{}</script>\n<div id=\"{ROOT_ID}\"></div>",
        nonce_attr(nonce),
        script_safe_json(&json)
    )
}

/// The document for omega's `view` of a first visit: client-rendered with the kit's own page
/// script (it carries the CSP nonce), or with the SSR server's head and body.
#[must_use]
pub fn render(settings: &Arc<Settings>, view: &omega::View<'_>) -> String {
    let nonce = view.data.get(NONCE).and_then(serde_json::Value::as_str);
    let ssr_head: Vec<String> = if view.ssr {
        view.head.lines().map(str::to_owned).collect()
    } else {
        Vec::new()
    };
    // inertia_meta_tags: with `serverHead` on the client, SSR output
    // carries the tags in its own head; write only the ones it lacks.
    let meta_tags = meta::head_html_missing_from(
        view.page.props.get(settings.meta_prop()),
        settings.head_attribute(),
        &ssr_head,
    );
    let body = if view.ssr {
        view.body.to_owned()
    } else {
        root(view.page, nonce)
    };
    Document {
        app_name: &settings.app_name,
        vite_tags: &vite::shared(&settings.vite).tags(nonce),
        meta_tags: &meta_tags,
        nonce,
        ssr_head: &ssr_head.join("\n"),
        body: &body,
    }
    .render()
}

/// Everything the layout needs.
pub struct Document<'a> {
    pub app_name: &'a str,
    pub vite_tags: &'a str,
    /// Server-managed head tags (`inertia_meta_tags`); with SSR, only those its head lacks.
    pub meta_tags: &'a str,
    pub nonce: Option<&'a str>,
    /// The SSR server's head tags, newline-separated; empty without SSR.
    pub ssr_head: &'a str,
    /// The page script and root element, or the SSR server's body (which carries both).
    pub body: &'a str,
}

// The Rails layout's PWA manifest link is an ERB comment, so it never reaches the browser.
// For an installable app, add `public/manifest.json` and a
// `<link rel="manifest" href="/manifest.json">` below the meta tags.

const DARK_MODE_SCRIPT: &str = r#"
      document.documentElement.classList.toggle(
        "dark",
        localStorage.appearance === "dark" ||
          (!("appearance" in localStorage) && window.matchMedia("(prefers-color-scheme: dark)").matches),
      );
    "#;

impl Document<'_> {
    /// Renders the full HTML page. With SSR, its head is inserted into `<head>` and its body
    /// (which already carries the page script and root div) goes into `<body>` as-is.
    #[must_use]
    pub fn render(&self) -> String {
        let nonce = nonce_attr(self.nonce);
        let app_name = escape_attr(self.app_name);
        // SSR head (or a server-managed title tag) carries its own <title>.
        let title = if self.ssr_head.contains("<title") || self.meta_tags.contains("<title") {
            String::new()
        } else {
            format!("<title data-inertia>{app_name}</title>\n    ")
        };
        format!(
            r#"<!DOCTYPE html>
<html>
  <head>
    {title}<meta name="viewport" content="width=device-width,initial-scale=1">
    <meta name="apple-mobile-web-app-capable" content="yes">
    <meta name="application-name" content="{app_name}">
    <meta name="mobile-web-app-capable" content="yes">

    <link rel="icon" href="/icon.png" type="image/png">
    <link rel="icon" href="/icon.svg" type="image/svg+xml">
    <link rel="apple-touch-icon" href="/icon.png">

    <script{nonce}>{DARK_MODE_SCRIPT}</script>

    {vite}
    {meta_tags}
    {ssr_head}
  </head>

  <body>
    {body}
  </body>
</html>
"#,
            vite = self.vite_tags,
            meta_tags = self.meta_tags,
            ssr_head = self.ssr_head,
            body = self.body,
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn page(props: serde_json::Value) -> omega::Page {
        let serde_json::Value::Object(props) = props else {
            panic!()
        };
        omega::Page {
            component: "Home".into(),
            props,
            url: "/".into(),
            version: "v".into(),
            ..omega::Page::default()
        }
    }

    #[test]
    fn script_json_cannot_break_out() {
        let p = page(json!({"x": "</script><script>alert(1)</script>&\u{2028}\u{2029}"}));
        let html = root(&p, Some("abc"));
        assert!(!html.contains("</script><script>"));
        assert!(html.contains("\\u003c/script\\u003e"));
        assert!(html.contains("\\u0026\\u2028\\u2029"));
        assert!(html.starts_with(r#"<script data-page="app" type="application/json" nonce="abc">"#));
        // Still valid JSON that decodes to the original.
        let start = html.find('>').unwrap() + 1;
        let end = html.find("</script>").unwrap();
        let v: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(v["props"]["x"], p.props["x"]);
    }

    #[test]
    fn ssr_body_is_inserted_as_is_and_head_replaces_title() {
        let doc = Document {
            app_name: "Kit",
            vite_tags: "<!--vite-->",
            meta_tags: "",
            nonce: None,
            ssr_head: "<title data-inertia>SSR</title>",
            body: "<script data-page=\"app\">{}</script><div id=\"app\">hi</div>",
        }
        .render();
        assert_eq!(doc.matches("data-page=").count(), 1);
        assert!(doc.contains("<div id=\"app\">hi</div>"));
        assert_eq!(doc.matches("<title").count(), 1);
    }

    #[test]
    fn csr_document_has_title_theme_script_and_root() {
        let root = root(&page(json!({})), Some("n"));
        let doc = Document {
            app_name: "Kit <&>",
            vite_tags: "",
            meta_tags: "",
            nonce: Some("n"),
            ssr_head: "",
            body: &root,
        }
        .render();
        assert!(doc.contains("<title data-inertia>Kit &lt;&amp;&gt;</title>"));
        assert!(doc.contains("<script nonce=\"n\">"));
        assert!(doc.contains("prefers-color-scheme: dark"));
        assert!(doc.contains("<div id=\"app\"></div>"));
        assert!(doc.contains("/icon.svg"));
    }

    #[test]
    fn server_managed_title_replaces_the_default_one() {
        let doc = Document {
            app_name: "Kit",
            vite_tags: "",
            meta_tags:
                "<title inertia=\"title\">Page</title>\n<meta name=\"d\" inertia=\"meta-name-d\">",
            nonce: None,
            ssr_head: "",
            body: "",
        }
        .render();
        assert_eq!(doc.matches("<title").count(), 1);
        assert!(doc.contains("<title inertia=\"title\">Page</title>"));
        assert!(doc.contains("<meta name=\"d\" inertia=\"meta-name-d\">"));
    }
}
