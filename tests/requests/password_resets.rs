//! `spec/requests/identity/password_resets_spec.rb`, plus: a reset token is rejected after the
//! password changes and after 20 minutes, and a reset logs out every session.

use chrono::Duration;
use inertia_rust_starter_kit::{
    controllers::identity::password_resets::RESET_REQUESTED,
    models::{sessions, tokens::Purpose},
    route_table,
};
use sea_orm::{ActiveModelTrait, ActiveValue, IntoActiveModel};
use serde_json::json;
use serial_test::serial;

use super::*;

fn with_sid(path: &str, sid: &str) -> String {
    format!(
        "{path}?{}",
        serde_urlencoded::to_string([("sid", sid)]).unwrap()
    )
}

fn new_password(sid: &str) -> serde_json::Value {
    json!({
        "sid": sid,
        "password": "NewPassword1*3*",
        "password_confirmation": "NewPassword1*3*",
    })
}

#[tokio::test]
#[serial]
async fn get_new_renders_the_forgot_password_page() {
    with_app(|server, _ctx| async move {
        let res = server.get(route_table::NEW_IDENTITY_PASSWORD_RESET).await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("identity/password_resets/new"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_with_a_verified_user_sends_a_password_reset_email() {
    with_app(|server, ctx| async move {
        let res = server
            .post(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({ "email": ONE }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        let mails = deliveries(&ctx);
        assert_eq!(mails.len(), 1);
        assert!(mails[0].contains("Subject: Reset your password"));
        assert!(mails[0].contains("To: one@example.com"));
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(page["flash"]["notice"], RESET_REQUESTED);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_with_an_unverified_user_does_not_send_a_password_reset_email() {
    with_app(|server, ctx| async move {
        let mut one = user(&ctx, ONE).await.into_active_model();
        one.verified = ActiveValue::Set(false);
        one.update(&ctx.db).await.unwrap();

        let res = server
            .post(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({ "email": ONE }))
            .await;
        // Same reply as for a verified account, so the form can't enumerate accounts
        // (deliberate divergence from the Rails kit; docs/PARITY.md). No mail is sent.
        assert!(deliveries(&ctx).is_empty());
        assert_redirect(&res, route_table::SIGN_IN);
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(page["flash"]["notice"], RESET_REQUESTED);
        assert!(page["flash"].get("alert").is_none());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_with_a_nonexistent_email_does_not_send_a_password_reset_email() {
    with_app(|server, ctx| async move {
        let res = server
            .post(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({ "email": "missing@example.com" }))
            .await;
        // Same reply as for a verified account, so the form can't enumerate accounts
        // (deliberate divergence from the Rails kit; docs/PARITY.md). No mail is sent.
        assert!(deliveries(&ctx).is_empty());
        assert_redirect(&res, route_table::SIGN_IN);
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(page["flash"]["notice"], RESET_REQUESTED);
        assert!(page["flash"].get("alert").is_none());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_edit_renders_the_reset_page_with_a_valid_token() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        let page = inertia_get(
            &server,
            &ctx,
            &with_sid(route_table::EDIT_IDENTITY_PASSWORD_RESET, &sid),
        )
        .await;
        assert_eq!(page["component"], "identity/password_resets/edit");
        assert_eq!(page["props"]["email"], ONE);
        assert_eq!(page["props"]["sid"], sid);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_edit_rejects_an_invalid_reset_token() {
    with_app(|server, _ctx| async move {
        let res = server
            .get(&with_sid(
                route_table::EDIT_IDENTITY_PASSWORD_RESET,
                "invalid",
            ))
            .await;
        assert_redirect(&res, route_table::NEW_IDENTITY_PASSWORD_RESET);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_with_a_valid_token_updates_the_password_and_logs_out_every_session() {
    with_app(|server, ctx| async move {
        let one = user(&ctx, ONE).await;
        let sid = token_for(&ctx, &one, Purpose::PasswordReset);
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(
            page["flash"]["notice"],
            "Your password was reset successfully. Please sign in"
        );
        let one = user(&ctx, ONE).await;
        assert!(one.authenticate("NewPassword1*3*"));
        assert!(sessions::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap()
            .is_empty());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_with_an_expired_token_rejects_the_password_change() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        travel(&ctx, Duration::minutes(30));

        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::NEW_IDENTITY_PASSWORD_RESET);
        let page = inertia_get(&server, &ctx, route_table::NEW_IDENTITY_PASSWORD_RESET).await;
        assert_eq!(
            page["flash"]["alert"],
            "That password reset link is invalid"
        );
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_reset_token_expires_after_exactly_20_minutes() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        let edit = with_sid(route_table::EDIT_IDENTITY_PASSWORD_RESET, &sid);

        travel(&ctx, Duration::minutes(19));
        assert_eq!(server.get(&edit).await.status_code(), 200);
        travel(&ctx, Duration::minutes(20) + Duration::seconds(1));
        assert_redirect(
            &server.get(&edit).await,
            route_table::NEW_IDENTITY_PASSWORD_RESET,
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_reset_token_is_rejected_after_the_password_changes() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);

        // The same link cannot be used twice.
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({
                "sid": sid,
                "password": "AnotherPass1*3*",
                "password_confirmation": "AnotherPass1*3*",
            }))
            .await;
        assert_redirect(&res, route_table::NEW_IDENTITY_PASSWORD_RESET);
        assert!(user(&ctx, ONE).await.authenticate("NewPassword1*3*"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_with_mismatched_password_confirmation_rejects_the_password_change() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({
                "sid": sid,
                "password": "NewPassword1*3*",
                "password_confirmation": "different",
            }))
            .await;
        let edit = with_sid(route_table::EDIT_IDENTITY_PASSWORD_RESET, &sid);
        assert_redirect(&res, &edit);
        let page = inertia_get(&server, &ctx, &edit).await;
        assert_eq!(
            page["props"]["errors"]["password_confirmation"],
            json!(["doesn't match Password"])
        );
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
    })
    .await;
}

/// The full flow: request a reset, follow the emailed link, set a new password, sign in.
#[tokio::test]
#[serial]
async fn the_emailed_link_resets_the_password() {
    with_app(|server, ctx| async move {
        server
            .post(route_table::IDENTITY_PASSWORD_RESET)
            .json(&json!({ "email": ONE }))
            .await;
        let sid = sid_from_mail(&deliveries(&ctx)[0]);
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        let res = server
            .post(route_table::SIGN_IN)
            .json(&json!({ "email": ONE, "password": "NewPassword1*3*" }))
            .await;
        assert_redirect(&res, "/acme");
    })
    .await;
}

/// Review #16, Rails parity: the reset controller skips authentication, so `Current.session`
/// is nil and the `after_update` callback deletes every session of the user. A browser that
/// is signed in as that user is signed out too.
#[tokio::test]
#[serial]
async fn a_reset_while_signed_in_as_the_same_user_logs_out_every_session_including_this_one() {
    with_app(|mut server, ctx| async move {
        let current = sign_in(&mut server, &ctx, ONE).await;
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        let one = user(&ctx, ONE).await;
        assert!(sessions::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap()
            .is_empty());
        assert!(sessions::Model::find_by_token_with_user(&ctx.db, &current)
            .await
            .is_err());
        assert_redirect(
            &server.get(route_table::DASHBOARD).await,
            route_table::SIGN_IN,
        );
        // Another user's sessions are untouched.
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, TWO_SESSION)
                .await
                .is_ok()
        );
    })
    .await;
}

/// Review #16, guest flow: every session of the user goes, other users keep theirs.
#[tokio::test]
#[serial]
async fn a_guest_reset_logs_out_every_session_of_that_user_only() {
    with_app(|server, ctx| async move {
        let one = user(&ctx, ONE).await;
        assert!(!sessions::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap()
            .is_empty());
        let sid = token_for(&ctx, &one, Purpose::PasswordReset);
        let res = server
            .put(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        assert!(user(&ctx, ONE).await.authenticate("NewPassword1*3*"));
        assert!(sessions::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap()
            .is_empty());
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, TWO_SESSION)
                .await
                .is_ok()
        );
    })
    .await;
}

/// Review #18: `has_secure_password` ignores a missing or empty password, so the update
/// "succeeds" without changing anything — and the link stays usable (the digest is the same).
#[tokio::test]
#[serial]
async fn a_reset_without_a_password_changes_nothing() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        for body in [json!({ "sid": sid }), json!({ "sid": sid, "password": "" })] {
            let res = server
                .patch(route_table::IDENTITY_PASSWORD_RESET)
                .json(&body)
                .await;
            assert_redirect(&res, route_table::SIGN_IN);
        }
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, ONE_SESSION)
                .await
                .is_ok()
        );
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&new_password(&sid))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
    })
    .await;
}
