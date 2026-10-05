//! `spec/requests/settings/{emails,passwords,sessions}_spec.rb`, plus profile updates and:
//! other sessions are invalidated after a password change.

use inertia_rust_starter_kit::{models::sessions, route_table};
use serde_json::json;
use serial_test::serial;

use super::*;

#[tokio::test]
#[serial]
async fn get_settings_email_renders_the_email_settings_page() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.get(route_table::SETTINGS_EMAIL).await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("settings/emails/show"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_settings_email_with_a_valid_challenge_updates_the_email() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": "updated@example.com", "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert_eq!(page["flash"]["notice"], "Your email has been changed");
        let updated = user(&ctx, "updated@example.com").await;
        assert!(!updated.verified);
        let mails = deliveries(&ctx);
        assert_eq!(mails.len(), 1);
        assert!(mails[0].contains("To: updated@example.com"));
        assert!(mails[0].contains("Subject: Verify your email"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_settings_email_with_the_same_email_redirects_without_a_notice_or_mail() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": ONE, "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert!(page.get("flash").is_none_or(|f| f.get("notice").is_none()));
        assert!(deliveries(&ctx).is_empty());
        assert!(user(&ctx, ONE).await.verified);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_settings_email_with_an_invalid_challenge_returns_inertia_errors() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": "updated@example.com", "password_challenge": "wrongpassword" }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "password_challenge": ["is invalid"] })
        );
        assert_eq!(user(&ctx, ONE).await.email, ONE);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_settings_password_renders_the_password_settings_page() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.get(route_table::SETTINGS_PASSWORD).await;
        assert_eq!(res.status_code(), 200);
        assert!(res.text().contains("settings/passwords/show"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_settings_password_with_a_valid_challenge_updates_the_password() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .json(&json!({
                "password": "NewPassword1*3*",
                "password_confirmation": "NewPassword1*3*",
                "password_challenge": PASSWORD,
            }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PASSWORD);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(page["flash"]["notice"], "Your password has been changed");
        assert!(user(&ctx, ONE).await.authenticate("NewPassword1*3*"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_password_change_logs_out_every_other_session_but_this_one() {
    with_app(|mut server, ctx| async move {
        let current = sign_in(&mut server, &ctx, ONE).await;
        server
            .patch(route_table::SETTINGS_PASSWORD)
            .json(&json!({
                "password": "NewPassword1*3*",
                "password_confirmation": "NewPassword1*3*",
                "password_challenge": PASSWORD,
            }))
            .await;
        let one = user(&ctx, ONE).await;
        let left: Vec<String> = sessions::Model::list_for_user(&ctx.db, one.id)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.token)
            .collect();
        assert_eq!(left, vec![current]);
        // This browser is still signed in; the fixture browser is not.
        assert_eq!(
            server
                .get(route_table::SETTINGS_PROFILE)
                .await
                .status_code(),
            200
        );
        server.clear_cookies();
        sign_in_with_token(&mut server, &ctx, ONE_SESSION);
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

#[tokio::test]
#[serial]
async fn patch_settings_password_with_an_invalid_challenge_returns_inertia_errors() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .json(&json!({
                "password": "NewPassword1*3*",
                "password_confirmation": "NewPassword1*3*",
                "password_challenge": "wrongpassword",
            }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PASSWORD);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "password_challenge": ["is invalid"] })
        );
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, ONE_SESSION)
                .await
                .is_ok()
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_settings_sessions_renders_the_sessions_index() {
    with_app(|mut server, ctx| async move {
        let current = sign_in(&mut server, &ctx, ONE).await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_SESSIONS).await;
        assert_eq!(page["component"], "settings/sessions/index");
        let sessions = page["props"]["sessions"].as_array().unwrap();
        // Newest first, own sessions only, exposed by token with exactly these keys.
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0]["id"], current);
        assert_eq!(sessions[1]["id"], ONE_SESSION);
        assert_eq!(sessions[1]["user_agent"], "Fixture Browser/1.0 (user one)");
        assert_eq!(sessions[1]["ip_address"], "127.0.0.1");
        let mut keys: Vec<&str> = sessions[1]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["created_at", "id", "ip_address", "user_agent"]);
        // Rails' `as_json` timestamp format: UTC, milliseconds, `Z`.
        assert_eq!(sessions[1]["created_at"], "2025-08-01T12:00:00.000Z");
        assert_eq!(
            page["props"]["auth"]["user"]["created_at"],
            "2025-08-01T12:00:00.000Z"
        );
        assert_eq!(page["props"]["auth"]["session"]["id"], current);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn patch_settings_profile_updates_the_name_or_returns_errors() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .json(&json!({ "name": "Renamed" }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PROFILE);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(page["flash"]["notice"], "Your profile has been updated");
        assert_eq!(page["props"]["auth"]["user"]["name"], "Renamed");

        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .json(&json!({ "name": " " }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PROFILE);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "name": ["can't be blank"] })
        );
        assert_eq!(user(&ctx, ONE).await.name, "Renamed");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn get_settings_appearance_renders_the_appearance_page() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_APPEARANCE).await;
        assert_eq!(page["component"], "settings/appearance");
    })
    .await;
}

/// Review #17: Rails' `resource … only: :update` answers PUT as well as PATCH.
#[tokio::test]
#[serial]
async fn put_is_an_alias_for_patch_on_every_settings_update() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .put(route_table::SETTINGS_PROFILE)
            .json(&json!({ "name": "Put Name" }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PROFILE);
        assert_eq!(user(&ctx, ONE).await.name, "Put Name");

        let res = server
            .put(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": "put@example.com", "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert!(!user(&ctx, "put@example.com").await.verified);

        let res = server
            .put(route_table::SETTINGS_PASSWORD)
            .add_header("x-inertia", "true")
            .json(&json!({
                "password": "NewPassword1*3*",
                "password_confirmation": "NewPassword1*3*",
                "password_challenge": PASSWORD,
            }))
            .await;
        // Inertia PUT redirects become 303, like PATCH.
        assert_eq!(res.status_code(), 303);
        assert!(user(&ctx, "put@example.com")
            .await
            .authenticate("NewPassword1*3*"));
    })
    .await;
}

/// Review #18: Rails only normalizes email; names are stored as typed.
#[tokio::test]
#[serial]
async fn profile_names_are_stored_untrimmed() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        server
            .patch(route_table::SETTINGS_PROFILE)
            .json(&json!({ "name": " Ada " }))
            .await;
        assert_eq!(user(&ctx, ONE).await.name, " Ada ");
    })
    .await;
}

/// Review #18: an omitted attribute keeps its value (`user.update(params.permit(...))`),
/// an explicitly empty one is still validated.
#[tokio::test]
#[serial]
async fn omitted_fields_keep_their_value_and_empty_ones_are_validated() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let before = user(&ctx, ONE).await;

        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .json(&json!({}))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PROFILE);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(page["flash"]["notice"], "Your profile has been updated");
        assert_eq!(page["props"]["errors"], json!({}));
        assert_eq!(user(&ctx, ONE).await.name, before.name);

        server
            .patch(route_table::SETTINGS_PROFILE)
            .json(&json!({ "name": "" }))
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "name": ["can't be blank"] })
        );

        // Email: omitted keeps it (no notice, no mail); empty is invalid.
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert_eq!(user(&ctx, ONE).await.email, ONE);
        assert!(deliveries(&ctx).is_empty());
        server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": "", "password_challenge": PASSWORD }))
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "email": ["can't be blank", "is invalid"] })
        );
    })
    .await;
}

/// Review #18: `Settings::PasswordsController#update` without a password runs
/// `@user.update(password_challenge: …)`: the challenge is still checked, nothing else
/// changes, and the success notice is shown (no other session is logged out).
#[tokio::test]
#[serial]
async fn a_password_update_without_a_password_only_checks_the_challenge() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .json(&json!({ "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PASSWORD);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(page["flash"]["notice"], "Your password has been changed");
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
        assert!(
            sessions::Model::find_by_token_with_user(&ctx.db, ONE_SESSION)
                .await
                .is_ok()
        );

        server
            .patch(route_table::SETTINGS_PASSWORD)
            .json(&json!({ "password_challenge": "wrongpassword" }))
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "password_challenge": ["is invalid"] })
        );
    })
    .await;
}

/// Review N8: `params.permit` tells a missing attribute (kept) from an explicit nil (assigned,
/// then validated like a blank value) from a value (assigned), for JSON and form bodies alike.
/// A form cannot express nil, so it only has the missing and value (including `""`) cases.
#[tokio::test]
#[serial]
async fn missing_null_and_given_name_and_email_are_three_different_updates() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let before = user(&ctx, ONE).await;
        let profile_errors = |page: Value| page["props"]["errors"].clone();

        // Name: null is blank.
        let res = server
            .patch(route_table::SETTINGS_PROFILE)
            .json(&json!({ "name": null }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PROFILE);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(profile_errors(page), json!({ "name": ["can't be blank"] }));
        assert_eq!(user(&ctx, ONE).await.name, before.name);

        // Name, form-encoded: missing keeps it, empty is blank, a value is stored.
        server
            .patch(route_table::SETTINGS_PROFILE)
            .form(&[("unrelated", "x")])
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(page["flash"]["notice"], "Your profile has been updated");
        assert_eq!(user(&ctx, ONE).await.name, before.name);
        server
            .patch(route_table::SETTINGS_PROFILE)
            .form(&[("name", "")])
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PROFILE).await;
        assert_eq!(profile_errors(page), json!({ "name": ["can't be blank"] }));
        server
            .patch(route_table::SETTINGS_PROFILE)
            .form(&[("name", "Form Name")])
            .await;
        assert_eq!(user(&ctx, ONE).await.name, "Form Name");

        // Email: null fails presence and format; nothing is written or mailed.
        server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": null, "password_challenge": PASSWORD }))
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "email": ["can't be blank", "is invalid"] })
        );
        assert_eq!(user(&ctx, ONE).await.email, ONE);
        assert!(deliveries(&ctx).is_empty());

        // Email, form-encoded: missing keeps it, empty is invalid, a value changes it.
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .form(&[("password_challenge", PASSWORD)])
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        assert_eq!(user(&ctx, ONE).await.email, ONE);
        assert!(deliveries(&ctx).is_empty());
        server
            .patch(route_table::SETTINGS_EMAIL)
            .form(&[("email", ""), ("password_challenge", PASSWORD)])
            .await;
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ "email": ["can't be blank", "is invalid"] })
        );
        server
            .patch(route_table::SETTINGS_EMAIL)
            .form(&[
                ("email", "Form@Example.com"),
                ("password_challenge", PASSWORD),
            ])
            .await;
        let changed = user(&ctx, "form@example.com").await;
        assert_eq!(changed.id, before.id);
        assert!(!changed.verified);
        assert_eq!(deliveries(&ctx).len(), 1);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_null_password_challenge_is_as_invalid_as_a_missing_one() {
    // Rails' `with_defaults(password_challenge: "")` fills only a *missing* key, so a JSON
    // `null` skips the challenge there (docs/PARITY.md). Here it is checked like `""`.
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_EMAIL)
            .json(&json!({ "email": "stolen@example.com", "password_challenge": null }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_EMAIL);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_EMAIL).await;
        assert_eq!(
            page["props"]["errors"]["password_challenge"],
            json!(["is invalid"])
        );
        assert_eq!(user(&ctx, ONE).await.email, ONE);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_null_new_password_is_blank_like_rails() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server
            .patch(route_table::SETTINGS_PASSWORD)
            .json(&json!({ "password": null, "password_challenge": PASSWORD }))
            .await;
        assert_redirect(&res, route_table::SETTINGS_PASSWORD);
        let page = inertia_get(&server, &ctx, route_table::SETTINGS_PASSWORD).await;
        assert_eq!(
            page["props"]["errors"]["password"],
            json!(["can't be blank"])
        );
        assert!(page.get("flash").is_none_or(|f| f.get("notice").is_none()));
    })
    .await;
}
