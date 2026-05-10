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
///   `index.js:508`. Custom Content-Type from a user rule may
///   override the default (`Object.assign(defaultHeaders, related)`).
/// - **HTML client + no custom page** — emit the generic
///   `<h1>STATUS REASON</h1>\n` fallback, optionally apply custom
///   headers matched against the original request path
///   (mirrors reference's `getHeaders(.., absolutePath, null)` at
///   `index.js:519`), then FORCE `Content-Type: text/html;
///   charset=utf-8` last — reference does the same at `index.js:520`,
///   so a user rule cannot override the fallback's content type
///   (Codex review round 3 P1).
///
/// `skip_fallback_headers = true` is set by callers whose reference
/// equivalent invokes `getHeaders` with an `absolutePath` outside the
/// served root (lexical-escape 400, malformed-decode 400, symlink-
/// escape 400) — empirically those calls fail to match common rules
/// like `**` (the `path.relative` slasher-normalized form is brittle).
/// Skipping cleanly mirrors that empirical absence (D-015).
pub async fn error_response(
    status: StatusCode,
    request_headers: &HeaderMap,
    root: &Path,
    header_rules: &[HeaderRuleCompiled],
    request_path: &str,
    skip_fallback_headers: bool,
) -> Response<Body> {
    if accepts_json(request_headers) {
        // JSON branch: skip custom-header application entirely.
        return json_response(status);
    }

    if let Some(bytes) = read_custom_page(status, root).await {
        // Custom `<status>.html` branch always applies custom headers
        // matched against `/<status>.html`. Mirrors reference's
        // `getHeaders(.., errorPage, stats)` call at `index.js:508`.
        // Codex review round 3 P1 — the prior round-2 fix passed `&[]`
        // here for traversal/decode 400 sites, which incorrectly
        // suppressed custom headers on the custom-page branch too.
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
    let mut resp = if skip_fallback_headers {
        resp
    } else {
        apply_custom_headers(resp, request_path, header_rules)
    };
    // Force Content-Type to `text/html; charset=utf-8` AFTER any
    // custom-header application, mirroring reference's order at
    // `index.js:519-520`. A user rule that sets Content-Type via the
    // `headers` array does NOT override the fallback's content type.
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
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
