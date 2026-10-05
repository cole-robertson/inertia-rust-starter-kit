//! `Identity::EmailVerificationsController`: follow the emailed link (public), or resend it.

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde::Deserialize;

use crate::{
    auth::Authenticated,
    controllers::{
        clock,
        rate_limit::{Action, RateLimited},
        settings, NoPrecognition, Params,
    },
    inertia::redirect::Redirect,
    mailers::user_mailer::UserMailer,
    models::{tokens::Purpose, users},
    route_table,
};

#[derive(Debug, Default, Deserialize)]
pub struct Sid {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    pub sid: String,
}

/// Following the link writes (`verified`), so Precognition is refused before the token is
/// even looked at.
async fn show(
    _: NoPrecognition,
    State(ctx): State<AppContext>,
    Params(params): Params<Sid>,
) -> Result<Response> {
    let settings = settings(&ctx)?;
    let Ok(user) = users::Model::find_by_token_for(
        &ctx.db,
        Purpose::EmailVerification,
        &params.sid,
        settings.secret_key_base.as_bytes(),
        &*clock(&ctx),
    )
    .await
    else {
        return Ok(Redirect::to(route_table::SETTINGS_EMAIL)
            .alert("That email verification link is invalid")
            .into_response());
    };
    // The email changed after the token was checked: the link is no longer valid.
    match user.verify_email(&ctx.db).await {
        Ok(_) => {}
        Err(ModelError::EntityNotFound) => {
            return Ok(Redirect::to(route_table::SETTINGS_EMAIL)
                .alert("That email verification link is invalid")
                .into_response());
        }
        Err(err) => return Err(err.into()),
    }
    Ok(Redirect::to(route_table::ROOT)
        .notice("Thank you for verifying your email address")
        .into_response())
}

pub struct ResendVerification;
impl Action for ResendVerification {
    const NAME: &'static str = "email_verifications.create";
    const FALLBACK: &'static str = route_table::ROOT;
}

async fn create(
    _: NoPrecognition,
    Authenticated(current): Authenticated,
    _: RateLimited<ResendVerification>,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
) -> Result<Response> {
    UserMailer::email_verification(&ctx, &current.user).await?;
    Ok(Redirect::back(&headers, route_table::ROOT)
        .notice("We sent a verification email to your email address")
        .into_response())
}

pub fn routes() -> Routes {
    Routes::new().add(
        route_table::IDENTITY_EMAIL_VERIFICATION,
        get(show).post(create),
    )
}
