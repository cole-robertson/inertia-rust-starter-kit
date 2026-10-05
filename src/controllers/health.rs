//! `GET /up` (Rails' `rails/health#show`): 200 when the app booted. No auth, no Inertia.
//! HTML (a green page) by default, `{"status":"up","timestamp":…}` for JSON requests.

use axum::http::{header, HeaderMap};
use loco_rs::prelude::*;

use crate::{inertia::exceptions::accepts_json_first, route_table};

pub const UP_HTML: &str =
    r#"<!DOCTYPE html><html><body style="background-color: green"></body></html>"#;

async fn show(headers: HeaderMap) -> Result<Response> {
    let (content_type, body) = if accepts_json_first(&headers) {
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        (
            "application/json; charset=utf-8",
            serde_json::json!({ "status": "up", "timestamp": timestamp }).to_string(),
        )
    } else {
        ("text/html; charset=utf-8", UP_HTML.to_owned())
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::VARY, "Accept"),
        ],
        body,
    )
        .into_response())
}

pub fn routes() -> Routes {
    Routes::new().add(route_table::UP, get(show))
}
