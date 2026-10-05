#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;
mod m20220101_000001_users;
mod m20250801_000002_sessions;
#[cfg(feature = "bench")]
mod m20260928_000003_bench_events;

mod m20260930_232957_accounts;
mod m20260930_234104_memberships;
mod m20260930_234357_invitations;
mod m20260930_235218_add_last_account_id_to_users;
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        #[allow(unused_mut)]
        let mut migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(m20220101_000001_users::Migration),
            Box::new(m20250801_000002_sessions::Migration),
            Box::new(m20260930_232957_accounts::Migration),
            Box::new(m20260930_234104_memberships::Migration),
            Box::new(m20260930_234357_invitations::Migration),
            Box::new(m20260930_235218_add_last_account_id_to_users::Migration),
            // inject-above (do not remove this comment)
        ];
        // Benchmark-only table; exists only in `--features bench` builds.
        #[cfg(feature = "bench")]
        migrations.push(Box::new(m20260928_000003_bench_events::Migration));
        migrations
    }
}
