//! Review #4: a `Precognition: true` request never writes, sends mail, spends a reset token
//! or changes the session. Endpoints with a validation-only mode answer with their errors
//! (422, or 204 + `Precognition-Success`); the rest refuse with 400 before any side effect.
//! Every case compares the users and sessions tables and the mail deliveries before and after.

use axum::http::StatusCode;
use inertia_rust_starter_kit::{
    controllers::rate_limit::LIMIT, inertia::cookies, models::tokens::Purpose, route_table,
};
use serde_json::{json, Value};
use serial_test::serial;

use super::*;

/// Send `method path body` with `Precognition: true` and assert it changed nothing: same rows,
/// no mail, no session cookie set or cleared.
async fn precognitive(
    server: &TestServer,
    ctx: &AppContext,
    method: &str,
    path: &str,
    body: &Value,
) -> TestResponse {
    let before = db_snapshot(ctx).await;
    let req = match method {
        "post" => server.post(path),
        "patch" => server.patch(path),
        "put" => server.put(path),
        "delete" => server.delete(path),
        _ => unreachable!(),
    };
    let res = req.add_header("precognition", "true").json(body).await;
    assert_eq!(db_snapshot(ctx).await, before, "{method} {path} wrote");
    assert!(deliveries(ctx).is_empty(), "{method} {path} sent mail");
    assert!(
        res.maybe_cookie(cookies::SESSION_COOKIE).is_none(),
        "{method} {path} touched the session cookie"
    );
    res
}

#[track_caller]
fn assert_unsupported(res: &TestResponse) {
    assert_eq!(res.status_code(), StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.text(), "Precognition not supported");
}

#[track_caller]
fn assert_errors(res: &TestResponse, errors: &Value) {
    assert_eq!(res.status_code(), 422, "{}", res.text());
    assert_eq!(res.header("precognition"), "true");
    assert_eq!(res.json::<Value>()["errors"], *errors);
}

#[track_caller]
fn assert_valid(res: &TestResponse) {
    assert_eq!(res.status_code(), 204, "{}", res.text());
    assert_eq!(res.header("precognition-success"), "true");
}

#[tokio::test]
#[serial]
async fn sign_in_refuses_precognition_before_creating_a_session() {
    with_app(|server, ctx| async move {
        let credentials = json!({ "email": ONE, "password": PASSWORD });
        let res = precognitive(&server, &ctx, "post", route_table::SIGN_IN, &credentials).await;
        assert_unsupported(&res);
        // It spent no rate-limit token either: the full budget is still there.
        for _ in 0..LIMIT {
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
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn sign_up_validates_without_creating_a_user_session_or_mail() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        let res = precognitive(
            &server,
            &ctx,
            "post",
            route_table::SIGN_UP,
            &json!({ "name": "", "email": ONE, "password": "short" }),
        )
        .await;
        assert_errors(
            &res,
            &json!({
                "email": ["has already been taken"],
                "name": ["can't be blank"],
                "password": ["is too short (minimum is 12 characters)"],
            }),
        );
        let res = precognitive(
            &server,
            &ctx,
            "post",
            route_table::SIGN_UP,
            &json!({
                "name": "New", "email": "new@example.com",
                "password": PASSWORD, "password_confirmation": PASSWORD,
            }),
        )
        .await;
        assert_valid(&res);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn live_validation_does_not_spend_sign_up_rate_limit_attempts() {
    with_sign_up(SignUp::Open, |server, ctx| async move {
        // Far more keystroke-driven validations than the 10-per-window budget.
        for _ in 0..25 {
            let res = precognitive(
                &server,
                &ctx,
                "post",
                route_table::SIGN_UP,
                &json!({ "name": "x", "email": "typing@example.com", "password": "short" }),
            )
            .await;
            assert_eq!(res.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
        }
        // The real submission still goes through.
        let res = server
            .post(route_table::SIGN_UP)
            .json(&json!({
                "name": "Typist", "email": "typing@example.com",
                "password": PASSWORD, "password_confirmation": PASSWORD,
            }))
            .await;
        assert_eq!(res.status_code(), StatusCode::FOUND);
        assert_eq!(res.header("location"), "/typist-s-account");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn account_deletion_refuses_precognition_even_with_the_right_challenge() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = precognitive(
            &server,
            &ctx,
            "delete",
            route_table::USERS,
            &json!({ "password_challenge": PASSWORD }),
        )
        .await;
        assert_unsupported(&res);
        assert_eq!(
            server
                .get(route_table::SETTINGS_PROFILE)
                .await
                .status_code(),
            200
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn session_logout_refuses_precognition() {
    with_app(|mut server, ctx| async move {
        let current = sign_in(&mut server, &ctx, ONE).await;
        for token in [current.as_str(), ONE_SESSION] {
            let res = precognitive(
                &server,
                &ctx,
                "delete",
                &route_table::session_path(token),
                &json!({}),
            )
            .await;
            assert_unsupported(&res);
        }
        assert_eq!(
            server
                .get(route_table::SETTINGS_PROFILE)
                .await
                .status_code(),
            200
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn password_reset_request_refuses_precognition_without_mailing() {
    with_app(|server, ctx| async move {
        let res = precognitive(
            &server,
            &ctx,
            "post",
            route_table::IDENTITY_PASSWORD_RESET,
            &json!({ "email": ONE }),
        )
        .await;
        assert_unsupported(&res);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn password_reset_validates_without_changing_the_password_or_spending_the_token() {
    with_app(|server, ctx| async move {
        let sid = token_for(&ctx, &user(&ctx, ONE).await, Purpose::PasswordReset);
        let res = precognitive(
            &server,
            &ctx,
            "patch",
            route_table::IDENTITY_PASSWORD_RESET,
            &json!({ "sid": sid, "password": "short", "password_confirmation": "nope" }),
        )
        .await;
        assert_errors(
            &res,
            &json!({
                "password": ["is too short (minimum is 12 characters)"],
                "password_confirmation": ["doesn't match Password"],
            }),
        );
        let valid = json!({
            "sid": sid,
            "password": "NewPassword1*3*",
            "password_confirmation": "NewPassword1*3*",
        });
        for method in ["patch", "put"] {
            let res = precognitive(
                &server,
                &ctx,
                method,
                route_table::IDENTITY_PASSWORD_RESET,
                &valid,
            )
            .await;
            assert_valid(&res);
        }
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
        // The link still works for the real submission.
        let res = server
            .patch(route_table::IDENTITY_PASSWORD_RESET)
            .json(&valid)
            .await;
        assert_redirect(&res, route_table::SIGN_IN);
        assert!(user(&ctx, ONE).await.authenticate("NewPassword1*3*"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn profile_update_validates_without_writing() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let path = route_table::SETTINGS_PROFILE;
        let res = precognitive(&server, &ctx, "patch", path, &json!({ "name": "" })).await;
        assert_errors(&res, &json!({ "name": ["can't be blank"] }));
        let res = precognitive(&server, &ctx, "patch", path, &json!({ "name": "Renamed" })).await;
        assert_valid(&res);
        assert_ne!(user(&ctx, ONE).await.name, "Renamed");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn password_update_validates_without_writing_or_logging_out_other_sessions() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let path = route_table::SETTINGS_PASSWORD;
        let res = precognitive(
            &server,
            &ctx,
            "patch",
            path,
            &json!({ "password": "short", "password_challenge": "wrongpassword" }),
        )
        .await;
        assert_errors(
            &res,
            &json!({
                "password": ["is too short (minimum is 12 characters)"],
                "password_challenge": ["is invalid"],
            }),
        );
        let res = precognitive(
            &server,
            &ctx,
            "patch",
            path,
            &json!({
                "password": "NewPassword1*3*",
                "password_confirmation": "NewPassword1*3*",
                "password_challenge": PASSWORD,
            }),
        )
        .await;
        assert_valid(&res);
        assert!(user(&ctx, ONE).await.authenticate(PASSWORD));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn email_update_validates_without_writing_or_mailing() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let path = route_table::SETTINGS_EMAIL;
        let res = precognitive(
            &server,
            &ctx,
            "patch",
            path,
            &json!({ "email": "two@example.com", "password_challenge": "wrongpassword" }),
        )
        .await;
        assert_errors(
            &res,
            &json!({
                "email": ["has already been taken"],
                "password_challenge": ["is invalid"],
            }),
        );
        let res = precognitive(
            &server,
            &ctx,
            "patch",
            path,
            &json!({ "email": "fresh@example.com", "password_challenge": PASSWORD }),
        )
        .await;
        assert_valid(&res);
        assert_eq!(user(&ctx, ONE).await.email, ONE);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn resending_verification_refuses_precognition_without_mailing() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = precognitive(
            &server,
            &ctx,
            "post",
            route_table::IDENTITY_EMAIL_VERIFICATION,
            &json!({}),
        )
        .await;
        assert_unsupported(&res);
    })
    .await;
}

/// Review N6: following a verification link writes `verified`, so a precognitive GET with a
/// valid token is refused before the token is processed and the user stays unverified.
#[tokio::test]
#[serial]
async fn email_verification_link_refuses_precognition_without_verifying() {
    use sea_orm::{ActiveModelTrait, ActiveValue, IntoActiveModel};
    with_app(|server, ctx| async move {
        let mut unverified = user(&ctx, ONE).await.into_active_model();
        unverified.verified = ActiveValue::Set(false);
        let unverified = unverified.update(&ctx.db).await.unwrap();
        let sid = token_for(&ctx, &unverified, Purpose::EmailVerification);
        let path = format!(
            "{}?{}",
            route_table::IDENTITY_EMAIL_VERIFICATION,
            serde_urlencoded::to_string([("sid", sid.as_str())]).unwrap()
        );

        let before = db_snapshot(&ctx).await;
        let res = server.get(&path).add_header("precognition", "true").await;
        assert_unsupported(&res);
        assert_eq!(db_snapshot(&ctx).await, before);
        assert!(!user(&ctx, ONE).await.verified);

        // The same link still works without the header: the refusal did not spend it.
        server.get(&path).await;
        assert!(user(&ctx, ONE).await.verified);
    })
    .await;
}
