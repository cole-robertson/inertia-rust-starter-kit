//! `AccountChannel`: everything that happens in an account, for its members.
//!
//! Subscribe with `{account: "<slug>"}`; anyone who isn't a member is rejected. The kit
//! broadcasts `{"type": "members"}` when someone joins, leaves or changes role, and the members
//! page reloads its `members` prop (`useLiveReload`). It tracks presence, so the same page shows
//! who else has it open.
//!
//! Your own account-wide events go here too: `AccountChannel::broadcast_to(account.id,
//! json!({"type": "projects"}))` after the write commits.

use loco_rs::prelude::*;
use serde::Serialize;
use serde_json::Value;

use crate::{
    auth::CurrentSession,
    live::{self, Channel, Subscription},
};

pub struct AccountChannel;

impl AccountChannel {
    pub const NAME: &'static str = "AccountChannel";

    /// `AccountChannel.broadcast_to(account, payload)`.
    pub fn broadcast_to(account_id: i64, payload: impl Serialize) {
        live::broadcast_to(Self::NAME, account_id, payload);
    }
}

#[async_trait]
impl Channel for AccountChannel {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn subscribed(
        &self,
        ctx: &AppContext,
        user: &CurrentSession,
        params: &Value,
    ) -> Result<Subscription> {
        let Some(membership) = super::membership(ctx, user, params).await else {
            return live::reject();
        };
        Ok(Subscription::stream_for(membership.account_id))
    }

    fn tracks_presence(&self) -> bool {
        true
    }
}
