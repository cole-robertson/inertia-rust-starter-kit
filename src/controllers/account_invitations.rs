//! `Accounts::InvitationsController`: invite someone (with precognition) and revoke a pending
//! invitation. Owners and admins only. Started from `cargo loco generate controller
//! account_invitations`, then written by hand.

use axum::{extract::Path, http::HeaderMap};
use loco_rs::prelude::*;
use serde::Deserialize;

use crate::{
    auth::CurrentAccount,
    controllers::{clock, precognitive, NoPrecognition, Params},
    inertia::redirect::Redirect,
    mailers::invitation_mailer::InvitationMailer,
    models::{
        invitations::{self, InvitationParams},
        users::SaveError,
    },
    route_table,
};

#[derive(Debug, Deserialize)]
struct InvitationPath {
    #[allow(dead_code)]
    account_slug: String,
    id: i64,
}

async fn create(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<InvitationParams>,
) -> Result<Response> {
    if !current.is_manager() {
        return Ok(current.forbidden(&headers));
    }
    let now = clock(&ctx).now();
    let errors = params.errors(&ctx.db, current.account.id, now).await?;
    if let Some(res) = precognitive(&headers, &errors) {
        return Ok(res);
    }
    let members = route_table::account_members_path(&current.account.slug);
    match invitations::Model::create(
        &ctx.db,
        current.account.id,
        current.session.user.id,
        &params,
        now,
    )
    .await
    {
        Ok(invitation) => {
            InvitationMailer::invite(&ctx, &invitation).await?;
            Ok(Redirect::to(members)
                .notice(format!("Invitation sent to {}", invitation.email))
                .into_response())
        }
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(members).errors(errors).into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn destroy(
    _: NoPrecognition,
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path(path): Path<InvitationPath>,
    headers: HeaderMap,
) -> Result<Response> {
    if !current.is_manager() {
        return Ok(current.forbidden(&headers));
    }
    invitations::Model::find_pending_in_account(
        &ctx.db,
        current.account.id,
        path.id,
        clock(&ctx).now(),
    )
    .await?
    .revoke(&ctx.db)
    .await?;
    Ok(
        Redirect::to(route_table::account_members_path(&current.account.slug))
            .notice("Invitation revoked")
            .into_response(),
    )
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::ACCOUNT_INVITATIONS, post(create))
        .add(route_table::ACCOUNT_INVITATION, delete(destroy))
}
