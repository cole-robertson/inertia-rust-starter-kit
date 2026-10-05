//! The app's live channels (`src/live/`): Action Cable's `app/channels/`. Each is registered in
//! [`all`]; `cargo loco generate channel <name>` writes one and adds it here.

use std::sync::Arc;

use loco_rs::app::AppContext;
use serde_json::Value;

use crate::{
    auth::CurrentSession,
    live::Channel,
    models::{accounts, memberships},
};

pub mod account;

/// Every channel clients may subscribe to.
#[must_use]
pub fn all() -> Vec<Arc<dyn Channel>> {
    vec![
        Arc::new(account::AccountChannel),
        // channels-inject (do not remove this comment: `cargo loco generate channel` adds above it)
    ]
}

/// `user`'s membership in the account named by `params.account` (a slug), or `None` when there
/// is no such account or they aren't in it: the usual first check in `subscribed`.
pub async fn membership(
    ctx: &AppContext,
    user: &CurrentSession,
    params: &Value,
) -> Option<memberships::Model> {
    let account = accounts::Model::find_by_slug(&ctx.db, params["account"].as_str()?)
        .await
        .ok()?;
    memberships::Model::find_for(&ctx.db, account.id, user.user.id)
        .await
        .ok()
}
