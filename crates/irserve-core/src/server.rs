use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response};
use axum::Router;
use tokio::net::TcpListener;

use crate::clean_urls::CleanUrlsView;
use crate::config::ServeConfig;
use crate::cors::apply_cors;
use crate::custom_headers::{compile_rules as compile_header_rules, HeaderRuleCompiled};
use crate::dispatch::dispatch;
use crate::listing::{DirectoryListingView, UnlistedFilter};
use crate::redirects::{compile_rules as compile_redirect_rules, RedirectRuleCompiled};
use crate::rewrites::{compile_rules as compile_rewrite_rules, RewriteRuleCompiled};
use crate::{Error, ServerConfig};

struct AppState {
    root: PathBuf,
    serve_config: ServeConfig,
    clean_urls_view: CleanUrlsView,
    listing_view: DirectoryListingView,
    unlisted_filter: UnlistedFilter,
    redirect_rules: Vec<RedirectRuleCompiled>,
    rewrite_rules: Vec<RewriteRuleCompiled>,
    header_rules: Vec<HeaderRuleCompiled>,
    cors: bool,
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
    // Redirect rule compilation surfaces patterns that fail to
    // produce a valid `regex::Regex` (the rare case — most malformed
    // sources are recovered to a Literal segment by
    // `classify_pattern_segment`'s globset-error fallback, Codex
    // round 10 P2). Anything that still fails is reported via stderr
    // and skipped; other rules continue to work. Mirrors the
    // reference's silent try/catch around `pathToRegExp` + minimatch
    // fallback in `sourceMatches` (`serve-handler/src/index.js:38-67`).
    let (redirect_rules, invalid_redirects) =
        compile_redirect_rules(&config.serve_config.redirects);
    for inv in &invalid_redirects {
        eprintln!(
            "warning: redirect source {:?} skipped (invalid pattern): {}",
            inv.source, inv.error
        );
    }
    // Rewrite rule compilation surfaces patterns that fail to produce
    // a valid `regex::Regex`. Same contract as redirects: invalid
    // rules are reported via stderr and skipped; other rules continue
    // to work. Mirrors the reference's silent try/catch around
    // `pathToRegExp` + minimatch fallback in `sourceMatches`
    // (`serve-handler/src/index.js:38-67`).
    let (rewrite_rules, invalid_rewrites) = compile_rewrite_rules(&config.serve_config.rewrites);
    for inv in &invalid_rewrites {
        eprintln!(
            "warning: rewrite source {:?} skipped (invalid pattern): {}",
            inv.source, inv.error
        );
    }
    // Custom-headers rule compilation (Stage 6f, SRV-HDR-001). Same
    // contract as redirects/rewrites: invalid sources are reported via
    // stderr and skipped; remaining rules continue to apply. Headers
    // run as a post-dispatch pass so they layer onto every response,
    // including 4xx error pages — mirrors the reference's getHeaders
    // call at `serve-handler/src/index.js:519`.
    let (header_rules, invalid_headers) = compile_header_rules(&config.serve_config.headers);
    for inv in &invalid_headers {
        eprintln!(
            "warning: header source {:?} skipped (invalid pattern): {}",
            inv.source, inv.error
        );
    }
    // Stage 6g: directoryListing scope. Same compile-once-per-server
    // pattern as cleanUrls. Invalid glob patterns are reported via
    // stderr and treated as never-matching, mirroring `minimatch`'s
    // silent fallback at `serve-handler/src/index.js:38-67`.
    let (listing_view, invalid_listings) =
        DirectoryListingView::from_config(&config.serve_config.directory_listing);
    for inv in &invalid_listings {
        eprintln!(
            "warning: directoryListing pattern {:?} skipped (invalid glob): {}",
            inv.pattern, inv.error
        );
    }
    // Stage 6g Slice 4: `unlisted` defaults (`.DS_Store`, `.git`)
    // plus user globs. Compiled once; the dispatcher consults
    // `is_excluded` per directory entry on every listing render.
    let (unlisted_filter, invalid_unlisted) =
        UnlistedFilter::from_config(&config.serve_config.unlisted);
    for inv in &invalid_unlisted {
        eprintln!(
            "warning: unlisted pattern {:?} skipped (invalid glob): {}",
            inv.pattern, inv.error
        );
    }
    let state: SharedState = Arc::new(AppState {
        root: config.root,
        serve_config: config.serve_config,
        clean_urls_view,
        listing_view,
        unlisted_filter,
        redirect_rules,
        rewrite_rules,
        header_rules,
        cors: config.cors,
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
    let response = dispatch(
        req,
        state.root.as_path(),
        &state.serve_config,
        &state.clean_urls_view,
        &state.listing_view,
        &state.unlisted_filter,
        &state.redirect_rules,
        &state.rewrite_rules,
        &state.header_rules,
    )
    .await;
    // SRV-CLI-010: layer CORS headers post-dispatch (after `apply_custom_headers`
    // inside `dispatch`) so they ride on every response — including 3xx
    // redirects, which `apply_custom_headers` deliberately skips. Mirrors
    // the reference's unconditional emission at
    // `third_party/serve/source/utilities/server.ts:65-70`.
    if state.cors {
        apply_cors(response)
    } else {
        response
    }
}
