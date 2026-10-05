//! Benchmark-only `bench_events` table (`--features bench`): the same columns as the Rails
//! kit's `bench/rails-kit-io.patch` migration.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum BenchEvents {
    Table,
    Id,
    Payload,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(BenchEvents::Table)
                .col(
                    ColumnDef::new(BenchEvents::Id)
                        .integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(ColumnDef::new(BenchEvents::Payload).text().not_null())
                .col(
                    ColumnDef::new(BenchEvents::CreatedAt)
                        .timestamp_with_time_zone()
                        .not_null(),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(BenchEvents::Table).to_owned())
            .await
    }
}
