use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response};
use axum::Router;
use tokio::net::TcpListener;

use crate::dispatch::dispatch;
use crate::{Error, ServerConfig};

type SharedRoot = Arc<PathBuf>;

pub async fn serve(config: ServerConfig) -> Result<(), Error> {
    let state: SharedRoot = Arc::new(config.root);
    let app: Router = Router::new().fallback(handler).with_state(state);

    let listener = TcpListener::bind(config.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handler(State(root): State<SharedRoot>, req: Request<Body>) -> Response<Body> {
    dispatch(req, root.as_path()).await
}
