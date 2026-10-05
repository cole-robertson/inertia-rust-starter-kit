//! `Server-Timing: app;dur=<ms>` on GET/HEAD responses: how long this request spent in the app,
//! from the outermost app layer until the response was ready (every middleware, the handler,
//! the database, SSR when it's on). Excludes the network and HTTP parsing. The browser exposes it
//! as `PerformanceResourceTiming.serverTiming`; the home page shows it.
//!
//! With it, `db;desc="N queries";dur=<ms>`: how many SQL queries the request ran and their total
//! time (src/db.rs counts them per request). Tests read the count back (`assert_max_queries`).
//!
//! Only on safe methods: a precise server-side duration on `POST /sign_in` would make the
//! password check's timing (which `users::Model::authenticate_by` evens out for unknown
//! emails) easier to measure from outside.

use std::time::Instant;

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
    Router,
};

pub const SERVER_TIMING: HeaderName = HeaderName::from_static("server-timing");

pub fn layer(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(middleware))
}

async fn middleware(req: Request, next: Next) -> Response {
    let timed = matches!(*req.method(), Method::GET | Method::HEAD);
    let started = Instant::now();
    let (mut res, queries, db_time) = crate::db::count_queries(next.run(req)).await;
    if timed {
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let db_ms = db_time.as_secs_f64() * 1000.0;
        let plural = if queries == 1 { "query" } else { "queries" };
        let value = format!("app;dur={ms:.3}, db;desc=\"{queries} {plural}\";dur={db_ms:.3}");
        if let Ok(value) = HeaderValue::from_str(&value) {
            res.headers_mut().append(SERVER_TIMING, value);
        }
    }
    res
}
