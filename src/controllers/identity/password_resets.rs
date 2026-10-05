//! `Identity::PasswordResetsController`: request a reset link, then set a new password.

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::{
    controllers::{
        clock,
        identity::email_verifications::Sid,
        precognitive,
        rate_limit::{Action, RateLimited},
        render, settings, NoPrecognition, Params,
    },
    inertia::{redirect::Redirect, render::Inertia},
    mailers::user_mailer::UserMailer,
    models::{
        tokens::Purpose,
        users::{self, PasswordParams, SaveError},
    },
    route_table,
};

/// The one reply to a reset request, whatever the email: see `create`.
pub const RESET_REQUESTED: &str =
    "If that email belongs to a verified account, we've sent reset instructions to it";

pub struct PasswordReset;
impl Action for PasswordReset {
    const NAME: &'static str = "identity.password_resets.create";
    const FALLBACK: &'static str = route_table::NEW_IDENTITY_PASSWORD_RESET;
}

#[derive(Debug, Default, Deserialize)]
struct Email {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    email: String,
}

#[derive(Debug, Default, Deserialize)]
struct Update {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    sid: String,
    #[serde(flatten)]
    password: PasswordParams,
}

/// `set_user`: the user behind a valid reset token, or the "invalid link" redirect.
async fn user_for(
    ctx: &AppContext,
    sid: &str,
) -> Result<std::result::Result<users::Model, Response>> {
    let settings = settings(ctx)?;
    Ok(users::Model::find_by_token_for(
        &ctx.db,
        Purpose::PasswordReset,
        sid,
        settings.secret_key_base.as_bytes(),
        &*clock(ctx),
    )
    .await
    .map_err(|_| invalid_link()))
}

fn invalid_link() -> Response {
    Redirect::to(route_table::NEW_IDENTITY_PASSWORD_RESET)
        .alert("That password reset link is invalid")
        .into_response()
}

fn edit_path(sid: &str) -> String {
    format!(
        "{}?{}",
        route_table::EDIT_IDENTITY_PASSWORD_RESET,
        serde_urlencoded::to_string([("sid", sid)]).unwrap_or_default()
    )
}

async fn new(inertia: Inertia) -> Result<Response> {
    render(inertia, "identity/password_resets/new", json!({})).await
}

async fn edit(
    State(ctx): State<AppContext>,
    inertia: Inertia,
    Params(params): Params<Sid>,
) -> Result<Response> {
    let user = match user_for(&ctx, &params.sid).await? {
        Ok(user) => user,
        Err(res) => return Ok(res),
    };
    render(
        inertia,
        "identity/password_resets/edit",
        json!({ "email": user.email, "sid": params.sid }),
    )
    .await
}

async fn create(
    _: NoPrecognition,
    _: RateLimited<PasswordReset>,
    State(ctx): State<AppContext>,
    Params(params): Params<Email>,
) -> Result<Response> {
    // Same reply whether or not a verified account exists, so the form can't be used to
    // enumerate accounts. The Rails kit answers differently for unknown/unverified emails;
    // this is a deliberate divergence (docs/PARITY.md). Only verified accounts get mail.
    match users::Model::find_verified_by_email(&ctx.db, &params.email).await {
        Ok(user) => UserMailer::password_reset(&ctx, &user).await?,
        Err(ModelError::EntityNotFound) => {}
        Err(err) => return Err(err.into()),
    }
    Ok(Redirect::to(route_table::SIGN_IN)
        .notice(RESET_REQUESTED)
        .into_response())
}

async fn update(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<Update>,
) -> Result<Response> {
    // Validation only: the password rules need no user, so the token is neither checked
    // nor consumed here.
    if let Some(res) = precognitive(
        &headers,
        &users::Model::password_reset_errors(&params.password),
    ) {
        return Ok(res);
    }
    let user = match user_for(&ctx, &params.sid).await? {
        Ok(user) => user,
        Err(res) => return Ok(res),
    };
    match user.reset_password(&ctx.db, &params.password).await {
        Ok(_) => Ok(Redirect::to(route_table::SIGN_IN)
            .notice("Your password was reset successfully. Please sign in")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(edit_path(&params.sid))
            .errors(errors)
            .into_response()),
        // Another reset with this link won the race: the token is spent.
        Err(SaveError::Stale) => Ok(invalid_link()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    // `skip_before_action :authenticate` with no `perform_authentication`: these pages never
    // see the session, so `auth` is null on them even for a signed-in browser.
    Routes::new()
        .add(route_table::NEW_IDENTITY_PASSWORD_RESET, get(new))
        .add(route_table::EDIT_IDENTITY_PASSWORD_RESET, get(edit))
        .add(
            route_table::IDENTITY_PASSWORD_RESET,
            post(create).patch(update).put(update),
        )
        .layer(axum::middleware::from_fn(crate::auth::without_session))
}
