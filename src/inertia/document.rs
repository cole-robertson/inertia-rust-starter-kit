//! The HTML root document (mirrors the Rails kit's layouts/application.html.erb).

use super::{
    page::Page,
    ssr::SsrOutput,
    vite::{escape_attr, nonce_attr},
};

/// Root element id (`<div id="app">` / `<script data-page="app">`).
pub const ROOT_ID: &str = "app";

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
pub fn root(page: &Page, nonce: Option<&str>) -> String {
    format!(
        "<script data-page=\"{ROOT_ID}\" type=\"application/json\"{}>{}</script>\n<div id=\"{ROOT_ID}\"></div>",
        nonce_attr(nonce),
        script_safe_json(&page.to_json())
    )
}

/// Everything the layout needs.
pub struct Document<'a> {
    pub app_name: &'a str,
    pub page: &'a Page,
    pub vite_tags: &'a str,
    /// Server-managed head tags (`inertia_meta_tags`); empty with SSR.
    pub meta_tags: &'a str,
    pub nonce: Option<&'a str>,
    pub ssr: Option<SsrOutput>,
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
    /// Renders the full HTML page. With SSR output, its head is inserted into
    /// `<head>` and its body (which already carries the page script and root
    /// div) goes into `<body>` as-is; otherwise the client-side root is emitted.
    #[must_use]
    pub fn render(&self) -> String {
        let nonce = nonce_attr(self.nonce);
        let app_name = escape_attr(self.app_name);
        let (ssr_head, body) = match &self.ssr {
            Some(ssr) => (ssr.head.join("\n"), ssr.body.clone()),
            None => (String::new(), root(self.page, self.nonce)),
        };
        // SSR head (or a server-managed title tag) carries its own <title>.
        let title = if ssr_head.contains("<title") || self.meta_tags.contains("<title") {
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
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map};

    use super::*;
    use crate::inertia::resolver::Metadata;

    fn page(props: serde_json::Value) -> Page {
        let serde_json::Value::Object(props) = props else {
            panic!()
        };
        Page {
            component: "Home".into(),
            props,
            url: "/".into(),
            version: "v".into(),
            encrypt_history: false,
            clear_history: false,
            flash: None,
            shared_props: None,
            preserve_fragment: false,
            metadata: Metadata::default(),
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
        let p = page(json!({}));
        let doc = Document {
            app_name: "Kit",
            page: &p,
            vite_tags: "<!--vite-->",
            meta_tags: "",
            nonce: None,
            ssr: Some(SsrOutput {
                head: vec!["<title data-inertia>SSR</title>".into()],
                body: "<script data-page=\"app\">{}</script><div id=\"app\">hi</div>".into(),
            }),
        }
        .render();
        assert_eq!(doc.matches("data-page=").count(), 1);
        assert!(doc.contains("<div id=\"app\">hi</div>"));
        assert_eq!(doc.matches("<title").count(), 1);
        let _ = Map::<String, serde_json::Value>::new();
    }

    #[test]
    fn csr_document_has_title_theme_script_and_root() {
        let p = page(json!({}));
        let doc = Document {
            app_name: "Kit <&>",
            page: &p,
            vite_tags: "",
            meta_tags: "",
            nonce: Some("n"),
            ssr: None,
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
        let p = page(json!({}));
        let doc = Document {
            app_name: "Kit",
            page: &p,
            vite_tags: "",
            meta_tags:
                "<title inertia=\"title\">Page</title>\n<meta name=\"d\" inertia=\"meta-name-d\">",
            nonce: None,
            ssr: None,
        }
        .render();
        assert_eq!(doc.matches("<title").count(), 1);
        assert!(doc.contains("<title inertia=\"title\">Page</title>"));
        assert!(doc.contains("<meta name=\"d\" inertia=\"meta-name-d\">"));
    }
}
