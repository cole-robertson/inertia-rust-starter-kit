//! Benchmark-only endpoints (`--features bench`; see docs/BENCHMARK.md, "I/O-bound workloads").
//! The Rails kit gets the same three endpoints from `bench/rails-kit-io.patch`.
//!
//! - `GET /bench/upstream`: one HTTP GET to a mock upstream (`BENCH_UPSTREAM_URL`), returns
//!   once the (checked, parsed) response is in.
//! - `POST /bench/write`: inserts one `bench_events` row with the posted `payload`.
//! - `GET /bench/read`: the latest 20 rows plus the row count.
//!
//! Unauthenticated and CSRF-exempt (`inertia::csrf` skips `/bench/` in bench builds); every
//! other layer runs as in production.

use std::sync::Arc;

use loco_rs::prelude::*;
use sea_orm::{ActiveValue, PaginatorTrait, QueryOrder, QuerySelect};
use serde::Deserialize;
use serde_json::json;

use crate::models::bench_events;

pub const DEFAULT_UPSTREAM_URL: &str = "http://127.0.0.1:9900/slow?ms=100";

/// One shared client (connection pool) for the upstream, like a real app's API client.
#[derive(Clone)]
struct Upstream {
    http: reqwest::Client,
    url: Arc<str>,
}

fn upstream(ctx: &AppContext) -> Result<Upstream> {
    ctx.shared_store
        .get::<Upstream>()
        .ok_or_else(|| Error::Message("bench upstream client missing".into()))
}

async fn upstream_call(State(ctx): State<AppContext>) -> Result<Response> {
    let up = upstream(&ctx)?;
    let res = up.http.get(&*up.url).send().await.map_err(Error::wrap)?;
    if !res.status().is_success() {
        return Err(Error::Message(format!(
            "upstream answered {}",
            res.status()
        )));
    }
    let body: serde_json::Value = res.json().await.map_err(Error::wrap)?;
    format::json(json!({ "ok": true, "upstream": body }))
}

#[derive(Deserialize)]
struct WriteParams {
    payload: String,
}

async fn write(State(ctx): State<AppContext>, Json(params): Json<WriteParams>) -> Result<Response> {
    let row = bench_events::ActiveModel {
        payload: ActiveValue::Set(params.payload),
        created_at: ActiveValue::Set(chrono::Utc::now().into()),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    format::json(json!({ "ok": true, "id": row.id }))
}

async fn read(State(ctx): State<AppContext>) -> Result<Response> {
    let latest = bench_events::Entity::find()
        .order_by_desc(bench_events::Column::Id)
        .limit(20)
        .all(&ctx.db)
        .await?;
    let count = bench_events::Entity::find().count(&ctx.db).await?;
    format::json(json!({ "ok": true, "count": count, "latest": latest }))
}

/// Store the shared upstream client. `BENCH_UPSTREAM_URL` is read from the environment on
/// purpose: this is benchmark-only code and must not add keys to the kit's config files.
pub fn install(ctx: &AppContext) {
    if ctx.shared_store.contains::<Upstream>() {
        return;
    }
    let url = std::env::var("BENCH_UPSTREAM_URL").unwrap_or_else(|_| DEFAULT_UPSTREAM_URL.into());
    ctx.shared_store.insert(Upstream {
        http: reqwest::Client::new(),
        url: url.into(),
    });
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/bench")
        .add("/upstream", get(upstream_call))
        .add("/write", post(write))
        .add("/read", get(read))
}
