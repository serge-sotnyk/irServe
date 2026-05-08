use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use clap::{ArgAction, Parser};
use irserve_core::{run, ServerConfig};

#[derive(Parser)]
#[command(
    name = "irserve",
    version,
    about = "Strict-L0 Rust port of vercel/serve",
    disable_version_flag = true,
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: (),

    #[arg(short = 'l', long = "listen", value_name = "PORT")]
    listen: Option<u16>,

    #[arg(short = 'n', long = "no-clipboard")]
    no_clipboard: bool,

    #[arg(value_name = "DIRECTORY", default_value = ".")]
    directory: PathBuf,
}

fn resolve_port(cli_listen: Option<u16>) -> u16 {
    if let Some(p) = cli_listen {
        return p;
    }
    if let Ok(env_port) = std::env::var("PORT") {
        if let Ok(p) = env_port.parse::<u16>() {
            return p;
        }
    }
    3000
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let _ = cli.no_clipboard; // accepted as no-op per D-005

    let port = resolve_port(cli.listen);
    let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
    let root = cli.directory.canonicalize()?;

    let config = ServerConfig { root, listen };
    run(config).await?;
    Ok(())
}
