use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE};
use axum::http::{HeaderMap, Response, StatusCode};

const JSON_ENVELOPE: &str =
    r#"{"error":{"code":"not_found","message":"The requested path could not be found"}}"#;

const HTML_BODY: &str = "<h1>404 Not Found</h1>\n";

pub fn not_found_response(headers: &HeaderMap) -> Response<Body> {
    if accepts_json(headers) {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(
                CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            )
            .body(Body::from(JSON_ENVELOPE))
            .expect("404 json response should always build")
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )
            .body(Body::from(HTML_BODY))
            .expect("404 html response should always build")
    }
}

fn accepts_json(headers: &HeaderMap) -> bool {
    let Some(accept) = headers.get(axum::http::header::ACCEPT) else {
        return false;
    };
    let Ok(value) = accept.to_str() else {
        return false;
    };
    value
        .split(',')
        .any(|part| part.trim().to_ascii_lowercase().starts_with("application/json"))
}
