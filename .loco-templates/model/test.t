{#- The kit's model test for `cargo loco generate model|scaffold`: Loco's stock one uses `insta`,
    which this kit does not depend on. See .loco-templates/README.md. -#}
{% set plural_snake = name | plural | snake_case -%}
{% set model = name | plural | pascal_case -%}
to: "tests/models/{{plural_snake}}.rs"
message: "A test for model `{{model}}` was added. Run with `cargo test`."
skip_exists: true
injections:
- into: "tests/models/mod.rs"
  append: true
  content: "mod {{plural_snake}};"
---
use {{pkg_name}}::{app::App, models::_entities::{{plural_snake}}};
use loco_rs::testing::prelude::*;
use sea_orm::EntityTrait;
use serial_test::serial;

/// The `{{model}}` entity matches the table its migration created: selecting every column
/// fails if the migration and the entity disagree (a renamed column, a type that does not
/// round-trip, a migration that never ran). Extend it as the model grows.
#[tokio::test]
#[serial]
async fn can_query_{{plural_snake}}() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();

    {{plural_snake}}::Entity::find()
        .all(&boot.app_context.db)
        .await
        .expect("`{{plural_snake}}` should be queryable: entity and migration must agree");
}
