{#- The kit's Inertia controller for `cargo loco generate scaffold` (replaces Loco's JSON API one).
    Also registers the resource's paths and routes in src/route_table.rs and the controller in
    src/app.rs. Read .loco-templates/README.md before editing.

    A resource with a required `account_id` (the default: src/generate.rs adds
    `account:references`) lives under `/{account_slug}`, takes `CurrentAccount` and scopes every
    query to it; with `--global` it is the plain signed-in resource. Columns the forms can't
    edit are left out of params and pages with a note (the column classification below is the
    same in dto.t and src/tasks/scaffold_pages.rs: keep them in step). -#}
{%- set supported = ["String", "i16", "i64", "f32", "f64", "bool", "Date"] -%}
{%- set known_tables = get_env(name="KIT_GENERATE_TABLES", default="") -%}
{%- set tables = known_tables | split(pat=",") -%}
{%- set account_tables = get_env(name="KIT_GENERATE_ACCOUNT_TABLES", default="") | split(pat=",") -%}
{%- set_global scoped = false -%}
{%- for f in fields -%}
{%- if f.field_name == "account_id" and f.rust_type == "i64" %}{% set_global scoped = true %}{% endif -%}
{%- endfor -%}
{%- set_global skipped = [] -%}
{%- set_global selects = [] -%}
{%- set_global account_selects = [] -%}
{%- for f in fields -%}
{%- set base = f.rust_type | replace(from="Option<", to="") | replace(from=">", to="") -%}
{%- set is_ref = f.field_name is ending_with("_id") and f.rust_type in ["i64", "Option<i64>"] -%}
{%- set parent = f.field_name | trim_end_matches(pat="_id") | plural -%}
{%- if f.nullable %}{% set required = "" %}{% else %}{% set required = "; required, so set it in `create`" %}{% endif -%}
{%- if scoped and f.field_name == "account_id" -%}
{%- elif f.is_enum or base not in supported -%}
{%- set_global skipped = skipped | concat(with=f.field_name ~ " (" ~ f.rust_type ~ ": no form input" ~ required ~ ")") -%}
{%- elif is_ref and f.field_name != "user_id" and known_tables != "" and parent not in tables -%}
{%- set_global skipped = skipped | concat(with=f.field_name ~ " (no `" ~ parent ~ "` table to pick from" ~ required ~ ")") -%}
{%- elif is_ref and f.field_name != "user_id" -%}
{%- set association = f.field_name | trim_end_matches(pat="_id") -%}
{%- set_global selects = selects | concat(with=association) -%}
{%- if scoped and parent in account_tables %}{% set_global account_selects = account_selects | concat(with=association) %}{% endif -%}
{%- endif -%}
{%- endfor -%}
{%- set upper_plural = snake_plural | upper_case -%}
{%- set upper_singular = snake_singular | upper_case -%}
{%- set camel_plural = snake_plural | camel_case -%}
{%- set label = snake_singular | replace(from="_", to=" ") | capitalize -%}
{%- if scoped %}{% set prefix = "/{account_slug}/" %}{% else %}{% set prefix = "/" %}{% endif -%}
to: src/controllers/{{ snake_plural }}.rs
skip_exists: true
message: "Inertia controller `{{ pascal_plural }}Controller` and its routes were added{% if scoped %}, under /{account_slug} (every query scoped to the account){% else %} (global: no account scope){% endif %}.{% if not auth %} (`--no-auth` is ignored: kit scaffolds live in the signed-in app layout.){% endif %}{% if skipped | length > 0 %} Not in the form or the page props: {{ skipped | join(sep=", ") }}.{% endif %} Next: `cargo loco task scaffold:pages resource:{{ snake_plural }}` for the React pages."
injections:
- into: src/controllers/mod.rs
  after_last: "^pub mod \\w+;"
  content: "pub mod {{ snake_plural }};"
- into: src/app.rs
  after: "AppRoutes::empty\\(\\)"
  content: "            .add_route(controllers::{{ snake_plural }}::routes())"
- into: src/route_table.rs
  before: "// scaffold:paths"
  content: |-
    pub const {{ upper_plural }}: &str = "{{ prefix }}{{ snake_plural }}";
    pub const NEW_{{ upper_singular }}: &str = "{{ prefix }}{{ snake_plural }}/new";
    pub const {{ upper_singular }}: &str = "{{ prefix }}{{ snake_plural }}/{id}";
    pub const EDIT_{{ upper_singular }}: &str = "{{ prefix }}{{ snake_plural }}/{id}/edit";
{%- if scoped %}

    /// `/{account_slug}/{{ snake_plural }}`.
    #[must_use]
    pub fn {{ snake_plural }}_path(slug: &str) -> String {
        {{ upper_plural }}.replace("{account_slug}", &encode_segment(slug))
    }

    /// `/{account_slug}/{{ snake_plural }}/new`.
    #[must_use]
    pub fn new_{{ snake_singular }}_path(slug: &str) -> String {
        NEW_{{ upper_singular }}.replace("{account_slug}", &encode_segment(slug))
    }

    /// `/{account_slug}/{{ snake_plural }}/{id}`.
    #[must_use]
    pub fn {{ snake_singular }}_path(slug: &str, id: i64) -> String {
        {{ snake_plural }}_path(slug) + "/" + &id.to_string()
    }

    /// `/{account_slug}/{{ snake_plural }}/{id}/edit`.
    #[must_use]
    pub fn edit_{{ snake_singular }}_path(slug: &str, id: i64) -> String {
        {{ snake_singular }}_path(slug, id) + "/edit"
    }
{%- else %}

    /// `/{{ snake_plural }}/{id}` with the id filled in.
    #[must_use]
    pub fn {{ snake_singular }}_path(id: i64) -> String {
        {{ upper_singular }}.replace("{id}", &id.to_string())
    }

    /// `/{{ snake_plural }}/{id}/edit` with the id filled in.
    #[must_use]
    pub fn edit_{{ snake_singular }}_path(id: i64) -> String {
        EDIT_{{ upper_singular }}.replace("{id}", &id.to_string())
    }
{%- endif %}
- into: src/route_table.rs
  before: "// scaffold:routes"
  content: |2-
            route("{{ snake_plural }}.index", Get, {{ upper_plural }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "index", None)),
            route("{{ snake_plural }}.create", Post, {{ upper_plural }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "create", None)),
            route("{{ snake_plural }}.new", Get, NEW_{{ upper_singular }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "new", Some("new{{ pascal_singular }}"))),
            route("{{ snake_plural }}.edit", Get, EDIT_{{ upper_singular }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "edit", Some("edit{{ pascal_singular }}"))),
            route("{{ snake_plural }}.show", Get, {{ upper_singular }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "show", Some("{{ camel_singular }}"))),
            route("{{ snake_plural }}.update", Patch, {{ upper_singular }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "update", None)),
            route("{{ snake_plural }}.destroy", Delete, {{ upper_singular }}, ts("{{ pascal_plural }}Controller", "{{ camel_plural }}", "destroy", None)),
---
{%- if scoped %}
//! `{{ pascal_plural }}Controller`: Rails' scaffold controller rendered through Inertia, inside an
//! account (`/{account_slug}/{{ snake_plural }}`). Generated by `cargo loco generate scaffold` from
//! `.loco-templates/scaffold/api/controller.t`. Every handler takes [`CurrentAccount`] (a 404
//! for non-members) and passes the account to the model, so another account's id is a 404 too;
//! the params, validation and queries are on the model (`src/models/{{ snake_plural }}.rs`).

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde_json::{json{% if selects | length > 0 %}, Value{% endif %}};

use crate::{
    auth::CurrentAccount,
    controllers::{precognitive, render, NoPrecognition, Params},
    inertia::{redirect::Redirect, render::Inertia},
    models::{
        {{ snake_plural }}::{self, {{ pascal_singular }}Params, {{ pascal_singular }}Props},
        users::SaveError,
    },
    route_table,
};

async fn index(current: CurrentAccount, State(ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
    let {{ snake_plural }}: Vec<{{ pascal_singular }}Props> = {{ snake_plural }}::Model::list(&ctx.db, current.account.id)
        .await?
        .iter()
        .map({{ snake_plural }}::Model::to_props)
        .collect();
    render(inertia, "{{ snake_plural }}/index", json!({ "{{ snake_plural }}": {{ snake_plural }} })).await
}

async fn show(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path((_, id)): Path<(String, i64)>,
    inertia: Inertia,
) -> Result<Response> {
    let {{ snake_singular }} = {{ snake_plural }}::Model::find_in_account(&ctx.db, current.account.id, id).await?;
    render(inertia, "{{ snake_plural }}/show", json!({ "{{ snake_singular }}": {{ snake_singular }}.to_props() })).await
}

{%- if selects | length > 0 %}
/// The form's choices for each `belongs_to` select{% if account_selects | length > 0 %} (rows of this account only){% endif %}.
async fn form_options(ctx: &AppContext{% if account_selects | length > 0 %}, account_id: i64{% endif %}) -> Result<Value> {
    Ok(json!({
{%- for a in selects %}
        "{{ a }}_options": {{ snake_plural }}::Model::{{ a }}_options(&ctx.db{% if a in account_selects %}, account_id{% endif %}).await?,
{%- endfor %}
    }))
}

async fn new(current: CurrentAccount, State(ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
    render(inertia, "{{ snake_plural }}/new", form_options(&ctx{% if account_selects | length > 0 %}, current.account.id{% endif %}).await?).await
}
{%- else %}
async fn new(_: CurrentAccount, inertia: Inertia) -> Result<Response> {
    render(inertia, "{{ snake_plural }}/new", json!({})).await
}
{%- endif %}

async fn edit(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path((_, id)): Path<(String, i64)>,
    inertia: Inertia,
) -> Result<Response> {
    let {{ snake_singular }} = {{ snake_plural }}::Model::find_in_account(&ctx.db, current.account.id, id).await?;
{%- if selects | length > 0 %}
    let mut props = form_options(&ctx{% if account_selects | length > 0 %}, current.account.id{% endif %}).await?;
    props["{{ snake_singular }}"] = json!({{ snake_singular }}.to_props());
    render(inertia, "{{ snake_plural }}/edit", props).await
{%- else %}
    render(inertia, "{{ snake_plural }}/edit", json!({ "{{ snake_singular }}": {{ snake_singular }}.to_props() })).await
{%- endif %}
}

async fn create(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<{{ pascal_singular }}Params>,
) -> Result<Response> {
    if let Some(res) = precognitive(&headers, &params.errors()) {
        return Ok(res);
    }
    let slug = &current.account.slug;
    match {{ snake_plural }}::Model::create(&ctx.db, current.account.id, &params).await {
        Ok({{ snake_singular }}) => Ok(Redirect::to(route_table::{{ snake_singular }}_path(slug, {{ snake_singular }}.id))
            .notice("{{ label }} was successfully created.")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::new_{{ snake_singular }}_path(slug))
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn update(
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path((_, id)): Path<(String, i64)>,
    headers: HeaderMap,
    Params(params): Params<{{ pascal_singular }}Params>,
) -> Result<Response> {
    if let Some(res) = precognitive(&headers, &params.errors()) {
        return Ok(res);
    }
    let slug = &current.account.slug;
    let {{ snake_singular }} = {{ snake_plural }}::Model::find_in_account(&ctx.db, current.account.id, id).await?;
    match {{ snake_singular }}.update(&ctx.db, &params).await {
        Ok({{ snake_singular }}) => Ok(Redirect::to(route_table::{{ snake_singular }}_path(slug, {{ snake_singular }}.id))
            .notice("{{ label }} was successfully updated.")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::edit_{{ snake_singular }}_path(slug, id))
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn destroy(
    _: NoPrecognition,
    current: CurrentAccount,
    State(ctx): State<AppContext>,
    Path((_, id)): Path<(String, i64)>,
) -> Result<Response> {
    {{ snake_plural }}::Model::find_in_account(&ctx.db, current.account.id, id)
        .await?
        .destroy(&ctx.db)
        .await?;
    Ok(Redirect::to(route_table::{{ snake_plural }}_path(&current.account.slug))
        .notice("{{ label }} was successfully destroyed.")
        .into_response())
}
{%- else %}
//! `{{ pascal_plural }}Controller`: Rails' scaffold controller rendered through Inertia, for
//! signed-in users, outside accounts (`--global`). Generated by `cargo loco generate scaffold` from
//! `.loco-templates/scaffold/api/controller.t`; the params, validation and queries it calls
//! are on the model (`src/models/{{ snake_plural }}.rs`).

use axum::http::HeaderMap;
use loco_rs::prelude::*;
use serde_json::{json{% if selects | length > 0 %}, Value{% endif %}};

use crate::{
    auth::Authenticated,
    controllers::{precognitive, render, NoPrecognition, Params},
    inertia::{redirect::Redirect, render::Inertia},
    models::{
        {{ snake_plural }}::{self, {{ pascal_singular }}Params, {{ pascal_singular }}Props},
        users::SaveError,
    },
    route_table,
};

async fn index(_: Authenticated, State(ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
    let {{ snake_plural }}: Vec<{{ pascal_singular }}Props> = {{ snake_plural }}::Model::list(&ctx.db)
        .await?
        .iter()
        .map({{ snake_plural }}::Model::to_props)
        .collect();
    render(inertia, "{{ snake_plural }}/index", json!({ "{{ snake_plural }}": {{ snake_plural }} })).await
}

async fn show(
    _: Authenticated,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    inertia: Inertia,
) -> Result<Response> {
    let {{ snake_singular }} = {{ snake_plural }}::Model::find_by_id(&ctx.db, id).await?;
    render(inertia, "{{ snake_plural }}/show", json!({ "{{ snake_singular }}": {{ snake_singular }}.to_props() })).await
}

{%- if selects | length > 0 %}
/// The form's choices for each `belongs_to` select.
async fn form_options(ctx: &AppContext) -> Result<Value> {
    Ok(json!({
{%- for a in selects %}
        "{{ a }}_options": {{ snake_plural }}::Model::{{ a }}_options(&ctx.db).await?,
{%- endfor %}
    }))
}

async fn new(_: Authenticated, State(ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
    render(inertia, "{{ snake_plural }}/new", form_options(&ctx).await?).await
}
{%- else %}
async fn new(_: Authenticated, inertia: Inertia) -> Result<Response> {
    render(inertia, "{{ snake_plural }}/new", json!({})).await
}
{%- endif %}

async fn edit(
    _: Authenticated,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    inertia: Inertia,
) -> Result<Response> {
    let {{ snake_singular }} = {{ snake_plural }}::Model::find_by_id(&ctx.db, id).await?;
{%- if selects | length > 0 %}
    let mut props = form_options(&ctx).await?;
    props["{{ snake_singular }}"] = json!({{ snake_singular }}.to_props());
    render(inertia, "{{ snake_plural }}/edit", props).await
{%- else %}
    render(inertia, "{{ snake_plural }}/edit", json!({ "{{ snake_singular }}": {{ snake_singular }}.to_props() })).await
{%- endif %}
}

async fn create(
    _: Authenticated,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Params(params): Params<{{ pascal_singular }}Params>,
) -> Result<Response> {
    if let Some(res) = precognitive(&headers, &params.errors()) {
        return Ok(res);
    }
    match {{ snake_plural }}::Model::create(&ctx.db, &params).await {
        Ok({{ snake_singular }}) => Ok(Redirect::to(route_table::{{ snake_singular }}_path({{ snake_singular }}.id))
            .notice("{{ label }} was successfully created.")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::NEW_{{ upper_singular }})
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn update(
    _: Authenticated,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Params(params): Params<{{ pascal_singular }}Params>,
) -> Result<Response> {
    if let Some(res) = precognitive(&headers, &params.errors()) {
        return Ok(res);
    }
    let {{ snake_singular }} = {{ snake_plural }}::Model::find_by_id(&ctx.db, id).await?;
    match {{ snake_singular }}.update(&ctx.db, &params).await {
        Ok({{ snake_singular }}) => Ok(Redirect::to(route_table::{{ snake_singular }}_path({{ snake_singular }}.id))
            .notice("{{ label }} was successfully updated.")
            .into_response()),
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::edit_{{ snake_singular }}_path(id))
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn destroy(
    _: NoPrecognition,
    _: Authenticated,
    State(ctx): State<AppContext>,
    Path(id): Path<i64>,
) -> Result<Response> {
    {{ snake_plural }}::Model::find_by_id(&ctx.db, id)
        .await?
        .destroy(&ctx.db)
        .await?;
    Ok(Redirect::to(route_table::{{ upper_plural }})
        .notice("{{ label }} was successfully destroyed.")
        .into_response())
}
{%- endif %}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::{{ upper_plural }}, get(index).post(create))
        .add(route_table::NEW_{{ upper_singular }}, get(new))
        .add(route_table::EDIT_{{ upper_singular }}, get(edit))
        .add(
            route_table::{{ upper_singular }},
            get(show).patch(update).put(update).delete(destroy),
        )
}
