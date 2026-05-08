#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

pub struct ServerConfig {
    pub root: PathBuf,
    pub listen: SocketAddr,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bind error: {0}")]
    Bind(#[from] std::io::Error),
}

pub async fn run(_config: ServerConfig) -> Result<(), Error> {
    unimplemented!("irserve-core::run is implemented in Stage-5b Slice 2")
}
