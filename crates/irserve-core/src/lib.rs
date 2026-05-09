#![forbid(unsafe_code)]

mod clean_urls;
pub mod config;
mod dispatch;
mod mime;
mod normalize;
mod notfound;
mod redirects;
mod resolve;
mod server;
mod trailing_slash;

use std::net::SocketAddr;
use std::path::PathBuf;

pub use config::{
    load_serve_json, BoolOrGlobs, ConfigError, ConfigSource, HeaderItem, HeaderRule,
    LoadedConfig, RedirectRule, RewriteRule, ServeConfig,
};

pub struct ServerConfig {
    pub root: PathBuf,
    pub listens: Vec<SocketAddr>,
    pub serve_config: ServeConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn run(config: ServerConfig) -> Result<(), Error> {
    server::serve(config).await
}
