use clap::{Args, Parser, Subcommand};
use gpg_bridge::{GpgOpts, bridge};
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
async fn main() -> io::Result<()> {
    pretty_env_logger::init();
    let cfg = App::parse();

    match cfg.command {
        Command::GpgBridge { global_opts: opts } => {
            println!("Starting gpg-bridge using config {opts:?}");
            bridge(GpgOpts {
                listen_address: opts.extra,
                local_gpg_socket_path: opts.extra_socket,
            })
            .await
        }
    }
}
