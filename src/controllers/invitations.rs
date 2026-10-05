//! `InvitationsController`: the page an invitation link opens, and accepting it.
//! Started from `cargo loco generate controller invitations`, then written by hand.

use axum::extract::Path;
use loco_rs::{model::ModelError, prelude::*};
use serde_json::json;

use crate::{
    auth::{Authenticated, MaybeAuthenticated},
    controllers::{clock, render, NoPrecognition},
    inertia::{redirect::Redirect, render::Inertia},
    models::{invitations, memberships},
    route_table,
};

async fn show(
    MaybeAuthenticated(current): MaybeAuthenticated,
    State(ctx): State<AppContext>,
    Path(token): Path<String>,
    inertia: Inertia,
) -> Result<Response> {
    let details = invitations::Model::find_by_token(&ctx.db, &token).await?;
    render(
        inertia,
        "invitations/show",
        json!({
            "account_name": details.account.name,
            "inviter_name": details.inviter.name,
            "email": details.invitation.email,
            "signed_in_as": current.map(|c| c.user.email),
            "expired": !details.invitation.is_pending(clock(&ctx).now()),
        }),
    )
    .await
}

async fn accept(
    _: NoPrecognition,
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
    Path(token): Path<String>,
) -> Result<Response> {
    let details = invitations::Model::find_by_token(&ctx.db, &token).await?;
    let invitation = &details.invitation;
    let back = route_table::invitation_path(&token);
    let now = clock(&ctx).now();
    if !invitation.is_pending(now) {
        return Ok(Redirect::to(back)
            .alert("This invitation has expired")
            .into_response());
    }
    if current.user.email != invitation.email {
        return Ok(Redirect::to(back)
            .alert(format!("This invitation is for {}", invitation.email))
            .into_response());
    }
    match invitation.accept(&ctx.db, current.user.id, now).await {
        Ok(membership) => {
            memberships::members_changed(membership.account_id);
            Ok(
                Redirect::to(route_table::account_path(&details.account.slug))
                    .notice(format!("Welcome to {}", details.account.name))
                    .into_response(),
            )
        }
        // Accepted or expired between the check and the update.
        Err(ModelError::EntityNotFound) => Ok(Redirect::to(back)
            .alert("This invitation has expired")
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::INVITATION, get(show))
        .add(route_table::ACCEPT_INVITATION, post(accept))
}
