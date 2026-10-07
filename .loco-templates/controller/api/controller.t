{#- The kit's controller for `cargo loco generate controller <name> [actions]...` (replaces Loco's
    JSON API one). Read .loco-templates/README.md before editing.

    - Read actions (anything but create/update/destroy) are Inertia pages `<name>/<action>`;
      a flat controller with any read action (or none at all) also gets `index`. One with only
      write actions gets no page.
    - `show:<param>` is the member page at `<name>/{param}` (Rails' `resources ... only: :show`):
      `show:id` takes an i64, `show:key` a String, `show:*key` a glob, the rest of the path with
      its slashes (Rails' `*key`).
    - create/update/destroy are write handlers that redirect back (to the Referer, else a
      fallback page), with validation errors on the redirect and Precognition refused.
    - `todos/completions` is a nested controller (Rails' `Todos::CompletionsController`): a
      singular resource at `/{account_slug}/todos/{todo_id}/completion`, write actions only.
    - Everything lives under `/{account_slug}` and takes `CurrentAccount`, unless `--global`.

    The command line is planned by src/generate.rs, which passes its choices as KIT_GENERATE_*
    environment variables; tests/controller_generator.rs renders this the same way. -#}
{%- set scoped = get_env(name="KIT_GENERATE_SCOPE", default="account") != "global" -%}
{%- set parent = get_env(name="KIT_GENERATE_PARENT", default="") -%}
{%- set nested = parent != "" -%}
{%- set writes = ["create", "update", "destroy"] -%}
{%- if nested -%}
{%- set child = name | split(pat="/") | last | snake_case -%}
{%- set joined = parent ~ "_" ~ child -%}
{%- set parent_pascal = parent | pascal_case -%}
{%- set child_pascal = child | pascal_case -%}
{%- set parent_singular = get_env(name="KIT_GENERATE_PARENT_SINGULAR") -%}
{%- set child_singular = get_env(name="KIT_GENERATE_CHILD_SINGULAR") -%}
{%- set parent_scoped = scoped and get_env(name="KIT_GENERATE_PARENT_SCOPED", default="") == "1" -%}
{%- set parent_module = get_env(name="KIT_GENERATE_PARENT_MODULE") -%}
{%- set file = "src/controllers/" ~ parent ~ "/" ~ child ~ ".rs" -%}
{%- set module_path = parent ~ "::" ~ child -%}
{%- set pascal = parent_pascal ~ "::" ~ child_pascal -%}
{%- set ts_module = parent_pascal ~ "/" ~ child_pascal ~ "Controller" -%}
{%- set camel = joined | camel_case -%}
{%- set route_prefix = parent ~ "." ~ child -%}
{%- set upper_joined = parent_singular ~ "_" ~ child_singular -%}
{%- set upper = upper_joined | upper_case -%}
{%- set helper = parent_singular ~ "_" ~ child_singular ~ "_path" -%}
{%- set params_name = child_singular -%}
{%- set params_pascal = child_singular | pascal_case -%}
{%- set params_type = params_pascal ~ "Params" -%}
{%- set parent_id = parent_singular ~ "_id" -%}
{%- else -%}
{%- set file_name = name | snake_case -%}
{%- set singular = get_env(name="KIT_GENERATE_SINGULAR", default=file_name) -%}
{%- set file = "src/controllers/" ~ file_name ~ ".rs" -%}
{%- set module_path = file_name -%}
{%- set pascal = file_name | pascal_case -%}
{%- set ts_module = pascal ~ "Controller" -%}
{%- set camel = file_name | camel_case -%}
{%- set route_prefix = file_name -%}
{%- set upper = file_name | upper_case -%}
{%- set params_name = singular -%}
{%- set params_pascal = singular | pascal_case -%}
{%- set params_type = params_pascal ~ "Params" -%}
{%- set singular_upper = singular | upper_case -%}
{%- endif -%}
{%- if scoped %}{% set prefix = "/{account_slug}/" %}{% else %}{% set prefix = "/" %}{% endif -%}
{#- pages: the GET pages at `<name>` (index) and `<name>/<action>`. show_param: `show:<param>`'s
    param, the member page at `<name>/{param}` (`show_glob`: `{*param}`). -#}
{%- set_global pages = [] -%}
{%- set_global reads = false -%}
{%- set_global show_param = "" -%}
{%- set_global show_glob = false -%}
{%- set_global actions_written = [] -%}
{%- for action in actions -%}
{%- set parts = action.name | split(pat=":") -%}
{%- if parts | length > 1 -%}
{%- set raw = parts | last -%}
{%- if nested or parts | first != "show" or parts | length > 2 or raw | trim_start_matches(pat="*") == "" -%}
{{ throw(message="`" ~ action.name ~ "`: only a flat controller's `show` takes a param (`show:id` an i64, any other name a String, e.g. `show:slug` or `show:token`, or a glob `show:*key` for the rest of the path), the member page at /" ~ name ~ "/{param}") }}
{%- endif -%}
{%- set_global show_glob = raw is starting_with("*") -%}
{%- set_global show_param = raw | trim_start_matches(pat="*") -%}
{%- set_global reads = true -%}
{%- elif action.name in writes -%}
{%- if action.name not in actions_written %}{% set_global actions_written = actions_written | concat(with=action.name) %}{% endif -%}
{%- elif action.verb != "get" -%}
{{ throw(message="`" ~ action.name ~ "`: write actions are `create`, `update` and `destroy` (Rails' names); rename it, or add it to the generated controller by hand.") }}
{%- elif nested -%}
{{ throw(message="`" ~ action.name ~ "`: a nested controller (`" ~ name ~ "`) writes create/update/destroy only; put the page on `" ~ parent ~ "`'s controller, or generate a flat one.") }}
{%- else -%}
{%- set_global reads = true -%}
{%- if action.name != "index" and action.name not in pages %}{% set_global pages = pages | concat(with=action.name) %}{% endif -%}
{%- endif -%}
{%- endfor -%}
{#- `index` comes with any page (as Loco's does), and with no actions at all; a controller of
    write actions only gets no page. -#}
{%- if not nested and (reads or actions_written | length == 0) %}{% set_global pages = ["index"] | concat(with=pages) %}{% endif -%}
{%- set has_index = "index" in pages -%}
{%- set has_show = show_param != "" -%}
{%- set has_pages = pages | length > 0 or has_show -%}
{%- set member_writes = "update" in actions_written or "destroy" in actions_written -%}
{%- if show_glob %}{% set member_segment_text = "*" ~ show_param %}{% else %}{% set member_segment_text = show_param %}{% endif -%}
{%- if has_show and (show_param != "id" or show_glob) and member_writes -%}
{{ throw(message="`show:" ~ member_segment_text ~ "`: update and destroy take the record's `{id}` at the same path; use `show:id` with them, or add the write routes by hand.") }}
{%- endif -%}
{%- if has_show %}{% set member_param = show_param %}{% else %}{% set member_param = "id" %}{% endif -%}
{%- if show_glob %}{% set member_segment = "{*" ~ member_param ~ "}" %}{% else %}{% set member_segment = "{" ~ member_param ~ "}" %}{% endif -%}
{%- if member_param == "id" and not show_glob %}{% set member_type = "i64" %}{% else %}{% set member_type = "String" %}{% endif -%}
{%- if nested and actions_written | length == 0 -%}
{{ throw(message="a nested controller (`" ~ name ~ "`) needs at least one of create, update, destroy") }}
{%- endif -%}
{%- if has_show and "show" in pages -%}
{{ throw(message="`show` and `show:" ~ member_segment_text ~ "` are both the `show` action; keep one") }}
{%- endif -%}
{%- set member = not nested and (member_writes or has_show) -%}
{#- The collection path (`<name>`): the index page and `create`. -#}
{%- set collection = has_index or "create" in actions_written -%}
{%- set takes_params = "create" in actions_written or "update" in actions_written -%}
to: {{ file }}
skip_exists: true
message: "Controller `{{ pascal }}Controller` and its routes were added{% if scoped %}, under /{account_slug}{% else %} (global: no account scope){% endif %}.{% if auth %} (`--auth` is implied: kit controllers are for signed-in users.){% endif %}{% if has_pages %} Next: `cargo loco task scaffold:pages controller:{{ module_path | replace(from="::", to="/") }}` for the React pages.{% endif %}{% if actions_written | length > 0 %} The write handlers redirect back; put the writes themselves on a model.{% endif %}"
injections:
{%- if nested %}
- into: {{ parent_module }}
  append: true
  skip_if: "^pub mod {{ child }};"
  content: "pub mod {{ child }};"
{%- else %}
- into: src/controllers/mod.rs
  after_last: "^pub mod \\w+;"
  content: "pub mod {{ file_name }};"
{%- endif %}
- into: src/app.rs
  after: "AppRoutes::empty\\(\\)"
  content: "            .add_route(controllers::{{ module_path }}::routes())"
- into: src/route_table.rs
  before: "// scaffold:paths"
  content: |-
{%- if nested %}
    pub const {{ upper }}: &str = "{{ prefix }}{{ parent }}/{{ "{" }}{{ parent_id }}}/{{ child_singular }}";

    /// `{{ prefix }}{{ parent }}/{{ "{" }}{{ parent_id }}}/{{ child_singular }}` with the {% if scoped %}slug and {% endif %}id filled in.
    #[must_use]
    pub fn {{ helper }}({% if scoped %}slug: &str, {% endif %}{{ parent_id }}: i64) -> String {
        {{ upper }}{% if scoped %}.replace("{account_slug}", &encode_segment(slug)){% endif %}.replace("{{ "{" }}{{ parent_id }}}", &{{ parent_id }}.to_string())
    }
{%- else %}
{%- if collection %}
    pub const {{ upper }}: &str = "{{ prefix }}{{ file_name }}";
{%- endif %}
{%- for page in pages %}{% if page != "index" %}
    pub const {{ upper }}_{{ page | upper_case }}: &str = "{{ prefix }}{{ file_name }}/{{ page }}";
{%- endif %}{% endfor %}
{%- if member %}
{%- if show_glob %}
    /// The rest of the path is `{{ member_param }}`, slashes included (a glob), so it stays the last segment.
{%- endif %}
    pub const {{ singular_upper }}: &str = "{{ prefix }}{{ file_name }}/{{ member_segment }}";
{%- endif %}
{%- if scoped and collection %}

    /// `/{account_slug}/{{ file_name }}`.
    #[must_use]
    pub fn {{ file_name }}_path(slug: &str) -> String {
        {{ upper }}.replace("{account_slug}", &encode_segment(slug))
    }
{%- endif %}
{%- if member %}

    /// `{{ prefix }}{{ file_name }}/{{ member_segment }}` with the {% if scoped %}slug and {% endif %}{{ member_param | replace(from="_", to=" ") }} filled in{% if show_glob %}: each segment encoded, the slashes kept{% endif %}.
    #[must_use]
    pub fn {{ singular }}_path({% if scoped %}slug: &str, {% endif %}{{ member_param }}: {% if member_type == "i64" %}i64{% else %}&str{% endif %}) -> String {
{%- if show_glob %}
        let {{ member_param }}: Vec<String> = {{ member_param }}.split('/').map(encode_segment).collect();
{%- endif %}
        {{ singular_upper }}{% if scoped %}.replace("{account_slug}", &encode_segment(slug)){% endif %}.replace("{{ member_segment }}", &{% if member_type == "i64" %}{{ member_param }}.to_string(){% elif show_glob %}{{ member_param }}.join("/"){% else %}encode_segment({{ member_param }}){% endif %})
    }
{%- endif %}
{%- endif %}
- into: src/route_table.rs
  before: "// scaffold:routes"
  content: |2-
{%- for page in pages %}
            route("{{ route_prefix }}.{{ page }}", Get, {{ upper }}{% if page != "index" %}_{{ page | upper_case }}{% endif %}, ts("{{ ts_module }}", "{{ camel }}", "{{ page | camel_case }}", None)),
{%- endfor %}
{%- if has_show %}
            route("{{ route_prefix }}.show", Get, {{ singular_upper }}, ts("{{ ts_module }}", "{{ camel }}", "show", None)),
{%- endif %}
{%- for action in actions_written %}
{%- if nested %}{% set constant = upper %}{% elif action == "create" %}{% set constant = upper %}{% else %}{% set constant = singular_upper %}{% endif %}
{%- if action == "create" %}{% set method = "Post" %}{% elif action == "update" %}{% set method = "Patch" %}{% else %}{% set method = "Delete" %}{% endif %}
            route("{{ route_prefix }}.{{ action }}", {{ method }}, {{ constant }}, ts("{{ ts_module }}", "{{ camel }}", "{{ action }}", None)),
{%- endfor %}
---
{%- if nested %}
//! `{{ pascal }}Controller`: the {{ child_singular | replace(from="_", to=" ") }} of a {{ parent_singular | replace(from="_", to=" ") }}, as a singular resource
//! (`{{ prefix }}{{ parent }}/{{ "{" }}{{ parent_id }}}/{{ child_singular }}`, Rails' `resource :{{ child_singular }}` inside
//! `resources :{{ parent }}`). Generated by `cargo loco generate controller {{ parent }}/{{ child }}` from
//! `.loco-templates/controller/api/controller.t`. Each action redirects back (to the page the
//! form was on), with validation errors when there are any; put the write itself on the
//! model and call it here.
{%- else %}
//! `{{ pascal }}Controller`{% if has_pages %}: Inertia pages{% endif %}{% if actions_written | length > 0 %}{% if has_pages %} and{% else %}:{% endif %} write actions that redirect back{% endif %}, for
//! {% if scoped %}members of the account in the URL (`/{account_slug}/{{ file_name }}`){% else %}signed-in users (global: no account scope){% endif %}. Generated by
//! `cargo loco generate controller` from `.loco-templates/controller/api/controller.t`.{% if has_pages %} Each
//! page renders `frontend/pages/{{ file_name }}/<action>.tsx`; put its queries on a model and pass the
//! result as props.{% endif %}
{%- endif %}

{% if actions_written | length > 0 %}use axum::http::HeaderMap;
{% endif -%}
use loco_rs::prelude::*;
{%- if takes_params %}
use serde::Deserialize;
{%- endif %}
{%- if has_pages %}
use serde_json::json;
{%- endif %}

use crate::{
    auth::{% if scoped %}CurrentAccount{% else %}Authenticated{% endif %},
{%- set_global used = [] -%}
{%- if has_pages %}{% set_global used = used | concat(with="render") %}{% endif -%}
{%- if actions_written | length > 0 %}{% set_global used = used | concat(with="NoPrecognition") %}{% endif -%}
{%- if takes_params %}{% set_global used = used | concat(with="Params") %}{% endif %}
    controllers::{% if used | length > 1 %}{ {{- used | join(sep=", ") }}}{% else %}{{ used | first }}{% endif %},
{%- if has_pages and actions_written | length > 0 %}
    inertia::{redirect::Redirect, render::Inertia},
{%- elif has_pages %}
    inertia::render::Inertia,
{%- else %}
    inertia::redirect::Redirect,
{%- endif %}
{%- if parent_scoped and takes_params %}
    models::{ {{- parent }}, users::Errors},
{%- elif parent_scoped %}
    models::{{ parent }},
{%- elif takes_params %}
    models::users::Errors,
{%- endif %}
    route_table,
};
{%- if takes_params %}

/// `params.require(:{{ params_name }}).permit(...)`: the form's fields. Add them here
/// (`#[serde(default, deserialize_with = "crate::models::cast::form_value")] pub title:
/// Option<String>`), and their checks to [`{{ params_type }}::errors`]; usually both move to a
/// model, as the scaffold's do.
#[derive(Debug, Default, Deserialize)]
struct {{ params_type }} {}

impl {{ params_type }} {
    /// The validation errors (`{field: [messages]}`), empty when the params are fine.
    fn errors(&self) -> Errors {
        Errors::new()
    }
}
{%- endif %}
{%- if not nested %}
{% for page in pages %}
async fn {{ page }}(_: {% if scoped %}CurrentAccount{% else %}Authenticated{% endif %}, State(_ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
    render(inertia, "{{ file_name }}/{{ page }}", json!({})).await
}
{% endfor -%}
{%- if has_show %}
/// `GET {{ prefix }}{{ file_name }}/{{ member_segment }}`{% if show_glob %}: `{{ member_param }}` is the rest of the path, slashes
/// included{% endif %}. Look the record up by it{% if scoped %} in the account (another account's is a 404){% endif %} and pass it as props.
async fn show(
    _: {% if scoped %}CurrentAccount{% else %}Authenticated{% endif %},
    State(_ctx): State<AppContext>,
    Path({% if scoped %}(_, {{ member_param }}){% else %}{{ member_param }}{% endif %}): Path<{% if scoped %}(String, {{ member_type }}){% else %}{{ member_type }}{% endif %}>,
    inertia: Inertia,
) -> Result<Response> {
    render(inertia, "{{ file_name }}/show", json!({ "{{ member_param }}": {{ member_param }} })).await
}
{% endif -%}
{%- endif %}
{%- for action in actions_written %}
{%- if nested %}
{%- if action == "create" %}{% set verb = "POST" %}{% elif action == "update" %}{% set verb = "PATCH" %}{% else %}{% set verb = "DELETE" %}{% endif %}

/// `{{ verb }} {{ prefix }}{{ parent }}/{{ "{" }}{{ parent_id }}}/{{ child_singular }}`.
async fn {{ action }}(
    _: NoPrecognition,
    {% if scoped %}current: CurrentAccount{% else %}_: Authenticated{% endif %},
    State({% if parent_scoped %}ctx{% else %}_ctx{% endif %}): State<AppContext>,
    Path({% if scoped %}(_, {% endif %}{% if parent_scoped %}{{ parent_id }}{% else %}_{{ parent_id }}{% endif %}{% if scoped %}){% endif %}): Path<{% if scoped %}(String, i64){% else %}i64{% endif %}>,
    headers: HeaderMap,
{%- if action != "destroy" %}
    Params(params): Params<{{ params_type }}>,
{%- endif %}
) -> Result<Response> {
{%- if parent_scoped %}
    // Another account's {{ parent_singular | replace(from="_", to=" ") }} is not found: a 404.
    let {{ parent_singular }} = {{ parent }}::Model::find_in_account(&ctx.db, current.account.id, {{ parent_id }}).await?;
    let back = Redirect::back(&headers, route_table::{{ parent_singular }}_path(&current.account.slug, {{ parent_singular }}.id));
{%- elif scoped %}
    // Look the {{ parent_singular | replace(from="_", to=" ") }} up in the account here (`find_in_account`), so another
    // account's id is a 404.
    let back = Redirect::back(&headers, route_table::account_path(&current.account.slug));
{%- else %}
    let back = Redirect::back(&headers, route_table::ROOT);
{%- endif %}
{%- if action != "destroy" %}
    let errors = params.errors();
    if !errors.is_empty() {
        return Ok(back.errors(errors).into_response());
    }
{%- endif %}
    // The write goes here, as a model method{% if parent_scoped %} (e.g. `{{ parent_singular }}.{{ action }}_{{ child_singular }}(&ctx.db)`){% endif %}.
    Ok(back.into_response())
}
{%- else %}
{%- if action == "create" %}{% set verb = "POST" %}{% set path = prefix ~ file_name %}{% elif action == "update" %}{% set verb = "PATCH" %}{% set path = prefix ~ file_name ~ "/{id}" %}{% else %}{% set verb = "DELETE" %}{% set path = prefix ~ file_name ~ "/{id}" %}{% endif %}

/// `{{ verb }} {{ path }}`.
async fn {{ action }}(
    _: NoPrecognition,
    {% if scoped %}current: CurrentAccount{% else %}_: Authenticated{% endif %},
    State(_ctx): State<AppContext>,
{%- if action != "create" %}
    Path({% if scoped %}(_, _id){% else %}_id{% endif %}): Path<{% if scoped %}(String, i64){% else %}i64{% endif %}>,
{%- endif %}
    headers: HeaderMap,
{%- if action != "destroy" %}
    Params(params): Params<{{ params_type }}>,
{%- endif %}
) -> Result<Response> {
    let back = Redirect::back(&headers, {% if scoped and collection %}route_table::{{ file_name }}_path(&current.account.slug){% elif scoped %}route_table::account_path(&current.account.slug){% elif collection %}route_table::{{ upper }}{% else %}route_table::ROOT{% endif %});
{%- if action != "destroy" %}
    let errors = params.errors();
    if !errors.is_empty() {
        return Ok(back.errors(errors).into_response());
    }
{%- endif %}
    // The write goes here, as a model method{% if action != "create" %} on the record `_id` names{% if scoped %}, looked up in
    // the account (`find_in_account`, so another account's id is a 404){% endif %}{% endif %}.
    Ok(back.into_response())
}
{%- endif %}
{%- endfor %}

pub fn routes() -> Routes {
    Routes::new()
{%- for page in pages %}
        .add(route_table::{{ upper }}{% if page != "index" %}_{{ page | upper_case }}{% endif %}, get({{ page }}){% if page == "index" and "create" in actions_written %}.post(create){% endif %})
{%- endfor %}
{%- if not nested and not has_index and "create" in actions_written %}
        .add(route_table::{{ upper }}, post(create))
{%- endif %}
{%- if nested %}
{%- set_global chain = [] %}
{%- for action in actions_written %}
{%- if action == "create" %}{% set_global chain = chain | concat(with="post(create)") %}{% elif action == "update" %}{% set_global chain = chain | concat(with="patch(update)") %}{% else %}{% set_global chain = chain | concat(with="delete(destroy)") %}{% endif %}
{%- endfor %}
        .add(route_table::{{ upper }}, {{ chain | join(sep=".") }})
{%- elif member %}
{%- set_global chain = [] %}
{%- if has_show %}{% set_global chain = chain | concat(with="get(show)") %}{% endif %}
{%- if "update" in actions_written %}{% set_global chain = chain | concat(with="patch(update).put(update)") %}{% endif %}
{%- if "destroy" in actions_written %}{% set_global chain = chain | concat(with="delete(destroy)") %}{% endif %}
        .add(route_table::{{ singular_upper }}, {{ chain | join(sep=".") }})
{%- endif %}
}
