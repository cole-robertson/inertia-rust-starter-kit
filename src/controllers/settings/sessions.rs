//! `Settings::SessionsController#index`: this user's sessions, newest first. `id` is the
//! session's random token (what `DELETE /sessions/:id` and `auth.session.id` use).

use loco_rs::prelude::*;
use serde_json::json;

use crate::{
    auth::Authenticated,
    controllers::render,
    inertia::render::Inertia,
    models::sessions::{self, SessionProps},
    route_table,
};

async fn index(
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
    inertia: Inertia,
) -> Result<Response> {
    let sessions: Vec<SessionProps> = sessions::Model::list_for_user(&ctx.db, current.user.id)
        .await?
        .into_iter()
        .map(SessionProps::from)
        .collect();
    render(
        inertia,
        "settings/sessions/index",
        json!({ "sessions": sessions }),
    )
    .await
}

pub fn routes() -> Routes {
    Routes::new().add(route_table::SETTINGS_SESSIONS, get(index))
}
