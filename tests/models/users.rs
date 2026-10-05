//! The User model: the Rails kit's `app/models/user.rb` behaviour.

use chrono::{Duration, Utc};
use inertia_rust_starter_kit::{
    app::App,
    models::{
        sessions::{self, RequestDetails},
        tokens::{FixedClock, Purpose, SystemClock},
        users::{self, PasswordParams, SaveError, SignUpParams},
    },
};
use loco_rs::{app::AppContext, model::ModelError, testing::prelude::*};
use serial_test::serial;

const PASSWORD: &str = "Secret1*3*5*";
const KEY: &[u8] = b"model-test-secret-key-base-0123456789abcdef0123456789abcdef";

async fn boot() -> AppContext {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    boot.app_context
}

fn invalid(err: SaveError) -> std::collections::BTreeMap<String, Vec<String>> {
    match err {
        SaveError::Invalid(errors) => errors.into_inner(),
        err @ (SaveError::Stale | SaveError::Model(_)) => {
            panic!("expected validation errors, got {err}")
        }
    }
}

fn sign_up_params(email: &str) -> SignUpParams {
    SignUpParams {
        name: "New User".into(),
        email: email.into(),
        password: PASSWORD.into(),
        password_confirmation: Some(PASSWORD.into()),
    }
}

#[tokio::test]
#[serial]
async fn sign_up_normalizes_email_and_hashes_with_argon2id() {
    let ctx = boot().await;
    let user = users::Model::sign_up(&ctx.db, &sign_up_params("  New@Example.COM "))
        .await
        .unwrap();
    assert_eq!(user.email, "new@example.com");
    assert!(!user.verified);
    assert!(user.password_digest.starts_with("$argon2id$"));
    assert!(user.authenticate(PASSWORD));
}

#[tokio::test]
#[serial]
async fn sign_up_reports_every_invalid_attribute_with_rails_messages() {
    let ctx = boot().await;
    let errors = invalid(
        users::Model::sign_up(
            &ctx.db,
            &SignUpParams {
                name: String::new(),
                email: "invalid".into(),
                password: "short".into(),
                password_confirmation: Some("different".into()),
            },
        )
        .await
        .unwrap_err(),
    );
    assert_eq!(errors["name"], vec!["can't be blank"]);
    assert_eq!(errors["email"], vec!["is invalid"]);
    assert_eq!(
        errors["password"],
        vec!["is too short (minimum is 12 characters)"]
    );
    assert_eq!(
        errors["password_confirmation"],
        vec!["doesn't match Password"]
    );
}

#[tokio::test]
#[serial]
async fn sign_up_rejects_a_taken_email_case_insensitively() {
    let ctx = boot().await;
    let errors = invalid(
        users::Model::sign_up(&ctx.db, &sign_up_params("ONE@example.com"))
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["email"], vec!["has already been taken"]);
}

#[tokio::test]
#[serial]
async fn authenticate_by_accepts_only_the_right_password() {
    let ctx = boot().await;
    let user = users::Model::authenticate_by(&ctx.db, " One@Example.com", PASSWORD)
        .await
        .unwrap()
        .expect("fixture user signs in");
    assert_eq!(user.email, "one@example.com");

    assert!(
        users::Model::authenticate_by(&ctx.db, "one@example.com", "wrongpassword")
            .await
            .unwrap()
            .is_none()
    );
    // Unknown email still does a (dummy) verify and returns None, not an error.
    assert!(
        users::Model::authenticate_by(&ctx.db, "missing@example.com", PASSWORD)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[serial]
async fn changing_email_requires_the_challenge_and_unverifies() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    assert!(user.verified);

    let errors = invalid(
        user.clone()
            .change_email(&ctx.db, Some("updated@example.com"), "wrongpassword")
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["password_challenge"], vec!["is invalid"]);

    let (user, changed) = user
        .change_email(&ctx.db, Some("Updated@Example.com"), PASSWORD)
        .await
        .unwrap();
    assert!(changed);
    assert_eq!(user.email, "updated@example.com");
    assert!(!user.verified, "a new address must be verified again");

    let (user, changed) = user
        .change_email(&ctx.db, Some("updated@example.com"), PASSWORD)
        .await
        .unwrap();
    assert!(!changed, "same email is not a change");
    assert!(!user.verified);
}

#[tokio::test]
#[serial]
async fn changing_email_to_a_taken_address_fails() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let errors = invalid(
        user.change_email(&ctx.db, Some("two@example.com"), PASSWORD)
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["email"], vec!["has already been taken"]);
}

#[tokio::test]
#[serial]
async fn changing_password_keeps_only_the_current_session() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let current = sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    let other = sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    // User two's session must survive user one's password change.
    let two = users::Model::find_by_email(&ctx.db, "two@example.com")
        .await
        .unwrap();
    let two_session = sessions::Model::create_for_user(&ctx.db, &two, &RequestDetails::default())
        .await
        .unwrap();

    let params = PasswordParams {
        password: Some(Some("NewPassword1*3*".into())),
        password_confirmation: Some("NewPassword1*3*".into()),
    };
    let errors = invalid(
        user.clone()
            .change_password(&ctx.db, &params, "wrongpassword", Some(current.id))
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["password_challenge"], vec!["is invalid"]);

    let user = user
        .change_password(&ctx.db, &params, PASSWORD, Some(current.id))
        .await
        .unwrap();
    assert!(user.authenticate("NewPassword1*3*"));
    assert!(!user.authenticate(PASSWORD));

    let remaining: Vec<_> = sessions::Model::list_for_user(&ctx.db, user.id)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(remaining, vec![current.id]);
    assert!(!remaining.contains(&other.id));
    assert!(
        sessions::Model::find_by_token_with_user(&ctx.db, &two_session.token)
            .await
            .is_ok()
    );
}

#[tokio::test]
#[serial]
async fn changing_password_validates_length_and_confirmation() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let errors = invalid(
        user.change_password(
            &ctx.db,
            &PasswordParams {
                password: Some(Some("short".into())),
                password_confirmation: Some("different".into()),
            },
            PASSWORD,
            None,
        )
        .await
        .unwrap_err(),
    );
    assert!(errors.contains_key("password"));
    assert!(errors.contains_key("password_confirmation"));
}

#[tokio::test]
#[serial]
async fn resetting_password_signs_out_everywhere() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    let params = PasswordParams {
        password: Some(Some("NewPassword1*3*".into())),
        password_confirmation: Some("NewPassword1*3*".into()),
    };
    let user = user.reset_password(&ctx.db, &params).await.unwrap();
    assert!(sessions::Model::list_for_user(&ctx.db, user.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
#[serial]
async fn email_verification_token_verifies_until_it_expires_or_the_email_changes() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let now = Utc::now();
    let token = user.generate_token_for(Purpose::EmailVerification, KEY, &FixedClock(now));

    let found = users::Model::find_by_token_for(
        &ctx.db,
        Purpose::EmailVerification,
        &token,
        KEY,
        &FixedClock(now + Duration::days(1)),
    )
    .await
    .unwrap();
    assert_eq!(found.id, user.id);

    // `travel 3.days`
    assert!(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::EmailVerification,
        &token,
        KEY,
        &FixedClock(now + Duration::days(3)),
    )
    .await
    .is_err());

    // Changing the email invalidates the outstanding link.
    user.change_email(&ctx.db, Some("changed@example.com"), PASSWORD)
        .await
        .unwrap();
    assert!(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::EmailVerification,
        &token,
        KEY,
        &FixedClock(now),
    )
    .await
    .is_err());
}

#[tokio::test]
#[serial]
async fn password_reset_token_dies_after_twenty_minutes_or_a_password_change() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let now = Utc::now();
    let token = user.generate_token_for(Purpose::PasswordReset, KEY, &FixedClock(now));

    assert!(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::PasswordReset,
        &token,
        KEY,
        &FixedClock(now + Duration::minutes(19)),
    )
    .await
    .is_ok());
    // `travel 30.minutes`
    assert!(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::PasswordReset,
        &token,
        KEY,
        &FixedClock(now + Duration::minutes(30)),
    )
    .await
    .is_err());

    // Using the token once (changing the password) makes it unusable.
    let params = PasswordParams {
        password: Some(Some("NewPassword1*3*".into())),
        password_confirmation: Some("NewPassword1*3*".into()),
    };
    user.reset_password(&ctx.db, &params).await.unwrap();
    assert!(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::PasswordReset,
        &token,
        KEY,
        &FixedClock(now),
    )
    .await
    .is_err());
}

#[tokio::test]
#[serial]
async fn an_email_verification_token_is_not_a_password_reset_token() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let token = user.generate_token_for(Purpose::EmailVerification, KEY, &SystemClock);
    assert!(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::PasswordReset,
        &token,
        KEY,
        &SystemClock
    )
    .await
    .is_err());
}

#[tokio::test]
#[serial]
async fn verify_email_and_find_verified_by_email() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let (user, _) = user
        .change_email(&ctx.db, Some("fresh@example.com"), PASSWORD)
        .await
        .unwrap();
    assert!(
        users::Model::find_verified_by_email(&ctx.db, "fresh@example.com")
            .await
            .is_err()
    );
    let user = user.verify_email(&ctx.db).await.unwrap();
    assert!(user.verified);
    assert!(
        users::Model::find_verified_by_email(&ctx.db, "FRESH@example.com")
            .await
            .is_ok()
    );
}

#[tokio::test]
#[serial]
async fn destroy_requires_the_challenge_and_removes_sessions() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let session = sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();

    let errors = invalid(
        user.clone()
            .destroy_with_challenge(&ctx.db, "wrongpassword")
            .await
            .unwrap_err(),
    );
    assert_eq!(
        errors["password_challenge"],
        vec!["Password challenge is invalid"]
    );

    user.clone()
        .destroy_with_challenge(&ctx.db, PASSWORD)
        .await
        .unwrap();
    assert!(users::Model::find_by_id(&ctx.db, user.id).await.is_err());
    assert!(
        sessions::Model::find_by_token_with_user(&ctx.db, &session.token)
            .await
            .is_err()
    );
}

#[tokio::test]
#[serial]
async fn update_profile_requires_a_name() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let errors = invalid(
        user.clone()
            .update_profile(&ctx.db, Some("  "))
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["name"], vec!["can't be blank"]);
    let user = user.update_profile(&ctx.db, Some("Renamed")).await.unwrap();
    assert_eq!(user.name, "Renamed");
}

// ---------------------------------------------------------------------------
// Atomicity (review #2): each test runs the first half of one flow, commits a competing
// change, then runs the second half — the interleaving a concurrent request produces.
// ---------------------------------------------------------------------------

fn new_password(password: &str) -> PasswordParams {
    PasswordParams {
        password: Some(Some(password.into())),
        password_confirmation: Some(password.into()),
    }
}

#[tokio::test]
#[serial]
async fn verification_checked_before_an_email_change_does_not_verify_the_new_email() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let token = user.generate_token_for(Purpose::EmailVerification, KEY, &SystemClock);
    // A: the link is checked against the old email.
    let checked = users::Model::find_by_token_for(
        &ctx.db,
        Purpose::EmailVerification,
        &token,
        KEY,
        &SystemClock,
    )
    .await
    .unwrap();
    // Meanwhile the email changes (and is unverified).
    user.change_email(&ctx.db, Some("new@example.com"), PASSWORD)
        .await
        .unwrap();
    // B: verifying with the stale load must not verify the new address.
    assert!(matches!(
        checked.verify_email(&ctx.db).await,
        Err(ModelError::EntityNotFound)
    ));
    let now = users::Model::find_by_email(&ctx.db, "new@example.com")
        .await
        .unwrap();
    assert!(!now.verified);
}

#[tokio::test]
#[serial]
async fn of_two_resets_validated_with_the_same_token_exactly_one_wins() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let token = user.generate_token_for(Purpose::PasswordReset, KEY, &SystemClock);
    let load = || {
        users::Model::find_by_token_for(&ctx.db, Purpose::PasswordReset, &token, KEY, &SystemClock)
    };
    // Both requests validate the token before either writes.
    let first = load().await.unwrap();
    let second = load().await.unwrap();

    first
        .reset_password(&ctx.db, &new_password("FirstWinner1*3*"))
        .await
        .unwrap();
    let session = sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    assert!(matches!(
        second
            .reset_password(&ctx.db, &new_password("SecondLoser1*3*"))
            .await,
        Err(SaveError::Stale)
    ));
    let user = users::Model::find_by_id(&ctx.db, user.id).await.unwrap();
    assert!(user.authenticate("FirstWinner1*3*"));
    assert!(!user.authenticate("SecondLoser1*3*"));
    // The losing reset rolled back entirely: it deleted no sessions.
    assert!(
        sessions::Model::find_by_token_with_user(&ctx.db, &session.token)
            .await
            .is_ok()
    );
}

#[tokio::test]
#[serial]
async fn a_sign_in_checked_before_a_reset_creates_no_session() {
    let ctx = boot().await;
    // A: the old password is verified.
    let authenticated = users::Model::authenticate_by(&ctx.db, "one@example.com", PASSWORD)
        .await
        .unwrap()
        .unwrap();
    // Meanwhile a reset changes the password and deletes every session.
    authenticated
        .clone()
        .reset_password(&ctx.db, &new_password("NewPassword1*3*"))
        .await
        .unwrap();
    // B: the session insert re-checks the digest and refuses.
    let session = sessions::Model::create_for_authenticated_user(
        &ctx.db,
        &authenticated,
        &RequestDetails::default(),
    )
    .await
    .unwrap();
    assert!(session.is_none());
    assert!(sessions::Model::list_for_user(&ctx.db, authenticated.id)
        .await
        .unwrap()
        .is_empty());
    // Without the competing change the same call signs in.
    let fresh = users::Model::authenticate_by(&ctx.db, "one@example.com", "NewPassword1*3*")
        .await
        .unwrap()
        .unwrap();
    assert!(sessions::Model::create_for_authenticated_user(
        &ctx.db,
        &fresh,
        &RequestDetails::default()
    )
    .await
    .unwrap()
    .is_some());
}

#[tokio::test]
#[serial]
async fn a_password_change_whose_challenge_went_stale_is_rejected() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let stale = user.clone();
    user.reset_password(&ctx.db, &new_password("ResetMeanwhile1*"))
        .await
        .unwrap();
    // The challenge matched the digest `stale` was loaded with, not the current one.
    let errors = invalid(
        stale
            .change_password(&ctx.db, &new_password("NewPassword1*3*"), PASSWORD, None)
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["password_challenge"], vec!["is invalid"]);
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    assert!(user.authenticate("ResetMeanwhile1*"));
}

#[tokio::test]
#[serial]
async fn an_email_change_whose_challenge_went_stale_is_rejected() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let stale = user.clone();
    user.reset_password(&ctx.db, &new_password("ResetMeanwhile1*"))
        .await
        .unwrap();
    // Both a real change and a no-op one (email omitted or unchanged) are refused.
    for email in [Some("attacker@example.com"), Some("one@example.com"), None] {
        let errors = invalid(
            stale
                .clone()
                .change_email(&ctx.db, email, PASSWORD)
                .await
                .unwrap_err(),
        );
        assert_eq!(
            errors["password_challenge"],
            vec!["is invalid"],
            "{email:?}"
        );
    }
    let user = users::Model::find_by_id(&ctx.db, stale.id).await.unwrap();
    assert_eq!(user.email, "one@example.com");
    assert_eq!(user.verified, stale.verified);
    assert!(user.authenticate("ResetMeanwhile1*"));
}

#[tokio::test]
#[serial]
async fn an_account_deletion_whose_challenge_went_stale_deletes_nothing() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let stale = user.clone();
    user.reset_password(&ctx.db, &new_password("ResetMeanwhile1*"))
        .await
        .unwrap();
    // The owner signs back in after the reset; that session must survive the stale delete.
    let user = users::Model::find_by_id(&ctx.db, stale.id).await.unwrap();
    let session = sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    let errors = invalid(
        stale
            .destroy_with_challenge(&ctx.db, PASSWORD)
            .await
            .unwrap_err(),
    );
    assert_eq!(
        errors["password_challenge"],
        vec!["Password challenge is invalid"]
    );
    assert!(users::Model::find_by_id(&ctx.db, user.id).await.is_ok());
    assert!(
        sessions::Model::find_by_token_with_user(&ctx.db, &session.token)
            .await
            .is_ok()
    );
}

// ---------------------------------------------------------------------------
// Rails attribute semantics (review #18)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn names_are_stored_exactly_as_given() {
    let ctx = boot().await;
    let mut params = sign_up_params("ada@example.com");
    params.name = " Ada ".into();
    let user = users::Model::sign_up(&ctx.db, &params).await.unwrap();
    assert_eq!(user.name, " Ada ");
    let user = user
        .update_profile(&ctx.db, Some("  Lovelace "))
        .await
        .unwrap();
    assert_eq!(user.name, "  Lovelace ");
}

#[tokio::test]
#[serial]
async fn omitted_update_fields_keep_their_value_and_empty_ones_still_validate() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let name = user.name.clone();
    let user = user.update_profile(&ctx.db, None).await.unwrap();
    assert_eq!(user.name, name);
    let errors = invalid(
        user.clone()
            .update_profile(&ctx.db, Some(""))
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["name"], vec!["can't be blank"]);

    let (user, changed) = user.change_email(&ctx.db, None, PASSWORD).await.unwrap();
    assert!(!changed);
    assert_eq!(user.email, "one@example.com");
    let errors = invalid(
        user.clone()
            .change_email(&ctx.db, Some(""), PASSWORD)
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["email"], vec!["can't be blank", "is invalid"]);
    // The challenge is required even when nothing changes.
    let errors = invalid(
        user.change_email(&ctx.db, None, "wrongpassword")
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["password_challenge"], vec!["is invalid"]);
}

/// `has_secure_password`: `password=` ignores nil and `""`, and the length and confirmation
/// validations allow nil, so an update without a new password just re-saves.
#[tokio::test]
#[serial]
async fn a_password_change_without_a_new_password_keeps_the_password_and_sessions() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let digest = user.password_digest.clone();
    let before = sessions::Model::list_for_user(&ctx.db, user.id)
        .await
        .unwrap()
        .len();
    assert!(before > 0);
    for params in [
        PasswordParams::default(),
        PasswordParams {
            password: Some(Some(String::new())),
            password_confirmation: Some("does not matter".into()),
        },
    ] {
        let saved = user
            .clone()
            .change_password(&ctx.db, &params, PASSWORD, None)
            .await
            .unwrap();
        assert_eq!(saved.password_digest, digest);
        let errors = invalid(
            user.clone()
                .change_password(&ctx.db, &params, "wrongpassword", None)
                .await
                .unwrap_err(),
        );
        assert_eq!(errors["password_challenge"], vec!["is invalid"]);
    }
    assert_eq!(
        sessions::Model::list_for_user(&ctx.db, user.id)
            .await
            .unwrap()
            .len(),
        before
    );
    // On create a missing password is `can't be blank` only (length allows nil).
    let mut params = sign_up_params("blank@example.com");
    params.password = String::new();
    params.password_confirmation = None;
    let errors = invalid(users::Model::sign_up(&ctx.db, &params).await.unwrap_err());
    assert_eq!(errors["password"], vec!["can't be blank"]);
}

// `task seed:demo` (src/tasks/seed_demo.rs): the demo admin on a deploy.

async fn seed_demo(ctx: &AppContext, email: &str, password: &str) -> loco_rs::Result<()> {
    let vars = loco_rs::task::Vars::from_cli_args(vec![
        ("email".into(), email.into()),
        ("password".into(), password.into()),
    ]);
    loco_rs::boot::run_task::<App>(ctx, Some(&"seed:demo".to_string()), &vars).await
}

#[tokio::test]
#[serial]
async fn seed_demo_creates_a_verified_user_who_can_sign_in_and_is_idempotent() {
    let ctx = boot().await;
    seed_demo(&ctx, " Admin@Example.com ", "password12345!")
        .await
        .unwrap();
    seed_demo(&ctx, "admin@example.com", "password12345!")
        .await
        .unwrap();

    let admin = users::Model::authenticate_by(&ctx.db, "admin@example.com", "password12345!")
        .await
        .unwrap()
        .expect("the demo admin signs in with the seeded password");
    assert!(admin.verified);
    assert!(admin.password_digest.starts_with("$argon2id$"));
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
    let rows = users::Entity::find()
        .filter(users::Column::Email.eq("admin@example.com"))
        .count(&ctx.db)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
#[serial]
async fn seed_demo_gives_a_new_demo_admin_one_personal_account() {
    use inertia_rust_starter_kit::models::accounts;
    let ctx = boot().await;
    for _ in 0..2 {
        seed_demo(&ctx, "admin@example.com", "password12345!")
            .await
            .unwrap();
    }
    let admin = users::Model::find_by_email(&ctx.db, "admin@example.com")
        .await
        .unwrap();
    let mine = accounts::Model::list_for_user(&ctx.db, admin.id)
        .await
        .unwrap();
    let names: Vec<&str> = mine.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Admin's account"]);
}

#[tokio::test]
#[serial]
async fn seed_demo_with_an_account_makes_the_demo_admin_an_owner_of_it() {
    use inertia_rust_starter_kit::models::{accounts, memberships};
    let ctx = boot().await;
    let vars = loco_rs::task::Vars::from_cli_args(vec![
        ("email".into(), "admin@example.com".into()),
        ("password".into(), "password12345!".into()),
        ("account".into(), "acme".into()),
    ]);
    for _ in 0..2 {
        loco_rs::boot::run_task::<App>(&ctx, Some(&"seed:demo".to_string()), &vars)
            .await
            .unwrap();
    }
    let admin = users::Model::find_by_email(&ctx.db, "admin@example.com")
        .await
        .unwrap();
    let acme = accounts::Model::find_by_slug(&ctx.db, "acme")
        .await
        .unwrap();
    let membership = memberships::Model::find_for(&ctx.db, acme.id, admin.id)
        .await
        .expect("the demo admin is a member of acme");
    assert_eq!(membership.role(), memberships::Role::Owner);
    // Only that account: no personal one on top.
    assert_eq!(
        accounts::Model::list_for_user(&ctx.db, admin.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
#[serial]
async fn seed_demo_on_an_existing_user_verifies_it_resets_the_password_and_ends_its_sessions() {
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
    let ctx = boot().await;
    let user = users::Model::sign_up(&ctx.db, &sign_up_params("taken@example.com"))
        .await
        .unwrap();
    sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();

    seed_demo(&ctx, "taken@example.com", "password12345!")
        .await
        .unwrap();

    let user = users::Model::find_by_id(&ctx.db, user.id).await.unwrap();
    assert!(user.verified);
    assert!(user.authenticate("password12345!"));
    assert!(!user.authenticate(PASSWORD));
    let left = sessions::Entity::find()
        .filter(sessions::Column::UserId.eq(user.id))
        .count(&ctx.db)
        .await
        .unwrap();
    assert_eq!(left, 0, "sessions signed in with the old password are gone");
}

#[tokio::test]
#[serial]
async fn seed_demo_refuses_a_short_password_and_writes_nothing() {
    use sea_orm::{EntityTrait, PaginatorTrait};
    let ctx = boot().await;
    let before = users::Entity::find().count(&ctx.db).await.unwrap();
    let err = seed_demo(&ctx, "admin@example.com", "short")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("too short"), "{err}");
    assert_eq!(users::Entity::find().count(&ctx.db).await.unwrap(), before);
}

// ---------------------------------------------------------------------------
// has_secure_password / normalizes details found by bench/parity (docs/PARITY.md)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn a_password_over_72_bytes_is_too_long_like_has_secure_password() {
    let ctx = boot().await;
    // 72 bytes is the most bcrypt uses; Rails counts bytes, so 37 two-byte chars (74 bytes)
    // are too long while 72 ASCII chars are fine.
    for (password, too_long) in [
        ("x".repeat(72), false),
        ("x".repeat(73), true),
        ("é".repeat(37), true),
    ] {
        let params = SignUpParams {
            password: password.clone(),
            password_confirmation: Some(password.clone()),
            ..sign_up_params("long@example.com")
        };
        let errors = users::Model::sign_up_errors(&ctx.db, &params)
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            errors.get("password").cloned().unwrap_or_default(),
            if too_long {
                vec!["is too long".to_string()]
            } else {
                vec![]
            },
            "{} bytes",
            password.len()
        );
    }
}

#[tokio::test]
#[serial]
async fn a_null_new_password_clears_the_digest_and_is_blank_like_rails() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let params = PasswordParams {
        password: Some(None),
        password_confirmation: None,
    };
    let errors = invalid(
        user.change_password(&ctx.db, &params, PASSWORD, None)
            .await
            .unwrap_err(),
    );
    assert_eq!(errors["password"], ["can't be blank"]);
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    assert!(user.authenticate(PASSWORD), "nothing was written");
}

#[test]
fn email_normalization_strips_like_ruby_strip_and_downcases_unicode() {
    // Ruby's `strip` removes ASCII whitespace and NUL only.
    assert_eq!(
        users::normalize_email("\0\t\n\x0b\x0c\r Ann@Example.COM \r\n\0"),
        "ann@example.com"
    );
    assert_eq!(
        users::normalize_email("\u{a0}ann@example.com\u{3000}"),
        "\u{a0}ann@example.com\u{3000}"
    );
    assert_eq!(users::normalize_email("ÀNN@example.com"), "ànn@example.com");
}
