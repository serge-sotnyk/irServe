use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE};
use axum::http::{Method, Request, Response, StatusCode};

use crate::mime::mime_for;
use crate::notfound::not_found_response;
use crate::resolve::{resolve, ResolveOutcome};

pub async fn dispatch(req: Request<Body>, root: &Path) -> Response<Body> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .expect("405 response should always build");
    }

    let url_path = req.uri().path().to_string();
    let outcome = resolve(&url_path, root).await;

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => match tokio::fs::read(&p).await {
            Ok(bytes) => file_response(&p, bytes),
            Err(_) => not_found_response(req.headers()),
        },
        ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => {
            not_found_response(req.headers())
        }
    }
}

fn file_response(path: &Path, bytes: Vec<u8>) -> Response<Body> {
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(mime) = mime_for(path) {
        builder = builder.header(
            CONTENT_TYPE,
            HeaderValue::from_static(mime),
        );
    }
    builder
        .body(Body::from(bytes))
        .expect("file response should always build")
}
