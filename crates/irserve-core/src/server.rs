use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response};
use axum::Router;
use tokio::net::TcpListener;

use crate::clean_urls::CleanUrlsView;
use crate::config::ServeConfig;
use crate::dispatch::dispatch;
use crate::{Error, ServerConfig};

struct AppState {
    root: PathBuf,
    serve_config: ServeConfig,
    clean_urls_view: CleanUrlsView,
}

type SharedState = Arc<AppState>;

pub async fn serve(config: ServerConfig) -> Result<(), Error> {
    // Mirrors the reference's behavior at
    // `serve-handler/src/index.js:38-67` (via `minimatch`): invalid
    // cleanUrls glob patterns are silently treated as never-matching
    // and the server keeps running. Surface a stderr warning per
    // skipped pattern so users notice the typo.
    let (clean_urls_view, invalid_globs) =
        CleanUrlsView::from_config(&config.serve_config.clean_urls);
    for inv in &invalid_globs {
        eprintln!(
            "warning: cleanUrls pattern {:?} skipped (invalid glob): {}",
            inv.pattern, inv.error
        );
    }
    let state: SharedState = Arc::new(AppState {
        root: config.root,
        serve_config: config.serve_config,
        clean_urls_view,
    });
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

async fn handler(State(state): State<SharedState>, req: Request<Body>) -> Response<Body> {
    dispatch(
        req,
        state.root.as_path(),
        &state.serve_config,
        &state.clean_urls_view,
    )
    .await
}
