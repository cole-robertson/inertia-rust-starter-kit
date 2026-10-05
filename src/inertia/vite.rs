//! Vite asset tags and the Inertia asset version.
//!
//! Dev server: `@vite/client`, the React Refresh preamble and the entry modules
//! straight from the dev server. Built: tags from `manifest.json`, read once.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::config::ViteSettings;
use loco_rs::{Error, Result};

pub const ENTRYPOINT: &str = "frontend/entrypoints/inertia.tsx";
pub const STYLESHEET: &str = "frontend/entrypoints/application.css";
/// Built assets are served under this prefix (vite.config.ts `base`).
pub const BASE: &str = "/vite/";

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestEntry {
    pub file: String,
    #[serde(default)]
    pub css: Vec<String>,
    #[serde(default)]
    pub imports: Vec<String>,
}

pub type Manifest = HashMap<String, ManifestEntry>;

/// Tags and version, computed once at boot.
#[derive(Debug, Clone)]
pub struct Vite {
    dev_server_url: Option<String>,
    manifest: Option<Manifest>,
    version: String,
}

impl Vite {
    /// Loads the manifest (unless the dev server is on).
    ///
    /// # Errors
    /// When `require_manifest` (production) and the manifest is missing or
    /// unreadable. Otherwise a missing manifest only drops the tags (logged).
    #[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
    pub fn load(settings: &ViteSettings, require_manifest: bool) -> Result<Self> {
        if settings.dev_server {
            return Ok(Self::dev(&settings.dev_server_url));
        }
        match std::fs::read(&settings.manifest_path) {
            Ok(bytes) => Self::from_manifest_bytes(&bytes).map_err(|e| {
                Error::string(&format!(
                    "vite manifest {} is invalid: {e}",
                    settings.manifest_path
                ))
            }),
            Err(e) if require_manifest => Err(Error::string(&format!(
                "vite manifest {} is missing ({e}); run `npx vite build`",
                settings.manifest_path
            ))),
            Err(e) => {
                tracing::warn!(path = %settings.manifest_path, error = %e,
                    "vite manifest missing; pages render without asset tags");
                Ok(Self {
                    dev_server_url: None,
                    manifest: None,
                    version: "missing-manifest".into(),
                })
            }
        }
    }

    #[must_use]
    pub fn dev(dev_server_url: &str) -> Self {
        Self {
            dev_server_url: Some(dev_server_url.trim_end_matches('/').to_owned()),
            manifest: None,
            version: "dev".into(),
        }
    }

    /// # Errors
    /// When the bytes are not a Vite manifest.
    pub fn from_manifest_bytes(bytes: &[u8]) -> serde_json::Result<Self> {
        let manifest: Manifest = serde_json::from_slice(bytes)?;
        let version = hex::encode(Sha256::digest(bytes));
        Ok(Self {
            dev_server_url: None,
            manifest: Some(manifest),
            version,
        })
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn dev_server_url(&self) -> Option<&str> {
        self.dev_server_url.as_deref()
    }

    /// `<head>` tags for the stylesheet and the Inertia entry.
    #[must_use]
    pub fn tags(&self, nonce: Option<&str>) -> String {
        let nonce_attr = nonce_attr(nonce);
        if let Some(dev) = &self.dev_server_url {
            return format!(
                concat!(
                    "<script type=\"module\"{n}>\n",
                    "import RefreshRuntime from \"{d}/@react-refresh\"\n",
                    "RefreshRuntime.injectIntoGlobalHook(window)\n",
                    "window.$RefreshReg$ = () => {{}}\n",
                    "window.$RefreshSig$ = () => (type) => type\n",
                    "window.__vite_plugin_react_preamble_installed__ = true\n",
                    "</script>\n",
                    "<script type=\"module\" src=\"{d}/@vite/client\"{n}></script>\n",
                    "<link rel=\"stylesheet\" href=\"{d}/{css}\">\n",
                    "<script type=\"module\" src=\"{d}/{js}\"{n}></script>"
                ),
                n = nonce_attr,
                d = escape_attr(dev),
                css = STYLESHEET,
                js = ENTRYPOINT,
            );
        }
        let Some(manifest) = &self.manifest else {
            return String::new();
        };
        // rails_vite's `vite_tags "application.css", "inertia.tsx"`: per entry in that order,
        // its imports as modulepreloads (depth first, each file once), then the entry tag,
        // then the entry's own CSS. No `crossorigin` (the manifest has no `integrity`).
        let mut out = Vec::new();
        let mut preloaded: Vec<&str> = Vec::new();
        let mut stylesheets: Vec<&str> = Vec::new();
        for key in [STYLESHEET, ENTRYPOINT] {
            let Some(entry) = manifest.get(key) else {
                continue;
            };
            let mut seen = Vec::new();
            let imported = imports(manifest, entry, &mut seen);
            for &(file, _) in &imported {
                if !preloaded.contains(&file) {
                    preloaded.push(file);
                    out.push(format!(
                        "<link rel=\"modulepreload\" href=\"{}\"{nonce_attr}>",
                        asset_url(file)
                    ));
                }
            }
            if key.ends_with(".css") {
                out.push(format!(
                    "<link rel=\"stylesheet\" href=\"{}\"{nonce_attr}>",
                    asset_url(&entry.file)
                ));
            } else {
                out.push(format!(
                    "<script src=\"{}\" type=\"module\"{nonce_attr}></script>",
                    asset_url(&entry.file)
                ));
            }
            // The entry's CSS, then (unlike rails_vite, as Vite's backend-integration guide
            // requires) the CSS of the chunks it imports. The kit's own chunks have none.
            let chunk_css = imported.iter().flat_map(|(_, css)| css.iter());
            for css in entry.css.iter().chain(chunk_css) {
                if !stylesheets.contains(&css.as_str()) {
                    stylesheets.push(css);
                    out.push(format!(
                        "<link rel=\"stylesheet\" href=\"{}\"{nonce_attr}>",
                        asset_url(css)
                    ));
                }
            }
        }
        out.join("\n")
    }
}

type CacheKey = (bool, String, String);

fn cache() -> &'static Mutex<HashMap<CacheKey, Arc<Vite>>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, Arc<Vite>>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn cache_key(settings: &ViteSettings) -> CacheKey {
    (
        settings.dev_server,
        settings.dev_server_url.clone(),
        settings.manifest_path.clone(),
    )
}

/// Loads and caches the assets for `settings` at boot.
///
/// # Errors
/// See [`Vite::load`]; production passes `require_manifest = true`.
#[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
pub fn init(settings: &ViteSettings, require_manifest: bool) -> Result<Arc<Vite>> {
    let vite = Arc::new(Vite::load(settings, require_manifest)?);
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(cache_key(settings), vite.clone());
    Ok(vite)
}

/// The cached assets for `settings`, loading them (leniently) on first use.
#[must_use]
pub fn shared(settings: &ViteSettings) -> Arc<Vite> {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(cache_key(settings))
        .or_insert_with(|| {
            Arc::new(Vite::load(settings, false).unwrap_or_else(|e| {
                tracing::error!(error = %e, "vite assets unavailable");
                Vite {
                    dev_server_url: None,
                    manifest: None,
                    version: "invalid-manifest".into(),
                }
            }))
        })
        .clone()
}

/// rails_vite's `resolve_imports`: an entry's imports (file and CSS), depth first, each
/// manifest key once.
fn imports<'m>(
    manifest: &'m Manifest,
    entry: &'m ManifestEntry,
    seen: &mut Vec<&'m str>,
) -> Vec<(&'m str, &'m [String])> {
    let mut files = Vec::new();
    for key in &entry.imports {
        if seen.contains(&key.as_str()) {
            continue;
        }
        seen.push(key);
        if let Some(imported) = manifest.get(key) {
            files.push((imported.file.as_str(), imported.css.as_slice()));
            files.extend(imports(manifest, imported, seen));
        }
    }
    files
}

fn asset_url(file: &str) -> String {
    escape_attr(&format!("{BASE}{}", file.trim_start_matches('/')))
}

pub(crate) fn nonce_attr(nonce: Option<&str>) -> String {
    nonce
        .filter(|n| !n.is_empty())
        .map(|n| format!(" nonce=\"{}\"", escape_attr(n)))
        .unwrap_or_default()
}

/// HTML attribute/text escaping.
#[must_use]
pub fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
      "frontend/entrypoints/inertia.tsx": {"file": "assets/inertia-abc.js", "isEntry": true,
        "imports": ["_vendor.js"], "css": ["assets/inertia-abc.css"]},
      "_vendor.js": {"file": "assets/vendor-123.js", "css": ["assets/vendor.css"]},
      "frontend/entrypoints/application.css": {"file": "assets/application-xyz.css", "isEntry": true}
    }"#;

    #[test]
    fn prod_tags_match_rails_vite_order_and_attributes() {
        let vite = Vite::from_manifest_bytes(MANIFEST.as_bytes()).unwrap();
        assert_eq!(
            vite.tags(Some("n1")),
            [
                r#"<link rel="stylesheet" href="/vite/assets/application-xyz.css" nonce="n1">"#,
                r#"<link rel="modulepreload" href="/vite/assets/vendor-123.js" nonce="n1">"#,
                r#"<script src="/vite/assets/inertia-abc.js" type="module" nonce="n1"></script>"#,
                r#"<link rel="stylesheet" href="/vite/assets/inertia-abc.css" nonce="n1">"#,
                r#"<link rel="stylesheet" href="/vite/assets/vendor.css" nonce="n1">"#,
            ]
            .join("\n")
        );
        assert_eq!(
            vite.tags(None).lines().next(),
            Some(r#"<link rel="stylesheet" href="/vite/assets/application-xyz.css">"#)
        );
    }

    #[test]
    fn version_is_sha256_of_manifest_bytes() {
        let vite = Vite::from_manifest_bytes(MANIFEST.as_bytes()).unwrap();
        assert_eq!(
            vite.version(),
            hex::encode(Sha256::digest(MANIFEST.as_bytes()))
        );
        assert_eq!(Vite::dev("http://x").version(), "dev");
    }

    #[test]
    fn dev_tags_point_at_dev_server_with_nonce() {
        let tags = Vite::dev("http://localhost:5173/").tags(Some("n1"));
        assert!(
            tags.contains("import RefreshRuntime from \"http://localhost:5173/@react-refresh\"")
        );
        assert!(tags.contains(
            r#"<script type="module" src="http://localhost:5173/@vite/client" nonce="n1">"#
        ));
        assert!(tags.contains(
            r#"<script type="module" src="http://localhost:5173/frontend/entrypoints/inertia.tsx" nonce="n1">"#
        ));
        assert!(tags.starts_with(r#"<script type="module" nonce="n1">"#));
    }

    #[test]
    fn missing_manifest_is_an_error_only_when_required() {
        let settings = ViteSettings {
            dev_server: false,
            dev_server_url: String::new(),
            manifest_path: "/nonexistent/manifest.json".into(),
        };
        assert!(Vite::load(&settings, true).is_err());
        assert_eq!(Vite::load(&settings, false).unwrap().tags(None), "");
    }
}
