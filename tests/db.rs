//! Every pooled SQLite connection carries the PRAGMAs (src/db.rs), not just the one Loco's
//! boot-time PRAGMA block happened to run on.

use inertia_rust_starter_kit::app::App;
use loco_rs::{app::Hooks, boot::StartMode, environment::Environment};
use sea_orm::sqlx::{self, Row};
use serial_test::serial;

async fn pragma(conn: &mut sqlx::SqliteConnection, name: &str) -> String {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA {name}")))
        .fetch_one(conn)
        .await
        .unwrap();
    row.try_get::<String, _>(0)
        .or_else(|_| row.try_get::<i64, _>(0).map(|n| n.to_string()))
        .unwrap()
}

#[tokio::test]
#[serial]
async fn every_pooled_sqlite_connection_is_wal_synchronous_normal_with_busy_timeout() {
    let mut config = App::load_config(&Environment::Test).await.unwrap();
    config.database.min_connections = 1;
    config.database.max_connections = 4;
    // Boot like production. The test config's recreate runs sea-orm-migration's
    // `PRAGMA foreign_keys = OFF` ... `= ON` through the pool, and the two can land on different
    // connections, leaving one with foreign keys off (a test-only artifact).
    config.database.dangerously_recreate = false;
    let boot = App::boot(StartMode::ServerOnly, &Environment::Test, config)
        .await
        .unwrap();
    let pool = boot.app_context.db.get_sqlite_connection_pool();

    // Hold four connections at once so they are four distinct connections.
    let mut conns = Vec::new();
    for _ in 0..4 {
        conns.push(pool.acquire().await.unwrap());
    }
    for (i, conn) in conns.iter_mut().enumerate() {
        let got = [
            pragma(conn, "journal_mode").await,
            pragma(conn, "synchronous").await,
            pragma(conn, "busy_timeout").await,
            pragma(conn, "foreign_keys").await,
            pragma(conn, "cache_size").await,
            pragma(conn, "mmap_size").await,
            pragma(conn, "journal_size_limit").await,
        ];
        assert_eq!(
            got,
            ["wal", "1", "5000", "1", "2000", "134217728", "67108864"],
            "connection {i}: journal_mode, synchronous, busy_timeout, foreign_keys, cache_size, \
             mmap_size, journal_size_limit"
        );
    }
}

#[tokio::test]
#[serial]
async fn the_sqlite_queue_file_is_in_wal_mode() {
    let boot = loco_rs::testing::prelude::boot_test::<App>().await.unwrap();
    let Some(loco_rs::config::QueueConfig::Sqlite(q)) = &boot.app_context.config.queue else {
        panic!("test config uses the SQLite queue");
    };
    let mut conn = <sqlx::SqliteConnection as sqlx::Connection>::connect(&q.uri)
        .await
        .unwrap();
    assert_eq!(pragma(&mut conn, "journal_mode").await, "wal");
}

/// Sum of SQLite's re-prepare counter over the connection's cached statements whose SQL
/// contains `needle`.
async fn reprepares(conn: &mut sqlx::SqliteConnection, needle: &str) -> i32 {
    use libsqlite3_sys::{
        sqlite3_next_stmt, sqlite3_sql, sqlite3_stmt_status, SQLITE_STMTSTATUS_REPREPARE,
    };
    let mut handle = conn.lock_handle().await.unwrap();
    let db = handle.as_raw_handle().as_ptr();
    let mut total = 0;
    // SAFETY: the handle is locked out of sqlx's worker thread for this scope, the statements
    // belong to this connection, and nothing is finalized while we walk the list.
    unsafe {
        let mut stmt = sqlite3_next_stmt(db, std::ptr::null_mut());
        while !stmt.is_null() {
            let sql = std::ffi::CStr::from_ptr(sqlite3_sql(stmt)).to_string_lossy();
            if sql.contains(needle) {
                total += sqlite3_stmt_status(stmt, SQLITE_STMTSTATUS_REPREPARE, 0);
            }
            stmt = sqlite3_next_stmt(db, stmt);
        }
    }
    total
}

/// The signed-in session lookup runs on every request. sea-orm's `.one()` binds `LIMIT ?`, which
/// makes SQLite re-prepare the statement on every call; `db::First` avoids it (docs/PROFILING.md).
#[tokio::test]
#[serial]
async fn the_session_lookup_is_prepared_once_not_on_every_request() {
    use inertia_rust_starter_kit::models::sessions;
    use loco_rs::testing::prelude::*;

    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();
    // One connection, so every lookup reuses the same cached prepared statement.
    let pool = boot.app_context.db.get_sqlite_connection_pool();
    let single = sqlx::pool::PoolOptions::<sqlx::Sqlite>::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(single.clone());

    for _ in 0..5 {
        sessions::Model::find_by_token_with_user(&db, "11111111-1111-4111-8111-111111111111")
            .await
            .unwrap();
    }
    let mut conn = single.acquire().await.unwrap();
    assert_eq!(
        reprepares(&mut conn, r#"FROM "sessions""#).await,
        0,
        "the session lookup was re-prepared: a bound LIMIT is back (use db::First, not .one())"
    );
}

/// Two pooled connections, interleaved the way two sign-ups can be: the first transaction reads
/// (its validations), a second connection commits a write, then the first writes. Opened by
/// `db::begin_write` (`BEGIN IMMEDIATE`), the first holds the write lock from `BEGIN`, so the
/// second waits (`busy_timeout`) and both succeed. With a plain deferred `db.begin()` the first
/// write fails at once with `SQLITE_BUSY_SNAPSHOT` (code 517, "database is locked").
#[tokio::test]
#[serial]
async fn a_write_transaction_that_reads_first_survives_a_concurrent_commit() {
    use std::time::Duration;

    use inertia_rust_starter_kit::db::begin_write;
    use sea_orm::ConnectionTrait;

    let mut config = App::load_config(&Environment::Test).await.unwrap();
    config.database.max_connections = 4;
    config.database.dangerously_recreate = false;
    let boot = App::boot(StartMode::ServerOnly, &Environment::Test, config)
        .await
        .unwrap();
    let db = boot.app_context.db.clone();
    db.execute_unprepared(
        "DROP TABLE IF EXISTS txn_probe; CREATE TABLE txn_probe (id INTEGER PRIMARY KEY, who TEXT)",
    )
    .await
    .unwrap();

    let txn = begin_write(&db).await.unwrap();
    txn.execute_unprepared("SELECT count(*) FROM txn_probe")
        .await
        .unwrap();
    // Another connection writes while the first transaction is open after its read.
    let other = {
        let db = db.clone();
        tokio::spawn(async move {
            db.execute_unprepared("INSERT INTO txn_probe (who) VALUES ('other')")
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let first = txn
        .execute_unprepared("INSERT INTO txn_probe (who) VALUES ('first')")
        .await;
    assert!(
        first.is_ok(),
        "the first transaction's write after its read failed: {first:?}"
    );
    txn.commit().await.unwrap();
    other
        .await
        .unwrap()
        .expect("the other writer waited for the lock and wrote");

    let rows = db
        .query_all_raw(sea_orm::Statement::from_string(
            db.get_database_backend(),
            "SELECT who FROM txn_probe ORDER BY id",
        ))
        .await
        .unwrap();
    let who: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String>("", "who").unwrap())
        .collect();
    assert_eq!(
        who,
        ["first", "other"],
        "the other writer waited for the first to commit"
    );
    db.execute_unprepared("DROP TABLE txn_probe").await.unwrap();
}
