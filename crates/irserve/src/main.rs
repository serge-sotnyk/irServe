use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use clap::{ArgAction, Parser};
use irserve_core::{load_serve_json, run, ServerConfig};

#[derive(Parser)]
#[command(
    name = "irserve",
    version,
    about = "Strict-L0 Rust port of vercel/serve",
    disable_version_flag = true
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: (),

    #[arg(
        short = 'l',
        long = "listen",
        value_name = "PORT",
        action = ArgAction::Append
    )]
    listen: Vec<u16>,

    #[arg(short = 'n', long = "no-clipboard")]
    no_clipboard: bool,

    #[arg(short = 'c', long = "config", value_name = "PATH")]
    config: Option<PathBuf>,

    #[arg(value_name = "DIRECTORY", default_value = ".")]
    directory: PathBuf,
}

fn resolve_listens(cli_listens: Vec<u16>) -> Vec<u16> {
    if !cli_listens.is_empty() {
        return cli_listens;
    }
    if let Ok(env_port) = std::env::var("PORT") {
        if let Ok(p) = env_port.parse::<u16>() {
            return vec![p];
        }
    }
    vec![3000]
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let _ = cli.no_clipboard; // accepted as no-op per D-005

    let listens: Vec<SocketAddr> = resolve_listens(cli.listen)
        .into_iter()
        .map(|p| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), p))
        .collect();

    let loaded = load_serve_json(&cli.directory, cli.config.as_deref())?;
    if let Some(loaded) = &loaded {
        use irserve_core::ConfigSource::*;
        match loaded.source {
            NowJson => eprintln!(
                "warning: `now.json` is deprecated; please use `serve.json`"
            ),
            PackageJson => eprintln!(
                "warning: configuration via `package.json#static` is deprecated; please use `serve.json`"
            ),
            ServeJson | Explicit => {}
        }
    }
    let serve_config = loaded.map(|l| l.config).unwrap_or_default();

    let public_segment = serve_config.public.as_deref().unwrap_or(".");
    let root = cli.directory.join(public_segment).canonicalize()?;

    let config = ServerConfig {
        root,
        listens,
        serve_config,
    };
    run(config).await?;
    Ok(())
}
