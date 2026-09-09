use clap::{Args, Parser, Subcommand};
#[cfg(unix)]
use gpg_bridge::{ClientOptions, client::run_client_until};
use gpg_bridge::{ServerOptions, run_server};
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
    Server {
        #[clap(flatten)]
        options: ServerArgs,
    },
    #[cfg(unix)]
    Client {
        #[clap(flatten)]
        options: ClientArgs,
    },
}

#[cfg(unix)]
#[derive(Debug, Args)]
pub struct ClientArgs {
    #[arg(long)]
    listen_socket: PathBuf,
    #[arg(long)]
    server_address: String,
    #[arg(long)]
    server_name: String,
    #[arg(long)]
    server_ca_cert: PathBuf,
    #[arg(long)]
    client_cert: PathBuf,
    #[arg(long)]
    client_key: PathBuf,
    #[arg(long, default_value_t = 64)]
    max_connections: usize,
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let cfg = App::parse();

    match cfg.command {
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
        #[cfg(unix)]
        Command::Client { options } => {
            let (sender, mut receiver) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                let _ = gpg_bridge::shutdown::notify_shutdown(sender).await;
            });
            run_client_until(
                ClientOptions::new(
                    options.listen_socket,
                    options.server_address,
                    options.server_name,
                    options.server_ca_cert,
                    options.client_cert,
                    options.client_key,
                    options.max_connections,
                )?,
                &mut receiver,
            )
            .await?;
        }
    }
    Ok(())
}
