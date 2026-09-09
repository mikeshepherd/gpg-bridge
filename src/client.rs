#![cfg(unix)]

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use log::{info, warn};
use socket2::{SockRef, TcpKeepalive};
use tokio::net::{TcpStream, UnixStream};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior, interval, timeout};
use tokio_rustls::TlsConnector;

use crate::unix_socket::OwnedUnixListener;
use crate::{
    ClientOptions, relay,
    retry::{PRE_RELAY_RECONNECT_WINDOW, ReconnectBackoff},
    tls,
};

const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const SOCKET_MONITOR_INTERVAL: Duration = Duration::from_secs(1);
const TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

fn configure_tcp(stream: &TcpStream) {
    if let Err(error) = stream.set_nodelay(true) {
        warn!("could not enable TCP_NODELAY: {error}");
    }
    if let Err(error) = SockRef::from(stream)
        .set_tcp_keepalive(&TcpKeepalive::new().with_time(Duration::from_secs(30)))
    {
        warn!("could not configure TCP keepalive: {error}");
    }
}

fn transient_tls_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::TimedOut
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::BrokenPipe
    )
}

async fn establish(
    connector: &TlsConnector,
    name: tokio_rustls::rustls::pki_types::ServerName<'static>,
    address: &str,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, bool> {
    let stream = timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .map_err(|_| true)?
        .map_err(|_| true)?;
    configure_tcp(&stream);
    let stream = timeout(TLS_HANDSHAKE_TIMEOUT, connector.connect(name, stream))
        .await
        .map_err(|_| true)?
        .map_err(|error| transient_tls_error(&error))?;
    tls::validate_alpn(stream.get_ref().1.alpn_protocol()).map_err(|_| false)?;
    Ok(stream)
}

async fn relay_session(
    session_id: u64,
    mut local: UnixStream,
    connector: TlsConnector,
    name: tokio_rustls::rustls::pki_types::ServerName<'static>,
    address: String,
    mut shutdown: watch::Receiver<bool>,
) {
    let started = Instant::now();
    let mut backoff = ReconnectBackoff::new();
    loop {
        if started.elapsed() >= PRE_RELAY_RECONNECT_WINDOW {
            warn!("client session {session_id} exhausted its pre-relay reconnect window");
            return;
        }
        match establish(&connector, name.clone(), &address).await {
            Ok(mut remote) => match relay::relay(&mut local, &mut remote, &mut shutdown).await {
                Ok(stats) => info!(
                    "client session {session_id} completed: sent {} bytes and received {} bytes",
                    stats.left_to_right, stats.right_to_left
                ),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    info!("client session {session_id} cancelled");
                }
                Err(error) => warn!("client session {session_id} relay failed closed: {error}"),
            },
            Err(true) => {
                let delay = backoff.next_delay();
                if started.elapsed().saturating_add(delay) >= PRE_RELAY_RECONNECT_WINDOW {
                    warn!("client session {session_id} exhausted its pre-relay reconnect window");
                    return;
                }
                tokio::select! {
                    () = tokio::time::sleep(delay) => {},
                    changed = shutdown.changed() => { let _ = changed; return; },
                }
                continue;
            }
            Err(false) => {
                warn!("client session {session_id} rejected by TLS identity or ALPN validation");
            }
        }
        return;
    }
}

/// Runs the Unix socket client until the supplied shutdown signal is set.
///
/// # Errors
///
/// Returns startup TLS, socket ownership, or socket-monitoring errors.
pub async fn run_client_until(
    options: ClientOptions,
    shutdown: &mut watch::Receiver<bool>,
) -> io::Result<()> {
    let config = tls::client_config(
        &options.server_ca_cert,
        &options.client_cert,
        &options.client_key,
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(options.server_name.clone())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?
        .to_owned();
    let mut listener = OwnedUnixListener::bind(options.listen_socket).await?;
    let permits = Arc::new(Semaphore::new(options.max_connections.get()));
    let connector = TlsConnector::from(Arc::new(config));
    let mut sessions = JoinSet::new();
    let session_ids = AtomicU64::new(1);
    let mut monitor = interval(SOCKET_MONITOR_INTERVAL);
    monitor.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            changed = shutdown.changed() => { let _ = changed; break; }
            _ = monitor.tick() => listener.verify_or_rebind()?,
            accepted = listener.accept() => match accepted {
                Ok((local, _)) => match permits.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let session_id = session_ids.fetch_add(1, Ordering::Relaxed);
                        let session_connector = connector.clone(); let session_name = name.clone();
                        let address = options.server_address.clone(); let session_shutdown = shutdown.clone();
                        sessions.spawn(async move { let _permit = permit; relay_session(session_id, local, session_connector, session_name, address, session_shutdown).await; });
                    }
                    Err(_) => warn!("client connection limit reached; rejecting Unix session"),
                },
                Err(error) => warn!("Unix socket accept failed: {error}"),
            },
            Some(result) = sessions.join_next(), if !sessions.is_empty() => if let Err(error) = result { warn!("client session task failed: {error}"); },
        }
    }
    let _ = timeout(TASK_DRAIN_TIMEOUT, async {
        while sessions.join_next().await.is_some() {}
    })
    .await;
    sessions.abort_all();
    Ok(())
}
