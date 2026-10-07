//! `MembersController`: the members page, role changes and removal (or leaving).
//! Started from `cargo loco generate controller members`, then written by hand.

use axum::{extract::Path, http::HeaderMap};
use loco_rs::prelude::*;
use serde::Deserialize;

use crate::{
    auth::CurrentAccount,
    controllers::{clock, NoPrecognition, Params},
    inertia::{lazy, redirect::Redirect, render::Inertia, Props},
    models::{accounts, invitations, memberships, users::SaveError},
    route_table,
};

#[derive(Debug, Deserialize)]
struct MemberPath {
    #[allow(dead_code)]
    account_slug: String,
    id: i64,
}

#[derive(Debug, Default, Deserialize)]
struct RoleParams {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    role: String,
}

/// The alert a failed role rule shows: `errors.full_messages.to_sentence`.
fn alert(err: SaveError) -> Result<String> {
    match err {
        SaveError::Invalid(errors) => Ok(errors
            .into_inner()
            .into_values()
            .flatten()
            .collect::<Vec<_>>()
            .join(" and ")),
        err => Err(err.into()),
    }
}

async fn index(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    inertia: Inertia,
) -> Result<Response> {
    let manager = current.is_manager();
    let account_id = current.account.id;
    let (db, db2) = (ctx.db.clone(), ctx.db.clone());
    let now = clock(&ctx).now();
    // Lazy: a partial reload that asks for one list (the live reload asks for both) doesn't
    // query the other.
    inertia
        .render(
            "members/index",
            Props::new()
                .with(
                    "members",
                    lazy(move || async move {
                        Ok(memberships::Model::members_of(&db, account_id)
                            .await?
                            .iter()
                            .map(memberships::Member::to_props)
                            .collect::<Vec<_>>())
                    }),
                )
                .with(
                    "invitations",
                    lazy(move || async move {
                        Ok(if manager {
                            invitations::Model::pending_props(&db2, account_id, now).await?
                        } else {
                            Vec::new()
                        })
                    }),
                )
                .with("can_manage", manager),
        )
        .await
}

async fn update(
    _: NoPrecognition,
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path(path): Path<MemberPath>,
    headers: HeaderMap,
    Params(params): Params<RoleParams>,
) -> Result<Response> {
    let membership =
        memberships::Model::find_in_account(&ctx.db, current.account.id, path.id).await?;
    if !current.is_manager() {
        return Ok(current.forbidden(&headers));
    }
    let members = route_table::account_members_path(&current.account.slug);
    match membership.change_role(&ctx.db, &params.role).await {
        Ok(_) => Ok(Redirect::to(members).notice("Role updated").into_response()),
        Err(err) => Ok(Redirect::to(members).alert(alert(err)?).into_response()),
    }
}

async fn destroy(
    _: NoPrecognition,
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path(path): Path<MemberPath>,
    headers: HeaderMap,
) -> Result<Response> {
    let membership =
        memberships::Model::find_in_account(&ctx.db, current.account.id, path.id).await?;
    let leaving = membership.id == current.membership.id;
    if !leaving && !current.is_manager() {
        return Ok(current.forbidden(&headers));
    }
    let members = route_table::account_members_path(&current.account.slug);
    if let Err(err) = membership.remove(&ctx.db).await {
        return Ok(Redirect::to(members).alert(alert(err)?).into_response());
    }
    if leaving {
        let user = &current.session.user;
        let home = home_path(&ctx, user.id, user.last_account_id).await?;
        return Ok(Redirect::to(home)
            .notice(format!("You left {}", current.account.name))
            .into_response());
    }
    Ok(Redirect::to(members)
        .notice("Member removed")
        .into_response())
}

/// The signed-in home (`home_path`): the account visited last if the user is still in it,
/// else the first one joined, else `/accounts/new`.
///
/// # Errors
/// Database errors.
pub async fn home_path(
    ctx: &AppContext,
    user_id: i64,
    last_account_id: Option<i64>,
) -> Result<String> {
    Ok(
        match accounts::Model::default_for_user(&ctx.db, user_id, last_account_id).await? {
            Some(account) => route_table::account_path(&account.slug),
            None => route_table::NEW_ACCOUNT.to_owned(),
        },
    )
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::ACCOUNT_MEMBERS, get(index))
        .add(route_table::ACCOUNT_MEMBER, patch(update).delete(destroy))
}
