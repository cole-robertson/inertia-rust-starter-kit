{#- Replaces Loco's scaffold DTO. For the kit this template writes the resource's request test
    and fills in the model file `cargo loco db entities` just created (params, casting and
    validation, finders, create/update/destroy). Read .loco-templates/README.md first.

    The column classification is the same as controller.t's (keep them in step):
    - `account_id` (a required `i64`): the resource belongs to an account. It is never a param;
      `create` takes it, and every finder filters by it.
    - a column with no form input (tstz, uuid, json, enum, …) or a reference to a table that
      doesn't exist (`creator_id` without a `creators` table): skipped, with a note.
    - a `references` column (an `i64` named `<association>_id`, Loco's field vars don't say
      more; scaffold_pages.rs uses the same rule) is checked like `belongs_to`. All but
      `user_id` also get a select. When the parent table belongs to accounts too, only the
      current account's rows count. -#}
{%- set supported = ["String", "i16", "i64", "f32", "f64", "bool", "Date"] -%}
{%- set known_tables = get_env(name="KIT_GENERATE_TABLES", default="") -%}
{%- set tables = known_tables | split(pat=",") -%}
{%- set account_tables = get_env(name="KIT_GENERATE_ACCOUNT_TABLES", default="") | split(pat=",") -%}
{%- set upper_plural = snake_plural | upper_case -%}
{%- set upper_singular = snake_singular | upper_case -%}
{%- set label = snake_singular | replace(from="_", to=" ") | capitalize -%}
{%- set_global scoped = false -%}
{%- for f in fields -%}
{%- if f.field_name == "account_id" and f.rust_type == "i64" %}{% set_global scoped = true %}{% endif -%}
{%- endfor -%}
{%- set_global editable = [] -%}
{%- set_global refs = [] -%}
{%- set_global selects = [] -%}
{%- set_global account_refs = [] -%}
{%- for f in fields -%}
{%- set base = f.rust_type | replace(from="Option<", to="") | replace(from=">", to="") -%}
{%- set is_ref = f.field_name is ending_with("_id") and f.rust_type in ["i64", "Option<i64>"] -%}
{%- set parent = f.field_name | trim_end_matches(pat="_id") | plural -%}
{%- if scoped and f.field_name == "account_id" -%}
{%- elif f.is_enum or base not in supported -%}
{%- elif is_ref and f.field_name != "user_id" and known_tables != "" and parent not in tables -%}
{%- else -%}
{%- set_global editable = editable | concat(with=f.field_name) -%}
{%- if is_ref -%}
{%- set_global refs = refs | concat(with=f.field_name) -%}
{%- if f.field_name != "user_id" %}{% set_global selects = selects | concat(with=f.field_name) %}{% endif -%}
{%- if scoped and parent in account_tables %}{% set_global account_refs = account_refs | concat(with=f.field_name) %}{% endif -%}
{%- endif -%}
{%- endif -%}
{%- endfor -%}
to: tests/requests/{{ snake_plural }}.rs
skip_exists: true
message: "Model logic for `{{ pascal_singular }}` (src/models/{{ snake_plural }}.rs) and its request test (tests/requests/{{ snake_plural }}.rs) were added."
injections:
- into: tests/requests/mod.rs
  after_last: "^mod \\w+;"
  content: "mod {{ snake_plural }};"
- into: src/models/{{ snake_plural }}.rs
  remove_lines: "^(impl (Model|ActiveModel|Entity) \\{\\}|// implement your .*)$"
  content: ""
- into: src/models/{{ snake_plural }}.rs
  after: "^use sea_orm::entity::prelude::\\*;"
  skip_if: "{{ pascal_singular }}Params"
  content: |-
    use loco_rs::model::{ModelError, ModelResult};
    use sea_orm::{IntoActiveModel, QueryOrder};
    use serde::Deserialize;
    use serde_json::json;

    use super::{
        _entities::{{ snake_plural }}::Column,
        cast,
        users::{Errors, SaveError},
    };
    use crate::db::First;
- into: src/models/{{ snake_plural }}.rs
  append: true
  skip_if: "pub struct {{ pascal_singular }}Params"
  content: |-
    /// `params.require(:{{ snake_singular }}).permit(...)`: every attribute as the form submitted it. Blank
    /// and missing are the same, like a Rails form, which always submits every field.{% if scoped %} The
    /// account is not a param: it comes from the URL.{% endif %}
    #[derive(Debug, Default, Deserialize)]
    pub struct {{ pascal_singular }}Params {
    {%- for f in fields %}{% if f.field_name in editable %}
        #[serde(default, deserialize_with = "cast::form_value")]
        pub {{ f.field_name }}: Option<String>,
    {%- endif %}{% endfor %}
    }

    impl {{ pascal_singular }}Params {
        /// Cast each attribute to its column type and assign it to `item`, returning the
        /// validation errors (Rails' `errors` after `valid?`).
        fn assign(&self, item: &mut ActiveModel) -> Errors {
            let mut errors = Errors::new();
    {%- for f in fields %}{% if f.field_name in editable %}
    {%- set base = f.rust_type | replace(from="Option<", to="") | replace(from=">", to="") %}
    {%- if base == "String" %}{% set cast = "string" %}
    {%- elif base == "bool" %}{% set cast = "boolean" %}
    {%- elif base == "Date" %}{% set cast = "date" %}
    {%- else %}{% set cast = "number" %}
    {%- endif %}
    {%- set attribute = f.field_name %}
    {%- if f.field_name in refs %}{% set cast = "reference" %}{% set attribute = f.field_name | trim_end_matches(pat="_id") %}{% endif %}
    {%- if f.nullable and base != "bool" %}{% set cast = "optional_" ~ cast %}{% endif %}
    {%- set value = "cast::" ~ cast ~ '(&mut errors, "' ~ attribute ~ '", self.' ~ f.field_name ~ ".as_deref())" %}
            item.{{ f.field_name }} = sea_orm::ActiveValue::Set({% if base == "bool" and f.nullable %}Some({{ value }}){% else %}{{ value }}{% endif %});
    {%- endif %}{% endfor %}
            errors
        }

        /// The validation errors, without writing (Precognition).{% if refs | length > 0 %} Casting only: whether
        /// the parent row exists is checked on save ([`Self::validate`]).{% endif %}
        #[must_use]
        pub fn errors(&self) -> Errors {
            self.assign(&mut <ActiveModel as Default>::default())
        }
    {%- if refs | length > 0 %}

        /// [`Self::assign`], then `belongs_to`'s check that each parent row exists{% if account_refs | length > 0 %} (in this
        /// {{ snake_singular | replace(from="_", to=" ") }}'s account, for parents that belong to accounts){% endif %}: a missing one is "must exist" on
        /// the association, as in Rails 8, not a foreign-key error.
        async fn validate(&self, db: &impl ConnectionTrait, item: &mut ActiveModel) -> ModelResult<Errors> {
            let mut errors = self.assign(item);
    {%- for f in fields %}{% if f.field_name in refs %}
    {%- set name = f.field_name %}
    {%- set association = name | trim_end_matches(pat="_id") %}
    {%- set parent = association | plural %}
    {%- if name in account_refs %}{% set find = "super::_entities::" ~ parent ~ "::Entity::find_by_id(id).filter(super::_entities::" ~ parent ~ "::Column::AccountId.eq(*item.account_id.as_ref()))" %}
    {%- else %}{% set find = "super::_entities::" ~ parent ~ "::Entity::find_by_id(id)" %}{% endif %}
            {% if f.nullable %}if let Some(id) = *item.{{ name }}.as_ref() {{ "{" }}{% else %}{{ "{" }}
                let id = *item.{{ name }}.as_ref();{% endif %}
                if errors.get("{{ association }}").is_none()
                    && {{ find }}
                        .first(db)
                        .await?
                        .is_none()
                {
                    errors.add("{{ association }}", "must exist");
                }
            }
    {%- endif %}{% endfor %}
            Ok(errors)
        }
    {%- endif %}
    }

    impl Model {
    {%- if scoped %}
        /// The account's {{ snake_plural | replace(from="_", to=" ") }}, oldest first (`Current.account.{{ snake_plural }}`).
        ///
        /// # Errors
        /// Database errors.
        pub async fn list(db: &DatabaseConnection, account_id: i64) -> ModelResult<Vec<Self>> {
            Ok(Entity::find()
                .filter(Column::AccountId.eq(account_id))
                .order_by_asc(Column::Id)
                .all(db)
                .await?)
        }

        /// `Current.account.{{ snake_plural }}.find(id)`: another account's id is not found.
        ///
        /// # Errors
        /// `ModelError::EntityNotFound` (a 404) when the account has no such {{ snake_singular | replace(from="_", to=" ") }}.
        pub async fn find_in_account(db: &DatabaseConnection, account_id: i64, id: i64) -> ModelResult<Self> {
            // `.first`, not `.one`: see `crate::db::First` (a bound LIMIT re-prepares the query).
            Entity::find_by_id(id)
                .filter(Column::AccountId.eq(account_id))
                .first(db)
                .await?
                .ok_or(ModelError::EntityNotFound)
        }

        /// `Current.account.{{ snake_plural }}.create(params)`.
        ///
        /// # Errors
        /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
        pub async fn create(
            db: &DatabaseConnection,
            account_id: i64,
            params: &{{ pascal_singular }}Params,
        ) -> Result<Self, SaveError> {
            let mut item = ActiveModel {
                account_id: sea_orm::ActiveValue::Set(account_id),
                ..Default::default()
            };
{%- if refs | length > 0 %}
            // The "must exist" reads and the write in one write transaction (`crate::db::begin_write`).
            let txn = crate::db::begin_write(db).await?;
            let errors = params.validate(&txn, &mut item).await?;
            if !errors.is_empty() {
                return Err(SaveError::Invalid(errors));
            }
            let saved = item.insert(&txn).await?;
            txn.commit().await?;
            Ok(saved)
{%- else %}
            let errors = params.assign(&mut item);
            if !errors.is_empty() {
                return Err(SaveError::Invalid(errors));
            }
            Ok(item.insert(db).await?)
{%- endif %}
        }
    {%- else %}
        /// Every {{ snake_singular | replace(from="_", to=" ") }}, oldest first.
        ///
        /// # Errors
        /// Database errors.
        pub async fn list(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
            Ok(Entity::find().order_by_asc(Column::Id).all(db).await?)
        }

        /// # Errors
        /// `ModelError::EntityNotFound` (a 404) when there is no such {{ snake_singular | replace(from="_", to=" ") }}.
        pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> ModelResult<Self> {
            // `.first`, not `.one`: see `crate::db::First` (a bound LIMIT re-prepares the query).
            Entity::find_by_id(id)
                .first(db)
                .await?
                .ok_or(ModelError::EntityNotFound)
        }

        /// `{{ pascal_singular }}.create(params)`.
        ///
        /// # Errors
        /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
        pub async fn create(
            db: &DatabaseConnection,
            params: &{{ pascal_singular }}Params,
        ) -> Result<Self, SaveError> {
            let mut item = <ActiveModel as Default>::default();
{%- if refs | length > 0 %}
            // The "must exist" reads and the write in one write transaction (`crate::db::begin_write`).
            let txn = crate::db::begin_write(db).await?;
            let errors = params.validate(&txn, &mut item).await?;
            if !errors.is_empty() {
                return Err(SaveError::Invalid(errors));
            }
            let saved = item.insert(&txn).await?;
            txn.commit().await?;
            Ok(saved)
{%- else %}
            let errors = params.assign(&mut item);
            if !errors.is_empty() {
                return Err(SaveError::Invalid(errors));
            }
            Ok(item.insert(db).await?)
{%- endif %}
        }
    {%- endif %}

        /// `{{ snake_singular }}.update(params)`.
        ///
        /// # Errors
        /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
        pub async fn update(
            self,
            db: &DatabaseConnection,
            params: &{{ pascal_singular }}Params,
        ) -> Result<Self, SaveError> {
            let mut item = self.into_active_model();
{%- if refs | length > 0 %}
            // The "must exist" reads and the write in one write transaction (`crate::db::begin_write`).
            let txn = crate::db::begin_write(db).await?;
            let errors = params.validate(&txn, &mut item).await?;
            if !errors.is_empty() {
                return Err(SaveError::Invalid(errors));
            }
            let saved = item.update(&txn).await?;
            txn.commit().await?;
            Ok(saved)
{%- else %}
            let errors = params.assign(&mut item);
            if !errors.is_empty() {
                return Err(SaveError::Invalid(errors));
            }
            Ok(item.update(db).await?)
{%- endif %}
        }

        /// `{{ snake_singular }}.destroy`.
        ///
        /// # Errors
        /// Database errors.
        pub async fn destroy(self, db: &DatabaseConnection) -> ModelResult<()> {
            self.delete(db).await?;
            Ok(())
        }

        /// The page props for this {{ snake_singular | replace(from="_", to=" ") }}: what the frontend's `{{ pascal_singular }}` type
        /// describes (`frontend/pages/{{ snake_plural }}/form.tsx`).
        #[must_use]
        pub fn to_props(&self) -> serde_json::Value {
            json!({
                "id": self.id,
    {%- for f in fields %}{% if f.field_name in editable %}
                "{{ f.field_name }}": self.{{ f.field_name }},
    {%- endif %}{% endfor %}
            })
        }
    {%- for name in selects %}
    {%- set association = name | trim_end_matches(pat="_id") %}
    {%- set parent = association | plural %}

        /// The choices for the form's `{{ name }}` select: {% if name in account_refs %}the account's{% else %}every{% endif %} {{ association | replace(from="_", to=" ") }} as
        /// `{ id, label }`{% if name in account_refs %}, the same rows `validate` accepts{% else %}, unscoped like [`Self::list`] (scope both the same way){% endif %}.
        ///
        /// # Errors
        /// Database errors.
        pub async fn {{ association }}_options(db: &DatabaseConnection{% if name in account_refs %}, account_id: i64{% endif %}) -> ModelResult<Vec<serde_json::Value>> {
            let rows = super::_entities::{{ parent }}::Entity::find()
    {%- if name in account_refs %}
                .filter(super::_entities::{{ parent }}::Column::AccountId.eq(account_id))
    {%- endif %}
                .order_by_asc(super::_entities::{{ parent }}::Column::Id)
                .into_json()
                .all(db)
                .await?;
            Ok(rows
                .iter()
                .map(|row| {
                    // The label is the `name` column, else `title`, else `#id`: change it here.
                    let label = ["name", "title"]
                        .iter()
                        .find_map(|column| row[*column].as_str().map(str::to_owned))
                        .unwrap_or_else(|| format!("#{}", row["id"]));
                    json!({ "id": row["id"], "label": label })
                })
                .collect())
        }
    {%- endfor %}
    }
---
{% set_global required = [] -%}
{% for f in fields -%}{% if f.field_name in editable -%}
{% if f.field_name in refs %}{% if not f.nullable %}{% set association = f.field_name | trim_end_matches(pat="_id") %}{% set_global required = required | concat(with=association ~ `": ["must exist"]`) %}{% endif -%}
{% elif not f.nullable and f.rust_type != "bool" %}{% set_global required = required | concat(with=f.field_name ~ `": ["can't be blank"]`) %}{% endif -%}
{% endif %}{% endfor -%}
{% if scoped -%}
{% set index = "&route_table::" ~ snake_plural ~ `_path("acme")` -%}
{% set new_path = "&route_table::new_" ~ snake_singular ~ `_path("acme")` -%}
{% else -%}
{% set index = "route_table::" ~ upper_plural -%}
{% set new_path = "route_table::NEW_" ~ upper_singular -%}
{% endif -%}
//! `{{ pascal_plural }}Controller`: Rails' scaffold request spec, generated by
//! `cargo loco generate scaffold` (`.loco-templates/scaffold/api/dto.t`).{% if scoped %} The
//! {{ snake_plural | replace(from="_", to=" ") }} live in an account: the seeds' Acme (one@ owner, two@ member) and Globex (two@ owner).{% endif %}

use {{ pkg_name }}::{models::{% if selects | length > 0 %}{ {%- for name in selects %}_entities::{{ name | trim_end_matches(pat="_id") | plural }}, {% endfor %}{{ snake_plural }}}{% else %}{{ snake_plural }}{% endif %}, route_table};
use sea_orm::{% if selects | length > 0 %}{ActiveModelTrait, EntityTrait, PaginatorTrait}{% else %}{EntityTrait, PaginatorTrait}{% endif %};
use serde_json::{json, Value};
use serial_test::serial;

use super::*;
{%- if scoped %}

/// The seeds' accounts.
const ACME: i64 = 1;
const GLOBEX: i64 = 2;
const TWO: &str = "two@example.com";
{%- endif %}

/// What the form submits: every value is a string, as in a browser form.
fn attributes(n: u8) -> Value {
    json!({
{%- for f in fields %}{% if f.field_name in editable %}
{%- set base = f.rust_type | replace(from="Option<", to="") | replace(from=">", to="") %}
{%- if base == "String" %}
        "{{ f.field_name }}": format!("{{ f.label }} {n}"),
{%- elif base == "bool" %}
        "{{ f.field_name }}": if n == 1 { "1" } else { "0" },
{%- elif base == "Date" %}
        "{{ f.field_name }}": format!("2026-01-0{n}"),
{%- elif base == "f32" or base == "f64" %}
        "{{ f.field_name }}": format!("{n}.5"),
{%- else %}
        "{{ f.field_name }}": n.to_string(),
{%- endif %}
{%- endif %}{% endfor %}
    })
}

async fn count(ctx: &AppContext) -> u64 {
    {{ snake_plural }}::Entity::find().count(&ctx.db).await.unwrap()
}
{%- if selects | length > 0 %}

/// The rows `attributes(n)` points {% for name in selects %}{{ name }}{% if not loop.last %}, {% endif %}{% endfor %} at: id `n` for each `n` in `ids`, every
/// other column its type's default{% if account_refs | length > 0 %}, and those that belong to accounts in `account_id`{% endif %}. A parent with a
/// required foreign key of its own needs that column set here.
async fn create_parents(ctx: &AppContext, {% if account_refs | length > 0 %}account_id{% else %}_account_id{% endif %}: i64, ids: &[i64]) {
    for &id in ids {
{%- for name in selects %}
{%- set parent = name | trim_end_matches(pat="_id") | plural %}
        let mut row = {{ parent }}::ActiveModel::default_values();
        row.id = sea_orm::ActiveValue::Set(id);
{%- if name in account_refs %}
        row.account_id = sea_orm::ActiveValue::Set(account_id);
{%- endif %}
        row.insert(&ctx.db).await.expect("insert a {{ name | trim_end_matches(pat="_id") | replace(from="_", to=" ") }} for the test");
{%- endfor %}
    }
}
{%- endif %}

#[tokio::test]
#[serial]
async fn signed_out_visitors_are_sent_to_sign_in() {
    with_app(|server, _ctx| async move {
        assert_redirect(&server.get({{ index }}).await, route_table::SIGN_IN);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn create_show_edit_update_and_destroy_a_{{ snake_singular }}() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let page = inertia_get(&server, &ctx, {{ index }}).await;
        assert_eq!(page["component"], "{{ snake_plural }}/index");
        let page = inertia_get(&server, &ctx, {{ new_path }}).await;
        assert_eq!(page["component"], "{{ snake_plural }}/new");

{%- if selects | length > 0 %}
        create_parents(&ctx, {% if scoped %}ACME{% else %}0{% endif %}, &[1, 2]).await;
{%- endif %}

        let res = server.post({{ index }}).json(&attributes(1)).await;
        assert!(
            res.status_code().is_redirection(),
            "create answered {}: {}",
            res.status_code(),
            res.text()
        );
        assert_eq!(count(&ctx).await, 1);
        let created = {{ snake_plural }}::Entity::find().one(&ctx.db).await.unwrap().unwrap();
{%- if scoped %}
        assert_eq!(created.account_id, ACME, "the account comes from the URL");
        let path = route_table::{{ snake_singular }}_path("acme", created.id);
{%- else %}
        let path = route_table::{{ snake_singular }}_path(created.id);
{%- endif %}
        assert_redirect(&res, &path);
        let page = inertia_get(&server, &ctx, &path).await;
        assert_eq!(page["component"], "{{ snake_plural }}/show");
        assert_eq!(page["flash"]["notice"], "{{ label }} was successfully created.");
        assert_eq!(page["props"]["{{ snake_singular }}"], created.to_props());

{%- if scoped %}
        let edit = route_table::edit_{{ snake_singular }}_path("acme", created.id);
{%- else %}
        let edit = route_table::edit_{{ snake_singular }}_path(created.id);
{%- endif %}
        let page = inertia_get(&server, &ctx, &edit).await;
        assert_eq!(page["component"], "{{ snake_plural }}/edit");
        let res = server.patch(&path).json(&attributes(2)).await;
        assert_redirect(&res, &path);
        let page = inertia_get(&server, &ctx, &path).await;
        assert_eq!(page["flash"]["notice"], "{{ label }} was successfully updated.");
{%- if scoped %}
        let updated = {{ snake_plural }}::Model::find_in_account(&ctx.db, ACME, created.id).await.unwrap();
{%- else %}
        let updated = {{ snake_plural }}::Model::find_by_id(&ctx.db, created.id).await.unwrap();
{%- endif %}
        assert_ne!(updated, created);

        let res = server.delete(&path).await;
        assert_redirect(&res, {{ index }});
        assert_eq!(count(&ctx).await, 0);
        assert_eq!(server.get(&path).await.status_code(), 404);
    })
    .await;
}
{%- if scoped %}

/// `Current.account.{{ snake_plural }}`: a non-member gets 404 on the account's pages, and another
/// account's {{ snake_singular | replace(from="_", to=" ") }} is not found through your own account's URLs.
#[tokio::test]
#[serial]
async fn another_accounts_{{ snake_plural }}_are_not_found() {
    with_app(|mut server, ctx| async move {
        // two@ is in Globex (and Acme): make one there.
        sign_in(&mut server, &ctx, TWO).await;
{%- if selects | length > 0 %}
        create_parents(&ctx, GLOBEX, &[3]).await;
{%- endif %}
        let res = server.post(&route_table::{{ snake_plural }}_path("globex")).json(&attributes(3)).await;
        assert!(res.status_code().is_redirection(), "{}", res.text());
        let theirs = {{ snake_plural }}::Entity::find().one(&ctx.db).await.unwrap().unwrap();
        assert_eq!(theirs.account_id, GLOBEX);

        // one@ is only in Acme.
        server.clear_cookies();
        sign_in(&mut server, &ctx, ONE).await;
        let globex = route_table::{{ snake_singular }}_path("globex", theirs.id);
        for path in [route_table::{{ snake_plural }}_path("globex"), globex.clone()] {
            assert_eq!(server.get(&path).await.status_code(), 404, "{path}");
        }
        let through_acme = route_table::{{ snake_singular }}_path("acme", theirs.id);
        assert_eq!(server.get(&through_acme).await.status_code(), 404);
        assert_eq!(
            server.get(&route_table::edit_{{ snake_singular }}_path("acme", theirs.id)).await.status_code(),
            404
        );
        assert_eq!(server.patch(&through_acme).json(&attributes(1)).await.status_code(), 404);
        assert_eq!(server.delete(&through_acme).await.status_code(), 404);
        assert_eq!(server.delete(&globex).await.status_code(), 404);
        let page = inertia_get(&server, &ctx, &route_table::{{ snake_plural }}_path("acme")).await;
        assert_eq!(page["props"]["{{ snake_plural }}"], json!([]), "not listed in Acme");

        // Still there, unchanged.
        assert_eq!(count(&ctx).await, 1);
        let after = {{ snake_plural }}::Model::find_in_account(&ctx.db, GLOBEX, theirs.id).await.unwrap();
        assert_eq!(after, theirs);
    })
    .await;
}
{%- endif %}
{% if required | length > 0 %}
#[tokio::test]
#[serial]
async fn blank_required_attributes_are_rejected_with_rails_errors() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        let res = server.post({{ index }}).json(&json!({})).await;
        assert_redirect(&res, {{ new_path }});
        let page = inertia_get(&server, &ctx, {{ new_path }}).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ {% for pair in required %}"{{ pair }}{% if not loop.last %}, {% endif %}{% endfor %} })
        );
        assert_eq!(count(&ctx).await, 0);
    })
    .await;
}
{% endif -%}
{% if selects | length > 0 %}
#[tokio::test]
#[serial]
async fn a_missing_parent_is_a_validation_error_not_a_500() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        // `attributes` points at ids that don't exist yet: `belongs_to`'s "must exist".
        let res = server.post({{ index }}).json(&attributes(1)).await;
        assert_redirect(&res, {{ new_path }});
        let page = inertia_get(&server, &ctx, {{ new_path }}).await;
        assert_eq!(
            page["props"]["errors"],
            json!({ {% for name in selects %}"{{ name | trim_end_matches(pat="_id") }}": ["must exist"]{% if not loop.last %}, {% endif %}{% endfor %} })
        );
        assert_eq!(count(&ctx).await, 0);

        // The form's select lists them, labelled, once they do.
        create_parents(&ctx, {% if scoped %}ACME{% else %}0{% endif %}, &[1, 2]).await;
        let page = inertia_get(&server, &ctx, {{ new_path }}).await;
{%- for name in selects %}
        assert_eq!(page["props"]["{{ name | trim_end_matches(pat="_id") }}_options"][1]["id"], 2);
        assert!(page["props"]["{{ name | trim_end_matches(pat="_id") }}_options"][1]["label"].is_string());
{%- endfor %}
    })
    .await;
}
{% endif -%}
{% if account_refs | length > 0 %}
/// A parent from another account is not a choice and does not exist, as far as this account
/// is concerned: it is not in the select, and picking its id is "must exist".
#[tokio::test]
#[serial]
async fn a_parent_from_another_account_is_not_offered_and_does_not_exist() {
    with_app(|mut server, ctx| async move {
        sign_in(&mut server, &ctx, ONE).await;
        create_parents(&ctx, GLOBEX, &[1]).await;
        let page = inertia_get(&server, &ctx, {{ new_path }}).await;
{%- for name in account_refs %}
        assert_eq!(page["props"]["{{ name | trim_end_matches(pat="_id") }}_options"], json!([]));
{%- endfor %}
        let res = server.post({{ index }}).json(&attributes(1)).await;
        assert_redirect(&res, {{ new_path }});
        let page = inertia_get(&server, &ctx, {{ new_path }}).await;
{%- for name in account_refs %}
        assert_eq!(page["props"]["errors"]["{{ name | trim_end_matches(pat="_id") }}"], json!(["must exist"]));
{%- endfor %}
        assert_eq!(count(&ctx).await, 0);
    })
    .await;
}
{% endif -%}
