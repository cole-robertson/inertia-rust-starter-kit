//! `AccountsController`: create an account, its overview page and its settings. Started from
//! `cargo loco generate scaffold accounts` (`.loco-templates/scaffold/api/controller.t`), then
//! rewritten for the account scope: everything but new/create takes [`CurrentAccount`].

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde_json::json;

use crate::{
    auth::{Authenticated, CurrentAccount},
    controllers::{precognitive, render, Params},
    inertia::{defer, redirect::Redirect, render::Inertia, Props},
    models::{
        accounts::{self, AccountParams},
        memberships,
        users::SaveError,
    },
    route_table,
};

async fn new(_: Authenticated, inertia: Inertia) -> Result<Response> {
    render(inertia, "accounts/new", json!({})).await
}

async fn create(
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<AccountParams>,
) -> Result<Response> {
    if let Some(res) = precognitive(&headers, &params.errors()) {
        return Ok(res);
    }
    match accounts::Model::create_with_owner(&ctx.db, &params, current.user.id).await {
        Ok(account) => Ok(Redirect::to(route_table::account_path(&account.slug))
            .notice("Account created")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::NEW_ACCOUNT)
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn show(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    inertia: Inertia,
) -> Result<Response> {
    let db = ctx.db.clone();
    let account_id = current.account.id;
    inertia
        .render(
            "accounts/show",
            Props::new()
                .prop("account", current.account.to_props())
                .prop("membership", json!({ "role": current.membership.role }))
                .prop(
                    "members_count",
                    defer(move || async move {
                        Ok(memberships::Model::count_in(&db, account_id).await?)
                    }),
                ),
        )
        .await
}

async fn edit(current: CurrentAccount, inertia: Inertia) -> Result<Response> {
    render(
        inertia,
        "accounts/settings",
        json!({ "account": current.account.to_props() }),
    )
    .await
}

async fn update(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<AccountParams>,
) -> Result<Response> {
    if !current.is_manager() {
        return Ok(current.forbidden(&headers));
    }
    if let Some(res) = precognitive(&headers, &params.errors()) {
        return Ok(res);
    }
    let path = route_table::account_settings_path(&current.account.slug);
    match current.account.update(&ctx.db, &params).await {
        Ok(_) => Ok(Redirect::to(path).notice("Settings saved").into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(path).errors(errors).into_response()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::ACCOUNTS, post(create))
        .add(route_table::NEW_ACCOUNT, get(new))
        .add(route_table::ACCOUNT, get(show))
        .add(route_table::ACCOUNT_SETTINGS, get(edit).patch(update))
}
