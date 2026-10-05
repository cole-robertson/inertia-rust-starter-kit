//! `allow_browser versions: :modern` (the Rails kit's `ApplicationController`): a browser older
//! than Safari 17.2, Chrome 120, Firefox 121 or Opera 106, or any Internet Explorer, gets
//! `406 Not Acceptable` with `public/406-unsupported-browser.html`.
//!
//! A port of actionpack 8.1's `AllowBrowser::BrowserBlocker` and the part of the `useragent`
//! gem (0.16.11) it relies on: user-agent tokenization, browser detection in the gem's order
//! (Edge, IE, Opera, WeChat, Vivaldi, Chrome, iTunes, PlayStation, Podcast Addict, WebKit,
//! Gecko, …), the per-browser version, bot detection and the gem's version comparison. Every
//! verdict in `tests/fixtures/allow_browser.tsv` was produced by the Rails code itself.
//!
//! Like Rails it runs for controller actions only (it's a `before_action`), so `/up` and files
//! in `public/` are never blocked (see [`layer`]).

use std::{cmp::Ordering, sync::LazyLock};

use axum::{
    extract::{MatchedPath, Request},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use regex::Regex;

/// `allow_browser versions: :modern`: `{ safari: 17.2, chrome: 120, firefox: 121, opera: 106,
/// ie: false }`. `None` blocks every version (`ie: false`).
const MODERN: &[(&str, Option<&str>)] = &[
    ("safari", Some("17.2")),
    ("chrome", Some("120")),
    ("firefox", Some("121")),
    ("opera", Some("106")),
    ("ie", None),
];

/// Whether `user_agent` is blocked by `allow_browser versions: :modern`.
#[must_use]
pub fn blocked(user_agent: Option<&str>) -> bool {
    // `user_agent_version_reported?`: a present UA whose parsed version isn't empty.
    let Some(ua) = user_agent.filter(|ua| !ua.trim().is_empty()) else {
        return false;
    };
    let agent = Agent::parse(ua);
    if agent.version().is_nil() {
        return false;
    }
    let name = match agent.browser().to_lowercase().as_str() {
        "internet explorer" => "ie".to_owned(),
        other => other.to_owned(),
    };
    let Some((_, minimum)) = MODERN.iter().find(|(browser, _)| *browser == name) else {
        return false;
    };
    let below = match minimum {
        Some(minimum) => agent.version().cmp_version(&Version::new(minimum)) == Ordering::Less,
        None => true,
    };
    below && !agent.bot()
}

/// Apply the check to every routed request except `GET /up` (Rails' health controller isn't
/// an `ApplicationController`). Unrouted requests (files in `public/`, 404s) have no
/// `MatchedPath` and are never blocked, like Rails' static file server.
pub fn layer(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(middleware))
}

async fn middleware(req: Request, next: Next) -> Response {
    let routed = req
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|p| p.as_str() != crate::route_table::UP);
    let ua = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    if routed && blocked(ua) {
        return unsupported_browser().await;
    }
    next.run(req).await
}

async fn unsupported_browser() -> Response {
    let body = tokio::fs::read_to_string(
        std::path::Path::new(crate::inertia::public::PUBLIC_DIR)
            .join("406-unsupported-browser.html"),
    )
    .await
    .unwrap_or_else(|_| "Your browser is not supported".to_owned());
    (
        StatusCode::NOT_ACCEPTABLE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// The `useragent` gem, as far as BrowserBlocker needs it
// ---------------------------------------------------------------------------

/// `UserAgent::MATCHER`: product, version, and an optional `(comment)`.
static MATCHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^['"]*([^/\s]+)/?([^\s,]*)(\s\(([^)]*)\)|,gzip\(gfe\))?"#)
        .expect("MATCHER is a valid regex")
});
static WEBKIT_PRODUCT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^AppleWebKit$").expect("valid regex"));
static WEBKIT_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?<webkit>AppleWebKit)/(?<version>[\d.]+)").expect("valid regex")
});
static IOS_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"CPU (?:iPhone |iPod )?OS ([\d_]+) like Mac OS X").expect("valid regex")
});
static IE_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(MSIE\s|rv:)([\d.]+)").expect("valid regex"));
static IE_DETECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Trident.+rv:").expect("valid regex"));
static OPERA_MINI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Opera Mini/([\d.]+)").expect("valid regex"));
static IOS_OS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"iOS ([\d.]+)").expect("valid regex"));
static BOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)bot").expect("valid regex"));

/// One `product/version (comment; …)` token.
#[derive(Debug, Clone)]
struct Token {
    product: String,
    version: Version,
    comment: Option<Vec<String>>,
}

impl Token {
    fn has_comment(&self) -> bool {
        self.comment.as_ref().is_some_and(|c| !c.is_empty())
    }

    fn comment_any(&self, f: impl Fn(&str) -> bool) -> bool {
        self.comment
            .as_ref()
            .is_some_and(|c| c.iter().any(|s| f(s)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Edge,
    InternetExplorer,
    Opera,
    Wechat,
    Vivaldi,
    Chrome,
    ITunes,
    PlayStation,
    PodcastAddict,
    Webkit,
    Gecko,
    WindowsMediaPlayer,
    AppleCoreMedia,
    Libavformat,
    Other,
}

/// `UserAgent.parse`: the tokens, and which `Browsers::*` class claimed them.
#[derive(Debug)]
struct Agent {
    tokens: Vec<Token>,
    kind: Kind,
}

impl Agent {
    fn parse(ua: &str) -> Self {
        let mut rest = if ua.trim().is_empty() {
            "Mozilla/4.0 (compatible)".to_owned()
        } else {
            ua.to_owned()
        };
        let mut tokens = Vec::new();
        while let Some(m) = MATCHER.captures(&rest) {
            let whole = m.get(0).map_or(0, |g| g.end());
            let comment = m.get(4).map(|c| {
                c.as_str()
                    .split("; ")
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            });
            tokens.push(Token {
                product: m[1].to_owned(),
                version: Version::new(&m[2]),
                comment,
            });
            if whole == 0 {
                break;
            }
            rest = rest[whole..].trim_matches(ruby_space).to_owned();
        }
        let mut agent = Self {
            tokens,
            kind: Kind::Other,
        };
        agent.kind = agent.detect_kind();
        agent
    }

    fn detect(&self, product: &str) -> Option<&Token> {
        self.tokens
            .iter()
            .find(|t| t.product.to_lowercase() == product.to_lowercase())
    }

    fn first(&self) -> Option<&Token> {
        self.tokens.first()
    }

    fn last(&self) -> Option<&Token> {
        self.tokens.last()
    }

    /// `Base#application`: the first token.
    fn base_application(&self) -> Option<&Token> {
        self.first()
    }

    /// `application` for Chrome, WebKit, Vivaldi and AppleCoreMedia: the first token with a
    /// non-empty comment.
    fn commented_application(&self) -> Option<&Token> {
        self.tokens.iter().find(|t| t.has_comment())
    }

    fn application(&self) -> Option<&Token> {
        match self.kind {
            Kind::Chrome | Kind::Webkit | Kind::Vivaldi | Kind::AppleCoreMedia | Kind::ITunes => {
                self.commented_application()
            }
            _ => self.base_application(),
        }
    }

    /// `Browsers.extend`: the first class (in `Browsers::ALL` order) whose `extend?` is true.
    fn detect_kind(&self) -> Kind {
        let app = self.base_application();
        let app_comment = app.and_then(|a| a.comment.as_ref());
        if self.last().is_some_and(|t| t.product == "Edge") {
            return Kind::Edge;
        }
        if let Some(comment) = app_comment {
            if comment.get(1).is_some_and(|c| c.contains("MSIE"))
                || IE_DETECT.is_match(&comment.join("; "))
            {
                return Kind::InternetExplorer;
            }
        }
        if self.first().is_some_and(|t| t.product == "Opera")
            || self.last().is_some_and(|t| t.product == "OPR")
        {
            return Kind::Opera;
        }
        if self
            .tokens
            .iter()
            .any(|t| t.product.to_lowercase().contains("micromessenger"))
        {
            return Kind::Wechat;
        }
        if self.tokens.iter().any(|t| t.product == "Vivaldi") {
            return Kind::Vivaldi;
        }
        if self
            .tokens
            .iter()
            .any(|t| t.product == "Chrome" || t.product == "CriOS")
        {
            return Kind::Chrome;
        }
        if self.tokens.iter().any(|t| t.product == "iTunes") {
            return Kind::ITunes;
        }
        if let Some(first) = app_comment.and_then(|c| c.first()) {
            if first.contains("PLAYSTATION 3")
                || first.contains("PlayStation Vita")
                || first.contains("PlayStation 4")
            {
                return Kind::PlayStation;
            }
        }
        if self.tokens.len() >= 3
            && self.tokens[0].product == "Podcast"
            && self.tokens[1].product == "Addict"
            && self.tokens[2].product == "-"
        {
            return Kind::PodcastAddict;
        }
        if self.tokens.iter().any(|t| {
            WEBKIT_PRODUCT.is_match(&t.product) || t.comment_any(|c| WEBKIT_VERSION.is_match(c))
        }) {
            return Kind::Webkit;
        }
        if app.is_some_and(|a| a.product == "Mozilla") {
            return Kind::Gecko;
        }
        let base_version = app
            .map(|a| a.version.as_str().to_owned())
            .unwrap_or_default();
        if self
            .tokens
            .iter()
            .any(|t| ["NSPlayer", "Windows-Media-Player", "WMFSDK"].contains(&t.product.as_str()))
            && !["4.1.0.3856", "7.10.0.3059", "7.0.0.1956"].contains(&base_version.as_str())
        {
            return Kind::WindowsMediaPlayer;
        }
        if self.tokens.iter().any(|t| t.product == "AppleCoreMedia") {
            return Kind::AppleCoreMedia;
        }
        if self.tokens.iter().any(|t| {
            t.product == "Lavf" || (t.product == "NSPlayer" && base_version == "4.1.0.3856")
        }) {
            return Kind::Libavformat;
        }
        Kind::Other
    }

    /// The first comment of `application` (platform detection).
    fn app_comment(&self) -> Option<&Vec<String>> {
        self.application().and_then(|a| a.comment.as_ref())
    }

    fn webkit_platform(&self) -> Option<String> {
        let comment = self.app_comment()?;
        let first = comment.first()?;
        Some(if first.contains("Windows") {
            "Windows".into()
        } else if first == "BB10" {
            "BlackBerry".into()
        } else if comment.iter().any(|c| c.contains("Android")) {
            "Android".into()
        } else {
            first.clone()
        })
    }

    /// `Webkit#os`, as far as `browser` and `version` need it (Android / iOS detection).
    fn webkit_os(&self) -> Option<String> {
        let comment = self.app_comment()?;
        let c0 = comment.first().map(String::as_str).unwrap_or_default();
        let raw = if c0.contains("Windows NT") {
            Some(c0.to_owned())
        } else if comment.get(2).is_none() || comment.get(1).is_some_and(|c| c.contains("Android"))
        {
            comment.get(1).cloned()
        } else if let Some(ios) = comment.iter().find(|c| IOS_VERSION.is_match(c)) {
            Some(ios.clone())
        } else {
            comment.get(2).cloned()
        };
        raw.map(|os| normalize_ios(&os).unwrap_or(os))
    }

    fn browser(&self) -> String {
        match self.kind {
            Kind::Edge => "Edge".into(),
            Kind::InternetExplorer => "Internet Explorer".into(),
            Kind::Opera => "Opera".into(),
            Kind::Wechat => "Wechat Browser".into(),
            Kind::Vivaldi => "Vivaldi".into(),
            Kind::Chrome => {
                if self.detect("Iron").is_some() {
                    "Iron".into()
                } else {
                    "Chrome".into()
                }
            }
            Kind::ITunes => "iTunes".into(),
            Kind::PlayStation => self.playstation_browser().unwrap_or_default(),
            Kind::PodcastAddict => "Podcast Addict".into(),
            Kind::Webkit => {
                if self.webkit_os().is_some_and(|os| os.contains("Android")) {
                    "Android".into()
                } else if self.webkit_platform().as_deref() == Some("BlackBerry") {
                    "BlackBerry".into()
                } else {
                    "Safari".into()
                }
            }
            Kind::Gecko => ["PaleMoon", "Firefox", "Camino", "Iceweasel", "Seamonkey"]
                .iter()
                .find(|b| self.detect(b).is_some())
                .map_or_else(|| self.base_browser(), |b| (*b).to_owned()),
            Kind::WindowsMediaPlayer => "Windows Media Player".into(),
            Kind::AppleCoreMedia => "AppleCoreMedia".into(),
            Kind::Libavformat => "libavformat".into(),
            Kind::Other => self.base_browser(),
        }
    }

    fn base_browser(&self) -> String {
        self.base_application()
            .map(|a| a.product.clone())
            .unwrap_or_default()
    }

    fn playstation_browser(&self) -> Option<String> {
        let first = self.app_comment()?.first()?;
        if first.contains("PLAYSTATION 3") {
            Some("PS3 Internet Browser".into())
        } else if self.last().is_some_and(|t| t.product == "Silk") {
            Some("Silk".into())
        } else if first.contains("PlayStation 4") {
            Some("PS4 Internet Browser".into())
        } else {
            None
        }
    }

    fn version(&self) -> Version {
        match self.kind {
            Kind::Edge | Kind::Vivaldi => {
                self.last().map(|t| t.version.clone()).unwrap_or_default()
            }
            Kind::InternetExplorer => {
                let joined = self.app_comment().map(|c| c.join("; ")).unwrap_or_default();
                Version::new(
                    IE_VERSION
                        .captures(&joined)
                        .map_or("", |c| c.get(2).map_or("", |m| m.as_str())),
                )
            }
            Kind::Opera => {
                let mini = self
                    .base_application()
                    .and_then(|a| a.comment.as_ref())
                    .and_then(|c| c.iter().find(|s| s.contains("Opera Mini")));
                if let Some(mini) = mini {
                    Version::new(
                        OPERA_MINI
                            .captures(mini)
                            .and_then(|c| c.get(1))
                            .map_or("", |m| m.as_str()),
                    )
                } else if let Some(v) = self.detect("Version") {
                    v.version.clone()
                } else if let Some(v) = self.detect("OPR") {
                    v.version.clone()
                } else {
                    self.base_version()
                }
            }
            Kind::Wechat => self
                .detect("MicroMessenger")
                .map(|t| t.version.clone())
                .unwrap_or_default(),
            Kind::Chrome => {
                // `detect_product("CriOs")` is case-insensitive.
                self.detect("CriOS")
                    .or_else(|| self.detect("Chrome"))
                    .map(|t| t.version.clone())
                    .unwrap_or_default()
            }
            Kind::ITunes => self
                .detect("iTunes")
                .map(|t| t.version.clone())
                .unwrap_or_default(),
            Kind::Webkit => {
                let ios = self
                    .webkit_os()
                    .and_then(|os| IOS_OS.captures(&os).map(|c| c[1].to_owned()));
                if let Some(v) = self.detect("Version") {
                    v.version.clone()
                } else if let (Some(ios), "Safari") = (ios, self.browser().as_str()) {
                    Version::new(&ios.replace('_', "."))
                } else {
                    Version::new(webkit_build_version(&self.webkit_build()).unwrap_or(""))
                }
            }
            Kind::Gecko => {
                let browser = self.browser();
                match self.detect(&browser) {
                    Some(t) if !t.version.is_nil() => t.version.clone(),
                    _ => self.base_version(),
                }
            }
            Kind::PlayStation => self.playstation_version(),
            Kind::PodcastAddict => Version::default(),
            Kind::Libavformat if self.detect("NSPlayer").is_some() => Version::default(),
            _ => self.base_version(),
        }
    }

    fn playstation_version(&self) -> Version {
        if self.browser() == "Silk" {
            return self.last().map(|t| t.version.clone()).unwrap_or_default();
        }
        let os = self.app_comment().map(|c| c.join(" ")).unwrap_or_default();
        ["PLAYSTATION 3", "PlayStation 4", "PlayStation Vita"]
            .iter()
            .find(|p| os.contains(*p))
            .map_or_else(Version::default, |p| {
                Version::new(os.rsplit(&format!("{p} ")).next().unwrap_or_default())
            })
    }

    fn base_version(&self) -> Version {
        self.base_application()
            .map(|a| a.version.clone())
            .unwrap_or_default()
    }

    fn webkit_build(&self) -> String {
        if let Some(t) = self
            .tokens
            .iter()
            .find(|t| WEBKIT_PRODUCT.is_match(&t.product))
        {
            return t.version.as_str().to_owned();
        }
        self.tokens
            .iter()
            .filter_map(|t| t.comment.as_ref())
            .flatten()
            .find_map(|c| WEBKIT_VERSION.captures(c).map(|m| m["version"].to_owned()))
            .unwrap_or_default()
    }

    /// `Base#bot?`: no application, a comment matching /bot/i, `Chrome-Lighthouse`, or an
    /// application product containing "bot".
    fn bot(&self) -> bool {
        let Some(app) = self.application() else {
            return true;
        };
        self.tokens
            .iter()
            .any(|t| t.comment_any(|c| BOT.is_match(c)))
            || self.detect("Chrome-Lighthouse").is_some()
            || app.product.contains("bot")
    }
}

/// Ruby's `String#strip` characters: ASCII whitespace and NUL.
fn ruby_space(c: char) -> bool {
    matches!(c, '\0' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | ' ')
}

fn normalize_ios(os: &str) -> Option<String> {
    IOS_VERSION
        .captures(os)
        .map(|c| format!("iOS {}", c[1].replace('_', ".")))
}

/// `Webkit::BuildVersions`.
fn webkit_build_version(build: &str) -> Option<&'static str> {
    Some(match build {
        "85.7" => "1.0",
        "85.8.5" | "85.8.2" => "1.0.3",
        "124" => "1.2",
        "125.2" => "1.2.2",
        "125.4" => "1.2.3",
        "125.5.5" | "125.5.6" | "125.5.7" => "1.2.4",
        "312.1.1" | "312.1" => "1.3",
        "312.5" | "312.5.1" | "312.5.2" => "1.3.1",
        "312.8" | "312.8.1" => "1.3.2",
        "412" | "412.6" | "412.6.2" => "2.0",
        "412.7" => "2.0.1",
        "416.11" | "416.12" => "2.0.2",
        "417.9" | "418" => "2.0.3",
        "418.8" | "418.9" | "418.9.1" | "419" => "2.0.4",
        "425.13" => "2.2",
        "534.52.7" => "5.1.2",
        _ => return None,
    })
}

/// `UserAgent::Version`: comparable when it starts with digits; compared on up to six
/// segments, numbers against numbers, a trailing word below any number.
#[derive(Debug, Clone, Default)]
struct Version {
    raw: String,
    segments: Vec<Segment>,
    comparable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Num(u64),
    Word(String),
}

static DIGITS_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+$|^\d+\.").expect("valid regex"));
static SEGMENTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d+|[A-Za-z][0-9A-Za-z-]*$").expect("valid regex"));

impl Version {
    fn new(raw: &str) -> Self {
        if raw.trim().is_empty() {
            return Self {
                raw: raw.to_owned(),
                ..Self::default()
            };
        }
        // Ruby's `=~ /^…/` matches at the start of any line.
        let comparable = raw.lines().any(|line| DIGITS_START.is_match(line));
        let segments = if comparable {
            SEGMENTS
                .find_iter(raw)
                .map(|m| {
                    m.as_str()
                        .parse::<u64>()
                        .map_or_else(|_| Segment::Word(m.as_str().to_owned()), Segment::Num)
                })
                .collect()
        } else {
            vec![Segment::Word(raw.to_owned())]
        };
        Self {
            raw: raw.to_owned(),
            segments,
            comparable,
        }
    }

    fn is_nil(&self) -> bool {
        self.raw.trim().is_empty()
    }

    fn as_str(&self) -> &str {
        &self.raw
    }

    /// `Version#<=>`.
    fn cmp_version(&self, other: &Self) -> Ordering {
        if !self.comparable {
            return if self.raw == other.raw {
                Ordering::Equal
            } else {
                Ordering::Less
            };
        }
        let zero = Segment::Num(0);
        for i in 0..6 {
            let a = self.segments.get(i).unwrap_or(&zero);
            let b = other.segments.get(i).unwrap_or(&zero);
            let ord = match (a, b) {
                (Segment::Word(_), Segment::Num(_)) => Ordering::Less,
                (Segment::Num(_), Segment::Word(_)) => Ordering::Greater,
                (Segment::Num(x), Segment::Num(y)) => x.cmp(y),
                (Segment::Word(x), Segment::Word(y)) => x.cmp(y),
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of the fixture was produced by the Rails kit's own `BrowserBlocker`.
    #[test]
    fn matches_rails_on_every_fixture_user_agent() {
        let fixture = include_str!("../../tests/fixtures/allow_browser.tsv");
        let mut checked = 0;
        for line in fixture.lines().filter(|l| !l.starts_with('#')) {
            let cols: Vec<&str> = line.splitn(4, '\t').collect();
            let [verdict, browser, version, ua] = cols[..] else {
                panic!("bad fixture line: {line}");
            };
            let agent = Agent::parse(ua);
            assert_eq!(agent.browser(), browser, "browser of {ua}");
            assert_eq!(agent.version().as_str(), version, "version of {ua}");
            assert_eq!(blocked(Some(ua)), verdict == "BLOCK", "verdict for {ua}");
            checked += 1;
        }
        assert!(checked >= 60, "fixture has {checked} rows");
    }

    #[test]
    fn a_missing_or_blank_user_agent_is_allowed() {
        assert!(!blocked(None));
        assert!(!blocked(Some("")));
        assert!(!blocked(Some("   ")));
    }
}
