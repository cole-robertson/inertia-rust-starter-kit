//! `HomeController`: public; the shared `auth` prop shows who is signed in. A signed-in user
//! goes on to their home (`home_path`): the account visited last, else `/accounts/new`.

use loco_rs::prelude::*;
use serde_json::json;

use crate::{
    auth::MaybeAuthenticated,
    controllers::{members::home_path, render},
    inertia::{redirect::Redirect, render::Inertia},
    route_table,
};

async fn index(
    MaybeAuthenticated(current): MaybeAuthenticated,
    State(ctx): State<AppContext>,
    inertia: Inertia,
) -> Result<Response> {
    if let Some(current) = current {
        // A redirect without a flash of its own carries the incoming one (`flash.keep`).
        let home = home_path(&ctx, current.user.id, current.user.last_account_id).await?;
        return Ok(Redirect::to(home).into_response());
    }
    render(inertia, "home/index", json!({})).await
}

pub fn routes() -> Routes {
    Routes::new().add(route_table::ROOT, get(index))
}
