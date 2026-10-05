//! `Settings::EmailsController`. A changed email is unverified and gets a new verification mail.

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::Authenticated,
    controllers::{
        nullable, precognitive,
        rate_limit::{Action, RateLimited},
        render, Params,
    },
    inertia::{redirect::Redirect, render::Inertia},
    mailers::user_mailer::UserMailer,
    models::users::SaveError,
    route_table,
};

pub struct ChangeEmail;
impl Action for ChangeEmail {
    const NAME: &'static str = "settings.emails.update";
    const FALLBACK: &'static str = route_table::SETTINGS_EMAIL;
    const PRECOGNITION_SPENDS: bool = true;
}

#[derive(Debug, Default, Deserialize)]
struct Update {
    /// Missing keeps the current email (`params.permit(:email, …)`); `null` assigns nil.
    #[serde(default, deserialize_with = "nullable")]
    email: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    password_challenge: String,
}

async fn show(_: Authenticated, inertia: Inertia) -> Result<Response> {
    render(inertia, "settings/emails/show", json!({})).await
}

async fn update(
    Authenticated(current): Authenticated,
    _: RateLimited<ChangeEmail>,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<Update>,
) -> Result<Response> {
    // An explicit nil fails presence and format exactly like "" does.
    let email = params.email.map(Option::unwrap_or_default);
    let email = email.as_deref();
    if let Some(res) = precognitive(
        &headers,
        &current
            .user
            .email_change_errors(&ctx.db, email, &params.password_challenge)
            .await?,
    ) {
        return Ok(res);
    }
    match current
        .user
        .change_email(&ctx.db, email, &params.password_challenge)
        .await
    {
        Ok((user, true)) => {
            UserMailer::email_verification(&ctx, &user).await?;
            Ok(Redirect::to(route_table::SETTINGS_EMAIL)
                .notice("Your email has been changed")
                .into_response())
        }
        Ok((_, false)) => Ok(Redirect::to(route_table::SETTINGS_EMAIL).into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::SETTINGS_EMAIL)
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new().add(
        route_table::SETTINGS_EMAIL,
        get(show).patch(update).put(update),
    )
}
