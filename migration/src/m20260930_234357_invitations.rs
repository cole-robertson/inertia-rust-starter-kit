use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "invitations",
            &[
                ("id", ColType::PkAuto),
                ("email", ColType::String),
                ("role", ColType::StringWithDefault("member".to_string())),
                ("token_digest", ColType::StringUniq),
                ("expires_at", ColType::TimestampWithTimeZone),
                ("accepted_at", ColType::TimestampWithTimeZoneNull),
            ],
            // `inviter_id` references users: the first element is the table, not the association.
            &[("account", ""), ("users", "inviter_id")],
        )
        .await?;
        // Rails' `add_index :invitations, %i[account_id email]`: "unique pending" is a validation,
        // because whether an invitation is pending depends on the clock (`expires_at`).
        m.create_index(
            Index::create()
                .name("idx-invitations-account_id-email")
                .table(Alias::new("invitations"))
                .col(Alias::new("account_id"))
                .col(Alias::new("email"))
                .to_owned(),
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "invitations").await
    }
}
