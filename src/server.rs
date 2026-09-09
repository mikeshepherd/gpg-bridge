use std::io;
use std::sync::Arc;
use std::time::Duration;

use log::{error, info, warn};
use socket2::{SockRef, TcpKeepalive};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;

use crate::ServerOptions;
use crate::agent::AgentConnector;
use crate::relay;
use crate::tls::{self, TlsError};

const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("TLS startup configuration failed: {0}")]
    Tls(#[from] TlsError),
    #[error("failed to bind server listener: {0}")]
    Bind(#[source] io::Error),
}

fn configure_tcp(stream: &TcpStream) {
    if let Err(error) = stream.set_nodelay(true) {
        warn!("could not enable TCP_NODELAY: {error}");
    }
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(30));
    if let Err(error) = SockRef::from(stream).set_tcp_keepalive(&keepalive) {
        warn!("could not configure TCP keepalive: {error}");
    }
}

async fn session(
    stream: TcpStream,
    acceptor: TlsAcceptor,
    connector: AgentConnector,
    mut shutdown: watch::Receiver<bool>,
) {
    configure_tcp(&stream);
    let tls_stream = match timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            warn!("TLS session rejected: {error}");
            return;
        }
        Err(_) => {
            warn!("TLS handshake timed out");
            return;
        }
    };
    if let Err(error) = tls::validate_alpn(tls_stream.get_ref().1.alpn_protocol()) {
        warn!("TLS session rejected: {error}");
        return;
    }
    let mut agent = match connector.connect().await {
        Ok(stream) => stream,
        Err(error) => {
            warn!("agent session setup failed: {error}");
            return;
        }
    };
    let mut tls_stream = tls_stream;
    match relay::relay(&mut tls_stream, &mut agent, &mut shutdown).await {
        Ok(stats) => info!(
            "TLS session completed: sent {} bytes and received {} bytes",
            stats.left_to_right, stats.right_to_left
        ),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => info!("TLS session cancelled"),
        Err(error) => warn!("TLS relay failed: {error}"),
    }
}

/// Runs the server until the supplied shutdown signal is set.
///
/// # Errors
///
/// Returns startup TLS material or listener binding errors.
pub async fn run_server_until(
    options: ServerOptions,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), ServerError> {
    let config = tls::server_config(
        &options.client_ca_cert,
        &options.server_cert,
        &options.server_key,
    )?;
    let listener = TcpListener::bind(options.listen_address)
        .await
        .map_err(ServerError::Bind)?;
    info!(
        "gpg-bridge server listening on {}",
        listener.local_addr().map_err(ServerError::Bind)?
    );
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let connector = AgentConnector::new(options.agent_extra_socket);
    let permits = Arc::new(Semaphore::new(options.max_connections.get()));
    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => { let _ = changed; break; }
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => match permits.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let session_acceptor = acceptor.clone();
                        let session_connector = connector.clone();
                        let session_shutdown = shutdown.clone();
                        sessions.spawn(async move { let _permit = permit; session(stream, session_acceptor, session_connector, session_shutdown).await; });
                    }
                    Err(_) => warn!("connection limit reached; rejecting TCP session"),
                },
                Err(error) => warn!("server accept failed: {error}"),
            },
            Some(result) = sessions.join_next(), if !sessions.is_empty() => if let Err(error) = result { error!("server session task failed: {error}"); },
        }
    }
    let _ = timeout(TASK_DRAIN_TIMEOUT, async {
        while sessions.join_next().await.is_some() {}
    })
    .await;
    sessions.abort_all();
    Ok(())
}

/// Runs the server until Ctrl-C (or SIGTERM on Unix).
///
/// # Errors
///
/// Returns a startup or platform signal error.
pub async fn run_server(options: ServerOptions) -> Result<(), ServerError> {
    let (sender, mut receiver) = watch::channel(false);
    tokio::spawn(async move {
        if let Err(error) = crate::shutdown::notify_shutdown(sender).await {
            error!("shutdown signal handler failed: {error}");
        }
    });
    run_server_until(options, &mut receiver).await
}
