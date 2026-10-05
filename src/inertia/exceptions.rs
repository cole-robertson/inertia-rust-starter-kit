//! Rails' `ActionDispatch::PublicExceptions`: an error response (400, 404, 422, 500) becomes
//! `public/<status>.html`, or `{"status":404,"error":"Not Found"}` when the request asks for
//! JSON first. A known path with the wrong method is a 404 here too: Rails has no 405.
//!
//! Only bare error responses are rewritten: a response that is already an HTML page (the
//! public-file 404, the 406 browser page) is left alone for HTML requests, and Precognition
//! requests keep their own `{"errors": …}` / 400 answers. Headers the response already carries
//! (cookies, security headers) are kept; only the body and `Content-Type` change.

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};

use super::{precognition, public::PUBLIC_DIR};

pub fn layer(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(middleware))
}

/// Statuses that get a public page, as Rails' `config.exceptions_app` renders them.
const RENDERED: &[StatusCode] = &[
    StatusCode::BAD_REQUEST,
    StatusCode::NOT_FOUND,
    StatusCode::UNPROCESSABLE_ENTITY,
    StatusCode::INTERNAL_SERVER_ERROR,
];

async fn middleware(req: Request, next: Next) -> Response {
    let json = accepts_json_first(req.headers());
    let head = req.method() == Method::HEAD;
    let skip = precognition::is_precognition(req.headers());
    let res = next.run(req).await;
    if skip {
        return res;
    }
    let status = match res.status() {
        StatusCode::METHOD_NOT_ALLOWED => StatusCode::NOT_FOUND,
        s if RENDERED.contains(&s) => s,
        _ => return res,
    };
    let is_html = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    if is_html && !json && status == res.status() {
        return res;
    }
    let (mut parts, _) = res.into_parts();
    parts.status = status;
    parts.headers.remove(header::CONTENT_LENGTH);
    let (content_type, body) = if json {
        (
            "application/json; charset=utf-8",
            serde_json::json!({
                "status": status.as_u16(),
                "error": rack_reason(status),
            })
            .to_string(),
        )
    } else {
        (
            "text/html; charset=utf-8",
            public_page(status)
                .await
                .unwrap_or_else(|| rack_reason(status).to_owned()),
        )
    };
    parts
        .headers
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    let body = if head { String::new() } else { body };
    Response::from_parts(parts, Body::from(body)).into_response()
}

/// `Rack::Utils::HTTP_STATUS_CODES` for the statuses rendered here.
fn rack_reason(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "Bad Request",
        404 => "Not Found",
        422 => "Unprocessable Content",
        _ => "Internal Server Error",
    }
}

async fn public_page(status: StatusCode) -> Option<String> {
    tokio::fs::read_to_string(
        std::path::Path::new(PUBLIC_DIR).join(format!("{}.html", status.as_u16())),
    )
    .await
    .ok()
}

/// Rails' `request.formats.first == :json`: an `Accept` header that isn't a browser's
/// (`…, */*`) whose highest-ranked media type is `application/json`.
#[must_use]
pub fn accepts_json_first(headers: &HeaderMap) -> bool {
    let Some(accept) = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .filter(|a| !a.trim().is_empty())
    else {
        return false;
    };
    let browser_like = accept.split(',').count() > 1
        && accept.split(',').any(|part| part.trim().starts_with("*/*"));
    if browser_like {
        return false;
    }
    let mut ranked: Vec<(f32, usize, &str)> = accept
        .split(',')
        .enumerate()
        .map(|(i, part)| {
            let mut pieces = part.split(';');
            let media = pieces.next().unwrap_or_default().trim();
            let q = pieces
                .filter_map(|p| p.trim().strip_prefix("q="))
                .find_map(|q| q.parse::<f32>().ok())
                .unwrap_or(1.0);
            (q, i, media)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked
        .first()
        .is_some_and(|(_, _, media)| media.eq_ignore_ascii_case("application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT, HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn json_only_when_it_ranks_first_and_the_accept_is_not_a_browsers() {
        assert!(accepts_json_first(&accept("application/json")));
        assert!(accepts_json_first(&accept("application/json, text/plain")));
        assert!(accepts_json_first(&accept(
            "text/html;q=0.5, application/json"
        )));
        assert!(!accepts_json_first(&accept("text/html, application/json")));
        assert!(!accepts_json_first(&accept("application/json, */*")));
        assert!(!accepts_json_first(&accept("*/*")));
        assert!(!accepts_json_first(&accept(
            "text/html, application/xhtml+xml"
        )));
        assert!(!accepts_json_first(&HeaderMap::new()));
    }
}
