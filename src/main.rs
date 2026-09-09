use clap::{Args, Parser, Subcommand};
use gpg_bridge::{GpgOpts, NamedPipePath, SocketType, SshOpts, bridge};
use tokio::io;

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
    SshBridge {
        #[clap(flatten)]
        global_opts: SshArgs,
    },
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

#[derive(Debug, Args)]
pub struct SshArgs {
    /// Sets the path to the named pipe of the existing ssh agent
    #[arg(
        long,
        value_name("SSH_AGENT_SOCKET_PATH"),
        value_parser = clap::value_parser!(NamedPipePath)
    )]
    ssh_socket: NamedPipePath,
    /// Sets the path to the socket the agent will listen on
    #[arg(
        long,
        value_name("LISTENING_SOCKET"),
        value_parser = clap::value_parser!(NamedPipePath)
    )]
    listening_socket: NamedPipePath,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    pretty_env_logger::init();
    let cfg = App::parse();

    match cfg.command {
        Command::GpgBridge { global_opts: opts } => {
            println!("Starting gpg-bridge using config {opts:?}");
            bridge(SocketType::GPG(GpgOpts {
                listen_address: opts.extra,
                local_gpg_socket_path: opts.extra_socket,
            }))
            .await
        }
        Command::SshBridge { global_opts: opts } => {
            println!("Starting gpg-bridge using config {opts:?}");
            bridge(SocketType::SSH(SshOpts {
                ssh_socket: opts.ssh_socket,
                listening_socket: opts.listening_socket,
            }))
            .await
        }
    }
}
