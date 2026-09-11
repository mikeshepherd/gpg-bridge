use clap::{Args, Parser, Subcommand};
#[cfg(unix)]
use gpg_bridge::{ClientOptions, client::run_client_until};
use gpg_bridge::{ServerAgent, ServerOptions, run_server};
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
    /// Run the Windows Service Control Manager host for the bridge server.
    #[cfg(windows)]
    WindowsService {
        #[clap(flatten)]
        options: ServerArgs,
    },
    /// Run the Windows Service Control Manager host for Step CA certificate renewal.
    #[cfg(windows)]
    WindowsRenewalService {
        #[clap(flatten)]
        options: WindowsRenewalServiceArgs,
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
    #[cfg(unix)]
    #[arg(long)]
    listen_address: SocketAddr,
    #[cfg(windows)]
    #[arg(
        long,
        required_unless_present = "tailscale_listen_port",
        conflicts_with = "tailscale_listen_port"
    )]
    listen_address: Option<SocketAddr>,
    /// Resolve the Tailscale IPv4 address at startup and listen on this port.
    #[cfg(windows)]
    #[arg(long, conflicts_with = "listen_address")]
    tailscale_listen_port: Option<u16>,
    #[cfg(unix)]
    #[arg(
        long,
        required_unless_present = "agent_socket",
        conflicts_with = "agent_socket"
    )]
    agent_extra_socket: Option<PathBuf>,
    #[cfg(not(unix))]
    #[arg(long)]
    agent_extra_socket: PathBuf,
    /// Local Unix `GnuPG` extra socket; unavailable on Windows.
    #[cfg(unix)]
    #[arg(
        long,
        required_unless_present = "agent_extra_socket",
        conflicts_with = "agent_extra_socket"
    )]
    agent_socket: Option<PathBuf>,
    #[arg(long)]
    client_ca_cert: PathBuf,
    #[arg(long)]
    server_cert: PathBuf,
    #[arg(long)]
    server_key: PathBuf,
    #[arg(long, default_value_t = 64)]
    max_connections: usize,
}

#[cfg(windows)]
#[derive(Debug, Args)]
pub struct WindowsRenewalServiceArgs {
    #[arg(long)]
    renewal_script: PathBuf,
    #[arg(long)]
    step_executable: PathBuf,
    #[arg(long)]
    ca_url: String,
    #[arg(long)]
    root_ca_cert: PathBuf,
    #[arg(long)]
    server_cert: PathBuf,
    #[arg(long)]
    server_key: PathBuf,
    #[arg(long)]
    expected_dns_name: String,
    #[arg(long, default_value = "gpg-bridge")]
    bridge_service_name: String,
    #[arg(long)]
    log_path: PathBuf,
    #[arg(long, default_value_t = 21_600)]
    renewal_interval_seconds: u64,
}

#[cfg(unix)]
fn server_options(options: ServerArgs) -> Result<ServerOptions, Box<dyn std::error::Error>> {
    #[cfg(unix)]
    let listen_address = options.listen_address;
    let agent = {
        #[cfg(unix)]
        if let Some(socket) = options.agent_socket {
            ServerAgent::UnixSocket(socket)
        } else {
            ServerAgent::Gpg4winRedirect(options.agent_extra_socket.expect("required by Clap"))
        }
        #[cfg(not(unix))]
        ServerAgent::Gpg4winRedirect(options.agent_extra_socket)
    };
    Ok(ServerOptions::new_with_agent(
        listen_address,
        agent,
        options.client_ca_cert,
        options.server_cert,
        options.server_key,
        options.max_connections,
    )?)
}

#[cfg(windows)]
fn server_options(
    options: ServerArgs,
) -> Result<(ServerOptions, Option<u16>), Box<dyn std::error::Error>> {
    let tailscale_listen_port = options.tailscale_listen_port;
    let listen_address = match (options.listen_address, tailscale_listen_port) {
        (Some(address), None) => address,
        // The service wrapper replaces this placeholder after registering with
        // SCM. It must not run the Tailscale CLI before that registration.
        (None, Some(port)) => SocketAddr::from(([0, 0, 0, 0], port)),
        _ => unreachable!("Clap enforces exactly one listener mode"),
    };
    Ok((
        ServerOptions::new_with_agent(
            listen_address,
            ServerAgent::Gpg4winRedirect(options.agent_extra_socket),
            options.client_ca_cert,
            options.server_cert,
            options.server_key,
            options.max_connections,
        )?,
        tailscale_listen_port,
    ))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let cfg = App::parse();

    match cfg.command {
        Command::Server { options } => {
            #[cfg(unix)]
            run_server(server_options(options)?).await?;
            #[cfg(windows)]
            {
                let (mut options, tailscale_listen_port) = server_options(options)?;
                if let Some(port) = tailscale_listen_port {
                    options.listen_address = gpg_bridge::tailscale::listen_address(port)?;
                }
                run_server(options).await?;
            }
        }
        #[cfg(windows)]
        Command::WindowsService { options } => {
            let (options, tailscale_listen_port) = server_options(options)?;
            gpg_bridge::windows_service::run(options, tailscale_listen_port)?;
        }
        #[cfg(windows)]
        Command::WindowsRenewalService { options } => {
            gpg_bridge::windows_renewal_service::run(
                gpg_bridge::windows_renewal_service::RenewalOptions {
                    renewal_script: options.renewal_script,
                    step_executable: options.step_executable,
                    ca_url: options.ca_url,
                    root_ca_cert: options.root_ca_cert,
                    server_cert: options.server_cert,
                    server_key: options.server_key,
                    expected_dns_name: options.expected_dns_name,
                    bridge_service_name: options.bridge_service_name,
                    log_path: options.log_path,
                    renewal_interval: std::time::Duration::from_secs(
                        options.renewal_interval_seconds,
                    ),
                },
            )?;
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
