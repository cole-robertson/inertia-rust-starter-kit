//! `spec/requests/sessions_spec.rb`, `spec/system/sessions_spec.rb` (as a request test), plus:
//! cross-user session delete is a 404, and logging out the current session signs out.

use inertia_rust_starter_kit::{inertia::cookies, models::sessions, route_table};
use serde_json::json;
use serial_test::serial;

use super::*;

#[tokio::test]
#[serial]
async fn get_sign_in_renders_the_sign_in_page() {
    with_app(|server, _ctx| async move {
        let res = server.get(route_table::SIGN_IN).await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("sessions/new"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_sign_in_redirects_authenticated_users() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.get(route_table::SIGN_IN).await;
        assert_redirect(&res, route_table::ROOT);
        // `/` sends a signed-in user on to their account, carrying the flash.
        assert_redirect(&server.get(route_table::ROOT).await, "/acme");
        let page = inertia_get(&server, &ctx, "/acme").await;
        assert_eq!(page["flash"]["notice"], "You are already signed in");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_in_with_valid_credentials_signs_in_and_sets_a_session_cookie() {
    with_app(|server, _ctx| async move {
        let res = server
            .post(route_table::SIGN_IN)
            .json(&json!({ "email": ONE, "password": PASSWORD }))
            .await;
        // sign-in lands in the user's last used account (SPEC-C1), not /dashboard.
        assert_redirect(&res, "/acme");
        assert!(!res.cookie(cookies::SESSION_COOKIE).value().is_empty());

        let res = server.get(route_table::SETTINGS_PROFILE).await;
        assert_eq!(res.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_in_records_the_user_agent_and_ip_on_the_new_session() {
    with_app(|server, ctx| async move {
        server
            .post(route_table::SIGN_IN)
            .add_header("user-agent", "SpecBrowser/2.0")
            .json(&json!({ "email": ONE, "password": PASSWORD }))
            .await;
        let one = user(&ctx, ONE).await;
        let newest = sessions::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap();
        assert_eq!(newest[0].user_agent.as_deref(), Some("SpecBrowser/2.0"));
        assert_eq!(newest[0].ip_address.as_deref(), Some("127.0.0.1"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_in_with_invalid_credentials_redirects_back_with_an_alert() {
    with_app(|server, ctx| async move {
        let res = server
            .post(route_table::SIGN_IN)
            .json(&json!({ "email": ONE, "password": "wrongpassword" }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(
            page["flash"]["alert"],
            "That email or password is incorrect"
        );

        let res = server.get(route_table::DASHBOARD).await;
        assert_redirect(&res, route_table::SIGN_IN);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn delete_session_destroys_the_session() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        // `users(:one).sessions.last` is the fixture session, not the one we are using.
        let res = server.delete(&route_table::session_path(ONE_SESSION)).await;
        assert_redirect(&res, route_table::SETTINGS_SESSIONS);
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, ONE_SESSION)
                .await
                .is_err()
        );
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_SESSIONS).await;
        assert_eq!(page["flash"]["notice"], "That session has been logged out");
        assert_eq!(page["clearHistory"], true);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn delete_another_users_session_is_not_found_and_keeps_it() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.delete(&route_table::session_path(TWO_SESSION)).await;
        assert_eq!(res.status_code(), 404);
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, TWO_SESSION)
                .await
                .is_ok()
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn deleting_the_current_session_signs_this_browser_out() {
    with_app(|mut server, ctx| async move {
        let token = sign_in(&mut server, &ctx, ONE).await;
        let res = server.delete(&route_table::session_path(&token)).await;
        assert_redirect(&res, route_table::SETTINGS_SESSIONS);
        let res = server.get(route_table::SETTINGS_SESSIONS).await;
        assert_redirect(&res, route_table::SIGN_IN);
    })
    .await;
}

/// `spec/system/sessions_spec.rb`: sign in and see the dashboard, which is the account overview
/// here (the browser version is the Playwright test in e2e/).
#[tokio::test]
#[serial]
async fn signs_in_and_lands_in_the_last_used_account() {
    with_app(|server, ctx| async move {
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(page["component"], "sessions/new");
        let res = server
            .post(route_table::SIGN_IN)
            .add_header("x-inertia", "true")
            .json(&json!({ "email": ONE, "password": PASSWORD }))
            .await;
        assert_redirect(&res, "/acme");
        let page = inertia_get(&server, &ctx, "/acme").await;
        assert_eq!(page["component"], "accounts/show");
        assert_eq!(page["url"], "/acme");
        assert_eq!(page["props"]["auth"]["user"]["email"], ONE);
        assert_eq!(page["flash"]["notice"], "Signed in successfully");
    })
    .await;
}

/// The Rails kit's `/dashboard` still works: it is the account overview now, so it redirects
/// there (old links and bookmarks), and a guest still goes to sign in.
#[tokio::test]
#[serial]
async fn dashboard_redirects_to_the_account_overview() {
    with_app(|mut server, ctx| async move {
        assert_redirect(
            &server.get(route_table::DASHBOARD).await,
            route_table::SIGN_IN,
        );
        sign_in(&mut server, &ctx, ONE).await;
        assert_redirect(&server.get(route_table::DASHBOARD).await, "/acme");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_in_with_null_params_is_a_wrong_password_not_a_bad_request() {
    with_app(|server, ctx| async move {
        let res = server
            .post(route_table::SIGN_IN)
            .json(&json!({ "email": null, "password": null }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(
            page["flash"]["alert"],
            "That email or password is incorrect"
        );
    })
    .await;
}
