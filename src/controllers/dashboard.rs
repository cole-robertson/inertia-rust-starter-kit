//! `DashboardController`: the Rails kit's `/dashboard`. Accounts are core here, so the
//! dashboard is the account overview (`/{account_slug}`); this keeps old links and bookmarks
//! working by sending a signed-in user to their home (`home_path`).

use loco_rs::prelude::*;

use crate::{
    auth::Authenticated, controllers::members::home_path, inertia::redirect::Redirect, route_table,
};

async fn index(
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    // A redirect without a flash of its own carries the incoming one (`flash.keep`).
    let home = home_path(&ctx, current.user.id, current.user.last_account_id).await?;
    Ok(Redirect::to(home).into_response())
}

pub fn routes() -> Routes {
    Routes::new().add(route_table::DASHBOARD, get(index))
}
