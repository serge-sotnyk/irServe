use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE, LOCATION};
use axum::http::{Method, Request, Response, StatusCode};

use crate::config::ServeConfig;
use crate::mime::mime_for;
use crate::normalize::collapse_slashes;
use crate::notfound::not_found_response;
use crate::resolve::{resolve, ResolveOutcome};
use crate::trailing_slash::compute_trailing_slash_redirect;

pub async fn dispatch(
    req: Request<Body>,
    root: &Path,
    serve_config: &ServeConfig,
) -> Response<Body> {
    // Phase 1–2: method gate (existing).
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .expect("405 response should always build");
    }

    // Phase 3: silent multi-slash collapse (SRV-ROUT-005).
    let raw_path = req.uri().path();
    let url_path = collapse_slashes(raw_path);

    // Phase 4: cleanUrls 301 (Stage 6c).

    // Phase 5: trailingSlash 301 (SRV-ROUT-003 / SRV-ROUT-004).
    if let Some(target) = compute_trailing_slash_redirect(&url_path, serve_config.trailing_slash) {
        return redirect_301(&target);
    }

    // Phase 6: configured redirects (Stage 6d).
    // Phase 7: rewrites + --single (Stage 6e).
    // Phase 8: cleanUrls resolution (Stage 6c).

    // Phases 9–13: resolve → MIME → 404.
    let outcome = resolve(&url_path, root).await;

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => match tokio::fs::read(&p).await {
            Ok(bytes) => file_response(&p, bytes),
            Err(_) => not_found_response(req.headers()),
        },
        ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => not_found_response(req.headers()),
    }
}

fn redirect_301(target: &str) -> Response<Body> {
    let location = HeaderValue::from_str(target)
        .unwrap_or_else(|_| HeaderValue::from_static("/"));
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(LOCATION, location)
        .body(Body::empty())
        .expect("301 response should always build")
}

fn file_response(path: &Path, bytes: Vec<u8>) -> Response<Body> {
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(mime) = mime_for(path) {
        builder = builder.header(CONTENT_TYPE, HeaderValue::from_static(mime));
    }
    builder
        .body(Body::from(bytes))
        .expect("file response should always build")
}
