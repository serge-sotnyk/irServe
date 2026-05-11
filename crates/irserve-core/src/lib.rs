#![forbid(unsafe_code)]

mod clean_urls;
pub mod config;
mod cors;
mod custom_headers;
mod dispatch;
mod error;
mod etag;
mod listing;
mod mime;
mod normalize;
mod path_pattern;
mod redirects;
mod resolve;
mod rewrites;
mod server;
mod trailing_slash;

use std::net::SocketAddr;
use std::path::PathBuf;

pub use config::{
    load_serve_json, BoolOrGlobs, ConfigError, ConfigSource, HeaderItem, HeaderRule, LoadedConfig,
    RedirectRule, RewriteRule, ServeConfig,
};

pub struct ServerConfig {
    pub root: PathBuf,
    pub listens: Vec<SocketAddr>,
    pub serve_config: ServeConfig,
    /// SRV-CLI-010: when true, every response is layered with the four
    /// CORS headers the reference emits unconditionally
    /// (`third_party/serve/source/utilities/server.ts:65-70`). Applied
    /// post-dispatch in `server::handler`.
    pub cors: bool,
    /// SRV-CLI-016: when true, a busy `--listen` address is fatal
    /// instead of falling back to an ephemeral port. irserve enforces
    /// the documented contract; the reference declares the flag at
    /// `third_party/serve/source/utilities/cli.ts:158` but never reads
    /// it (vercel/serve#751, regression introduced in 14.0.0). See
    /// `docs/reference/serve/decisions.md` D-016.
    pub no_port_switching: bool,
    /// SRV-CLI-014: accepted under D-002 (terminal output not contractual).
    /// When true, irserve appends an elapsed-ms suffix to the per-request
    /// log line. No other observable effect.
    pub debug: bool,
    /// SRV-CLI-015: when true, irserve does NOT emit per-request log
    /// lines to stdout. When false, `handler` prints a single
    /// `{method} {path} -> {status}` line per request. Format is
    /// implementation-defined per D-002.
    pub no_request_logging: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// SRV-CLI-016: requested port was occupied and `--no-port-switching`
    /// disabled the fallback to an ephemeral port.
    #[error("listen address {addr} is already in use")]
    PortInUse { addr: SocketAddr },
}

pub async fn run(config: ServerConfig) -> Result<(), Error> {
    server::serve(config).await
}
