//! `spec/requests/users_spec.rb`, plus sign-up precognition (validates, never writes), and
//! sign-up `invitation_only` (`settings.sign_up`). Each sign-up test pins its mode with
//! `with_sign_up`, so it passes whatever config/test.yaml says.

use inertia_rust_starter_kit::{
    controllers::users::invitation_only_alert,
    models::{invitations::SEED_TOKEN, users::Entity as Users},
    route_table,
};
use sea_orm::{EntityTrait, PaginatorTrait};
use serde_json::{json, Value};
use serial_test::serial;

use super::*;

async fn user_count(ctx: &AppContext) -> u64 {
    Users::find().count(&ctx.db).await.unwrap()
}

fn new_user() -> Value {
    json!({
        "name": "New User",
        "email": "new@example.com",
        "password": PASSWORD,
        "password_confirmation": PASSWORD,
    })
}

#[tokio::test]
#[serial]
async fn get_sign_up_renders_the_sign_up_page() {
    with_sign_up(SignUp::Open, |server, _ctx| async move {
        let res = server.get(route_table::SIGN_UP).await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("users/new"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_sign_up_redirects_authenticated_users() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.get(route_table::SIGN_UP).await;
        assert_redirect(&res, route_table::ROOT);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_up_creates_a_new_user_signs_in_and_sends_verification() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let before = user_count(&ctx).await;
        let res = server.post(route_table::SIGN_UP).json(&new_user()).await;
        assert_eq!(user_count(&ctx).await, before + 1);
        // the new personal account (SPEC-C1), not /dashboard.
        assert_redirect(&res, "/new-user-s-account");

        let page = inertia_get(&server, &ctx, "/new-user-s-account").await;
        assert_eq!(page["props"]["auth"]["user"]["email"], "new@example.com");
        assert_eq!(page["props"]["auth"]["user"]["verified"], false);
        assert_eq!(
            page["flash"]["notice"],
            "Welcome! You have signed up successfully"
        );

        let mails = deliveries(&ctx);
        assert_eq!(mails.len(), 1);
        assert!(mails[0].contains("Subject: Verify your email"));
        assert!(mails[0].contains("To: new@example.com"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_up_rejects_an_invalid_user_with_errors() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let before = user_count(&ctx).await;
        let res = server
            .post(route_table::SIGN_UP)
            .json(&json!({
                "name": "",
                "email": "invalid",
                "password": "short",
                "password_confirmation": "short",
            }))
            .await;
        assert_eq!(user_count(&ctx).await, before);
        assert_redirect(&res, route_table::SIGN_UP);

        let page = inertia_get(&server, &ctx, route_table::SIGN_UP).await;
        assert_eq!(page["props"]["errors"]["name"], json!(["can't be blank"]));
        assert_eq!(page["props"]["errors"]["email"], json!(["is invalid"]));
        assert_eq!(
            page["props"]["errors"]["password"],
            json!(["is too short (minimum is 12 characters)"])
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_up_precognition_reports_errors_without_writing() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let before = user_count(&ctx).await;
        let res = server
            .post(route_table::SIGN_UP)
            .add_header("precognition", "true")
            .add_header("precognition-validate-only", "email")
            .json(&json!({ "name": "", "email": ONE, "password": "" }))
            .await;
        assert_eq!(res.status_code(), 422);
        assert_eq!(res.header("precognition"), "true");
        let body = res.json::<Value>();
        // Only the touched field is reported.
        assert_eq!(
            body["errors"],
            json!({ "email": ["has already been taken"] })
        );

        let res = server
            .post(route_table::SIGN_UP)
            .add_header("precognition", "true")
            .json(&new_user())
            .await;
        assert_eq!(res.status_code(), 204);
        assert_eq!(res.header("precognition-success"), "true");
        assert!(res.maybe_cookie("session_token").is_none());
        assert_eq!(user_count(&ctx).await, before);
        assert!(deliveries(&ctx).is_empty());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn delete_users_destroys_the_current_user_with_a_valid_password() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let before = user_count(&ctx).await;
        let res = server
            .delete(route_table::USERS)
            .json(&json!({ "password_challenge": PASSWORD }))
            .await;
        assert_eq!(user_count(&ctx).await, before - 1);
        assert_redirect(&res, route_table::ROOT);

        let page = inertia_get(&server, &ctx, route_table::ROOT).await;
        assert_eq!(page["flash"]["notice"], "Your account has been deleted");
        assert_eq!(page["clearHistory"], true);
        assert_eq!(page["props"]["auth"]["user"], Value::Null);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn delete_users_rejects_account_deletion_with_a_wrong_password() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let before = user_count(&ctx).await;
        let res = server
            .delete(route_table::USERS)
            .json(&json!({ "password_challenge": "wrongpassword" }))
            .await;
        assert_eq!(user_count(&ctx).await, before);
        assert_redirect(&res, route_table::SETTINGS_PROFILE);

        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "password_challenge": ["Password challenge is invalid"] })
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_up_with_null_params_is_invalid_not_a_bad_request() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let res = server
            .post(route_table::SIGN_UP)
            .json(&json!({ "name": null, "email": null, "password": null, "password_confirmation": null }))
            .await;
        assert_redirect(&res, route_table::SIGN_UP);
        let page = inertia_get(&server, &ctx, route_table::SIGN_UP).await;
        assert_eq!(
            page["props"]["errors"],
            json!({
                "email": ["can't be blank", "is invalid"],
                "name": ["can't be blank"],
                "password": ["can't be blank"],
            })
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn post_sign_up_rejects_a_password_over_72_bytes() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let long = "é".repeat(37);
        let res = server
            .post(route_table::SIGN_UP)
            .json(&json!({ "name": "Long", "email": "long@example.com", "password": long, "password_confirmation": long }))
            .await;
        assert_redirect(&res, route_table::SIGN_UP);
        let page = inertia_get(&server, &ctx, route_table::SIGN_UP).await;
        assert_eq!(page["props"]["errors"]["password"], json!(["is too long"]));
    })
    .await;
}

// `settings.sign_up: invitation_only` (`SIGN_UP=invitation_only`): sign-up only through a pending
// invitation, with the address it was sent to. The seeds' invitation is to three@example.com
// (`SEED_TOKEN`, admin of Acme).

fn invited_sign_up_page() -> String {
    format!("{}?invitation={SEED_TOKEN}", route_table::SIGN_UP)
}

fn invited_user() -> Value {
    json!({
        "name": "Three",
        "email": "three@example.com",
        "password": PASSWORD,
        "password_confirmation": PASSWORD,
        "invitation": SEED_TOKEN,
    })
}

#[tokio::test]
#[serial]
async fn invitation_only_sign_up_without_a_pending_invitation_goes_to_sign_in() {
    with_sign_up(SignUp::InvitationOnly, |server, ctx| async move {
        for path in [
            route_table::SIGN_UP.to_owned(),
            format!("{}?invitation=not-a-token", route_table::SIGN_UP),
        ] {
            assert_redirect(&server.get(&path).await, route_table::SIGN_IN);
        }
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert_eq!(
            page["flash"]["alert"],
            invitation_only_alert(&settings(&ctx).app_name)
        );
        assert!(page["flash"]["alert"]
            .as_str()
            .unwrap()
            .ends_with(" is invitation only. Ask an admin to invite you"));
        assert_eq!(page["props"]["invitation_only"], true, "no sign-up link");

        // Posting the form directly is refused the same way, precognition included, and
        // creates nobody (no personal account either).
        let before = user_count(&ctx).await;
        for invitation in ["", "not-a-token"] {
            let mut body = new_user();
            body["invitation"] = json!(invitation);
            assert_redirect(
                &server.post(route_table::SIGN_UP).json(&body).await,
                route_table::SIGN_IN,
            );
        }
        let res = server
            .post(route_table::SIGN_UP)
            .add_header("precognition", "true")
            .json(&json!({ "email": ONE }))
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        assert_eq!(user_count(&ctx).await, before);
        assert!(deliveries(&ctx).is_empty());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn invitation_only_sign_up_through_the_invitation_joins_its_account() {
    with_sign_up(SignUp::InvitationOnly, |server, ctx| async move {
        let page = inertia_get(&server, &ctx, &invited_sign_up_page()).await;
        assert_eq!(page["component"], "users/new");
        let before = user_count(&ctx).await;
        let res = server
            .post(route_table::SIGN_UP)
            .json(&invited_user())
            .await;
        assert_redirect(&res, "/acme");
        assert_eq!(user_count(&ctx).await, before + 1);
        let page = inertia_get(&server, &ctx, "/acme").await;
        assert_eq!(page["flash"]["notice"], "Welcome to Acme");
        assert_eq!(page["props"]["auth"]["user"]["verified"], true);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn invitation_only_refuses_another_email_on_the_form_and_an_expired_invitation() {
    with_sign_up(SignUp::InvitationOnly, |server, ctx| async move {
        let before = user_count(&ctx).await;
        let mut body = invited_user();
        body["email"] = json!("someone@example.com");
        let res = server.post(route_table::SIGN_UP).json(&body).await;
        assert_redirect(&res, &invited_sign_up_page());
        let page = inertia_get(&server, &ctx, &invited_sign_up_page()).await;
        assert_eq!(
            page["props"]["errors"]["email"],
            json!(["must be three@example.com, the address invited"])
        );

        travel(&ctx, chrono::Duration::days(8));
        assert_redirect(
            &server.get(&invited_sign_up_page()).await,
            route_table::SIGN_IN,
        );
        let res = server
            .post(route_table::SIGN_UP)
            .json(&invited_user())
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        assert_eq!(user_count(&ctx).await, before);
    })
    .await;
}

/// Open (the default) keeps the Rails kit's sign-in page: no props, the link always shown.
#[tokio::test]
#[serial]
async fn open_sign_up_leaves_the_sign_in_page_as_it_was() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let page = inertia_get(&server, &ctx, route_table::SIGN_IN).await;
        assert!(page["props"].get("invitation_only").is_none(), "{page}");
    })
    .await;
}
