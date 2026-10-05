//! `--features bench` only: the three benchmark endpoints (docs/BENCHMARK.md, "I/O-bound
//! workloads"). Forgery protection is ON, like production, to prove the `/bench/` CSRF exemption
//! is what lets the write through while every other POST still needs a token.

use axum::http::StatusCode;
use inertia_rust_starter_kit::{models::bench_events, route_table};
use sea_orm::{EntityTrait, PaginatorTrait};
use serde_json::{json, Value};
use serial_test::serial;

use super::*;

/// A one-route mock upstream on an ephemeral port; returns its `/slow` URL.
async fn mock_upstream() -> String {
    let app = axum::Router::new().route(
        "/slow",
        axum::routing::get(|| async { axum::Json(json!({ "slept_ms": 0 })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/slow")
}

#[tokio::test]
#[serial]
async fn bench_endpoints_call_upstream_write_rows_and_read_them_back() {
    std::env::set_var("BENCH_UPSTREAM_URL", mock_upstream().await);
    std::env::set_var("FORGERY_PROTECTION", "true");
    with_app(|server, ctx| async move {
        std::env::remove_var("FORGERY_PROTECTION");
        std::env::remove_var("BENCH_UPSTREAM_URL");

        let res = server.get("/bench/upstream").await;
        assert_eq!(res.status_code(), 200, "{}", res.text());
        assert_eq!(
            res.json::<Value>(),
            json!({ "ok": true, "upstream": { "slept_ms": 0 } })
        );

        let payload = "x".repeat(200);
        let mut ids = Vec::new();
        for _ in 0..3 {
            // No X-XSRF-TOKEN: only the /bench/ exemption lets this through.
            let res = server
                .post("/bench/write")
                .json(&json!({ "payload": payload }))
                .await;
            assert_eq!(res.status_code(), 200, "{}", res.text());
            let body = res.json::<Value>();
            assert_eq!(body["ok"], true);
            ids.push(body["id"].as_i64().unwrap());
        }
        assert!(ids.windows(2).all(|w| w[1] > w[0]), "ids {ids:?}");
        assert_eq!(
            bench_events::Entity::find().count(&ctx.db).await.unwrap(),
            3
        );

        let res = server.get("/bench/read").await;
        assert_eq!(res.status_code(), 200);
        let body = res.json::<Value>();
        assert_eq!(body["count"], 3);
        let latest = body["latest"].as_array().unwrap();
        assert_eq!(latest.len(), 3);
        assert_eq!(
            latest[0]["id"].as_i64(),
            ids.last().copied(),
            "newest first"
        );
        assert_eq!(latest[0]["payload"], payload.as_str());

        // The exemption is /bench/ only: a normal POST without a token is still refused.
        let res = server
            .post(route_table::SIGN_IN)
            .json(&json!({ "email": ONE, "password": PASSWORD }))
            .await;
        assert_eq!(res.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_failed_upstream_is_a_server_error_not_ok() {
    std::env::set_var("BENCH_UPSTREAM_URL", "http://127.0.0.1:9/unreachable");
    with_app(|server, _ctx| async move {
        std::env::remove_var("BENCH_UPSTREAM_URL");
        let res = server.get("/bench/upstream").await;
        assert_eq!(res.status_code(), 500);
    })
    .await;
}
