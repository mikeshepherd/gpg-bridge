pub mod agent;
#[cfg(unix)]
pub mod client;
pub mod error;
pub mod relay;
pub mod retry;
pub mod server;
pub mod shutdown;
pub mod tls;
#[cfg(unix)]
pub mod unix_socket;

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub listen_address: SocketAddr,
    pub agent_extra_socket: PathBuf,
    pub client_ca_cert: PathBuf,
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
    pub max_connections: NonZeroUsize,
}

#[cfg(unix)]
#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub listen_socket: PathBuf,
    pub server_address: String,
    pub server_name: String,
    pub server_ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
    pub max_connections: NonZeroUsize,
}

fn validate_options(
    path_values: impl IntoIterator<Item = PathBuf>,
    max_connections: usize,
) -> Result<NonZeroUsize, error::ConfigError> {
    for path in path_values {
        if path.as_os_str().is_empty() {
            return Err(error::ConfigError::EmptyPath);
        }
    }
    NonZeroUsize::new(max_connections).ok_or(error::ConfigError::ZeroMaxConnections)
}

impl ServerOptions {
    /// Creates validated server options.
    ///
    /// # Errors
    ///
    /// Returns an error for empty paths or a zero connection limit.
    pub fn new(
        listen_address: SocketAddr,
        agent_extra_socket: PathBuf,
        client_ca_cert: PathBuf,
        server_cert: PathBuf,
        server_key: PathBuf,
        max_connections: usize,
    ) -> Result<Self, error::ConfigError> {
        let max_connections = validate_options(
            [
                agent_extra_socket.clone(),
                client_ca_cert.clone(),
                server_cert.clone(),
                server_key.clone(),
            ],
            max_connections,
        )?;
        Ok(Self {
            listen_address,
            agent_extra_socket,
            client_ca_cert,
            server_cert,
            server_key,
            max_connections,
        })
    }
}

#[cfg(unix)]
impl ClientOptions {
    /// Creates validated Unix client options.
    ///
    /// # Errors
    ///
    /// Returns an error for empty values or a zero connection limit.
    pub fn new(
        listen_socket: PathBuf,
        server_address: String,
        server_name: String,
        server_ca_cert: PathBuf,
        client_cert: PathBuf,
        client_key: PathBuf,
        max_connections: usize,
    ) -> Result<Self, error::ConfigError> {
        if server_address.is_empty() {
            return Err(error::ConfigError::EmptyServerAddress);
        }
        if server_name.is_empty() {
            return Err(error::ConfigError::EmptyServerName);
        }
        let max_connections = validate_options(
            [
                listen_socket.clone(),
                server_ca_cert.clone(),
                client_cert.clone(),
                client_key.clone(),
            ],
            max_connections,
        )?;
        Ok(Self {
            listen_socket,
            server_address,
            server_name,
            server_ca_cert,
            client_cert,
            client_key,
            max_connections,
        })
    }
}

pub use server::run_server;
