//! Agent B's Inertia layers: flash, redirects, CSRF, security headers,
//! precognition, and the session cookie helpers.

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    extract::Extension,
    http::{header, HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use inertia_rust_starter_kit::inertia::{
    config::{self, Settings},
    cookies, csrf,
    flash::{self, FlashConsumed, FlashState, IncomingFlash, OutgoingFlash},
    headers::{self, CspNonce},
    precognition,
    redirect::{self, Redirect},
};
use serde_json::{json, Value};
use serial_test::serial;
use tower::ServiceExt;

fn settings_with(app_url: &str, forgery: bool, vite_dev: bool) -> Arc<Settings> {
    Arc::new(
        serde_json::from_value(json!({
            "secret_key_base": "test-secret-key-base-0123456789abcdef0123456789abcdef0123456789abcdef",
            "app_url": app_url,
            "app_name": "Test",
            "mail_from": "from@example.com",
            "encrypt_history": false,
            "forgery_protection": forgery,
            "vite": { "dev_server": vite_dev, "dev_server_url": "http://localhost:5173",
                      "manifest_path": "public/vite/.vite/manifest.json" },
            "ssr": { "enabled": false, "spawn": false, "bundle": "ssr/ssr.js", "timeout_ms": 1500 }
        }))
        .expect("settings fixture matches config::Settings"),
    )
}

fn settings() -> Arc<Settings> {
    settings_with("http://localhost:5150", true, false)
}

async fn send(app: &Router, req: Request<Body>) -> Response {
    app.clone().oneshot(req).await.unwrap()
}

async fn body_string(res: Response) -> String {
    String::from_utf8(to_bytes(res.into_body(), 1 << 20).await.unwrap().to_vec()).unwrap()
}

fn set_cookies(res: &Response) -> Vec<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect()
}

fn set_cookie<'a>(cookies: &'a [String], name: &str) -> Option<&'a String> {
    cookies.iter().find(|c| c.starts_with(&format!("{name}=")))
}

/// `name=value` pair from a Set-Cookie line, for replaying as a Cookie header.
fn pair(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().to_owned()
}

fn cookie_value(set_cookie: &str) -> String {
    pair(set_cookie).split_once('=').unwrap().1.to_owned()
}

// ---------------------------------------------------------------- flash

fn flash_app() -> Router {
    let router = Router::new()
        .route(
            "/set",
            post(|| async {
                Redirect::to("/show")
                    .notice("Saved")
                    .errors(json!({"email": ["is invalid"]}))
                    .error_bag("login")
                    .clear_history()
                    .preserve_fragment()
            }),
        )
        .route(
            "/show",
            get(
                |Extension(IncomingFlash(f)): Extension<IncomingFlash>| async move {
                    let mut res = Json((*f).clone()).into_response();
                    res.extensions_mut().insert(FlashConsumed);
                    res
                },
            ),
        )
        .route(
            "/bounce",
            get(|| async {
                let mut res = Redirect::to("/show").into_response();
                res.extensions_mut().insert(FlashConsumed);
                res
            }),
        )
        .route(
            "/stale",
            get(|| async {
                let mut res = StatusCode::CONFLICT.into_response();
                res.extensions_mut().insert(FlashConsumed);
                res
            }),
        )
        .route("/peek", get(|| async { "no render" }));
    flash::layer(router, settings())
}

async fn flash_cookie_after_set(app: &Router) -> String {
    let res = send(app, Request::post("/set").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(res.headers()[header::LOCATION], "/show");
    let cookies = set_cookies(&res);
    let flash = set_cookie(&cookies, "_flash")
        .expect("redirect writes _flash")
        .clone();
    assert!(
        flash.contains("HttpOnly") && flash.contains("SameSite=Lax") && flash.contains("Path=/")
    );
    assert!(
        !flash.contains("Saved"),
        "flash cookie must be encrypted: {flash}"
    );
    pair(&flash)
}

#[tokio::test]
async fn flash_roundtrips_through_the_encrypted_cookie_and_is_removed_after_render() {
    let app = flash_app();
    let cookie = flash_cookie_after_set(&app).await;

    let res = send(
        &app,
        Request::get("/show")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let removal = set_cookie(&set_cookies(&res), "_flash")
        .expect("render deletes _flash")
        .clone();
    assert!(
        removal.contains("Max-Age=0") || removal.contains("Expires"),
        "{removal}"
    );
    let got: FlashState = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(
        got,
        FlashState {
            notice: Some("Saved".into()),
            alert: None,
            errors: Some(json!({"email": ["is invalid"]})),
            error_bag: Some("login".into()),
            clear_history: true,
            preserve_fragment: true,
        }
    );
}

#[tokio::test]
async fn flash_survives_redirects_409s_and_non_rendering_responses() {
    let app = flash_app();
    let cookie = flash_cookie_after_set(&app).await;
    for path in ["/bounce", "/stale", "/peek"] {
        let res = send(
            &app,
            Request::get(path)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(
            set_cookie(&set_cookies(&res), "_flash").is_none(),
            "{path} must keep the flash cookie"
        );
    }
}

#[tokio::test]
async fn tampered_flash_cookie_is_ignored_and_deleted() {
    let app = flash_app();
    let res = send(
        &app,
        Request::get("/show")
            .header(header::COOKIE, "_flash=forged")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(set_cookie(&set_cookies(&res), "_flash").is_some());
    let got: FlashState = serde_json::from_str(&body_string(res).await).unwrap();
    assert!(got.is_empty());
}

#[tokio::test]
async fn outgoing_flash_from_a_handler_is_written() {
    let router = Router::new().route(
        "/",
        get(|| async {
            let mut res = "ok".into_response();
            res.extensions_mut().insert(OutgoingFlash(FlashState {
                alert: Some("Hi".into()),
                ..Default::default()
            }));
            res
        }),
    );
    let res = send(
        &flash::layer(router, settings()),
        Request::get("/").body(Body::empty()).unwrap(),
    )
    .await;
    assert!(set_cookie(&set_cookies(&res), "_flash").is_some());
}

#[test]
fn redirect_errors_accept_any_serialize() {
    #[derive(serde::Serialize)]
    struct Errors {
        name: Vec<&'static str>,
    }
    let r = Redirect::to("/x").errors(Errors {
        name: vec!["can't be blank"],
    });
    assert_eq!(r.flash().errors, Some(json!({"name": ["can't be blank"]})));
}

// ---------------------------------------------------------------- redirects

fn redirect_app() -> Router {
    let router = Router::new()
        .route(
            "/items",
            post(|| async { Redirect::to("/items") })
                .put(|| async { Redirect::to("/items") })
                .patch(|| async { Redirect::to("/items") })
                .delete(|| async { Redirect::to("/items") }),
        )
        .route(
            "/oauth",
            post(|| async {
                let mut res = (
                    StatusCode::FOUND,
                    [(header::LOCATION, "https://github.com/login/oauth")],
                    "body",
                )
                    .into_response();
                res.headers_mut()
                    .append(header::SET_COOKIE, "state=abc".parse().unwrap());
                res
            }),
        )
        .route(
            "/same-host",
            get(|| async { Redirect::to("http://localhost:5150/dashboard") }),
        )
        .route(
            "/request-host",
            get(|| async { Redirect::to("http://127.0.0.1:3000/x") }),
        )
        .route(
            "/other-port",
            get(|| async { Redirect::to("http://localhost:9999/x") }),
        )
        .route(
            "/other-scheme",
            get(|| async { Redirect::to("https://localhost:5150/x") }),
        )
        .route(
            "/loc",
            get(|headers: HeaderMap| async move {
                redirect::location(&headers, "https://stripe.com/pay")
            }),
        );
    redirect::layer(router, settings())
}

fn inertia(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("x-inertia", "true")
        .header(header::HOST, "localhost:5150")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn inertia_put_patch_delete_redirects_become_303_but_post_and_plain_requests_do_not() {
    let app = redirect_app();
    for m in [Method::PUT, Method::PATCH, Method::DELETE] {
        assert_eq!(
            send(&app, inertia(m.clone(), "/items")).await.status(),
            StatusCode::SEE_OTHER,
            "{m}"
        );
    }
    assert_eq!(
        send(&app, inertia(Method::POST, "/items")).await.status(),
        StatusCode::FOUND
    );
    let plain = Request::put("/items").body(Body::empty()).unwrap();
    assert_eq!(send(&app, plain).await.status(), StatusCode::FOUND);
}

#[tokio::test]
async fn external_redirect_on_inertia_request_becomes_409_with_location_keeping_cookies() {
    let app = redirect_app();
    let res = send(&app, inertia(Method::POST, "/oauth")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        res.headers()["x-inertia-location"],
        "https://github.com/login/oauth"
    );
    assert!(res.headers().get(header::LOCATION).is_none());
    assert_eq!(set_cookies(&res), vec!["state=abc".to_owned()]);
    assert_eq!(body_string(res).await, "");

    for path in ["/other-port", "/other-scheme"] {
        assert_eq!(
            send(&app, inertia(Method::GET, path)).await.status(),
            StatusCode::CONFLICT,
            "{path}"
        );
    }
    // Non-inertia requests are left alone.
    let plain = Request::post("/oauth").body(Body::empty()).unwrap();
    assert_eq!(send(&app, plain).await.status(), StatusCode::FOUND);
}

#[tokio::test]
async fn same_origin_absolute_redirects_stay_redirects() {
    let app = redirect_app();
    assert_eq!(
        send(&app, inertia(Method::GET, "/same-host"))
            .await
            .status(),
        StatusCode::FOUND
    );
    let mut req = inertia(Method::GET, "/request-host");
    req.headers_mut()
        .insert(header::HOST, "127.0.0.1:3000".parse().unwrap());
    assert_eq!(
        send(&app, req).await.status(),
        StatusCode::FOUND,
        "matches request Host"
    );
}

#[tokio::test]
async fn location_helper_is_409_for_inertia_and_302_otherwise() {
    let app = redirect_app();
    let res = send(&app, inertia(Method::GET, "/loc")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        res.headers()["x-inertia-location"],
        "https://stripe.com/pay"
    );
    let res = send(&app, Request::get("/loc").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(res.headers()[header::LOCATION], "https://stripe.com/pay");
}

#[test]
fn redirect_back_only_follows_same_origin_referers() {
    let mut h = HeaderMap::new();
    h.insert(header::HOST, "localhost:5150".parse().unwrap());
    assert_eq!(Redirect::back(&h, "/home").target(), "/home", "no referer");

    h.insert(
        header::REFERER,
        "http://localhost:5150/settings/profile?tab=a"
            .parse()
            .unwrap(),
    );
    assert_eq!(
        Redirect::back(&h, "/home").target(),
        "/settings/profile?tab=a"
    );

    h.insert(
        header::REFERER,
        "https://evil.example/phish".parse().unwrap(),
    );
    assert_eq!(Redirect::back(&h, "/home").target(), "/home");

    h.insert(header::REFERER, "http://localhost:6666/x".parse().unwrap());
    assert_eq!(
        Redirect::back(&h, "/home").target(),
        "/home",
        "different port"
    );

    h.insert(header::REFERER, "not a url".parse().unwrap());
    assert_eq!(Redirect::back(&h, "/home").target(), "/home");
}

#[test]
fn redirect_back_rejects_network_path_references_from_a_same_host_referer() {
    let mut h = HeaderMap::new();
    h.insert(header::HOST, "app.example".parse().unwrap());
    for referer in [
        "https://app.example//evil.example/path",
        "https://app.example//evil.example",
        "https://app.example/%5Cevil.example",
        "https://app.example/\\evil.example/x",
        "https://app.example/\\/evil.example",
    ] {
        h.insert(header::REFERER, referer.parse().unwrap());
        let target = Redirect::back(&h, "/home").target().to_owned();
        assert!(
            !target.starts_with("//") && !target.starts_with("/\\"),
            "{referer} -> {target}"
        );
        assert!(
            !redirect::is_external(&target, "https://app.example", Some("app.example")),
            "{referer} -> {target}"
        );
    }
    h.insert(
        header::REFERER,
        "https://app.example//evil.example/path".parse().unwrap(),
    );
    assert_eq!(Redirect::back(&h, "/home").target(), "/home");
    h.insert(
        header::REFERER,
        "https://app.example/a//b?x=//y".parse().unwrap(),
    );
    assert_eq!(
        Redirect::back(&h, "/home").target(),
        "/a//b?x=//y",
        "inner // is fine"
    );
}

#[test]
fn is_external_resolves_relative_locations_against_app_url() {
    let app = "https://app.example";
    let host = Some("app.example");
    for external in [
        "//evil.example/path",
        "/\\evil.example",
        "\\\\evil.example",
        "https://evil.example/",
        "http://app.example/",
        "https://app.example:8443/",
    ] {
        assert!(redirect::is_external(external, app, host), "{external}");
    }
    for internal in [
        "/settings",
        "settings",
        "?tab=a",
        "//app.example/x",
        "https://app.example/x",
    ] {
        assert!(!redirect::is_external(internal, app, host), "{internal}");
    }
    assert!(!redirect::is_local_path("//x") && !redirect::is_local_path("/\\x"));
    assert!(redirect::is_local_path("/x//y") && !redirect::is_local_path("x"));
}

#[tokio::test]
async fn scheme_relative_redirect_on_inertia_request_becomes_409() {
    let app = redirect::layer(
        Router::new().route("/r", get(|| async { Redirect::to("//evil.example/phish") })),
        settings_with("https://app.example", false, false),
    );
    let res = send(
        &app,
        Request::get("/r")
            .header("x-inertia", "true")
            .header(header::HOST, "app.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(res.headers()["x-inertia-location"], "//evil.example/phish");
}

/// A route answering `status` with `Location: /article#section`, a cookie and a flash.
fn fragment_app() -> Router {
    let router = Router::new().route(
        "/{status}",
        get(
            |axum::extract::Path(status): axum::extract::Path<u16>| async move {
                let mut res = Redirect::to("/article#section")
                    .notice("Saved")
                    .into_response();
                *res.status_mut() = StatusCode::from_u16(status).unwrap();
                res.headers_mut()
                    .append(header::SET_COOKIE, "state=abc".parse().unwrap());
                res
            },
        )
        .post(|| async { Redirect::to("/article#section") }),
    );
    flash::layer(redirect::layer(router, settings()), settings())
}

// inertia-laravel MiddlewareTest: test_redirect_with_hash_fragment_*.
#[tokio::test]
async fn inertia_redirect_to_a_fragment_becomes_409_with_x_inertia_redirect() {
    let app = fragment_app();
    for status in [201, 301, 302, 303, 307, 308] {
        let res = send(&app, inertia(Method::GET, &format!("/{status}"))).await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{status}");
        assert_eq!(res.headers()["x-inertia-redirect"], "/article#section");
        assert!(res.headers().get(header::LOCATION).is_none());
        let cookies = set_cookies(&res);
        assert!(cookies.contains(&"state=abc".to_owned()), "{cookies:?}");
        assert!(
            set_cookie(&cookies, "_flash").is_some(),
            "the flash still rides to the next visit"
        );
        assert_eq!(body_string(res).await, "");
    }
    let res = send(&app, inertia(Method::POST, "/302")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(res.headers()["x-inertia-redirect"], "/article#section");
}

#[tokio::test]
async fn fragment_redirects_stay_redirects_for_prefetches_and_plain_requests() {
    let app = fragment_app();
    for (name, value) in [
        ("purpose", "prefetch"),
        ("sec-purpose", "Prefetch"),
        ("x-moz", "PREFETCH"),
    ] {
        let mut req = inertia(Method::GET, "/302");
        req.headers_mut().insert(name, value.parse().unwrap());
        let res = send(&app, req).await;
        assert_eq!(res.status(), StatusCode::FOUND, "{name}: {value}");
        assert_eq!(res.headers()[header::LOCATION], "/article#section");
    }
    let plain = Request::get("/302").body(Body::empty()).unwrap();
    let res = send(&app, plain).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(res.headers()[header::LOCATION], "/article#section");
    // Without a fragment nothing changes.
    let res = send(&redirect_app(), inertia(Method::POST, "/items")).await;
    assert_eq!(res.status(), StatusCode::FOUND);
}

#[test]
fn prefetch_is_read_from_purpose_sec_purpose_or_x_moz_in_any_case() {
    let headers = |name: &'static str, value: &'static str| {
        let mut h = HeaderMap::new();
        h.insert(name, value.parse().unwrap());
        h
    };
    assert!(redirect::is_prefetch(&headers("purpose", "prefetch")));
    assert!(redirect::is_prefetch(&headers("purpose", "Prefetch")));
    assert!(redirect::is_prefetch(&headers("sec-purpose", "prefetch")));
    assert!(redirect::is_prefetch(&headers("x-moz", "PREFETCH")));
    assert!(!redirect::is_prefetch(&headers("purpose", "prerender")));
    assert!(!redirect::is_prefetch(&HeaderMap::new()));
}

// ---------------------------------------------------------------- CSRF

fn csrf_app(settings: Arc<Settings>) -> Router {
    let router = Router::new()
        .route("/", get(|| async { "page" }).post(|| async { "written" }))
        .route(
            "/sign_in",
            post(|| async {
                let mut res = "signed in".into_response();
                res.extensions_mut().insert(csrf::RotateCsrf);
                res
            }),
        );
    csrf::layer(router, settings)
}

/// GET `/` with no cookies; returns (cookie header to replay, xsrf token).
async fn csrf_bootstrap(app: &Router) -> (String, String) {
    let res = send(app, Request::get("/").body(Body::empty()).unwrap()).await;
    let cookies = set_cookies(&res);
    let secret = set_cookie(&cookies, "_csrf").expect("_csrf set").clone();
    let xsrf = set_cookie(&cookies, "XSRF-TOKEN")
        .expect("XSRF-TOKEN set")
        .clone();
    (
        format!("{}; {}", pair(&secret), pair(&xsrf)),
        cookie_value(&xsrf),
    )
}

fn csrf_post(path: &str, cookie: &str, token: Option<&str>) -> axum::http::request::Builder {
    let mut b = Request::post(path).header(header::COOKIE, cookie);
    if let Some(t) = token {
        b = b.header("x-xsrf-token", t);
    }
    b
}

#[tokio::test]
async fn csrf_cookie_flags() {
    let app = csrf_app(settings());
    let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
    let cookies = set_cookies(&res);
    let secret = set_cookie(&cookies, "_csrf").unwrap();
    let xsrf = set_cookie(&cookies, "XSRF-TOKEN").unwrap();
    assert!(
        secret.contains("HttpOnly") && secret.contains("SameSite=Lax") && secret.contains("Path=/")
    );
    assert!(
        !xsrf.contains("HttpOnly"),
        "the client must be able to read XSRF-TOKEN"
    );
    assert!(xsrf.contains("SameSite=Lax") && xsrf.contains("Path=/"));
    assert!(
        !secret.contains("Secure") && !xsrf.contains("Secure"),
        "http app_url"
    );

    let https = csrf_app(settings_with("https://app.example.com", true, false));
    let res = send(&https, Request::get("/").body(Body::empty()).unwrap()).await;
    for c in set_cookies(&res) {
        assert!(c.contains("Secure"), "{c}");
    }
}

#[tokio::test]
async fn csrf_accepts_a_valid_token_and_does_not_reissue_on_get() {
    let app = csrf_app(settings());
    let (cookie, token) = csrf_bootstrap(&app).await;

    let res = send(
        &app,
        csrf_post("/", &cookie, Some(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_string(res).await, "written");

    let alt = Request::post("/")
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &token);
    assert_eq!(
        send(&app, alt.body(Body::empty()).unwrap()).await.status(),
        StatusCode::OK
    );

    let res = send(
        &app,
        Request::get("/")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        set_cookies(&res).is_empty(),
        "valid cookies are not rewritten"
    );
}

#[tokio::test]
async fn csrf_rejects_missing_and_forged_tokens() {
    let app = csrf_app(settings());
    let (cookie, token) = csrf_bootstrap(&app).await;

    let res = send(
        &app,
        csrf_post("/", &cookie, None).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_string(res).await, csrf::REJECTION_MESSAGE);

    let res = send(
        &app,
        csrf_post("/", &cookie, Some("AAAA"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // A token from another browser (another secret) doesn't verify here.
    let (_, other_token) = csrf_bootstrap(&app).await;
    let res = send(
        &app,
        csrf_post("/", &cookie, Some(&other_token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Right token, but no secret cookie (e.g. attacker-set XSRF-TOKEN only).
    let res = send(
        &app,
        csrf_post("/", &format!("XSRF-TOKEN={token}"), Some(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Inertia requests get JSON.
    let res = send(
        &app,
        csrf_post("/", &cookie, None)
            .header("x-inertia", "true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(body["message"], csrf::REJECTION_MESSAGE);
}

#[tokio::test]
async fn csrf_rejects_cross_site_fetch_and_foreign_origin_even_with_a_valid_token() {
    let app = csrf_app(settings());
    let (cookie, token) = csrf_bootstrap(&app).await;

    let req = csrf_post("/", &cookie, Some(&token)).header("sec-fetch-site", "cross-site");
    assert_eq!(
        send(&app, req.body(Body::empty()).unwrap()).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let req = csrf_post("/", &cookie, Some(&token)).header(header::ORIGIN, "https://evil.example");
    assert_eq!(
        send(&app, req.body(Body::empty()).unwrap()).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let req = csrf_post("/", &cookie, Some(&token))
        .header(header::ORIGIN, "http://localhost:5150")
        .header("sec-fetch-site", "same-origin");
    assert_eq!(
        send(&app, req.body(Body::empty()).unwrap()).await.status(),
        StatusCode::OK
    );
}

/// `bin/dev` answers on `localhost` and `127.0.0.1` alike: outside production a loopback app_url
/// accepts the other loopback spellings on the same port, and nothing else.
#[tokio::test]
async fn csrf_in_development_treats_loopback_spellings_as_one_origin() {
    let app = csrf_app(settings()); // app_url http://localhost:5150, not production
    let (cookie, token) = csrf_bootstrap(&app).await;
    for origin in [
        "http://localhost:5150",
        "http://127.0.0.1:5150",
        "http://[::1]:5150",
    ] {
        assert_eq!(
            post_status(&app, &cookie, &token, "127.0.0.1:5150", origin).await,
            StatusCode::OK,
            "{origin}"
        );
    }
    for origin in [
        "http://127.0.0.1:5151",  // another local server
        "https://127.0.0.1:5150", // another scheme
        "http://192.168.1.5:5150",
        "http://localhost.evil.example:5150",
    ] {
        assert_eq!(
            post_status(&app, &cookie, &token, "127.0.0.1:5150", origin).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{origin}"
        );
    }
    // Production compares the origin exactly.
    let app = csrf_app(production(settings()));
    let (cookie, token) = csrf_bootstrap(&app).await;
    assert_eq!(
        post_status(
            &app,
            &cookie,
            &token,
            "127.0.0.1:5150",
            "http://127.0.0.1:5150"
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// `settings()` with `extra_hosts`, the second hostname a demo gives to "the other person".
fn settings_with_extra_hosts(extra: &str) -> Arc<Settings> {
    let mut settings = (*settings_with("https://app.example.com", true, false)).clone();
    settings.extra_hosts = extra.to_owned();
    Arc::new(settings)
}

async fn post_status(
    app: &Router,
    cookie: &str,
    token: &str,
    host: &str,
    origin: &str,
) -> StatusCode {
    let req = csrf_post("/", cookie, Some(token))
        .header(header::HOST, host)
        .header(header::ORIGIN, origin)
        .header("sec-fetch-site", "same-origin");
    send(app, req.body(Body::empty()).unwrap()).await.status()
}

#[tokio::test]
async fn csrf_accepts_an_extra_host_only_from_that_same_host() {
    let app = csrf_app(settings_with_extra_hosts(
        "two.app.example.com, three.app.example.com",
    ));
    let (cookie, token) = csrf_bootstrap(&app).await;
    let primary = ("app.example.com", "https://app.example.com");
    let two = ("two.app.example.com", "https://two.app.example.com");
    let three = ("three.app.example.com", "https://three.app.example.com");

    // Each host from itself: accepted.
    assert_eq!(
        post_status(&app, &cookie, &token, two.0, two.1).await,
        StatusCode::OK
    );
    assert_eq!(
        post_status(&app, &cookie, &token, primary.0, primary.1).await,
        StatusCode::OK
    );
    // A listed origin posting to another host, either way round: refused.
    for (host, origin) in [
        (primary.0, two.1),
        (two.0, primary.1),
        (two.0, three.1),
        (three.0, two.1),
    ] {
        assert_eq!(
            post_status(&app, &cookie, &token, host, origin).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{origin} -> {host}"
        );
    }
    // An unlisted host is not trusted from itself; only app_url's origin passes there.
    let evil = ("evil.example.com", "https://evil.example.com");
    assert_eq!(
        post_status(&app, &cookie, &token, evil.0, evil.1).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // http:// for a listed host (app_url is https) is a different origin: refused.
    assert_eq!(
        post_status(&app, &cookie, &token, two.0, "http://two.app.example.com").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn csrf_with_no_extra_hosts_trusts_only_app_url() {
    let app = csrf_app(settings_with_extra_hosts(""));
    let (cookie, token) = csrf_bootstrap(&app).await;
    assert_eq!(
        post_status(
            &app,
            &cookie,
            &token,
            "app.example.com",
            "https://app.example.com"
        )
        .await,
        StatusCode::OK
    );
    // Without the list, two.* is just another foreign origin, as before.
    assert_eq!(
        post_status(
            &app,
            &cookie,
            &token,
            "two.app.example.com",
            "https://two.app.example.com"
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // As before: the origin check ignores Host, so app_url's origin passes whatever the Host.
    assert_eq!(
        post_status(
            &app,
            &cookie,
            &token,
            "two.app.example.com",
            "https://app.example.com"
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn csrf_reissues_invalid_token_and_rotates_on_sign_in() {
    let app = csrf_app(settings());
    let (cookie, token) = csrf_bootstrap(&app).await;
    let secret_pair = cookie.split("; ").next().unwrap().to_owned();

    let res = send(
        &app,
        Request::get("/")
            .header(header::COOKIE, format!("{secret_pair}; XSRF-TOKEN=stale"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let cookies = set_cookies(&res);
    assert!(set_cookie(&cookies, "_csrf").is_none(), "secret kept");
    let fresh = cookie_value(set_cookie(&cookies, "XSRF-TOKEN").expect("stale token replaced"));
    assert_ne!(fresh, token);

    let res = send(
        &app,
        csrf_post("/sign_in", &cookie, Some(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookies = set_cookies(&res);
    let new_secret = pair(set_cookie(&cookies, "_csrf").expect("secret rotated"));
    let new_token = cookie_value(set_cookie(&cookies, "XSRF-TOKEN").expect("token reissued"));
    assert_ne!(new_secret, secret_pair);

    // The pre-sign-in token no longer works; the new one does.
    let new_cookie = format!("{new_secret}; XSRF-TOKEN={new_token}");
    let res = send(
        &app,
        csrf_post("/", &new_cookie, Some(&token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = send(
        &app,
        csrf_post("/", &new_cookie, Some(&new_token))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn csrf_disabled_skips_checks_and_cookies() {
    let app = csrf_app(settings_with("http://localhost:5150", false, false));
    let res = send(
        &app,
        Request::post("/")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
    assert!(set_cookies(&res).is_empty());
}

// ---------------------------------------------------------------- headers

fn headers_app(settings: Arc<Settings>) -> Router {
    let router = Router::new()
        .route(
            "/",
            get(|Extension(CspNonce(n)): Extension<CspNonce>| async move { n }),
        )
        .route(
            "/framed",
            get(|| async { ([(header::X_FRAME_OPTIONS, "SAMEORIGIN")], "ok") }),
        );
    headers::layer(router, settings)
}

#[tokio::test]
#[serial]
async fn csp_carries_a_fresh_per_request_nonce_matching_the_extension() {
    let app = headers_app(settings());
    let mut nonces = Vec::new();
    for _ in 0..2 {
        let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
        let csp = res.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_owned();
        let nonce = body_string(res).await;
        assert!(nonce.len() >= 22, "128-bit nonce");
        assert!(
            csp.contains(&format!("script-src 'self' 'nonce-{nonce}';")),
            "{csp}"
        );
        for d in [
            "default-src 'self'",
            "object-src 'none'",
            "base-uri 'self'",
            "form-action 'self'",
            "frame-ancestors 'none'",
            "img-src 'self' data:",
            "connect-src 'self';",
        ] {
            assert!(csp.contains(d), "missing {d}: {csp}");
        }
        assert!(!csp.contains("5173"), "no vite in non-dev CSP");
        nonces.push(nonce);
    }
    assert_ne!(nonces[0], nonces[1]);
}

#[tokio::test]
#[serial]
async fn csp_allows_the_vite_dev_server_and_hmr_socket_in_dev() {
    let app = headers_app(settings_with("http://localhost:5150", true, true));
    let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
    let csp = res.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    for d in [
        "script-src",
        "style-src",
        "img-src",
        "font-src",
        "connect-src",
    ] {
        let directive = csp.split("; ").find(|p| p.starts_with(d)).unwrap();
        assert!(directive.contains("http://localhost:5173"), "{directive}");
    }
    assert!(csp.contains("ws://localhost:5173"), "{csp}");
}

#[tokio::test]
#[serial]
async fn security_headers_are_set_without_overriding_handler_headers_and_hsts_only_in_production() {
    let app = headers_app(settings());
    let res = send(&app, Request::get("/framed").body(Body::empty()).unwrap()).await;
    let h = res.headers();
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        h[header::REFERRER_POLICY],
        "strict-origin-when-cross-origin"
    );
    assert_eq!(
        h[header::X_FRAME_OPTIONS],
        "SAMEORIGIN",
        "handler value kept"
    );
    assert_eq!(
        h["permissions-policy"],
        "camera=(), microphone=(), geolocation=()"
    );
    assert_eq!(h["cross-origin-opener-policy"], "same-origin");
    assert!(h.get(header::STRICT_TRANSPORT_SECURITY).is_none());

    // HSTS follows the resolved AppContext environment carried in Settings
    // (`--environment production`), not LOCO_ENV in the process env.
    std::env::set_var("LOCO_ENV", "production");
    let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
    std::env::set_var("LOCO_ENV", "test");
    assert!(res
        .headers()
        .get(header::STRICT_TRANSPORT_SECURITY)
        .is_none());

    let app = headers_app(production(settings()));
    let res = send(&app, Request::get("/").body(Body::empty()).unwrap()).await;
    assert_eq!(
        res.headers()[header::STRICT_TRANSPORT_SECURITY],
        headers::HSTS_VALUE
    );
    assert_eq!(res.headers()[header::X_FRAME_OPTIONS], "DENY");
}

fn production(settings: Arc<Settings>) -> Arc<Settings> {
    let mut s = (*settings).clone();
    s.production = true;
    Arc::new(s)
}

#[test]
fn cookies_are_secure_in_production_or_on_https() {
    let secure = |s: &Settings| cookies::session_token_cookie(s, "t").secure() == Some(true);
    assert!(!secure(&settings_with(
        "http://localhost:5150",
        true,
        false
    )));
    assert!(secure(&settings_with("https://app.example", true, false)));
    // Production over an http app_url (without the opt-out) still gets Secure.
    assert!(secure(&production(settings_with(
        "http://localhost:5150",
        true,
        false
    ))));
    assert!(cookies::is_secure(&production(settings())));
    // ...unless the operator explicitly opted into http for local testing:
    // a Secure cookie would be dropped by the browser and every form would 422.
    let mut local = (*production(settings_with("http://localhost:5150", true, false))).clone();
    local.allow_insecure_http = true;
    assert!(!cookies::is_secure(&local));
    // allow_insecure_http never weakens an https deployment.
    let mut https = (*production(settings_with("https://app.example", true, false))).clone();
    https.allow_insecure_http = true;
    assert!(cookies::is_secure(&https));
}

#[tokio::test]
#[serial]
async fn settings_from_ctx_takes_production_from_the_app_context_not_env_vars() {
    use loco_rs::environment::Environment;
    std::env::set_var("LOCO_ENV", "test");
    let mut ctx = loco_rs::tests_cfg::app::get_app_context().await;
    let mut raw = serde_json::to_value(json!({
        "secret_key_base": "k".repeat(64),
        "app_url": "https://app.example",
        "app_name": "T", "mail_from": "a@b.c",
        "encrypt_history": false, "forgery_protection": true,
        "vite": {"dev_server": false, "dev_server_url": "http://localhost:5173",
                 "manifest_path": "m.json"},
        "ssr": {"enabled": false, "spawn": false, "bundle": "ssr/ssr.js", "timeout_ms": 10},
        "production": false
    }))
    .unwrap();
    ctx.environment = Environment::Production;
    ctx.config.settings = Some(raw.clone());
    let s = Settings::from_ctx(&ctx).unwrap();
    assert!(s.production, "--environment production");
    assert!(cookies::is_secure(&s));

    ctx.environment = Environment::Development;
    raw["production"] = json!(true);
    ctx.config.settings = Some(raw.clone());
    assert!(
        !Settings::from_ctx(&ctx).unwrap().production,
        "YAML cannot set it"
    );

    // The shipped dev/test secrets and an http app_url refuse to boot production.
    ctx.environment = Environment::Production;
    for (key, value) in [
        ("secret_key_base", json!(config::SHIPPED_SECRETS[0])),
        ("secret_key_base", json!(config::SHIPPED_SECRETS[1])),
        ("app_url", json!("http://app.example")),
    ] {
        let mut bad = raw.clone();
        bad[key] = value;
        ctx.config.settings = Some(bad);
        assert!(Settings::from_ctx(&ctx).is_err(), "{key}");
    }
}

// ---------------------------------------------------------------- precognition

#[tokio::test]
async fn precognition_responses() {
    let mut h = HeaderMap::new();
    assert!(!precognition::is_precognition(&h));
    h.insert("precognition", "true".parse().unwrap());
    assert!(precognition::is_precognition(&h));

    let ok = precognition::respond(&json!({}));
    assert_eq!(ok.status(), StatusCode::NO_CONTENT);
    assert_eq!(ok.headers()["precognition"], "true");
    assert_eq!(ok.headers()["precognition-success"], "true");

    let errors = json!({"name": ["can't be blank"], "email": ["is invalid"]});
    let bad = precognition::respond(&errors);
    assert_eq!(bad.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.headers()["precognition"], "true");
    assert!(bad.headers().get("precognition-success").is_none());
    let body: Value = serde_json::from_str(&body_string(bad).await).unwrap();
    assert_eq!(body, json!({"errors": errors}));

    h.insert(
        "precognition-validate-only",
        "email, password".parse().unwrap(),
    );
    let only = precognition::respond_for(&h, &errors);
    assert_eq!(only.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = serde_json::from_str(&body_string(only).await).unwrap();
    assert_eq!(body, json!({"errors": {"email": ["is invalid"]}}));

    h.insert("precognition-validate-only", "password".parse().unwrap());
    assert_eq!(
        precognition::respond_for(&h, &errors).status(),
        StatusCode::NO_CONTENT,
        "untouched fields ignored"
    );
}

// ---------------------------------------------------------------- session cookie

#[test]
fn session_token_cookie_is_signed_permanent_httponly_lax() {
    let s = settings();
    let mut h = HeaderMap::new();
    cookies::set_session_token(&mut h, &s, "tok123");
    let line = h[header::SET_COOKIE].to_str().unwrap().to_owned();
    for attr in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=630720000"] {
        assert!(line.contains(attr), "missing {attr}: {line}");
    }
    assert!(!line.contains("Secure"));

    let mut req = HeaderMap::new();
    req.insert(header::COOKIE, pair(&line).parse().unwrap());
    assert_eq!(
        cookies::read_session_token(&req, &s).as_deref(),
        Some("tok123")
    );

    let mut forged = HeaderMap::new();
    forged.insert(header::COOKIE, "session_token=tok123".parse().unwrap());
    assert_eq!(cookies::read_session_token(&forged, &s), None);

    let https = settings_with("https://app.example.com", true, false);
    assert!(cookies::session_token_cookie(&https, "t")
        .secure()
        .unwrap_or(false));
}

#[test]
fn derived_keys_are_independent_per_purpose() {
    let s = settings();
    let a = cookies::flash_key(&s);
    let b = cookies::csrf_key(&s);
    let c = cookies::session_cookie_key(&s);
    assert_ne!(a.master(), b.master());
    assert_ne!(b.master(), c.master());
    assert_eq!(a.master(), cookies::flash_key(&s).master(), "deterministic");
    // Cached once per Settings, but still exactly the HMAC derivation of secret_key_base.
    for (key, label) in [(a, "flash"), (b, "csrf"), (c, "session")] {
        let fresh = cookies::derive_key(&s.secret_key_base, label);
        assert_eq!(key.master(), fresh.master(), "{label}");
    }
}
