use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::header::ACCEPT_ENCODING;
use axum::http::{Request, Response};
use axum::Router;
use tokio::net::TcpListener;

use crate::clean_urls::CleanUrlsView;
use crate::compression;
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
    debug: bool,
    no_request_logging: bool,
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
        debug: config.debug,
        no_request_logging: config.no_request_logging,
    });
    let app: Router = Router::new().fallback(handler).with_state(state);

    if config.listens.is_empty() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no listen addresses configured",
        )));
    }

    let allow_switching = !config.no_port_switching;
    let mut listeners = Vec::with_capacity(config.listens.len());
    for addr in &config.listens {
        listeners.push(bind_with_fallback(*addr, allow_switching).await?);
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

/// SRV-CLI-016: bind a listener with documented `--no-port-switching`
/// semantics. On `EADDRINUSE`:
/// - `allow_switching=true` (default) — retry once on `(addr.ip(), 0)`,
///   letting the OS pick a free ephemeral port. Mirrors the reference's
///   `serve-handler/source/utilities/server.ts:166-178` retry, but unlike
///   the reference we honor the flag (vercel/serve#751, D-016).
/// - `allow_switching=false` — surface `Error::PortInUse` so the caller
///   exits non-zero. The reference declares the flag but never reads it.
///
/// All other I/O errors propagate unchanged.
async fn bind_with_fallback(addr: SocketAddr, allow_switching: bool) -> Result<TcpListener, Error> {
    match TcpListener::bind(addr).await {
        Ok(l) => Ok(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if !allow_switching {
                eprintln!(
                    "error: listen address {addr} is already in use (--no-port-switching is set)"
                );
                return Err(Error::PortInUse { addr });
            }
            let fallback = SocketAddr::new(addr.ip(), 0);
            let listener = TcpListener::bind(fallback).await?;
            let actual = listener.local_addr()?;
            eprintln!("warning: listen address {addr} is already in use, switched to {actual}");
            Ok(listener)
        }
        Err(e) => Err(Error::Io(e)),
    }
}

async fn handler(State(state): State<SharedState>, req: Request<Body>) -> Response<Body> {
    // SRV-CLI-014/015: capture log metadata before dispatch consumes
    // the request. Format is implementation-defined per D-002 (terminal
    // output is not contractual).
    let log_meta =
        (!state.no_request_logging).then(|| (req.method().clone(), req.uri().path().to_string()));
    let start = state.debug.then(std::time::Instant::now);

    // SRV-CLI-012 (Stage 7e Codex round 1 P2): capture the request
    // method and `Accept-Encoding` BEFORE `dispatch` consumes the
    // request so the centralized compression pass downstream can
    // negotiate without re-reading the request.
    let req_method = req.method().clone();
    let req_accept_encoding = req.headers().get(ACCEPT_ENCODING).cloned();

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
    let response = if state.cors {
        apply_cors(response)
    } else {
        response
    };

    // SRV-CLI-012 (Stage 7e Codex round 1 P2): centralized compression
    // pass. Mirrors the reference's middleware which sits BETWEEN the
    // CORS-header injection and the `serve-handler` invocation at
    // `third_party/serve/source/utilities/server.ts:65-72` — the
    // ordering is CORS-then-compression. The pass internally skips
    // 206 (Range pre-empts compression), non-compressible MIMEs (no
    // Vary), `Cache-Control: no-transform` (no Vary), HEAD (Vary kept,
    // no encode), below-threshold bodies (Vary kept, no encode), and
    // no-acceptable-encoding negotiations.
    let response =
        compression::maybe_apply(response, &req_method, req_accept_encoding.as_ref(), &state.serve_config)
            .await;

    if let Some((method, path)) = log_meta {
        let status = response.status().as_u16();
        match start {
            Some(t) => {
                let ms = t.elapsed().as_millis();
                println!("{method} {path} -> {status} ({ms}ms)");
            }
            None => println!("{method} {path} -> {status}"),
        }
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn bind_retries_on_addr_in_use_when_switching_allowed() {
        let occupier = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let occupied = occupier.local_addr().unwrap();

        let new_listener = bind_with_fallback(occupied, true).await.unwrap();
        let new_addr = new_listener.local_addr().unwrap();
        assert_eq!(new_addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(new_addr.port(), occupied.port());
        assert_ne!(new_addr.port(), 0);
    }

    #[tokio::test]
    async fn bind_fails_on_addr_in_use_when_switching_disabled() {
        let occupier = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let occupied = occupier.local_addr().unwrap();

        let result = bind_with_fallback(occupied, false).await;
        match result {
            Err(Error::PortInUse { addr }) => assert_eq!(addr, occupied),
            other => panic!("expected PortInUse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn bind_succeeds_on_free_port() {
        let l1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let free = l1.local_addr().unwrap();
        drop(l1);
        // Tiny race window — the kernel may reuse the port. Acceptable
        // for a smoke test; if it ever flakes, switch to `bind_with_fallback`
        // through `serve()` directly.
        let listener = bind_with_fallback(free, false).await.unwrap();
        assert_eq!(listener.local_addr().unwrap().ip(), free.ip());
    }
}
