#![forbid(unsafe_code)]

mod dispatch;
mod resolve;
mod server;

use std::net::SocketAddr;
use std::path::PathBuf;

pub struct ServerConfig {
    pub root: PathBuf,
    pub listen: SocketAddr,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn run(config: ServerConfig) -> Result<(), Error> {
    server::serve(config).await
}
