//! SQLite connection settings on **every** pooled connection.
//!
//! Loco 1.2 (`loco_rs::db::connect`) runs its PRAGMA block once, on whichever pooled connection
//! executes it. `journal_mode=WAL` persists in the file, but `synchronous`, `cache_size` and
//! `mmap_size` are per connection, so every other connection kept SQLite's `synchronous=FULL`
//! (an fsync per commit): concurrent writes ran ~10x slower (docs/BENCHMARK.md, I/O section).
//! [`configure_sqlite_pool`] reopens the pool so each new connection gets the same settings as
//! Rails 8's SQLite adapter, which match Loco's block.

use std::{cell::Cell, future::Future, time::Duration};

use loco_rs::{app::AppContext, Result};
use sea_orm::{
    sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
    ConnectOptions, ConnectionTrait, Database, DbErr, EntityTrait, Select, SelectTwo,
    SqliteTransactionMode, TransactionOptions, TransactionTrait,
};

/// Begin a transaction that writes: `BEGIN IMMEDIATE` on SQLite (Rails 8's
/// `default_transaction_mode: :immediate`), a plain `BEGIN` elsewhere, a `SAVEPOINT` inside
/// another transaction.
///
/// A plain (deferred) `BEGIN` that reads and then writes fails at once with
/// `SQLITE_BUSY_SNAPSHOT` (extended code 517, "database is locked") if another connection
/// committed after its first read: the read snapshot can't be upgraded to a write, and
/// `busy_timeout` doesn't apply. `IMMEDIATE` takes the write lock at `BEGIN`, where
/// `busy_timeout` does wait. Use it for every transaction that writes; read-only ones may keep
/// `db.begin()`.
///
/// # Errors
/// When no connection is available or `BEGIN` fails (`SQLITE_BUSY` after `busy_timeout`).
pub async fn begin_write<C: TransactionTrait>(
    db: &C,
) -> std::result::Result<C::Transaction, DbErr> {
    db.begin_with_options(TransactionOptions {
        sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
        ..Default::default()
    })
    .await
}

/// Rails 8 / Loco values: WAL, `synchronous=NORMAL` (durable in WAL except on power loss,
/// never corrupt), 5 s busy wait instead of failing with `SQLITE_BUSY`, 128 MB mmap, a 64 MB
/// WAL size cap after checkpoints, and a 2,000-page cache.
fn sqlite_options(opts: SqliteConnectOptions) -> SqliteConnectOptions {
    opts.journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true)
        .pragma("mmap_size", "134217728")
        .pragma("journal_size_limit", "67108864")
        .pragma("cache_size", "2000")
}

/// For `Hooks::after_context`: replace `ctx.db` with a pool whose every connection uses
/// [`sqlite_options`], keeping the pool settings from `database:` in the config. Not SQLite, or
/// `database.run_on_start` set (the app asked for its own PRAGMAs): the pool is left as is.
///
/// # Errors
/// When the pool cannot be opened.
pub async fn configure_sqlite_pool(mut ctx: AppContext) -> Result<AppContext> {
    let cfg = &ctx.config.database;
    if !cfg.uri.starts_with("sqlite:") {
        return Ok(ctx);
    }
    if cfg.run_on_start.is_some() {
        tracing::info!("database.run_on_start is set; SQLite pool left as Loco opened it");
        return Ok(ctx);
    }
    let mut opt = ConnectOptions::new(&cfg.uri);
    opt.max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections)
        .connect_timeout(Duration::from_millis(cfg.connect_timeout))
        .idle_timeout(Duration::from_millis(cfg.idle_timeout))
        .sqlx_logging(cfg.enable_logging)
        .map_sqlx_sqlite_opts(sqlite_options);
    if let Some(ms) = cfg.acquire_timeout {
        opt.acquire_timeout(Duration::from_millis(ms));
    }
    let mut db = Database::connect(opt).await?;
    db.set_metric_callback(record_query);
    // Loco's pool is dropped here; its connections close with it.
    std::mem::replace(&mut ctx.db, db).close().await?;
    Ok(ctx)
}

tokio::task_local! {
    /// The queries this request has run so far: `(count, total time)`. Scoped per request by
    /// [`count_queries`]; a query outside a scope (a worker, a task, a spawned future) isn't
    /// counted.
    static QUERIES: Cell<(u32, Duration)>;
}

/// sea-orm's per-query metric callback: add one query and its time to the request's tally.
/// A `Cell` in a task-local: a lookup and a store per query, no locking.
fn record_query(info: &sea_orm::metric::Info<'_>) {
    let _ = QUERIES.try_with(|tally| {
        let (count, total) = tally.get();
        tally.set((count.saturating_add(1), total + info.elapsed));
    });
}

/// How many queries `fut` ran and how long they took (`Server-Timing: db`, `assert_max_queries`). Only
/// queries run on `fut`'s own task count.
pub async fn count_queries<F: Future>(fut: F) -> (F::Output, u32, Duration) {
    QUERIES
        .scope(Cell::new((0, Duration::ZERO)), async move {
            let out = fut.await;
            let (count, total) = QUERIES.with(Cell::get);
            (out, count, total)
        })
        .await
}

/// Loco's SQLite queue (`queue.kind: Sqlite`) opens its own pool with sqlx defaults and no
/// journal mode, so the queue file stays in rollback-journal mode, where a writer blocks every
/// reader. WAL is a property of the file, so setting it once here covers the queue's pool too.
/// (`synchronous` stays FULL on the queue's connections: Loco exposes no hook for it. The queue
/// only carries mail, so that is an fsync per job, not per request.)
///
/// # Errors
/// When the queue database cannot be opened.
pub async fn configure_sqlite_queue(ctx: &AppContext) -> Result<()> {
    let Some(loco_rs::config::QueueConfig::Sqlite(q)) = &ctx.config.queue else {
        return Ok(());
    };
    let mut opt = ConnectOptions::new(&q.uri);
    opt.max_connections(1)
        .sqlx_logging(false)
        .map_sqlx_sqlite_opts(|o| o.journal_mode(SqliteJournalMode::Wal));
    Database::connect(opt).await?.close().await?;
    Ok(())
}

/// `.first(db)`: the row a unique-key lookup found, like sea-orm's `.one(db)` but without a bound
/// `LIMIT`.
///
/// `.one()` appends `LIMIT ?` and binds `1`. SQLite's planner reads a bound LIMIT, so it marks
/// the prepared statement to be re-prepared whenever that parameter is bound again: every call
/// re-parsed and re-planned the SQL even though sqlx's statement cache hit. On the signed-in
/// session lookup that cost ~40% of the CPU time of `GET /dashboard` (docs/PROFILING.md).
///
/// Only for lookups by a unique key (primary key, token, unique email): it reads every matching
/// row, which is at most one there. For a query that can match many rows, keep `.one()` or
/// `.limit(n)`.
pub trait First {
    type Item;

    /// The matching row, or `None`.
    ///
    /// # Errors
    /// Database errors.
    fn first<C: ConnectionTrait>(
        self,
        db: &C,
    ) -> impl Future<Output = std::result::Result<Option<Self::Item>, DbErr>> + Send;
}

impl<E> First for Select<E>
where
    E: EntityTrait,
    E::Model: Send,
{
    type Item = E::Model;

    async fn first<C: ConnectionTrait>(
        self,
        db: &C,
    ) -> std::result::Result<Option<E::Model>, DbErr> {
        Ok(self.all(db).await?.into_iter().next())
    }
}

impl<E, F> First for SelectTwo<E, F>
where
    E: EntityTrait,
    F: EntityTrait,
    E::Model: Send,
    F::Model: Send,
{
    type Item = (E::Model, Option<F::Model>);

    async fn first<C: ConnectionTrait>(
        self,
        db: &C,
    ) -> std::result::Result<Option<Self::Item>, DbErr> {
        Ok(self.all(db).await?.into_iter().next())
    }
}
