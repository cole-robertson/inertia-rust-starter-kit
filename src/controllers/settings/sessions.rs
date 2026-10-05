//! `Settings::SessionsController#index`: this user's sessions, newest first. `id` is the
//! session's random token (what `DELETE /sessions/:id` and `auth.session.id` use).

use loco_rs::prelude::*;
use serde_json::{json, Value};

use crate::{
    auth::Authenticated, controllers::render, inertia::render::Inertia, models::sessions,
    route_table,
};

async fn index(
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
    inertia: Inertia,
) -> Result<Response> {
    let sessions: Vec<Value> = sessions::Model::list_for_user(&ctx.db, current.user.id)
        .await?
        .into_iter()
        .map(|s| {
            json!({
                "id": s.token,
                "user_agent": s.user_agent,
                "ip_address": s.ip_address,
                "created_at": crate::models::as_json_time(&s.created_at),
            })
        })
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
