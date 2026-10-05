//! `settings:` from config/*.yaml, validated once at boot and stored in
//! `ctx.shared_store` (see `App::after_context`).

use std::sync::{Arc, OnceLock};

use loco_rs::{app::AppContext, environment::Environment, Error, Result};
use serde::Deserialize;

/// Minimum `secret_key_base` length accepted in production.
pub const MIN_SECRET_LEN: usize = 64;

/// The `secret_key_base` defaults committed in config/development.yaml and
/// config/test.yaml. They are public, so production refuses them.
pub const SHIPPED_SECRETS: [&str; 2] = [
    "development-secret-key-base-0123456789abcdef0123456789abcdef0123456789abcdef",
    "test-secret-key-base-0123456789abcdef0123456789abcdef0123456789abcdef0123",
];

/// Substrings that mark a secret as one of the shipped (or derived) examples.
const SHIPPED_SECRET_MARKERS: [&str; 2] = ["development-secret", "test-secret"];

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub secret_key_base: String,
    /// Public base URL (no trailing slash needed): mail links, Origin checks,
    /// 409 `X-Inertia-Location`.
    pub app_url: String,
    pub app_name: String,
    pub mail_from: String,
    pub encrypt_history: bool,
    pub forgery_protection: bool,
    pub vite: ViteSettings,
    pub ssr: SsrSettings,
    /// Production normally refuses an `http://` `app_url`. Set this to boot a
    /// production build locally over plain http (cookies stay `Secure`, so
    /// use `localhost`, which browsers treat as a secure context). Off by default.
    #[serde(default)]
    pub allow_insecure_http: bool,
    /// inertia-rails `config.server_head`: `false` (default) sends meta tags
    /// as objects under the `_inertia_meta` prop; `true` sends them as HTML
    /// strings under `head` (Inertia v3 `serverHead`); a string names the prop.
    #[serde(default)]
    pub server_head: ServerHead,
    /// inertia-rails `config.use_data_inertia_head_attribute`: mark head tags
    /// with `data-inertia` instead of `inertia` (implied by `server_head`).
    #[serde(default)]
    pub use_data_inertia_head_attribute: bool,
    /// Other hostnames this same app answers on (comma-separated, e.g. `two.example.com`;
    /// `EXTRA_HOSTS`), each with its own host-only cookies and so its own session. Empty (the
    /// default): only `app_url`'s host. A non-safe request to a listed host passes the CSRF
    /// Origin check only when its `Origin` is that same host (src/inertia/csrf.rs); the 409
    /// version reload stays on it (src/inertia/version.rs). Absolute URLs (mail) keep `app_url`.
    #[serde(default)]
    pub extra_hosts: String,
    /// Who can sign up (`SIGN_UP`): `open` (the default, anyone) or `invitation_only` (only
    /// through a pending invitation, for the address it was sent to). See
    /// src/controllers/users.rs.
    #[serde(default)]
    pub sign_up: SignUp,
    /// Whether the resolved `AppContext.environment` is production. Set by
    /// [`Settings::from_ctx`], never read from YAML; security policy
    /// (HSTS, `Secure` cookies) keys off this, not process env vars.
    #[serde(skip)]
    pub production: bool,
    /// The cookie keys derived from `secret_key_base`, on first use (see [`Settings::cookie_keys`]).
    #[serde(skip)]
    cookie_keys: OnceLock<CookieKeys>,
}

/// One key per cookie purpose (src/inertia/cookies.rs).
#[derive(Debug, Clone)]
pub struct CookieKeys {
    pub flash: cookie::Key,
    pub csrf: cookie::Key,
    pub session: cookie::Key,
}

/// `settings.sign_up`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignUp {
    /// Anyone can sign up; without an invitation they get a personal account.
    #[default]
    Open,
    /// Sign-up only through a pending invitation, with the address it was sent to.
    InvitationOnly,
}

/// `settings.server_head`: `false`, `true`, or a custom prop name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ServerHead {
    Enabled(bool),
    Prop(String),
}

impl Default for ServerHead {
    fn default() -> Self {
        Self::Enabled(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViteSettings {
    pub dev_server: bool,
    pub dev_server_url: String,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SsrSettings {
    pub enabled: bool,
    pub spawn: bool,
    pub bundle: String,
    pub timeout_ms: u64,
    /// Node binary used when `spawn` is on.
    #[serde(default = "default_node")]
    pub node: String,
    /// SSR server render endpoint (used when the Vite dev server is off).
    #[serde(default = "default_ssr_url")]
    pub url: String,
}

/// A boolean setting read from an environment variable, for `#[serde(deserialize_with =
/// "bool_env")]` on a settings field. Loco renders `config/*.yaml` with Tera, which can only
/// substitute text, so write the YAML as a string:
///
/// ```yaml
/// settings:
///   signups_open: "<%= get_env(name="SIGNUPS_OPEN", default="") %>"
/// ```
///
/// `"1"`, `"true"`, `"yes"` and `"on"` (any case) are on; anything else (unset, `""`, `"0"`,
/// `"false"`) is off. A real YAML boolean works too.
///
/// # Errors
/// When the value is neither a boolean nor a string.
pub fn bool_env<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bool(bool),
        Text(String),
    }
    Ok(match Raw::deserialize(deserializer)? {
        Raw::Bool(b) => b,
        Raw::Text(t) => matches!(
            t.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
    })
}

fn default_node() -> String {
    "node".to_owned()
}

fn default_ssr_url() -> String {
    "http://127.0.0.1:13714/render".to_owned()
}

impl Settings {
    /// Reads and validates `settings:`.
    ///
    /// # Errors
    /// When the block is missing or malformed, or (in production) when
    /// `secret_key_base` is shorter than 64 chars or is a shipped example, or
    /// `app_url` is not an absolute `https://` URL (see `allow_insecure_http`).
    #[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
    pub fn from_ctx(ctx: &AppContext) -> Result<Arc<Self>> {
        let raw = ctx
            .config
            .settings
            .clone()
            .ok_or_else(|| Error::string("config: missing `settings:` block"))?;
        let mut settings: Self = serde_json::from_value(raw)
            .map_err(|e| Error::string(&format!("config: invalid `settings:` block: {e}")))?;
        settings.validate(&ctx.environment)?;
        settings.production = ctx.environment == Environment::Production;
        Ok(Arc::new(settings))
    }

    /// The flash, CSRF and session cookie keys, derived from `secret_key_base` once per
    /// `Settings` rather than on every request: each derivation is two HMAC-SHA256 runs, and
    /// the flash and CSRF layers need one on every request (docs/PROFILING.md). Set
    /// `secret_key_base` before the first call: the keys are not re-derived after it.
    pub fn cookie_keys(&self) -> &CookieKeys {
        self.cookie_keys.get_or_init(|| CookieKeys {
            flash: super::cookies::derive_key(&self.secret_key_base, "flash"),
            csrf: super::cookies::derive_key(&self.secret_key_base, "csrf"),
            session: super::cookies::derive_key(&self.secret_key_base, "session"),
        })
    }

    /// # Errors
    /// See [`Settings::from_ctx`].
    #[allow(clippy::result_large_err)] // loco_rs::Error is large; see src/bin/main.rs
    pub fn validate(&self, environment: &Environment) -> Result<()> {
        if *environment == Environment::Production {
            if self.secret_key_base.len() < MIN_SECRET_LEN {
                return Err(Error::string(&format!(
                    "settings.secret_key_base must be at least {MIN_SECRET_LEN} characters in production (set SECRET_KEY_BASE)"
                )));
            }
            if SHIPPED_SECRETS.contains(&self.secret_key_base.as_str())
                || SHIPPED_SECRET_MARKERS
                    .iter()
                    .any(|m| self.secret_key_base.contains(m))
            {
                return Err(Error::string(
                    "settings.secret_key_base is a published development/test example; set SECRET_KEY_BASE to a fresh secret (`openssl rand -hex 64`)",
                ));
            }
            if self.app_url.trim().is_empty() {
                return Err(Error::string(
                    "settings.app_url must be set in production (set HOST)",
                ));
            }
            let url = url::Url::parse(self.app_url.trim()).map_err(|e| {
                Error::string(&format!(
                    "settings.app_url must be an absolute URL in production (set HOST): {e}"
                ))
            })?;
            let https = url.scheme() == "https";
            if !(https || (self.allow_insecure_http && url.scheme() == "http")) {
                return Err(Error::string(
                    "settings.app_url must use https:// in production (set HOST; `allow_insecure_http: true` permits http for local testing only)",
                ));
            }
        }
        Ok(())
    }

    /// inertia-rails `configuration.meta_prop`: the prop carrying meta tags.
    #[must_use]
    pub fn meta_prop(&self) -> &str {
        match &self.server_head {
            ServerHead::Enabled(false) => "_inertia_meta",
            ServerHead::Enabled(true) => "head",
            ServerHead::Prop(name) => name,
        }
    }

    /// Whether `server_head` is on (meta tags serialized as HTML strings).
    #[must_use]
    pub fn server_head_enabled(&self) -> bool {
        self.server_head != ServerHead::Enabled(false)
    }

    /// inertia-rails `configuration.head_attribute`.
    #[must_use]
    pub fn head_attribute(&self) -> &'static str {
        if self.server_head_enabled() || self.use_data_inertia_head_attribute {
            "data-inertia"
        } else {
            "inertia"
        }
    }

    /// [`Settings::extra_hosts`], parsed: lowercase hostnames (with any `:port`), no blanks.
    #[must_use]
    pub fn extra_host_list(&self) -> Vec<String> {
        self.extra_hosts
            .split(',')
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect()
    }

    /// The origin (`scheme://host[:port]`, `app_url`'s scheme) of a request whose `Host`
    /// header is one of [`Settings::extra_hosts`]; `None` for any other host, including
    /// `app_url`'s own.
    #[must_use]
    pub fn extra_origin_for(&self, host: &str) -> Option<String> {
        let host = host.trim().to_ascii_lowercase();
        if !self.extra_host_list().contains(&host) {
            return None;
        }
        let scheme = url::Url::parse(self.app_url.trim())
            .map_or_else(|_| "https".to_owned(), |u| u.scheme().to_owned());
        url::Url::parse(&format!("{scheme}://{host}"))
            .ok()
            .map(|u| u.origin().ascii_serialization())
    }

    /// `app_url` without a trailing slash.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.app_url.trim_end_matches('/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_json(app_url: &str) -> serde_json::Value {
        serde_json::json!({
            "secret_key_base": "x", "app_url": app_url, "app_name": "T",
            "mail_from": "a@b.c", "encrypt_history": false, "forgery_protection": true,
            "vite": {"dev_server": false, "dev_server_url": "http://localhost:5173",
                     "manifest_path": "m.json"},
            "ssr": {"enabled": false, "spawn": false, "bundle": "ssr/ssr.js", "timeout_ms": 10}
        })
    }

    fn settings(secret: &str, app_url: &str) -> Settings {
        let mut raw = settings_json(app_url);
        raw["secret_key_base"] = secret.into();
        serde_json::from_value(raw).unwrap()
    }

    #[test]
    fn sign_up_is_open_unless_set_to_invitation_only() {
        assert_eq!(settings("x", "https://e.x").sign_up, SignUp::Open);
        let mut raw = settings_json("https://e.x");
        raw["sign_up"] = "invitation_only".into();
        let s: Settings = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(s.sign_up, SignUp::InvitationOnly);
        raw["sign_up"] = "closed".into();
        let err = serde_json::from_value::<Settings>(raw).unwrap_err();
        assert!(err.to_string().contains("invitation_only"), "{err}");
    }

    #[test]
    fn extra_hosts_are_off_by_default_and_parsed_from_a_string() {
        let s = settings("x", "https://example.com");
        assert!(s.extra_host_list().is_empty());
        assert_eq!(s.extra_origin_for("example.com"), None);
        let mut raw = settings_json("https://a.example");
        raw["extra_hosts"] = " Two.Example.com , ,b.example:8080".into();
        let s: Settings = serde_json::from_value(raw).unwrap();
        assert_eq!(s.extra_host_list(), ["two.example.com", "b.example:8080"]);
        assert_eq!(
            s.extra_origin_for("TWO.example.com").as_deref(),
            Some("https://two.example.com")
        );
        assert_eq!(
            s.extra_origin_for("b.example:8080").as_deref(),
            Some("https://b.example:8080")
        );
        assert_eq!(s.extra_origin_for("a.example"), None, "app_url's own host");
    }

    #[test]
    fn bool_env_reads_the_strings_an_env_var_gives() {
        #[derive(Deserialize)]
        struct Flags {
            #[serde(deserialize_with = "bool_env")]
            on: bool,
        }
        let read = |v: serde_json::Value| {
            serde_json::from_value::<Flags>(serde_json::json!({ "on": v }))
                .unwrap()
                .on
        };
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(read(on.into()), "{on}");
        }
        for off in ["", "0", "false", "no", "off", "2"] {
            assert!(!read(off.into()), "{off}");
        }
        assert!(read(true.into()));
        assert!(!read(false.into()));
        assert!(serde_json::from_value::<Flags>(serde_json::json!({ "on": 1 })).is_err());
    }

    #[test]
    fn production_refuses_short_secret() {
        let s = settings("short", "https://example.com");
        assert!(s.validate(&Environment::Production).is_err());
        assert!(s.validate(&Environment::Development).is_ok());
    }

    #[test]
    fn production_refuses_empty_app_url() {
        let s = settings(&"x".repeat(64), " ");
        assert!(s.validate(&Environment::Production).is_err());
        let s = settings(&"x".repeat(64), "https://example.com/");
        assert!(s.validate(&Environment::Production).is_ok());
        assert_eq!(s.base_url(), "https://example.com");
    }

    #[test]
    fn production_refuses_the_shipped_secrets() {
        for secret in SHIPPED_SECRETS {
            let s = settings(secret, "https://example.com");
            assert!(s.validate(&Environment::Production).is_err(), "{secret}");
            assert!(s.validate(&Environment::Development).is_ok());
        }
        let derived = format!("my-development-secret-{}", "x".repeat(64));
        assert!(settings(&derived, "https://example.com")
            .validate(&Environment::Production)
            .is_err());
        let derived = format!("{}test-secret", "x".repeat(64));
        assert!(settings(&derived, "https://example.com")
            .validate(&Environment::Production)
            .is_err());
    }

    #[test]
    fn production_requires_https_unless_explicitly_allowed() {
        let secret = "x".repeat(64);
        for bad in ["http://app.example.com", "app.example.com", "ftp://x.y"] {
            let s = settings(&secret, bad);
            assert!(s.validate(&Environment::Production).is_err(), "{bad}");
            assert!(s.validate(&Environment::Development).is_ok());
        }
        let mut s = settings(&secret, "http://localhost:5150");
        assert!(!s.allow_insecure_http, "off by default");
        s.allow_insecure_http = true;
        assert!(s.validate(&Environment::Production).is_ok());
        let mut s = settings(&secret, "ftp://x.y");
        s.allow_insecure_http = true;
        assert!(s.validate(&Environment::Production).is_err());
    }

    #[test]
    fn server_head_setting_picks_the_meta_prop_and_attribute() {
        let mut s = settings("x", "y");
        assert_eq!(
            (s.meta_prop(), s.head_attribute()),
            ("_inertia_meta", "inertia")
        );
        s.use_data_inertia_head_attribute = true;
        assert_eq!(s.head_attribute(), "data-inertia");
        s.use_data_inertia_head_attribute = false;
        s.server_head = serde_json::from_value(serde_json::json!(true)).unwrap();
        assert_eq!(
            (s.meta_prop(), s.head_attribute()),
            ("head", "data-inertia")
        );
        s.server_head = serde_json::from_value(serde_json::json!("seo")).unwrap();
        assert_eq!(s.meta_prop(), "seo");
    }

    #[test]
    fn ssr_url_and_node_default() {
        let s = settings("x", "y");
        assert_eq!(s.ssr.url, "http://127.0.0.1:13714/render");
        assert_eq!(s.ssr.node, "node");
    }
}
