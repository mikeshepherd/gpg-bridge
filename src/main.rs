use clap::{Args, Parser, Subcommand};
use gpg_bridge::{GpgOpts, ServerOptions, bridge, run_server};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[clap(name = "gpg-bridge", version)]
pub struct App {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    GpgBridge {
        #[clap(flatten)]
        global_opts: GpgArgs,
    },
    Server {
        #[clap(flatten)]
        options: ServerArgs,
    },
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[arg(long)]
    listen_address: SocketAddr,
    #[arg(long)]
    agent_extra_socket: PathBuf,
    #[arg(long)]
    client_ca_cert: PathBuf,
    #[arg(long)]
    server_cert: PathBuf,
    #[arg(long)]
    server_key: PathBuf,
    #[arg(long, default_value_t = 64)]
    max_connections: usize,
}

#[derive(Debug, Args)]
pub struct GpgArgs {
    /// Sets the listenning to bridge the extra socket
    #[arg(long, value_name("ADDRESS"))]
    extra: String,
    /// Sets the path to gnupg extra socket optionaly
    #[arg(long, value_name("PATH"))]
    extra_socket: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let cfg = App::parse();

    match cfg.command {
        Command::GpgBridge { global_opts: opts } => {
            bridge(GpgOpts {
                listen_address: opts.extra,
                local_gpg_socket_path: opts.extra_socket,
            })
            .await?;
        }
        Command::Server { options } => {
            run_server(ServerOptions::new(
                options.listen_address,
                options.agent_extra_socket,
                options.client_ca_cert,
                options.server_cert,
                options.server_key,
                options.max_connections,
            )?)
            .await?;
        }
    }
    Ok(())
}
