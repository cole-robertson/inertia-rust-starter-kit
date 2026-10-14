//! The Inertia page object of real pages: the `auth` shared prop, `errors` after a failed
//! update, flash after a redirect, and the initial HTML document.

use inertia_rust_starter_kit::route_table;
use serde_json::{json, Value};
use serial_test::serial;

use super::*;

#[tokio::test]
#[serial]
async fn a_signed_in_page_carries_the_auth_shared_prop() {
    with_app(|mut server, ctx| async move {
        let token = sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .get("/acme")
            .add_header("x-inertia", "true")
            .add_header("x-inertia-version", asset_version(&ctx))
            .await;
        assert_eq!(res.status_code(), 200);
        assert_eq!(res.header("x-inertia"), "true");
        assert!(res.headers().get_all("vary").iter().any(|v| v
            .to_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("x-inertia")));
        let page = res.json::<Value>();
        assert_eq!(page["component"], "accounts/show");
        assert_eq!(page["url"], "/acme");
        assert_eq!(page["props"]["errors"], json!({}));
        let one = user(&ctx, ONE).await;
        let auth = &page["props"]["auth"];
        assert_eq!(auth["session"], json!({ "id": token }));
        assert_eq!(auth["user"]["id"], one.id);
        assert_eq!(auth["user"]["name"], "Test User");
        assert_eq!(auth["user"]["email"], ONE);
        assert_eq!(auth["user"]["verified"], true);
        let mut keys: Vec<&str> = auth["user"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        // Never the password digest.
        assert_eq!(
            keys,
            [
                "created_at",
                "email",
                "id",
                "name",
                "updated_at",
                "verified"
            ]
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn errors_and_flash_survive_exactly_one_redirect() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .add_header("x-inertia", "true")
            .json(&json!({
                "password": "short",
                "password_confirmation": "nope",
                "password_challenge": "wrongpassword",
            }))
            .await;
        assert_eq!(res.status_code(), 303);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(
            page["props"]["errors"],
            json!({
                "password": ["is too short (minimum is 12 characters)"],
                "password_confirmation": ["doesn't match Password"],
                "password_challenge": ["is invalid"],
            })
        );
        // Consumed: the next visit is clean.
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(page["props"]["errors"], json!({}));

        server
            .patch(route_table::SETTINGS_PROFILE)
            .add_header("x-inertia", "true")
            .json(&json!({ "name": "Renamed" }))
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(
            page["flash"],
            json!({ "notice": "Your profile has been updated" })
        );
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert!(page
            .get("flash")
            .is_none_or(|f| f.as_object().is_none_or(|m| m.is_empty())));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_prefetch_does_not_take_the_flash_from_the_visit_that_follows() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .add_header("x-inertia", "true")
            .json(&json!({ "name": "Renamed" }))
            .await;
        assert_eq!(res.status_code(), 303);

        // Inertia's <Link prefetch> sends `Purpose: prefetch`.
        let res = server
            .get(route_table::SETTINGS_PROFILE)
            .add_header("x-inertia", "true")
            .add_header("x-inertia-version", asset_version(&ctx))
            .add_header("purpose", "prefetch")
            .await;
        assert_eq!(res.status_code(), 200);
        assert!(res.json::<Value>().get("flash").is_none());
        assert!(
            !res.headers()
                .get_all("set-cookie")
                .iter()
                .any(|c| c.to_str().unwrap().starts_with("_flash=")),
            "a prefetch leaves the flash cookie alone"
        );

        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(
            page["flash"],
            json!({ "notice": "Your profile has been updated" })
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn the_initial_visit_is_an_html_document_with_the_page_in_a_script_element() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.get(route_table::SETTINGS_SESSIONS).await;
        assert_eq!(res.status_code(), 200);
        let html = res.text();
        assert!(html.contains(r#"data-page="app""#), "{html}");
        assert!(html.contains(r#"<div id="app""#), "{html}");
        assert!(html.contains("settings/sessions/index"), "{html}");
        assert!(html.contains("one@example.com"), "{html}");
    })
    .await;
}

/// `app;dur=<ms>` from the `Server-Timing` header, or `None` when it's missing.
fn server_timing_ms(res: &axum_test::TestResponse) -> Option<f64> {
    let value = res.maybe_header("server-timing")?;
    value
        .to_str()
        .unwrap()
        .split(", ")
        .find_map(|entry| entry.strip_prefix("app;dur="))
        .map(|ms| ms.parse().expect("a number of milliseconds"))
}

/// Server-Timing's `db;desc="N queries";dur=<ms>` counts the SQL queries of this request only,
/// on every GET (the budget tests' `assert_max_queries` reads it).
#[tokio::test]
#[serial]
async fn server_timing_counts_the_sql_queries_of_this_request() {
    with_app(|mut server, ctx| async move {
        // A guest page reads no rows.
        let home = server.get(route_table::ROOT).await;
        let header = home.header("server-timing");
        let header = header.to_str().unwrap();
        assert!(header.contains("db;desc=\"0 queries\";dur="), "{header}");
        // A signed-in account page runs a few (session, membership, members count...).
        sign_in(&mut server, &ctx, ONE).await;
        server.get("/acme/members").await; // remembers Acme as the last account (a write)
        let full = super::budget::queries(&server.get("/acme/members").await);
        assert!(full >= 3, "{full}");
        // A partial reload of the plain prop skips the lists' queries, so it runs fewer.
        let partial = super::budget::partial(
            &server,
            &ctx,
            "/acme/members",
            "members/index",
            &["can_manage"],
        )
        .await;
        assert!(super::budget::queries(&partial) < full);
        // Per request: the same request twice counts the same.
        assert_eq!(
            super::budget::queries(&server.get("/acme/members").await),
            full
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn pages_report_how_long_the_app_spent_in_server_timing() {
    with_app(|mut server, ctx| async move {
        // HTML, an Inertia visit, a signed-in page and a 404 all carry it.
        let html = server.get(route_table::ROOT).await;
        let xhr = server
            .get(route_table::SIGN_IN)
            .add_header("x-inertia", "true")
            .add_header("x-inertia-version", asset_version(&ctx))
            .await;
        sign_in(&mut server, &ctx, ONE).await;
        let signed_in = server.get(route_table::SETTINGS_PROFILE).await;
        let missing = server.get("/no-such-page").await;
        for res in [&html, &xhr, &signed_in, &missing] {
            let ms = server_timing_ms(res).expect("a Server-Timing app;dur= header");
            // A real, positive measurement of this request, not a constant.
            assert!(ms > 0.0 && ms < 10_000.0, "{ms}");
        }

        // Not on writes: sign-in timing is kept as even as the password check makes it.
        server.clear_cookies();
        let res = server
            .post(route_table::SIGN_IN)
            .json(&json!({ "email": ONE, "password": PASSWORD }))
            .await;
        assert!(res.status_code().is_redirection());
        assert_eq!(server_timing_ms(&res), None);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn pages_are_compressed_when_loco_compression_is_on() {
    // Production turns Loco's compression middleware on (`COMPRESSION`, default true); it sits
    // inside the app's own layers, so a page must be rendered by the time it sees the response.
    let compress = |config: &mut loco_rs::config::Config| {
        config.server.middlewares.compression =
            Some(loco_rs::controller::middleware::compression::Compression { enable: true });
    };
    with_app_config(compress, |server, ctx| async move {
        for res in [
            server
                .get(route_table::SIGN_IN)
                .add_header("accept-encoding", "gzip")
                .await,
            server
                .get(route_table::SIGN_IN)
                .add_header("accept-encoding", "gzip")
                .add_header("x-inertia", "true")
                .add_header("x-inertia-version", asset_version(&ctx))
                .await,
        ] {
            assert_eq!(res.status_code(), 200);
            assert_eq!(res.header("content-encoding"), "gzip");
            let vary: Vec<String> = res
                .headers()
                .get_all("vary")
                .iter()
                .map(|v| v.to_str().unwrap().to_ascii_lowercase())
                .collect();
            let vary = vary.join(", ");
            assert!(
                vary.contains("x-inertia") && vary.contains("accept-encoding"),
                "{vary}"
            );
            assert!(!res.as_bytes().is_empty(), "a compressed page has a body");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn error_pages_are_readable_when_loco_compression_is_on() {
    // The exceptions layer replaces a handler's (already compressed) 404 body with the public
    // page; the response must not still claim the old body's Content-Encoding, or browsers
    // fail to decode the page (Chromium never finishes loading it).
    let compress = |config: &mut loco_rs::config::Config| {
        config.server.middlewares.compression =
            Some(loco_rs::controller::middleware::compression::Compression { enable: true });
    };
    with_app_config(compress, |server, _ctx| async move {
        let html = server
            .get("/invitations/0123456789abcdef0123456789abcdef")
            .add_header("accept-encoding", "gzip")
            .await;
        let json = server
            .get("/no/such/page")
            .add_header("accept-encoding", "gzip")
            .add_header("accept", "application/json")
            .await;
        for (res, body) in [
            (&html, "The page you were looking for doesn't exist"),
            (&json, r#"{"status":404,"error":"Not Found"}"#),
        ] {
            assert_eq!(res.status_code(), 404);
            let text = match res.headers().get("content-encoding") {
                Some(encoding) => {
                    assert_eq!(encoding, "gzip");
                    let mut text = String::new();
                    std::io::Read::read_to_string(
                        &mut flate2::read::GzDecoder::new(res.as_bytes().as_ref()),
                        &mut text,
                    )
                    .expect("the body is gzip, as Content-Encoding says");
                    text
                }
                None => res.text(),
            };
            assert!(text.contains(body), "{text}");
        }
    })
    .await;
}
