mod gpg;
mod util;

use tokio::io;

use crate::gpg::gpg_bridge;

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

/// A bridge that forwards all requests from certain stream to an agent on Windows.
///
/// `local_gpg_socket_path` should point to the path of gnupg UDS.
/// `listen_address` is a TCP address that will be forwarded over ssh
/// # Errors
///
/// Will return `Err` if there is a failure to connect to the underlying agent
pub async fn bridge(
    GpgOpts {
        listen_address,
        local_gpg_socket_path,
    }: GpgOpts,
) -> io::Result<()> {
    gpg_bridge(listen_address, local_gpg_socket_path).await
}
