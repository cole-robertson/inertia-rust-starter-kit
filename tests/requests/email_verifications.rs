//! `spec/requests/identity/email_verifications_spec.rb`, plus: a verification token is
//! rejected once the email it was issued for has changed.

use chrono::Duration;
use inertia_rust_starter_kit::{models::tokens::Purpose, route_table};
use sea_orm::{ActiveModelTrait, ActiveValue, IntoActiveModel};
use serde_json::json;
use serial_test::serial;

use super::*;

async fn unverify(ctx: &AppContext, email: &str) -> user_model::Model {
    let mut user = user(ctx, email).await.into_active_model();
    user.verified = ActiveValue::Set(false);
    user.update(&ctx.db).await.unwrap()
}

fn verification_path(sid: &str) -> String {
    format!(
        "{}?{}",
        route_table::IDENTITY_EMAIL_VERIFICATION,
        serde_urlencoded::to_string([("sid", sid)]).unwrap()
    )
}

#[tokio::test]
#[serial]
async fn get_with_a_valid_token_verifies_the_email() {
    with_app(|server, ctx| async move {
        let user = unverify(&ctx, ONE).await;
        let sid = token_for(&ctx, &user, Purpose::EmailVerification);

        let res = server.get(&verification_path(&sid)).await;
        assert_redirect(&res, route_table::ROOT);
        assert!(super::user(&ctx, ONE).await.verified);
        let page = inertia_get(&server, &ctx, route_table::ROOT).await;
        assert_eq!(
            page["flash"]["notice"],
            "Thank you for verifying your email address"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_used_verification_link_does_not_work_again() {
    // Deliberate divergence from the Rails kit, whose token is bound to the email only, so a
    // used link keeps "verifying" until it expires (docs/PARITY.md).
    with_app(|server, ctx| async move {
        let user = unverify(&ctx, ONE).await;
        let sid = token_for(&ctx, &user, Purpose::EmailVerification);

        assert_redirect(
            &server.get(&verification_path(&sid)).await,
            route_table::ROOT,
        );
        assert!(super::user(&ctx, ONE).await.verified);

        let res = server.get(&verification_path(&sid)).await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert!(super::user(&ctx, ONE).await.verified, "still verified");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_with_an_expired_token_does_not_verify_the_email() {
    with_app(|server, ctx| async move {
        let user = unverify(&ctx, ONE).await;
        let sid = token_for(&ctx, &user, Purpose::EmailVerification);

        travel(&ctx, Duration::days(3));

        let res = server.get(&verification_path(&sid)).await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert!(!super::user(&ctx, ONE).await.verified);
        // Following the redirect as a guest bounces to sign in; the alert survives the hop.
        let res = server.get(route_table::SETTINGS_EMAIL).await;
        assert_redirect(&res, route_table::SIGN_IN);
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(
            page["flash"]["alert"],
            "That email verification link is invalid"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_with_an_invalid_token_redirects_to_settings_email() {
    with_app(|server, _ctx| async move {
        let res = server.get(&verification_path("invalid")).await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_with_a_token_for_a_previous_email_is_rejected() {
    with_app(|server, ctx| async move {
        let user = unverify(&ctx, ONE).await;
        let sid = token_for(&ctx, &user, Purpose::EmailVerification);
        user.change_email(&ctx.db, Some("changed@example.com"), PASSWORD)
            .await
            .unwrap();

        let res = server.get(&verification_path(&sid)).await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert!(!super::user(&ctx, "changed@example.com").await.verified);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_resends_the_verification_email() {
    with_app(|mut server, ctx| async move {
        unverify(&ctx, ONE).await;
        sign_in(&mut server, &ctx, ONE).await;

        let res = server
            .post(route_table::IDENTITY_EMAIL_VERIFICATION)
            .json(&json!({}))
            .await;
        assert!(res.status_code().is_redirection());
        let mails = deliveries(&ctx);
        assert_eq!(mails.len(), 1);
        assert!(mails[0].contains("To: one@example.com"));
        assert!(mails[0].contains("Subject: Verify your email"));
        // The emailed link verifies the address.
        let sid = sid_from_mail(&mails[0]);
        server.get(&verification_path(&sid)).await;
        assert!(super::user(&ctx, ONE).await.verified);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_requires_authentication() {
    with_app(|server, ctx| async move {
        let res = server
            .post(route_table::IDENTITY_EMAIL_VERIFICATION)
            .json(&json!({}))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        assert!(deliveries(&ctx).is_empty());
    })
    .await;
}
