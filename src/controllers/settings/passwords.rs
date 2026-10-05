//! `Settings::PasswordsController`. A successful change logs out every other session.

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::Authenticated,
    controllers::{
        precognitive,
        rate_limit::{Action, RateLimited},
        render, Params,
    },
    inertia::{redirect::Redirect, render::Inertia},
    models::users::{PasswordParams, SaveError},
    route_table,
};

pub struct ChangePassword;
impl Action for ChangePassword {
    const NAME: &'static str = "settings.passwords.update";
    const FALLBACK: &'static str = route_table::SETTINGS_PASSWORD;
    const PRECOGNITION_SPENDS: bool = true;
}

#[derive(Debug, Default, Deserialize)]
struct Update {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    password_challenge: String,
    #[serde(flatten)]
    password: PasswordParams,
}

async fn show(_: Authenticated, inertia: Inertia) -> Result<Response> {
    render(inertia, "settings/passwords/show", json!({})).await
}

async fn update(
    Authenticated(current): Authenticated,
    _: RateLimited<ChangePassword>,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<Update>,
) -> Result<Response> {
    if let Some(res) = precognitive(
        &headers,
        &current
            .user
            .password_change_errors(&params.password, &params.password_challenge),
    ) {
        return Ok(res);
    }
    match current
        .user
        .change_password(
            &ctx.db,
            &params.password,
            &params.password_challenge,
            Some(current.session.id),
        )
        .await
    {
        Ok(_) => Ok(Redirect::to(route_table::SETTINGS_PASSWORD)
            .notice("Your password has been changed")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::SETTINGS_PASSWORD)
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new().add(
        route_table::SETTINGS_PASSWORD,
        get(show).patch(update).put(update),
    )
}
