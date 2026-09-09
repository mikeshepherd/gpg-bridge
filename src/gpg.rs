use crate::agent::AgentConnector;
use log::error;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};

async fn delegate(mut remote: TcpStream, connector: AgentConnector) -> io::Result<()> {
    let mut agent = connector
        .connect()
        .await
        .map_err(|error| io::Error::other(error.to_string()))?;
    let _ = tokio::io::copy_bidirectional(&mut remote, &mut agent).await?;
    Ok(())
}

/// Retains the temporary plaintext listener until the Unix client is available.
pub async fn gpg_bridge(listen_address: String, redirect_file: String) -> io::Result<()> {
    let listener = TcpListener::bind(&listen_address).await?;
    let connector = AgentConnector::new(redirect_file.into());
    loop {
        let (remote, _) = listener.accept().await?;
        let connector = connector.clone();
        tokio::spawn(async move {
            if let Err(error) = delegate(remote, connector).await {
                error!("legacy bridge session failed: {error}");
            }
        });
    }
}
