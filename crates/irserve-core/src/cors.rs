use axum::body::Body;
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::Response;

// SRV-CLI-010: when `--cors` is set, every response carries the four
// permissive CORS headers the reference emits unconditionally before
// `serve-handler` runs (`third_party/serve/source/utilities/server.ts:65-70`).
// Applied as a post-dispatch pass: the headers must layer onto every status
// (including 3xx, which `apply_custom_headers` deliberately skips).
const CORS_HEADERS: &[(&str, &str)] = &[
    ("access-control-allow-origin", "*"),
    ("access-control-allow-headers", "*"),
    ("access-control-allow-credentials", "true"),
    ("access-control-allow-private-network", "true"),
];

pub fn apply_cors(mut response: Response<Body>) -> Response<Body> {
    let headers = response.headers_mut();
    for (name, value) in CORS_HEADERS {
        // `from_static` panics on invalid input, but our four constants
        // are validated at compile time by the static-string contract.
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn empty_response(status: StatusCode) -> Response<Body> {
        Response::builder()
            .status(status)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn injects_all_four_headers_on_200() {
        let resp = apply_cors(empty_response(StatusCode::OK));
        let headers = resp.headers();
        assert_eq!(headers.get("access-control-allow-origin").unwrap(), "*");
        assert_eq!(headers.get("access-control-allow-headers").unwrap(), "*");
        assert_eq!(
            headers.get("access-control-allow-credentials").unwrap(),
            "true"
        );
        assert_eq!(
            headers.get("access-control-allow-private-network").unwrap(),
            "true"
        );
    }

    #[test]
    fn applies_to_redirects_unlike_custom_headers() {
        // `apply_custom_headers` skips 3xx responses; `apply_cors` must not.
        let resp = apply_cors(empty_response(StatusCode::MOVED_PERMANENTLY));
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );
    }

    #[test]
    fn applies_to_404() {
        let resp = apply_cors(empty_response(StatusCode::NOT_FOUND));
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );
    }

    #[test]
    fn overwrites_existing_header() {
        let mut resp = empty_response(StatusCode::OK);
        resp.headers_mut().insert(
            HeaderName::from_static("access-control-allow-origin"),
            HeaderValue::from_static("https://example.com"),
        );
        let resp = apply_cors(resp);
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );
    }
}
