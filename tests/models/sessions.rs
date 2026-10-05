//! The Session model: random public tokens, per-user scoping.

use inertia_rust_starter_kit::{
    app::App,
    models::{
        sessions::{self, RequestDetails},
        users,
    },
};
use loco_rs::{app::AppContext, prelude::ModelError, testing::prelude::*};
use serial_test::serial;

async fn boot() -> AppContext {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    boot.app_context
}

#[tokio::test]
#[serial]
async fn create_records_request_details_and_a_random_uuid_token() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let details = RequestDetails {
        user_agent: Some("Mozilla/5.0 Test".into()),
        ip_address: Some("10.0.0.1".into()),
    };
    let a = sessions::Model::create_for_user(&ctx.db, &user, &details)
        .await
        .unwrap();
    let b = sessions::Model::create_for_user(&ctx.db, &user, &details)
        .await
        .unwrap();
    assert_eq!(a.user_agent.as_deref(), Some("Mozilla/5.0 Test"));
    assert_eq!(a.ip_address.as_deref(), Some("10.0.0.1"));
    assert_eq!(a.user_id, user.id);
    assert!(uuid::Uuid::parse_str(&a.token).is_ok());
    assert_ne!(a.token, b.token);
}

#[tokio::test]
#[serial]
async fn find_by_token_with_user_resolves_the_owner() {
    let ctx = boot().await;
    let (session, user) =
        sessions::Model::find_by_token_with_user(&ctx.db, "11111111-1111-4111-8111-111111111111")
            .await
            .unwrap();
    assert_eq!(session.id, 1);
    assert_eq!(user.email, "one@example.com");
    assert!(matches!(
        sessions::Model::find_by_token_with_user(&ctx.db, "nope").await,
        Err(ModelError::EntityNotFound)
    ));
}

#[tokio::test]
#[serial]
async fn list_for_user_is_newest_first_and_scoped() {
    let ctx = boot().await;
    let user = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    let newer = sessions::Model::create_for_user(&ctx.db, &user, &RequestDetails::default())
        .await
        .unwrap();
    let list = sessions::Model::list_for_user(&ctx.db, user.id)
        .await
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, newer.id);
    assert!(list.iter().all(|s| s.user_id == user.id));
}

#[tokio::test]
#[serial]
async fn destroy_for_user_cannot_touch_another_users_session() {
    let ctx = boot().await;
    let one = users::Model::find_by_email(&ctx.db, "one@example.com")
        .await
        .unwrap();
    // Session 2 belongs to user two.
    assert!(matches!(
        sessions::Model::destroy_for_user(&ctx.db, one.id, "22222222-2222-4222-8222-222222222222")
            .await,
        Err(ModelError::EntityNotFound)
    ));
    assert!(sessions::Model::find_by_token_with_user(
        &ctx.db,
        "22222222-2222-4222-8222-222222222222"
    )
    .await
    .is_ok());

    let destroyed =
        sessions::Model::destroy_for_user(&ctx.db, one.id, "11111111-1111-4111-8111-111111111111")
            .await
            .unwrap();
    assert_eq!(destroyed.id, 1);
    assert!(sessions::Model::list_for_user(&ctx.db, one.id)
        .await
        .unwrap()
        .is_empty());
}
