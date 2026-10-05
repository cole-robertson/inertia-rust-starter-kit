use inertia_rust_starter_kit::{app::App, models::_entities::memberships};
use loco_rs::testing::prelude::*;
use sea_orm::EntityTrait;
use serial_test::serial;

/// The `Memberships` entity matches the table its migration created: selecting every column
/// fails if the migration and the entity disagree (a renamed column, a type that does not
/// round-trip, a migration that never ran). Extend it as the model grows.
#[tokio::test]
#[serial]
async fn can_query_memberships() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();

    memberships::Entity::find()
        .all(&boot.app_context.db)
        .await
        .expect("`memberships` should be queryable: entity and migration must agree");
}

/// The last owner can't be demoted or removed; a second owner unlocks both.
#[tokio::test]
#[serial]
async fn an_account_always_keeps_an_owner() {
    use inertia_rust_starter_kit::models::{
        memberships::{Model, LAST_OWNER_MESSAGE},
        users::SaveError,
    };

    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    let db = &boot.app_context.db;
    let owner = Model::find_for(db, 1, 1).await.unwrap(); // one, owner of Acme
    let base = |err: SaveError| match err {
        SaveError::Invalid(errors) => errors.into_inner().remove("base").unwrap(),
        err => panic!("{err}"),
    };
    let err = owner.clone().change_role(db, "admin").await.unwrap_err();
    assert_eq!(base(err), [LAST_OWNER_MESSAGE]);
    let err = owner.clone().remove(db).await.unwrap_err();
    assert_eq!(base(err), [LAST_OWNER_MESSAGE]);
    let err = owner.clone().change_role(db, "king").await.unwrap_err();
    assert_eq!(base(err), ["Role is not included in the list"]);

    Model::find_for(db, 1, 2)
        .await
        .unwrap()
        .change_role(db, "owner")
        .await
        .unwrap();
    let demoted = owner.change_role(db, "member").await.unwrap();
    assert_eq!(demoted.role, "member");
}

/// `(account_id, user_id)` is unique in the database too.
#[tokio::test]
#[serial]
async fn a_user_is_in_an_account_once() {
    use sea_orm::{ActiveModelTrait, ActiveValue};

    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    let dup = memberships::ActiveModel {
        account_id: ActiveValue::Set(1),
        user_id: ActiveValue::Set(1),
        role: ActiveValue::Set("member".into()),
        ..Default::default()
    }
    .insert(&boot.app_context.db)
    .await;
    assert!(dup.is_err(), "the unique index refuses a second membership");
}
