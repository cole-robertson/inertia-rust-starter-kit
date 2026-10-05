use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "memberships",
            &[
                ("id", ColType::PkAuto),
                ("role", ColType::StringWithDefault("member".to_string())),
            ],
            &[("account", ""), ("user", "")],
        )
        .await?;
        // A user belongs to an account once.
        m.create_index(
            Index::create()
                .name("idx-memberships-account_id-user_id")
                .table(Alias::new("memberships"))
                .col(Alias::new("account_id"))
                .col(Alias::new("user_id"))
                .unique()
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx-memberships-user_id")
                .table(Alias::new("memberships"))
                .col(Alias::new("user_id"))
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "memberships").await
    }
}
