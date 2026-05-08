use std::path::Path;

use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode};

use crate::resolve::{resolve, ResolveOutcome};

pub async fn dispatch(req: Request<Body>, root: &Path) -> Response<Body> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return empty_status(StatusCode::METHOD_NOT_ALLOWED);
    }

    let url_path = req.uri().path().to_string();
    let outcome = resolve(&url_path, root).await;

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => match tokio::fs::read(&p).await {
            Ok(bytes) => Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(bytes))
                .expect("static response should always build"),
            Err(_) => not_found_stub(),
        },
        ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => not_found_stub(),
    }
}

fn not_found_stub() -> Response<Body> {
    empty_status(StatusCode::NOT_FOUND)
}

fn empty_status(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("empty body response should always build")
}
