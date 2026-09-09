mod gpg;
mod protocol;
mod ssh;
mod util;

use std::str::FromStr;

use tokio::io;

use crate::{gpg::gpg_bridge, ssh::ssh_bridge};

#[derive(Clone, Copy)]
struct AgentMeta {
    port: u16,
    nonce: [u8; 16],
}

#[derive(Clone)]
pub struct GpgOpts {
    pub listen_address: String,
    pub local_gpg_socket_path: String,
}

#[derive(Clone, Debug)]
pub struct NamedPipePath(String);

impl FromStr for NamedPipePath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(String::from_str(s).map_err(|e| format!("{e}"))?))
    }
}

#[derive(Clone)]
pub struct SshOpts {
    pub ssh_socket: NamedPipePath,
    pub listening_socket: NamedPipePath,
}

#[derive(Clone)]
pub enum SocketType {
    GPG(GpgOpts),
    SSH(SshOpts),
}

/// A bridge that forwards all requests from certain stream to an agent on Windows.
///
/// `local_gpg_socket_path` should point to the path of gnupg UDS.
/// `listen_address` is a TCP address that will be forwarded over ssh
/// # Errors
///
/// Will return `Err` if there is a failure to connect to the underlying agent
pub async fn bridge(socket_type: SocketType) -> io::Result<()> {
    match socket_type {
        SocketType::GPG(GpgOpts {
            listen_address,
            local_gpg_socket_path,
        }) => gpg_bridge(listen_address, local_gpg_socket_path).await,
        SocketType::SSH(SshOpts {
            ssh_socket,
            listening_socket,
        }) => ssh_bridge(ssh_socket, listening_socket).await,
    }
}
