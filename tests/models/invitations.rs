use inertia_rust_starter_kit::{app::App, models::_entities::invitations};
use loco_rs::testing::prelude::*;
use sea_orm::EntityTrait;
use serial_test::serial;

/// The `Invitations` entity matches the table its migration created: selecting every column
/// fails if the migration and the entity disagree (a renamed column, a type that does not
/// round-trip, a migration that never ran). Extend it as the model grows.
#[tokio::test]
#[serial]
async fn can_query_invitations() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();

    invitations::Entity::find()
        .all(&boot.app_context.db)
        .await
        .expect("`invitations` should be queryable: entity and migration must agree");
}

/// Tokens are random, stored as a SHA-256 digest, and found by the plain token only.
#[tokio::test]
#[serial]
async fn tokens_are_stored_as_a_digest_and_found_by_the_plain_token() {
    use inertia_rust_starter_kit::models::invitations::{
        digest, generate_token, Model, SEED_TOKEN,
    };

    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    let db = &boot.app_context.db;
    let a = generate_token();
    assert_ne!(a, generate_token());
    assert_eq!(a.len(), 48);
    assert_eq!(
        digest("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let found = Model::find_by_token(db, SEED_TOKEN).await.unwrap();
    assert_eq!(found.invitation.email, "three@example.com");
    assert_eq!(found.invitation.token_digest, digest(SEED_TOKEN));
    assert!(Model::find_by_token(db, &found.invitation.token_digest)
        .await
        .is_err());
}

/// Accepting is once only, adds the membership with the invited role, and refuses an
/// expired invitation.
#[tokio::test]
#[serial]
async fn accept_adds_the_membership_once_and_refuses_when_expired() {
    use inertia_rust_starter_kit::models::{invitations::Model, memberships};

    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    let db = &boot.app_context.db;
    let invitation = invitations::Entity::find_by_id(1)
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let later = chrono::Utc::now() + chrono::Duration::days(8);
    assert!(invitation.accept(db, 2, later).await.is_err(), "expired");
    let now = chrono::Utc::now();
    let membership = invitation.accept(db, 1, now).await.unwrap();
    assert_eq!(
        membership.role, "owner",
        "an existing membership is kept as is"
    );
    assert!(invitation.accept(db, 1, now).await.is_err(), "only once");
    let reloaded: Model = invitations::Entity::find_by_id(1)
        .one(db)
        .await
        .unwrap()
        .unwrap();
    assert!(!reloaded.is_pending(now));
    assert_eq!(memberships::Model::count_in(db, 1).await.unwrap(), 2);
}
