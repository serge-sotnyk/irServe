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

    if config.listens.is_empty() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no listen addresses configured",
        )));
    }

    let mut listeners = Vec::with_capacity(config.listens.len());
    for addr in &config.listens {
        listeners.push(TcpListener::bind(addr).await?);
    }

    if listeners.len() == 1 {
        let listener = listeners.into_iter().next().expect("len==1");
        axum::serve(listener, app).await?;
        return Ok(());
    }

    // SRV-CLI-002: multiple `-l` flags are additive. Spawn one task per
    // listener; the program exits when any task errors or all complete.
    let mut handles = Vec::with_capacity(listeners.len());
    for listener in listeners {
        let app = app.clone();
        handles.push(tokio::spawn(
            async move { axum::serve(listener, app).await },
        ));
    }
    for handle in handles {
        match handle.await {
            Ok(serve_result) => serve_result?,
            Err(join_err) => {
                return Err(Error::Io(std::io::Error::other(join_err.to_string())));
            }
        }
    }
    Ok(())
}

async fn handler(State(root): State<SharedRoot>, req: Request<Body>) -> Response<Body> {
    dispatch(req, root.as_path()).await
}
