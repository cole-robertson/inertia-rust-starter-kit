use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // `token` is the public, unguessable identifier (UUIDv4) that goes in the
        // signed `session_token` cookie and in URLs (`DELETE /sessions/:id`). The
        // autoincrement `id` never leaves the server.
        create_table(
            m,
            "sessions",
            &[
                ("id", ColType::PkAuto),
                ("token", ColType::StringUniq),
                ("user_agent", ColType::StringNull),
                ("ip_address", ColType::StringNull),
            ],
            &[("user", "")],
        )
        .await?;
        m.create_index(
            Index::create()
                .name("idx-sessions-user_id")
                .table(Alias::new("sessions"))
                .col(Alias::new("user_id"))
                .to_owned(),
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "sessions").await?;
        Ok(())
    }
}
