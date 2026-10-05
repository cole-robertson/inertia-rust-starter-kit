//! Laravel Precognition / Inertia v3 form precognition.
//!
//! A request with `Precognition: true` asks "would this validate?" — the
//! handler runs validation only and answers with `respond(errors)`, never
//! writing. `Precognition-Validate-Only: a,b` limits the answer to fields
//! the user has touched.

use axum::{
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{Map, Value};

pub const PRECOGNITION: HeaderName = HeaderName::from_static("precognition");
pub const PRECOGNITION_SUCCESS: HeaderName = HeaderName::from_static("precognition-success");
pub const PRECOGNITION_VALIDATE_ONLY: HeaderName =
    HeaderName::from_static("precognition-validate-only");

pub fn is_precognition(headers: &HeaderMap) -> bool {
    headers.get(PRECOGNITION).is_some_and(|v| v == "true")
}

/// Field names from `Precognition-Validate-Only` (comma-separated), or `None`
/// when absent/empty (validate everything).
pub fn validate_only(headers: &HeaderMap) -> Option<Vec<String>> {
    let fields: Vec<String> = headers
        .get_all(PRECOGNITION_VALIDATE_ONLY)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect();
    (!fields.is_empty()).then_some(fields)
}

/// `errors` restricted to the `Precognition-Validate-Only` fields.
/// Non-object `errors` count as "no errors".
pub fn filter_errors(headers: &HeaderMap, errors: &Value) -> Map<String, Value> {
    let Some(errors) = errors.as_object() else {
        return Map::new();
    };
    match validate_only(headers) {
        None => errors.clone(),
        Some(only) => errors
            .iter()
            .filter(|(k, _)| only.iter().any(|f| f == *k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    }
}

/// The precognition answer for `errors` (`{"field": ["msg"]}`), filtered by
/// the request's `Precognition-Validate-Only` header.
pub fn respond_for(headers: &HeaderMap, errors: &Value) -> Response {
    respond_map(filter_errors(headers, errors))
}

/// The precognition answer for `errors` (`{"field": ["msg"]}`):
/// 204 + `Precognition-Success: true` when empty, else 422 `{"errors": …}`.
/// Always carries `Precognition: true`.
pub fn respond(errors: &Value) -> Response {
    respond_map(filter_errors(&HeaderMap::new(), errors))
}

fn respond_map(errors: Map<String, Value>) -> Response {
    let mut res = if errors.is_empty() {
        let mut res = StatusCode::NO_CONTENT.into_response();
        res.headers_mut()
            .insert(PRECOGNITION_SUCCESS, HeaderValue::from_static("true"));
        res
    } else {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "errors": errors })),
        )
            .into_response()
    };
    res.headers_mut()
        .insert(PRECOGNITION, HeaderValue::from_static("true"));
    res.headers_mut().append(
        axum::http::header::VARY,
        HeaderValue::from_static("Precognition"),
    );
    res
}
