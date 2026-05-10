use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE};
use axum::http::{HeaderMap, Response, StatusCode};

/// Build the response for any error status.
///
/// Mirrors `serve-handler/src/index.js:467-524` (`sendError`):
/// JSON-preferring clients get a status-keyed envelope; HTML clients
/// receive `<statusCode>.html` from the served root if present, falling
/// back to a generic `<h1>STATUS REASON</h1>` body. The custom-page
/// lookup applies to any error status (SRV-FILE-003), not only 404.
pub async fn error_response(
    status: StatusCode,
    request_headers: &HeaderMap,
    root: &Path,
) -> Response<Body> {
    if accepts_json(request_headers) {
        return json_response(status);
    }

    if let Some(bytes) = read_custom_page(status, root).await {
        return Response::builder()
            .status(status)
            .header(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )
            .body(Body::from(bytes))
            .expect("custom error page response should always build");
    }

    fallback_html_response(status)
}

fn json_response(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )
        .body(Body::from(json_envelope_for(status)))
        .expect("error json response should always build")
}

fn fallback_html_response(status: StatusCode) -> Response<Body> {
    let reason = status.canonical_reason().unwrap_or("Error");
    let body = format!("<h1>{} {}</h1>\n", status.as_u16(), reason);
    Response::builder()
        .status(status)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .body(Body::from(body))
        .expect("error html response should always build")
}

async fn read_custom_page(status: StatusCode, root: &Path) -> Option<Vec<u8>> {
    let path = root.join(format!("{}.html", status.as_u16()));
    tokio::fs::read(&path).await.ok()
}

fn json_envelope_for(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => r#"{"error":{"code":"bad_request","message":"Bad Request"}}"#,
        404 => {
            r#"{"error":{"code":"not_found","message":"The requested path could not be found"}}"#
        }
        _ => r#"{"error":{"code":"server_error","message":"Internal Server Error"}}"#,
    }
}

fn accepts_json(headers: &HeaderMap) -> bool {
    let Some(accept) = headers.get(axum::http::header::ACCEPT) else {
        return false;
    };
    let Ok(value) = accept.to_str() else {
        return false;
    };
    value.split(',').any(|part| {
        part.trim()
            .to_ascii_lowercase()
            .starts_with("application/json")
    })
}
