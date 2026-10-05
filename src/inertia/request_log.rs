//! Request logging with sensitive query parameters redacted (Rails'
//! `filter_parameters` for request URLs).
//!
//! Replaces Loco's `logger` middleware, which records the full URI, so a
//! password-reset link (`?sid=…`) would land in the logs verbatim. Tokens carried
//! in the path (`/invitations/{token}`, `/sessions/{id}`) are redacted too.
//! [`Middleware`] keeps Loco's name, config switch (`server.middlewares.logger`)
//! and position in the stack (inside `request_id`); `App::middlewares` swaps
//! it in. Span fields are the same as Loco's, except `http.uri` is redacted.

use axum::{http, Router as AXRouter};
use loco_rs::{
    app::AppContext,
    controller::middleware::{logger, request_id::LocoRequestId, MiddlewareLayer},
    environment::Environment,
    Result,
};
use serde::Serialize;
use tower_http::{
    add_extension::AddExtensionLayer,
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{MakeSpan, TraceLayer},
};

/// What a redacted query value is replaced with.
pub const FILTERED: &str = "[FILTERED]";

/// Query parameter names whose values never reach the logs: `sid`, `token`,
/// `invitation` (the invitation token on sign-in/sign-up links),
/// `password*`, `*_token` (case-insensitive; `user[password]`-style names
/// count by their innermost key).
#[must_use]
pub fn is_sensitive_param(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let inner = name
        .rsplit('[')
        .next()
        .unwrap_or(&name)
        .trim_end_matches(']');
    [name.as_str(), inner].iter().any(|n| {
        *n == "sid"
            || *n == "token"
            || *n == "invitation"
            || n.starts_with("password")
            || n.ends_with("_token")
    })
}

/// Path segments that hold a bearer secret: the segment after each of these
/// (`/invitations/{token}`, `/invitations/{token}/accept`, and `/sessions/{id}`,
/// whose id is the session's token).
const SENSITIVE_PATH_PREFIXES: [&str; 2] = ["/invitations/", "/sessions/"];

/// `path` with the token segment of a [`SENSITIVE_PATH_PREFIXES`] route replaced
/// by [`FILTERED`].
fn redact_path(path: &str) -> std::borrow::Cow<'_, str> {
    for prefix in SENSITIVE_PATH_PREFIXES {
        if let Some(rest) = path.strip_prefix(prefix) {
            let end = rest.find('/').unwrap_or(rest.len());
            if end > 0 {
                return format!("{prefix}{FILTERED}{}", &rest[end..]).into();
            }
        }
    }
    path.into()
}

/// `uri` with a path token ([`SENSITIVE_PATH_PREFIXES`]) and every sensitive
/// query value replaced by [`FILTERED`]. The rest of the query is kept
/// byte-for-byte.
#[must_use]
pub fn redact_uri(uri: &http::Uri) -> String {
    let path = redact_path(uri.path());
    let Some(query) = uri.query() else {
        return path.into_owned();
    };
    let redacted: Vec<String> = query
        .split('&')
        .map(|pair| {
            let (raw_name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = percent_decode(raw_name);
            if is_sensitive_param(&name) && !value.is_empty() {
                format!("{raw_name}={FILTERED}")
            } else {
                pair.to_owned()
            }
        })
        .collect();
    format!("{path}?{}", redacted.join("&"))
}

/// `text` with the value of every sensitive `?name=value` / `&name=value`
/// pair, and the token of every [`SENSITIVE_PATH_PREFIXES`] path, replaced by
/// [`FILTERED`], wherever it appears: absolute or relative URLs inside
/// free-form log lines (e.g. Inertia's `URL: /reset?sid=…`). A value ends at
/// `&`, `#`, whitespace, a quote, `<`, `>` or a control character (so a
/// trailing ANSI color reset survives); a path token also ends at `/` or `?`.
#[must_use]
pub fn redact_text(text: &str) -> std::borrow::Cow<'_, str> {
    static PAIR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static PATH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pair = PAIR.get_or_init(|| {
        regex::Regex::new(r#"([?&])([^=&?#\s"'<>\x00-\x1f]+)=([^&#\s"'<>\x00-\x1f]+)"#)
            .expect("valid regex")
    });
    let path = PATH.get_or_init(|| {
        regex::Regex::new(r#"(/(?:invitations|sessions)/)([^/?&#\s"'<>\x00-\x1f]+)"#)
            .expect("valid regex")
    });
    let text = path.replace_all(text, |c: &regex::Captures<'_>| {
        format!("{}{FILTERED}", &c[1])
    });
    match pair.replace_all(&text, |c: &regex::Captures<'_>| {
        if is_sensitive_param(&percent_decode(&c[2])) {
            format!("{}{}={FILTERED}", &c[1], &c[2])
        } else {
            c[0].to_owned()
        }
    }) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(out) => out.into(),
    }
}

fn percent_decode(raw: &str) -> String {
    let plus_as_space = raw.replace('+', " ");
    url::form_urlencoded::parse(format!("x={plus_as_space}").as_bytes())
        .next()
        .map_or_else(|| raw.to_owned(), |(_, v)| v.into_owned())
}

/// Builds the `http-request` span with a redacted URI.
#[derive(Clone, Debug)]
pub struct RedactedSpan;

impl<B> MakeSpan<B> for RedactedSpan {
    fn make_span(&mut self, request: &http::Request<B>) -> tracing::Span {
        let ext = request.extensions();
        let request_id = ext
            .get::<LocoRequestId>()
            .map_or_else(|| "req-id-none".to_string(), |r| r.get().to_string());
        let user_agent = request
            .headers()
            .get(http::header::USER_AGENT)
            .map_or("", |h| h.to_str().unwrap_or(""));
        let env: String = ext
            .get::<Environment>()
            .map(ToString::to_string)
            .unwrap_or_default();
        tracing::error_span!(
            "http-request",
            "http.method" = tracing::field::display(request.method()),
            "http.uri" = tracing::field::display(redact_uri(request.uri())),
            "http.version" = tracing::field::debug(request.version()),
            "http.user_agent" = tracing::field::display(user_agent),
            "environment" = tracing::field::display(env),
            request_id = tracing::field::display(request_id),
        )
    }
}

/// The trace layer the middleware installs (exposed for tests).
#[must_use]
pub fn trace_layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, RedactedSpan> {
    TraceLayer::new_for_http().make_span_with(RedactedSpan)
}

/// Drop-in replacement for Loco's `logger` middleware.
#[derive(Serialize, Debug)]
pub struct Middleware {
    enable: bool,
    environment: Environment,
}

impl Middleware {
    /// Honors `server.middlewares.logger.enable` (default on, like Loco).
    #[must_use]
    pub fn from_ctx(ctx: &AppContext) -> Self {
        let config = ctx
            .config
            .server
            .middlewares
            .logger
            .clone()
            .unwrap_or(logger::Config { enable: true });
        Self {
            enable: config.enable,
            environment: ctx.environment.clone(),
        }
    }
}

impl MiddlewareLayer for Middleware {
    fn name(&self) -> &'static str {
        "logger"
    }

    fn is_enabled(&self) -> bool {
        self.enable
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app
            .layer(trace_layer())
            .layer(AddExtensionLayer::new(self.environment.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_names() {
        for n in [
            "sid",
            "SID",
            "token",
            "password",
            "password_confirmation",
            "reset_token",
            "user[password]",
            "user[auth_token]",
            "invitation",
        ] {
            assert!(is_sensitive_param(n), "{n}");
        }
        for n in ["page", "tokens", "q", "sidebar", "user[name]"] {
            assert!(!is_sensitive_param(n), "{n}");
        }
    }

    #[test]
    fn redacts_only_sensitive_values() {
        let uri: http::Uri = "/identity/password_reset/edit?sid=abc.def--123&page=2&user%5Bpassword%5D=x&access_token=t"
            .parse()
            .unwrap();
        assert_eq!(
            redact_uri(&uri),
            "/identity/password_reset/edit?sid=[FILTERED]&page=2&user%5Bpassword%5D=[FILTERED]&access_token=[FILTERED]"
        );
        let uri: http::Uri = "/x".parse().unwrap();
        assert_eq!(redact_uri(&uri), "/x");
    }

    #[test]
    fn redacts_tokens_carried_in_the_path() {
        for (uri, logged) in [
            ("/invitations/inv-tok3n", "/invitations/[FILTERED]"),
            (
                "/invitations/inv-tok3n/accept",
                "/invitations/[FILTERED]/accept",
            ),
            ("/sessions/sess-tok3n?x=1", "/sessions/[FILTERED]?x=1"),
            // An account's own invitation list and ids are not secrets.
            ("/acme/invitations/7", "/acme/invitations/7"),
            ("/invitations/", "/invitations/"),
        ] {
            assert_eq!(redact_uri(&uri.parse().unwrap()), logged, "{uri}");
        }
        assert_eq!(
            redact_text("URL: https://h/invitations/inv-tok3n/accept?sid=s and \"/sessions/abc\""),
            "URL: https://h/invitations/[FILTERED]/accept?sid=[FILTERED] and \"/sessions/[FILTERED]\""
        );
    }

    #[test]
    fn redacts_sensitive_query_values_anywhere_in_a_line() {
        let line = "\x1b[2m  URL: /identity/password_reset/edit?sid=abc.def--1&page=2\x1b[0m";
        assert_eq!(
            redact_text(line),
            "\x1b[2m  URL: /identity/password_reset/edit?sid=[FILTERED]&page=2\x1b[0m"
        );
        assert_eq!(
            redact_text(
                r#"GET "https://h/x?q=1&user%5Bpassword%5D=p#f" and /y?token=t /z?Reset_Token=r"#
            ),
            r#"GET "https://h/x?q=1&user%5Bpassword%5D=[FILTERED]#f" and /y?token=[FILTERED] /z?Reset_Token=[FILTERED]"#
        );
        for untouched in [
            "no query here",
            "/x?page=2&sidebar=1",
            "a=b sid=c",
            "/x?sid=",
        ] {
            assert_eq!(redact_text(untouched), untouched);
        }
    }
}
