//! `inertia :appearance` in the Rails kit's settings namespace: a static page.

use loco_rs::prelude::*;
use serde_json::json;

use crate::{auth::Authenticated, controllers::render, inertia::render::Inertia, route_table};

async fn show(_: Authenticated, inertia: Inertia) -> Result<Response> {
    render(inertia, "settings/appearance", json!({})).await
}

pub fn routes() -> Routes {
    Routes::new().add(route_table::SETTINGS_APPEARANCE, get(show))
}
