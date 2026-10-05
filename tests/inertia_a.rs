//! Agent A's Inertia half: render (JSON vs HTML), props and partial reloads,
//! page metadata, shared props, script escaping, asset-version 409, SSR.
//! Uses a tiny axum Router, not the app's controllers.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{to_bytes, Body},
    extract::{Extension, Request},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use inertia_rust_starter_kit::inertia::{
    self,
    config::Settings,
    flash::{FlashConsumed, FlashState, IncomingFlash},
    headers::CspNonce,
    lazy, optional, Inertia, Prop, Props, ScrollMetadata, SharedProps, SharedPropsFn,
};
use loco_rs::app::AppContext;
use serde_json::{json, Value};
use serial_test::serial;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const MANIFEST: &str = r#"{
  "frontend/entrypoints/inertia.tsx": {"file": "assets/inertia-abc.js", "isEntry": true},
  "frontend/entrypoints/application.css": {"file": "assets/application-xyz.css", "isEntry": true}
}"#;

/// Settings with a real manifest file (unique per test so the vite cache
/// cannot leak between tests) and SSR pointed at `ssr_url` when given.
fn settings(test: &str, ssr_url: Option<&str>) -> Arc<Settings> {
    let dir = std::env::temp_dir().join(format!("inertia-a-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = dir.join("manifest.json");
    std::fs::write(&manifest, MANIFEST).unwrap();
    Arc::new(
        serde_json::from_value(json!({
            "secret_key_base": "x".repeat(64),
            "app_url": "https://app.example.com",
            "app_name": "Kit",
            "mail_from": "from@example.com",
            "encrypt_history": true,
            "forgery_protection": false,
            "vite": {"dev_server": false, "dev_server_url": "http://localhost:5173",
                     "manifest_path": manifest.to_str().unwrap()},
            "ssr": {"enabled": ssr_url.is_some(), "spawn": false, "bundle": "ssr/ssr.js",
                    "timeout_ms": 500, "url": ssr_url.unwrap_or("http://127.0.0.1:1/render")}
        }))
        .unwrap(),
    )
}

fn version() -> String {
    hex::encode(Sha256::digest(MANIFEST.as_bytes()))
}

async fn ctx_with(settings: Arc<Settings>) -> AppContext {
    let ctx = loco_rs::tests_cfg::app::get_app_context().await;
    ctx.shared_store.insert(settings);
    ctx
}

/// Counts evaluations of the lazy `expensive` prop (one per test router).
#[derive(Clone, Default)]
struct Calls(Arc<AtomicUsize>);

async fn feed(Extension(calls): Extension<Calls>, inertia: Inertia) -> loco_rs::Result<Response> {
    let props = Props::new()
        .prop("title", "Feed")
        .prop(
            "user",
            json!({"id": 1, "name": "Ada", "email": "ada@example.com"}),
        )
        .prop(
            "expensive",
            lazy(move || async move {
                calls.0.fetch_add(1, Ordering::SeqCst);
                Ok(42)
            }),
        )
        .prop("filters", optional(|| async { Ok(json!(["a"])) }))
        .prop("comments", inertia::defer(|| async { Ok(json!([1, 2])) }))
        .prop(
            "stats",
            inertia::defer(|| async { Ok(json!({"n": 1})) }).group("sidebar"),
        )
        .prop(
            "posts",
            inertia::merge(|| async { Ok(json!([{"id": 1}])) }).match_on("id"),
        )
        .prop("feed", Prop::value(json!({"data": [1]})).append_at("data"))
        .prop(
            "tree",
            inertia::deep_merge(|| async { Ok(json!({"a": {"b": 1}})) }),
        )
        .prop("recent", Prop::value(json!([3])).prepend())
        .prop(
            "plans",
            inertia::once(|| async { Ok(json!(["free", "pro"])) }).expires_at(1_900_000_000_000),
        )
        .prop(
            "countries",
            Prop::value(json!(["NZ"])).once_key("countries-v1"),
        )
        .prop(
            "users",
            inertia::scroll(ScrollMetadata::new("page", None, Some(2), 1), || async {
                Ok(json!({"data": [{"id": 1}]}))
            })
            .wrapper("data"),
        )
        .prop("nested.deep.value", "x")
        .prop("xss", "</script><script>alert(1)</script>\u{2028}");
    inertia.render("Feed/Index", props).await
}

async fn plain(inertia: Inertia) -> loco_rs::Result<Response> {
    inertia
        .render(
            "Plain",
            Props::new().prop("auth", json!({"override": true})),
        )
        .await
}

fn shared_props() -> SharedProps {
    let f: SharedPropsFn = Arc::new(|_parts, _ctx| {
        Box::pin(async {
            Ok(Props::new()
                .prop("auth", json!({"user": null}))
                .prop("app.name", "Kit"))
        })
    });
    SharedProps(f)
}

async fn app(settings: Arc<Settings>) -> Router {
    app_counting(settings, Calls::default()).await
}

async fn app_counting(settings: Arc<Settings>, calls: Calls) -> Router {
    let ctx = ctx_with(settings.clone()).await;
    ctx.shared_store.insert(shared_props());
    let router = Router::new()
        .route("/feed", get(feed))
        .route("/plain", get(plain))
        .route("/api", get(|| async { Json(json!({"ok": true})) }))
        .layer(Extension(calls))
        .with_state(ctx);
    let router = inertia::version::layer(router, settings);
    // Stand-ins for agent B's layers: a nonce and an incoming flash.
    router.layer(middleware::from_fn(
        |mut req: Request, next: Next| async move {
            req.extensions_mut().insert(CspNonce("test-nonce".into()));
            let flash = match req.headers().get("x-test-flash").map(|v| v.as_bytes()) {
                Some(b"errors") => FlashState {
                    notice: Some("Saved".into()),
                    errors: Some(json!({"email": ["is invalid"]})),
                    clear_history: true,
                    preserve_fragment: true,
                    ..FlashState::default()
                },
                _ => FlashState::default(),
            };
            req.extensions_mut().insert(IncomingFlash(Arc::new(flash)));
            next.run(req).await
        },
    ))
}

fn inertia_get(uri: &str) -> axum::http::request::Builder {
    Request::get(uri)
        .header("x-inertia", "true")
        .header("x-inertia-version", version())
}

async fn send(router: &Router, req: axum::http::request::Builder) -> Response {
    router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body(res: Response) -> String {
    String::from_utf8(to_bytes(res.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap()
}

async fn json_page(res: Response) -> Value {
    serde_json::from_str(&body(res).await).unwrap()
}

#[tokio::test]
async fn inertia_visit_returns_json_page_with_metadata() {
    let router = app(settings("json", None)).await;
    let res = send(&router, inertia_get("/feed?page=1")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-inertia"], "true");
    assert_eq!(res.headers()[header::VARY], "X-Inertia");
    assert!(res.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("application/json"));
    assert!(res.extensions().get::<FlashConsumed>().is_some());
    let page = json_page(res).await;

    assert_eq!(page["component"], "Feed/Index");
    assert_eq!(page["url"], "/feed?page=1");
    assert_eq!(page["version"], version());
    assert_eq!(page["encryptHistory"], true);
    assert_eq!(page["clearHistory"], false);
    assert!(page.get("flash").is_none());
    assert!(page.get("preserveFragment").is_none());
    // Like inertia_shared_data: the errors hash is shared, and listed first.
    assert_eq!(page["sharedProps"], json!(["errors", "auth", "app"]));

    let props = &page["props"];
    assert_eq!(props["errors"], json!({}));
    assert_eq!(props["auth"], json!({"user": null}));
    assert_eq!(props["app"], json!({"name": "Kit"}));
    assert_eq!(props["nested"], json!({"deep": {"value": "x"}}));
    assert_eq!(props["expensive"], 42);
    for absent in ["filters", "comments", "stats"] {
        assert!(
            props.get(absent).is_none(),
            "{absent} is not sent on first load"
        );
    }

    assert_eq!(
        page["deferredProps"],
        json!({"default": ["comments"], "sidebar": ["stats"]})
    );
    assert_eq!(
        page["mergeProps"],
        json!(["posts", "feed.data", "users.data"])
    );
    assert_eq!(page["prependProps"], json!(["recent"]));
    assert_eq!(page["deepMergeProps"], json!(["tree"]));
    assert_eq!(page["matchPropsOn"], json!(["posts.id"]));
    assert_eq!(
        page["onceProps"],
        json!({"plans": {"prop": "plans", "expiresAt": 1_900_000_000_000_i64},
               "countries-v1": {"prop": "countries"}})
    );
    assert_eq!(
        page["scrollProps"],
        json!({"users": {"pageName": "page", "previousPage": null, "nextPage": 2,
                         "currentPage": 1, "reset": false}})
    );
}

#[tokio::test]
async fn partial_reload_only_and_except_with_dot_paths() {
    let calls = Calls::default();
    let router = app_counting(settings("partial", None), calls.clone()).await;
    let res = send(
        &router,
        inertia_get("/feed")
            .header("x-inertia-partial-component", "Feed/Index")
            .header("x-inertia-partial-data", "user.name,comments,filters"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(
        page["props"],
        json!({"errors": {}, "user": {"name": "Ada"}, "comments": [1, 2], "filters": ["a"]})
    );
    assert_eq!(
        calls.0.load(Ordering::SeqCst),
        0,
        "a lazy prop that is not kept is never evaluated"
    );
    // Deferred metadata is not repeated on partial reloads.
    assert!(page.get("deferredProps").is_none());

    let res = send(
        &router,
        inertia_get("/feed")
            .header("x-inertia-partial-component", "Feed/Index")
            .header(
                "x-inertia-partial-except",
                "user.email,xss,feed,posts,users",
            ),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["props"]["user"], json!({"id": 1, "name": "Ada"}));
    assert!(page["props"].get("xss").is_none());
    assert_eq!(page["props"]["stats"], json!({"n": 1}));
    assert_eq!(
        page["mergeProps"],
        json!(null),
        "excluded merge props drop their metadata"
    );

    // A different partial component is treated as a full visit.
    let res = send(
        &router,
        inertia_get("/feed")
            .header("x-inertia-partial-component", "Other")
            .header("x-inertia-partial-data", "title"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["props"]["expensive"], 42);
    assert!(page["props"].get("comments").is_none());
    assert_eq!(
        calls.0.load(Ordering::SeqCst),
        2,
        "kept on the except reload and the full visit"
    );
}

#[tokio::test]
async fn reset_once_cache_and_scroll_intent() {
    let router = app(settings("reset", None)).await;
    let res = send(
        &router,
        inertia_get("/feed")
            .header("x-inertia-reset", "posts,users")
            .header("x-inertia-except-once-props", "plans,countries-v1")
            .header("x-inertia-infinite-scroll-merge-intent", "prepend"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["mergeProps"], json!(["feed.data"]));
    assert!(page.get("matchPropsOn").is_none());
    assert_eq!(page["scrollProps"]["users"]["reset"], true);
    assert!(
        page["props"].get("plans").is_none(),
        "cached once prop is not resent"
    );
    assert!(page["props"].get("countries").is_none());
    assert!(
        page["onceProps"]["plans"].is_object(),
        "once metadata is still sent"
    );

    // Not resetting: prepend intent moves the scroll wrapper to prependProps.
    let res = send(
        &router,
        inertia_get("/feed").header("x-inertia-infinite-scroll-merge-intent", "prepend"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["prependProps"], json!(["recent", "users.data"]));

    // Explicitly requesting a cached once prop sends it anyway.
    let res = send(
        &router,
        inertia_get("/feed")
            .header("x-inertia-partial-component", "Feed/Index")
            .header("x-inertia-partial-data", "plans")
            .header("x-inertia-except-once-props", "plans"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["props"]["plans"], json!(["free", "pro"]));
}

#[tokio::test]
async fn page_props_override_shared_and_flash_errors_surface() {
    let router = app(settings("flash", None)).await;
    let res = send(
        &router,
        inertia_get("/plain").header("x-test-flash", "errors"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["props"]["auth"], json!({"override": true}));
    assert_eq!(page["props"]["errors"], json!({"email": ["is invalid"]}));
    assert_eq!(page["flash"], json!({"notice": "Saved"}));
    assert_eq!(page["clearHistory"], true);
    assert_eq!(page["preserveFragment"], true);

    let res = send(
        &router,
        inertia_get("/plain")
            .header("x-test-flash", "errors")
            .header("x-inertia-error-bag", "signup"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(
        page["props"]["errors"],
        json!({"signup": {"email": ["is invalid"]}})
    );

    // Errors are always included, even when a partial reload excludes them.
    let res = send(
        &router,
        inertia_get("/plain")
            .header("x-test-flash", "errors")
            .header("x-inertia-partial-component", "Plain")
            .header("x-inertia-partial-data", "auth"),
    )
    .await;
    let page = json_page(res).await;
    assert_eq!(page["props"]["errors"], json!({"email": ["is invalid"]}));
}

#[tokio::test]
async fn first_visit_is_html_with_escaped_page_script_and_nonce() {
    let router = app(settings("html", None)).await;
    let res = send(&router, Request::get("/feed")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::VARY], "X-Inertia");
    assert!(res.headers().get("x-inertia").is_none());
    assert!(res.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    let html = body(res).await;
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<title data-inertia>Kit</title>"));
    assert!(html.contains(r#"<script data-page="app" type="application/json" nonce="test-nonce">"#));
    assert!(html.contains(r#"<div id="app"></div>"#));
    assert!(
        html.contains(r#"<script nonce="test-nonce">"#),
        "theme script carries the nonce"
    );
    assert!(html.contains(r#"href="/vite/assets/application-xyz.css""#));
    assert!(html.contains(r#"src="/vite/assets/inertia-abc.js""#));
    assert!(
        !html.contains("</script><script>alert(1)"),
        "page JSON cannot close its script"
    );
    assert!(
        html.contains(r"\u003c/script\u003e\u003cscript\u003ealert(1)\u003c/script\u003e\u2028")
    );

    let start = html.find(r#"nonce="test-nonce">{"#).unwrap() + r#"nonce="test-nonce">"#.len();
    let end = start + html[start..].find("</script>").unwrap();
    let page: Value = serde_json::from_str(&html[start..end]).unwrap();
    assert_eq!(
        page["props"]["xss"],
        "</script><script>alert(1)</script>\u{2028}"
    );
}

#[tokio::test]
async fn stale_version_on_inertia_get_is_409_with_full_location() {
    let router = app(settings("version", None)).await;
    let res = send(
        &router,
        Request::get("/feed?page=2")
            .header("x-inertia", "true")
            .header("x-inertia-version", "old"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        res.headers()["x-inertia-location"],
        "https://app.example.com/feed?page=2"
    );
    assert!(
        res.extensions().get::<FlashConsumed>().is_none(),
        "flash is kept on 409"
    );

    // A tab on an extra host reloads on that host; any other Host gets app_url.
    let mut extra = (*settings("version-extra", None)).clone();
    extra.extra_hosts = "two.app.example.com".into();
    let extra_router = app(Arc::new(extra)).await;
    for (host, location) in [
        ("two.app.example.com", "https://two.app.example.com/feed"),
        ("evil.example.com", "https://app.example.com/feed"),
    ] {
        let res = send(
            &extra_router,
            Request::get("/feed")
                .header("host", host)
                .header("x-inertia", "true")
                .header("x-inertia-version", "old"),
        )
        .await;
        assert_eq!(res.headers()["x-inertia-location"], location, "{host}");
    }

    // Non-Inertia requests and matching versions pass through.
    let res = send(
        &router,
        Request::get("/api").header("x-inertia-version", "old"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(&router, inertia_get("/feed")).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
#[serial] // shares the SSR warn! callsite with the log-capture test
async fn ssr_down_falls_back_to_client_rendering() {
    let router = app(settings("ssr-down", Some("http://127.0.0.1:1/render"))).await;
    let res = send(&router, Request::get("/plain")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body(res).await;
    assert!(html.contains(r#"<script data-page="app" type="application/json""#));
    assert!(html.contains(r#"<div id="app"></div>"#));
}

#[tokio::test]
#[serial] // shares the SSR warn! callsite with the log-capture test
async fn ssr_output_is_inserted_as_is() {
    let seen = Arc::new(std::sync::Mutex::new(None::<Value>));
    let seen2 = seen.clone();
    let ssr = Router::new().route(
        "/render",
        axum::routing::post(move |Json(page): Json<Value>| {
            let seen = seen2.clone();
            async move {
                *seen.lock().unwrap() = Some(page);
                Json(json!({
                    "head": ["<title data-inertia>From SSR</title>"],
                    "body": "<script data-page=\"app\" type=\"application/json\">{}</script><div id=\"app\"><h1>SSR</h1></div>"
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, ssr).await.unwrap() });

    let url = format!("http://{addr}/render");
    let router = app(settings("ssr-up", Some(&url))).await;
    let html = body(send(&router, Request::get("/plain")).await).await;
    assert!(html.contains(r#"<div id="app"><h1>SSR</h1></div>"#));
    assert_eq!(
        html.matches("data-page=").count(),
        1,
        "no second page script"
    );
    assert_eq!(html.matches("<title").count(), 1);
    assert!(html.contains("From SSR"));
    let posted = seen.lock().unwrap().clone().unwrap();
    assert_eq!(posted["component"], "Plain");

    // Inertia (JSON) visits never call the SSR server.
    *seen.lock().unwrap() = None;
    let _ = send(&router, inertia_get("/plain")).await;
    assert!(seen.lock().unwrap().is_none());
}

// ---------------------------------------------------------------- #20 sharedProps

#[tokio::test]
async fn shared_props_list_errors_even_when_the_page_overrides_them() {
    let router = app(settings("shared-errors", None)).await;
    let page = json_page(
        send(
            &router,
            inertia_get("/plain").header("x-test-flash", "errors"),
        )
        .await,
    )
    .await;
    assert_eq!(page["sharedProps"], json!(["errors", "auth", "app"]));
    assert_eq!(page["props"]["errors"], json!({"email": ["is invalid"]}));
}

// ---------------------------------------------------------------- #21 server head

async fn meta_page(inertia: Inertia) -> loco_rs::Result<Response> {
    inertia
        .meta(
            inertia::InertiaMeta::new().title("Dashboard").tag(
                inertia::MetaTag::new()
                    .attr("name", "description")
                    .attr("content", "Hi <there>"),
            ),
        )
        .render("Meta", Props::new().prop("x", 1i64))
        .await
}

async fn reserved_head(inertia: Inertia) -> loco_rs::Result<Response> {
    inertia
        .render("Meta", Props::new().prop("head", "mine"))
        .await
}

async fn meta_app(settings: Arc<Settings>, template: bool) -> Router {
    let ctx = ctx_with(settings.clone()).await;
    if template {
        let t: Arc<inertia::meta::TitleTemplate> =
            Arc::new(|title| title.map(|t| format!("{t} | Kit")));
        ctx.shared_store.insert(inertia::MetaTitleTemplate(t));
    }
    Router::new()
        .route("/meta", get(meta_page))
        .route("/reserved", get(reserved_head))
        .with_state(ctx)
}

fn with_server_head(value: Value) -> Arc<Settings> {
    let mut s = (*settings("meta-sh", None)).clone();
    s.server_head = serde_json::from_value(value).unwrap();
    Arc::new(s)
}

#[tokio::test]
async fn meta_tags_default_to_objects_under_inertia_meta_and_render_in_the_head() {
    let router = meta_app(settings("meta", None), true).await;
    let page = json_page(send(&router, inertia_get("/meta")).await).await;
    assert_eq!(
        page["props"]["_inertia_meta"],
        json!([
            {"tagName": "title", "headKey": "title", "innerContent": "Dashboard | Kit"},
            {"tagName": "meta", "headKey": "meta-name-description",
             "name": "description", "content": "Hi <there>"}
        ])
    );
    let res = send(&router, Request::get("/meta")).await;
    let html = body(res).await;
    let head = &html[..html.find("</head>").unwrap()];
    assert!(
        head.contains(r#"<title inertia="title">Dashboard | Kit</title>"#),
        "{head}"
    );
    assert!(head.contains(
        r#"<meta name="description" content="Hi &lt;there&gt;" inertia="meta-name-description">"#
    ));
    assert_eq!(html.matches("<title").count(), 1, "default title dropped");
}

#[tokio::test]
async fn server_head_sends_html_strings_with_data_inertia_and_reserves_the_prop() {
    let router = meta_app(with_server_head(json!(true)), false).await;
    let page = json_page(send(&router, inertia_get("/meta")).await).await;
    assert_eq!(
        page["props"]["head"],
        json!([
            r#"<title data-inertia="title">Dashboard</title>"#,
            r#"<meta name="description" content="Hi &lt;there&gt;" data-inertia="meta-name-description">"#
        ])
    );
    assert!(page["props"].get("_inertia_meta").is_none());
    let html = body(send(&router, Request::get("/meta")).await).await;
    assert!(html.contains(r#"<title data-inertia="title">Dashboard</title>"#));

    let res = router
        .clone()
        .oneshot(
            Request::get("/reserved")
                .header("x-inertia", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "reserved `head` prop"
    );

    // A custom prop name frees `head`.
    let router = meta_app(with_server_head(json!("seo")), false).await;
    let page = json_page(send(&router, inertia_get("/reserved")).await).await;
    assert_eq!(page["props"]["head"], "mine");
    let page = json_page(send(&router, inertia_get("/meta")).await).await;
    assert_eq!(page["props"]["seo"].as_array().unwrap().len(), 2);
}

#[tokio::test]
#[serial] // shares the SSR warn! callsite with the log-capture test
async fn ssr_without_server_head_still_gets_the_server_tags_but_one_title() {
    // An SSR head holding only the page's own <title>, as the React adapter
    // returns when the client entry has no `serverHead` option.
    let ssr = Router::new().route(
        "/render",
        axum::routing::post(|| async {
            Json(json!({"head": ["<title data-inertia=\"\">SSR</title>"],
                        "body": "<div id=\"app\" data-server-rendered=\"true\"></div>"}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, ssr).await.unwrap() });
    let url = format!("http://{addr}/render");
    let router = meta_app(settings("meta-ssr", Some(&url)), false).await;
    let html = body(send(&router, Request::get("/meta")).await).await;
    assert!(html.contains("data-server-rendered"), "{html}");
    assert_eq!(
        html.matches("meta-name-description").count(),
        1,
        "the tag SSR did not render is written by the server: {html}"
    );
    assert_eq!(html.matches("<title").count(), 1, "{html}");
}

// ------------------------------------------- N4: server_head through the real SSR bundle

/// A `node` able to run the built bundle (Node 22 from mise when present).
fn node_bin() -> String {
    let mise = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".local/share/mise/installs/node/22.23.2/bin/node");
    if mise.exists() {
        mise.to_string_lossy().into_owned()
    } else {
        "node".to_owned()
    }
}

/// Builds the SSR bundle with `VITE_INERTIA_SERVER_HEAD=true` into a scratch
/// dir (never ssr/, which a running app may be using) and returns its path.
/// `None` (the test is skipped, loudly) when node_modules is missing.
fn build_server_head_bundle(name: &str) -> Option<std::path::PathBuf> {
    if !std::path::Path::new("node_modules/@inertiajs/vite").exists() {
        eprintln!("SKIPPED: node_modules missing (run npm ci)");
        return None;
    }
    let out =
        std::path::PathBuf::from("tmp/test-ssr").join(format!("{name}-{}", std::process::id()));
    let node = std::path::PathBuf::from(node_bin());
    let path = match node.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(dir) => format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
        None => std::env::var("PATH").unwrap_or_default(),
    };
    let status = std::process::Command::new("npx")
        .args(["vite", "build", "--ssr", "--logLevel", "error", "--outDir"])
        .arg(&out)
        .env("PATH", path)
        .env("VITE_INERTIA_SERVER_HEAD", "true")
        .status()
        .expect("npx runs");
    assert!(status.success(), "vite build --ssr failed");
    Some(out.join("ssr.js"))
}

/// Runs the bundle on a free port through the app's own spawner.
async fn start_bundle(bundle: &std::path::Path) -> (tokio::process::Child, String) {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut command = tokio::process::Command::new(node_bin());
    command
        .arg(bundle)
        .env(inertia::ssr::PORT_ENV, port.to_string());
    let (child, _output) = inertia::ssr::spawn_redacted(command).unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    for _ in 0..100 {
        if http.get(format!("{base}/health")).send().await.is_ok() {
            return (child, format!("{base}/render"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("SSR bundle never listened on {port}");
}

async fn home_page(inertia: Inertia) -> loco_rs::Result<Response> {
    inertia
        .meta(
            inertia::InertiaMeta::new().title("Server title").tag(
                inertia::MetaTag::new()
                    .attr("name", "description")
                    .attr("content", "From <the> server"),
            ),
        )
        .render(
            "home/index",
            Props::new().prop("auth", json!({"user": null})),
        )
        .await
}

#[tokio::test]
#[serial] // runs node; keeps the SSR warn! callsite away from the log-capture tests
async fn server_head_tags_come_back_from_the_real_ssr_bundle_exactly_once() {
    let Some(bundle) = build_server_head_bundle("server-head") else {
        return;
    };
    let (mut child, url) = start_bundle(&bundle).await;

    let mut s = (*settings("meta-real-ssr", Some(&url))).clone();
    s.server_head = serde_json::from_value(json!(true)).unwrap();
    s.ssr.timeout_ms = 5000;
    let ctx = ctx_with(Arc::new(s)).await;
    let router: Router = Router::new().route("/", get(home_page)).with_state(ctx);
    let html = body(send(&router, Request::get("/")).await).await;
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(bundle.parent().unwrap());

    assert!(
        html.contains("data-server-rendered=\"true\""),
        "SSR rendered: {html}"
    );
    let head = &html[..html.find("</head>").unwrap()];
    let description = r#"<meta name="description" content="From &lt;the&gt; server" data-inertia="meta-name-description">"#;
    assert_eq!(head.matches(description).count(), 1, "{head}");
    assert_eq!(head.matches("<title").count(), 1, "{head}");
    // Inertia's head manager collects the page's own <Head title="Welcome">
    // after the server head, so the page title wins (formatted by the title
    // callback) and the server <title> is not repeated.
    assert!(
        head.contains(">Welcome - Inertia Rust Starter Kit</title>"),
        "{head}"
    );
}

// ---------------------------------------------------------------- #1 log redaction

/// Captures formatted log output (a tracing-subscriber test writer).
#[derive(Clone, Default)]
struct LogBuf(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuf {
    type Writer = Self;
    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

const SID: &str = "eyJfcmFpbHMiOnsiZGF0YSI6MTIzfX0--c0ffee1234deadbeef5150";

#[tokio::test]
#[serial] // shares the SSR warn! callsite with the log-capture test
async fn request_logs_and_ssr_errors_never_contain_the_reset_sid() {
    // An SSR server that fails and echoes the page (with its URL) back,
    // like Inertia's SSR error response.
    let ssr = Router::new().route(
        "/render",
        axum::routing::post(|b: String| async move {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("render failed for {b}"),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, ssr).await.unwrap() });
    let url = format!("http://{addr}/render");
    let settings = settings("logs", Some(&url));

    // The app's real middleware stack (Loco's defaults with our logger).
    let ctx = ctx_with(settings).await;
    ctx.shared_store.insert(shared_props());
    let mut router: axum::Router<AppContext> = Router::new().route("/plain", get(plain));
    let stack = inertia::middlewares(&ctx);
    let logger = stack.iter().find(|m| m.name() == "logger").unwrap();
    assert!(
        logger.config().unwrap().get("config").is_none(),
        "Loco's logger (which nests `config`) was replaced"
    );
    for m in stack.iter().filter(|m| m.is_enabled()) {
        router = m.apply(router).unwrap();
    }
    let router = router.with_state(ctx);

    let logs = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let uri =
        format!("/plain?sid={SID}&page=2&password_confirmation=hunter2&reset_token=rt-secret");
    let res = send(&router, Request::get(uri.as_str())).await;
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "SSR failure falls back to CSR"
    );
    let html = body(res).await;
    assert!(html.contains(SID), "the page itself still carries the URL");

    let out = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        out.contains("http-request"),
        "request span was logged: {out}"
    );
    assert!(out.contains("sid=[FILTERED]"), "{out}");
    assert!(out.contains("page=2"), "{out}");
    assert!(out.contains("inertia SSR failed"), "{out}");
    assert!(out.contains("500"), "SSR status is logged: {out}");
    assert!(out.contains("component=Plain"), "{out}");
    for secret in [SID, "hunter2", "rt-secret", "render failed for"] {
        assert!(!out.contains(secret), "`{secret}` leaked into logs:\n{out}");
    }
}

// ------------------------------------------------ N1: Node SSR output is redacted

#[tokio::test]
#[serial] // captures the process-wide default subscriber
async fn ssr_child_output_reaches_tracing_with_secrets_redacted() {
    let logs = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let script = format!(
        r#"console.log("URL: /identity/password_reset/edit?sid={SID}&page=2");
           console.error("\x1b[2m  URL: http://app/x?token=tok-secret&reset_token=rt-secret\x1b[0m");
           process.stdout.write("no newline /y?password=hunter2");"#
    );
    let mut command = tokio::process::Command::new(node_bin());
    command.args(["-e", &script]);
    let (mut child, output) = inertia::ssr::spawn_redacted(command).unwrap();
    assert!(child.wait().await.unwrap().success());
    output.await.unwrap();

    let out = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(out.contains("sid=[FILTERED]&page=2"), "{out}");
    assert!(
        out.contains("token=[FILTERED]&reset_token=[FILTERED]"),
        "{out}"
    );
    assert!(out.contains("password=[FILTERED]"), "{out}");
    assert!(out.contains("inertia SSR server stderr"), "{out}");
    for secret in [SID, "tok-secret", "rt-secret", "hunter2"] {
        assert!(!out.contains(secret), "`{secret}` leaked:\n{out}");
    }
}

#[tokio::test]
#[serial] // captures the process-wide default subscriber; runs node
async fn a_real_ssr_render_error_logs_the_page_url_redacted() {
    let Some(bundle) = build_server_head_bundle("render-error") else {
        return;
    };
    let logs = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(logs.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let (mut child, url) = start_bundle(&bundle).await;
    // An unknown component makes @inertiajs/core's server print its
    // formatted error, including `URL: <page.url>`.
    let page = json!({"component": "missing/page", "props": {}, "version": "1",
                      "url": format!("/identity/password_reset/edit?sid={SID}")});
    let res = reqwest::Client::new()
        .post(&url)
        .json(&page)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 500);
    let mut out = String::new();
    for _ in 0..50 {
        out = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        if out.contains("URL: ") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(bundle.parent().unwrap());

    assert!(out.contains("SSR ERROR"), "Inertia's formatter ran: {out}");
    assert!(
        out.contains("/identity/password_reset/edit?sid=[FILTERED]"),
        "{out}"
    );
    assert!(!out.contains(SID), "sid leaked:\n{out}");
}

// ---------------------------------------------------------------- #12 public files

async fn public_app() -> Router {
    let ctx = ctx_with(settings("public", None)).await;
    inertia::base_router()
        .route("/robots.txt", axum::routing::post(|| async { "posted" }))
        .route("/icon.svg", get(|| async { "route wins" }))
        .with_state(ctx)
}

#[tokio::test]
async fn public_root_files_are_served_only_when_no_route_matched() {
    let router = public_app().await;
    let res = send(&router, Request::get("/robots.txt")).await;
    // A route on the path owns it (405 for other methods), like any axum route.
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let router = inertia::base_router().with_state(ctx_with(settings("public2", None)).await);
    let res = send(&router, Request::get("/robots.txt")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        inertia::public::CACHE_CONTROL
    );
    assert_eq!(
        body(res).await,
        std::fs::read_to_string("public/robots.txt").unwrap()
    );
    let router = public_app().await;

    let res = send(&router, Request::head("/icon.png")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/png");

    let res = send(&router, Request::get("/icon.svg")).await;
    assert_eq!(body(res).await, "route wins", "files never shadow routes");
    let res = send(&router, Request::post("/robots.txt")).await;
    assert_eq!(body(res).await, "posted");

    // Only GET/HEAD fall through to files.
    let res = send(&router, Request::delete("/icon.png")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = send(&router, Request::get("/no/such/page")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    assert_eq!(
        body(res).await,
        std::fs::read_to_string("public/404.html").unwrap()
    );
}

#[tokio::test]
async fn vite_assets_are_immutable_and_missing_ones_are_real_404s() {
    let dir = std::env::temp_dir().join(format!("inertia-public-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("vite/assets")).unwrap();
    std::fs::create_dir_all(dir.join("vite/.vite")).unwrap();
    std::fs::write(dir.join("vite/assets/app-abc.js"), "console.log(1)").unwrap();
    std::fs::write(dir.join("vite/.vite/manifest.json"), "{}").unwrap();
    std::fs::write(dir.join("404.html"), "gone").unwrap();
    let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();

    let res = inertia::public::serve(&dir, get("/vite/assets/app-abc.js")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        inertia::public::IMMUTABLE
    );

    for missing in [
        "/vite/assets/example.js",
        "/vite/.vite/manifest.json",
        "/vite/",
    ] {
        let res = inertia::public::serve(&dir, get(missing)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{missing}");
        assert_eq!(
            res.headers()[header::CACHE_CONTROL],
            "no-cache",
            "{missing}"
        );
        assert_eq!(body(res).await, "gone");
    }
    let res = inertia::public::serve(&dir, get("/../Cargo.toml")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hidden_files_stay_hidden_however_the_path_is_encoded() {
    let dir = std::env::temp_dir().join(format!("inertia-public-enc-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::create_dir_all(dir.join("vite/.vite")).unwrap();
    std::fs::create_dir_all(dir.join("vite/assets")).unwrap();
    std::fs::write(dir.join(".git/config"), "SECRET").unwrap();
    std::fs::write(dir.join(".env"), "SECRET").unwrap();
    std::fs::write(dir.join("vite/.vite/manifest.json"), "SECRET").unwrap();
    std::fs::write(dir.join("vite/assets/a b.js"), "ok").unwrap();
    std::fs::write(dir.join("404.html"), "gone").unwrap();
    let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();

    for hidden in [
        "/.git/config",
        "/%2egit/config",
        "/%2Egit/config",
        "/%2egit%2fconfig",
        "/%2egit%2Fconfig",
        "/%2egit%5cconfig",
        "/%2eenv",
        "/vite/.vite/manifest.json",
        "/vite/%2evite/manifest.json",
        "/vite%2f%2evite%2fmanifest.json",
        "/vite/%2e%2e/.git/config",
        "/vite/%2e%2e/%2egit/config",
        "/vite/%2e%2e%2f%2egit%2fconfig",
        "/vite/assets/%00a.js",
        "/vite/assets/%zz.js",
    ] {
        let res = inertia::public::serve(&dir, get(hidden)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{hidden}");
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache", "{hidden}");
        assert_eq!(body(res).await, "gone", "{hidden}");
    }

    // Ordinary percent-encoding still works, with the decoded path deciding caching.
    let res = inertia::public::serve(&dir, get("/vite/assets/a%20b.js")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, "ok");
    let res = inertia::public::serve(&dir, get("/%76ite/assets/a%20b.js")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        inertia::public::IMMUTABLE
    );
}

/// Rails routes `/sign_in/` like `/sign_in`: `inertia::service` (what `App::serve` runs)
/// trims the trailing slash before routing.
#[tokio::test]
async fn the_served_app_routes_a_trailing_slash_like_rails() {
    use tower::ServiceExt;
    let router: Router = Router::new().route("/sign_in", get(|| async { "sign in" }));
    let service = inertia::service(router);
    let res = service
        .clone()
        .oneshot(Request::get("/sign_in/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = service
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "the root path stays `/`"
    );
}
