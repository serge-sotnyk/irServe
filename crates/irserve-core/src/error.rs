use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE};
use axum::http::{HeaderMap, Response, StatusCode};

use crate::custom_headers::{apply_custom_headers, HeaderRuleCompiled};

/// Build the response for any error status, mirroring per-branch the
/// reference's `sendError` at `serve-handler/src/index.js:467-524`:
///
/// - **JSON-preferring client** — emit a status-keyed envelope and
///   return WITHOUT applying custom headers. Reference returns at
///   `index.js:477-487` before reaching `getHeaders`.
/// - **HTML client + custom `<status>.html` exists** — emit the file
///   bytes with `text/html; charset=utf-8`, then apply custom headers
///   matched against the path of the custom page (`/<status>.html`),
///   matching reference's `getHeaders(.., errorPage, stats)` at
///   `index.js:508`.
/// - **HTML client + no custom page** — emit the generic
///   `<h1>STATUS REASON</h1>\n` fallback, then apply custom headers
///   matched against the original request path. Mirrors
///   reference's `getHeaders(.., absolutePath, null)` at
///   `index.js:519`. (Note: reference's `path.relative(current,
///   absolutePath)` followed by `slasher` produces a path roughly
///   equivalent to the request path — `Matcher::try_match` calls
///   `path_posix_resolve` which normalizes `..` segments, so passing
///   the raw decoded path here yields the same matching outcome.)
///
/// Codex review round 1 P1: the prior implementation applied custom
/// headers to all non-3xx responses uniformly via the top-level
/// `dispatch` wrapper, ignoring the JSON-skip rule and the
/// custom-page-vs-fallback path distinction. This per-branch wiring
/// is the corrective.
pub async fn error_response(
    status: StatusCode,
    request_headers: &HeaderMap,
    root: &Path,
    header_rules: &[HeaderRuleCompiled],
    request_path: &str,
) -> Response<Body> {
    if accepts_json(request_headers) {
        // JSON branch: skip custom-header application entirely.
        return json_response(status);
    }

    if let Some(bytes) = read_custom_page(status, root).await {
        let resp = Response::builder()
            .status(status)
            .header(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )
            .body(Body::from(bytes))
            .expect("custom error page response should always build");
        let custom_page_path = format!("/{}.html", status.as_u16());
        return apply_custom_headers(resp, &custom_page_path, header_rules);
    }

    let resp = fallback_html_response(status);
    apply_custom_headers(resp, request_path, header_rules)
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
