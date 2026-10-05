//! `Settings::ProfilesController`.

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::Authenticated,
    controllers::{nullable, precognitive, render, Params},
    inertia::{redirect::Redirect, render::Inertia},
    models::users::{self, SaveError},
    route_table,
};

#[derive(Debug, Default, Deserialize)]
struct Profile {
    /// Missing keeps the current name (`params.permit(:name)`); `null` assigns nil.
    #[serde(default, deserialize_with = "nullable")]
    name: Option<Option<String>>,
}

async fn show(_: Authenticated, inertia: Inertia) -> Result<Response> {
    render(inertia, "settings/profiles/show", json!({})).await
}

async fn update(
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<Profile>,
) -> Result<Response> {
    // An explicit nil fails `validates :name, presence: true` exactly like "" does.
    let name = params.name.map(Option::unwrap_or_default);
    let name = name.as_deref();
    if let Some(res) = precognitive(&headers, &users::Model::profile_errors(name)) {
        return Ok(res);
    }
    match current.user.update_profile(&ctx.db, name).await {
        Ok(_) => Ok(Redirect::to(route_table::SETTINGS_PROFILE)
            .notice("Your profile has been updated")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::SETTINGS_PROFILE)
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new().add(
        route_table::SETTINGS_PROFILE,
        get(show).patch(update).put(update),
    )
}
