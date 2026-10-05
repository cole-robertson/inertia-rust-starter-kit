//! Access control and abuse protection: guests bounced from protected pages, `/up`, the
//! credential rate limits, and the XSRF cookie/header flow with forgery protection on.

use axum::http::StatusCode;
use inertia_rust_starter_kit::{
    controllers::rate_limit::{ALERT, LIMIT},
    inertia::cookies,
    route_table,
};
use loco_rs::controller::middleware::remote_ip::{ClientIpSource, RemoteIpMiddleware};
use serde_json::json;
use serial_test::serial;

use super::*;

#[tokio::test]
#[serial]
async fn guests_are_redirected_to_sign_in_from_every_protected_page() {
    with_app(|server, _ctx| async move {
        for path in [
            route_table::DASHBOARD,
            route_table::SETTINGS_PROFILE,
            route_table::SETTINGS_PASSWORD,
            route_table::SETTINGS_EMAIL,
            route_table::SETTINGS_SESSIONS,
            route_table::SETTINGS_APPEARANCE,
        ] {
            assert_redirect(&server.get(path).await, route_table::SIGN_IN);
        }
        let session = route_table::session_path(ONE_SESSION);
        assert_redirect(&server.delete(&session).await, route_table::SIGN_IN);
        assert_redirect(
            &server.delete(route_table::USERS).json(&json!({})).await,
            route_table::SIGN_IN,
        );
        assert_redirect(
            &server
                .patch(route_table::SETTINGS_PROFILE)
                .json(&json!({ "name": "x" }))
                .await,
            route_table::SIGN_IN,
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_tampered_session_cookie_is_treated_as_signed_out() {
    with_app(|mut server, _ctx| async move {
        // Unsigned: the raw token without an HMAC.
        server.add_cookie(cookie::Cookie::new(cookies::SESSION_COOKIE, ONE_SESSION));
        assert_redirect(
            &server.get(route_table::DASHBOARD).await,
            route_table::SIGN_IN,
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn public_pages_render_for_guests() {
    with_app(|server, ctx| async move {
        let page = inertia_get(&server, &ctx, route_table::ROOT).await;
        assert_eq!(page["component"], "home/index");
        assert_eq!(
            page["props"]["auth"],
            json!({ "user": null, "session": null })
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn up_is_ok_without_authentication() {
    with_app(|server, _ctx| async move {
        let res = server.get(route_table::UP).await;
        assert_eq!(res.status_code(), 200);
        assert_eq!(
            res.text(),
            r#"<!DOCTYPE html><html><body style="background-color: green"></body></html>"#
        );
        let res = server
            .get(route_table::UP)
            .add_header("accept", "application/json")
            .await;
        assert_eq!(res.json::<serde_json::Value>()["status"], "up");
    })
    .await;
}

/// `remote_ip` on with production's source: the client is the rightmost `X-Forwarded-For`
/// entry, the one the (single, trusted) proxy appends.
fn trust_the_proxy(config: &mut loco_rs::config::Config) {
    config.server.middlewares.remote_ip = Some(RemoteIpMiddleware {
        enable: true,
        source: ClientIpSource::RightmostXForwardedFor,
    });
}

/// Spend `sign_in`'s whole budget with wrong passwords, as `forwarded_for`.
async fn exhaust_sign_in(server: &TestServer, forwarded_for: &str) {
    for _ in 0..LIMIT {
        let res = server
            .post(route_table::SIGN_IN)
            .add_header("x-forwarded-for", forwarded_for)
            .json(&json!({ "email": ONE, "password": "wrongpassword" }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
    }
}

/// Sign in with the right password as `forwarded_for`; `true` when a session was created.
async fn signs_in(server: &TestServer, forwarded_for: &str) -> bool {
    let res = server
        .post(route_table::SIGN_IN)
        .add_header("x-forwarded-for", forwarded_for)
        .json(&json!({ "email": ONE, "password": PASSWORD }))
        .await;
    res.maybe_cookie(cookies::SESSION_COOKIE).is_some()
}

#[tokio::test]
#[serial]
async fn sign_in_is_rate_limited_per_client_ip_behind_a_trusted_proxy() {
    // Open sign-up: the last step posts an (invalid) sign-up and expects its validation errors.
    let config = |config: &mut loco_rs::config::Config| {
        trust_the_proxy(config);
        config.settings.as_mut().unwrap()["sign_up"] = "open".into();
    };
    with_app_config(config, |mut server, ctx| async move {
        exhaust_sign_in(&server, "203.0.113.1").await;
        // Even correct credentials are refused once this client's bucket is empty.
        let res = server
            .post(route_table::SIGN_IN)
            .add_header("x-forwarded-for", "203.0.113.1")
            .add_header("referer", "http://localhost/sign_in")
            .json(&json!({ "email": ONE, "password": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        assert!(res.maybe_cookie(cookies::SESSION_COOKIE).is_none());
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(page["flash"]["alert"], ALERT);

        // Another client behind the same proxy (same socket peer) has its own bucket, and
        // its session records its own address.
        assert!(signs_in(&server, "198.51.100.7").await);
        let one = user(&ctx, ONE).await;
        let newest = &session_model::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap()[0];
        assert_eq!(newest.ip_address.as_deref(), Some("198.51.100.7"));

        // Other endpoints have their own budget.
        server.clear_cookies();
        let res = server
            .post(route_table::SIGN_UP)
            .add_header("x-forwarded-for", "203.0.113.1")
            .json(&json!({ "name": "", "email": "", "password": "" }))
            .await;
        assert_redirect(&res, route_table::SIGN_UP);
    })
    .await;
}

/// A client can prepend anything to `X-Forwarded-For`; the proxy appends the real address,
/// and only that rightmost entry counts.
#[tokio::test]
#[serial]
async fn a_spoofed_forwarded_for_prefix_does_not_reset_the_budget() {
    with_app_config(trust_the_proxy, |server, _ctx| async move {
        exhaust_sign_in(&server, "10.9.9.9, 203.0.113.1").await;
        for spoofed in ["1.2.3.4, 203.0.113.1", "198.51.100.7, 203.0.113.1"] {
            assert!(!signs_in(&server, spoofed).await, "{spoofed} got through");
        }
    })
    .await;
}

/// Without `remote_ip` (the test and development default) proxy headers are not trusted at
/// all: every request is the socket peer, whatever it claims.
#[tokio::test]
#[serial]
async fn forwarded_for_is_ignored_when_the_proxy_is_not_trusted() {
    with_app(|server, _ctx| async move {
        exhaust_sign_in(&server, "203.0.113.1").await;
        for spoofed in ["198.51.100.7", "1.2.3.4, 5.6.7.8"] {
            assert!(!signs_in(&server, spoofed).await, "{spoofed} got through");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn sign_up_and_password_reset_are_rate_limited() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        for _ in 0..LIMIT {
            server
                .post(route_table::SIGN_UP)
                .json(&json!({ "name": "", "email": "", "password": "" }))
                .await;
            server
                .post(route_table::IDENTITY_PASSWORD_RESET)
                .json(&json!({ "email": "missing@example.com" }))
                .await;
        }
        let res = server
            .post(route_table::SIGN_UP)
            .json(&json!({
                "name": "New", "email": "new@example.com",
                "password": PASSWORD, "password_confirmation": PASSWORD,
            }))
            .await;
        assert_redirect(&res, route_table::SIGN_UP);
        assert!(user_model::Model::find_by_email(&ctx.db, "new@example.com")
            .await
            .is_err());

        let res = server
            .post(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({ "email": ONE }))
            .await;
        assert_redirect(&res, route_table::NEW_IDENTITY_PASSWORD_RESET);
        assert!(deliveries(&ctx).is_empty());
        let page = inertia_get(&server, &ctx, route_table::NEW_IDENTITY_PASSWORD_RESET).await;
        assert_eq!(page["flash"]["alert"], ALERT);
    })
    .await;
}

/// The current password can't be guessed without limit: password change, email change and
/// account deletion are rate-limited per client IP (Rails' `rate_limit` default), one budget
/// each, and precognitive password checks spend it too. Once it's spent, even the right
/// password is refused.
#[tokio::test]
#[serial]
async fn current_password_checks_are_rate_limited() {
    with_app_config(trust_the_proxy, |mut server, ctx| async move {
        const IP: &str = "203.0.113.1";
        sign_in(&mut server, &ctx, ONE).await;
        let wrong = json!({
            "password": "NewPassword1*3*",
            "password_confirmation": "NewPassword1*3*",
            "password_challenge": "wrong",
        });
        for i in 0..LIMIT {
            // Alternating real and precognitive guesses.
            let req = server
                .patch(route_table::SETTINGS_PASSWORD)
                .add_header("x-forwarded-for", IP);
            let req = if i % 2 == 0 {
                req
            } else {
                req.add_header("precognition", "true")
            };
            let res = req.json(&wrong).await;
            assert_ne!(
                res.status_code(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "{}",
                res.text()
            );
        }
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .add_header("x-forwarded-for", IP)
            .add_header("precognition", "true")
            .json(&json!({ "password_challenge": PASSWORD }))
            .await;
        assert!(
            res.status_code().is_redirection(),
            "a spent budget still answers precognition: {}",
            res.status_code()
        );
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .add_header("x-forwarded-for", IP)
            .json(&json!({
                "password": "NewPassword1*3*",
                "password_confirmation": "NewPassword1*3*",
                "password_challenge": PASSWORD,
            }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PASSWORD);
        assert!(
            user(&ctx, ONE).await.authenticate(PASSWORD),
            "the password changed"
        );
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(page["flash"]["alert"], ALERT);

        // Email change and account deletion check the password too, with budgets of their own.
        for _ in 0..LIMIT {
            server
                .patch(route_table::SETTINGS_EMAIL)
                .add_header("x-forwarded-for", IP)
                .json(&json!({ "email": "x@example.com", "password_challenge": "wrong" }))
                .await;
            server
                .delete(route_table::USERS)
                .add_header("x-forwarded-for", IP)
                .json(&json!({ "password_challenge": "wrong" }))
                .await;
        }
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .add_header("x-forwarded-for", IP)
            .json(&json!({ "email": "x@example.com", "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert_eq!(user(&ctx, ONE).await.email, ONE, "the email changed");
        let res = server
            .delete(route_table::USERS)
            .add_header("x-forwarded-for", IP)
            .json(&json!({ "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PROFILE);
        user(&ctx, ONE).await; // still there

        // Another client has its own budget.
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .add_header("x-forwarded-for", "198.51.100.7")
            .add_header("precognition", "true")
            .json(&json!({ "password_challenge": PASSWORD }))
            .await;
        assert_eq!(res.status_code(), StatusCode::NO_CONTENT, "{}", res.text());
    })
    .await;
}

/// The app can't be used to mail an address over and over.
#[tokio::test]
#[serial]
async fn resending_the_verification_email_is_rate_limited() {
    with_app_config(trust_the_proxy, |mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        for _ in 0..LIMIT {
            server
                .post(route_table::IDENTITY_EMAIL_VERIFICATION)
                .add_header("x-forwarded-for", "203.0.113.1")
                .await;
        }
        let sent = deliveries(&ctx).len();
        assert_eq!(sent, LIMIT as usize);
        let res = server
            .post(route_table::IDENTITY_EMAIL_VERIFICATION)
            .add_header("x-forwarded-for", "203.0.113.1")
            .await;
        assert!(res.status_code().is_redirection());
        assert_eq!(deliveries(&ctx).len(), sent, "mailed past the limit");
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(page["flash"]["alert"], ALERT);
    })
    .await;
}

/// Forgery protection on (like production): a POST without the header is rejected; the
/// XSRF-TOKEN cookie from a GET, echoed as X-XSRF-TOKEN, gets through; sign-in rotates it.
#[tokio::test]
#[serial]
async fn xsrf_cookie_to_header_flow_with_forgery_protection_enabled() {
    std::env::set_var("FORGERY_PROTECTION", "true");
    with_app(|server, ctx| async move {
        std::env::remove_var("FORGERY_PROTECTION");
        let credentials = json!({ "email": ONE, "password": PASSWORD });

        let res = server.get(route_table::SIGN_IN).await;
        assert_eq!(res.status_code(), 200);
        let token = res.cookie(cookies::XSRF_COOKIE).value().to_owned();
        assert!(!token.is_empty());

        // The cookie alone is not enough: the header must echo it. Rejections render
        // public/422.html, like Rails, for Inertia requests too.
        let res = server.post(route_table::SIGN_IN).json(&credentials).await;
        assert_eq!(res.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(res.text(), public("422.html"));
        let res = server
            .post(route_table::SIGN_IN)
            .add_header("x-inertia", "true")
            .json(&credentials)
            .await;
        assert_eq!(res.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(res.text(), public("422.html"));

        let res = server
            .post(route_table::SIGN_IN)
            .add_header("x-xsrf-token", "not-the-token")
            .json(&credentials)
            .await;
        assert_eq!(res.status_code(), StatusCode::UNPROCESSABLE_ENTITY);

        let res = server
            .post(route_table::SIGN_IN)
            .add_header("x-inertia", "true")
            .add_header("x-xsrf-token", &token)
            .json(&credentials)
            .await;
        assert_redirect(&res, "/acme");
        // Sign-in rotated the CSRF secret, so the pre-login token no longer works.
        let rotated = res.cookie(cookies::XSRF_COOKIE).value().to_owned();
        assert_ne!(rotated, token);
        let stale = server
            .patch(route_table::SETTINGS_PROFILE)
            .add_header("x-xsrf-token", &token)
            .json(&json!({ "name": "Stale" }))
            .await;
        assert_eq!(stale.status_code(), StatusCode::UNPROCESSABLE_ENTITY);

        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .add_header("x-inertia", "true")
            .add_header("x-xsrf-token", &rotated)
            .json(&json!({ "name": "Fresh" }))
            .await;
        // Inertia PATCH redirects become 303.
        assert_eq!(res.status_code(), StatusCode::SEE_OTHER);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(page["props"]["auth"]["user"]["name"], "Fresh");
    })
    .await;
    std::env::remove_var("FORGERY_PROTECTION");
}

// ---------------------------------------------------------------------------
// Rails' error pages, browser gate and default headers (docs/PARITY.md)
// ---------------------------------------------------------------------------

const OLD_CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/100.0.4896.127 Safari/537.36";

fn public(file: &str) -> String {
    std::fs::read_to_string(format!("public/{file}")).unwrap()
}

#[tokio::test]
#[serial]
async fn old_browsers_get_406_on_pages_but_not_on_up_or_public_files() {
    with_app(|server, _ctx| async move {
        for path in [
            route_table::ROOT,
            route_table::SIGN_IN,
            route_table::DASHBOARD,
        ] {
            let res = server.get(path).add_header("user-agent", OLD_CHROME).await;
            assert_eq!(res.status_code(), StatusCode::NOT_ACCEPTABLE, "{path}");
            assert_eq!(res.text(), public("406-unsupported-browser.html"));
        }
        for path in [route_table::UP, "/robots.txt"] {
            let res = server.get(path).add_header("user-agent", OLD_CHROME).await;
            assert_eq!(res.status_code(), StatusCode::OK, "{path}");
        }
        // A bot is never blocked, whatever its version.
        let res = server
            .get(route_table::SIGN_IN)
            .add_header(
                "user-agent",
                "Mozilla/5.0 (compatible; Googlebot/2.1) Chrome/100.0.0.0",
            )
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_known_path_with_the_wrong_method_is_a_404_page_like_rails() {
    with_app(|server, _ctx| async move {
        let res = server.get("/sessions/abc").await;
        assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(res.text(), public("404.html"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn errors_render_public_pages_or_rails_json() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        // Another user's session: 404 page, not Loco's JSON.
        let res = server.delete(&route_table::session_path(TWO_SESSION)).await;
        assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(res.text(), public("404.html"));
        // A malformed body: 400 page.
        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .add_header("content-type", "application/json")
            .bytes("{bad".into())
            .await;
        assert_eq!(res.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(res.text(), public("400.html"));
        // JSON clients get PublicExceptions' JSON.
        let res = server
            .get("/nope")
            .add_header("accept", "application/json")
            .await;
        assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(
            res.json::<serde_json::Value>(),
            json!({"status": 404, "error": "Not Found"})
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn pages_and_redirects_carry_rails_default_headers() {
    with_app(|server, _ctx| async move {
        let res = server.get(route_table::SIGN_IN).await;
        assert_eq!(
            res.header("cache-control"),
            "max-age=0, private, must-revalidate"
        );
        assert_eq!(res.header("x-xss-protection"), "0");
        assert_eq!(res.header("x-permitted-cross-domain-policies"), "none");
        assert!(res.maybe_header("x-powered-by").is_none());
        let res = server.get(route_table::DASHBOARD).await;
        assert_eq!(res.header("cache-control"), "no-cache");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn password_reset_pages_see_a_guest_even_when_signed_in() {
    // Rails' `skip_before_action :authenticate` never loads the session there.
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let page = inertia_get(&server, &ctx, route_table::NEW_IDENTITY_PASSWORD_RESET).await;
        assert_eq!(
            page["props"]["auth"],
            json!({"user": null, "session": null})
        );
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(page["props"]["auth"]["user"]["email"], ONE);
    })
    .await;
}
