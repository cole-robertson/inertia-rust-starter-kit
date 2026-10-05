use inertia_rust_starter_kit::{app::App, models::_entities::accounts};
use loco_rs::testing::prelude::*;
use sea_orm::EntityTrait;
use serial_test::serial;

/// The `Accounts` entity matches the table its migration created: selecting every column
/// fails if the migration and the entity disagree (a renamed column, a type that does not
/// round-trip, a migration that never ran). Extend it as the model grows.
#[tokio::test]
#[serial]
async fn can_query_accounts() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();

    accounts::Entity::find()
        .all(&boot.app_context.db)
        .await
        .expect("`accounts` should be queryable: entity and migration must agree");
}

/// Slugs come from the name, are unique, skip reserved paths, and fit `[a-z0-9-]{3,40}`.
#[tokio::test]
#[serial]
async fn slugs_are_generated_unique_and_well_formed() {
    use inertia_rust_starter_kit::models::accounts::{is_slug, AccountParams, Model};

    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    let db = &boot.app_context.db;
    let create = |name: &str| {
        let params = AccountParams {
            name: Some(name.to_owned()),
        };
        async move { Model::create_with_owner(db, &params, 1).await.unwrap().slug }
    };
    assert_eq!(create("Acme").await, "acme-2");
    assert_eq!(create("Acme").await, "acme-3");
    assert_eq!(create("Invitations").await, "invitations-2");
    assert_eq!(create("Ünïcödé Ltd.").await, "unicode-ltd");
    let name = "Long name ".repeat(6);
    let long = create(name.trim()).await;
    assert!(is_slug(&long) && long.len() <= 40, "{long}");
    let long2 = create(name.trim()).await;
    assert!(
        is_slug(&long2) && long2.ends_with("-2") && long2.len() <= 40,
        "{long2}"
    );
}
