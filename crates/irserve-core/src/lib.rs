#![forbid(unsafe_code)]

mod clean_urls;
pub mod config;
mod cors;
mod custom_headers;
mod dispatch;
mod error;
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
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn run(config: ServerConfig) -> Result<(), Error> {
    server::serve(config).await
}
