use std::io;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelayStats {
    pub left_to_right: u64,
    pub right_to_left: u64,
}

/// Relays opaque bytes until EOF or cancellation.
///
/// # Errors
///
/// Returns I/O failures from either stream, or `Interrupted` after cancellation.
pub async fn relay<L, R>(
    left: &mut L,
    right: &mut R,
    shutdown: &mut watch::Receiver<bool>,
) -> io::Result<RelayStats>
where
    L: AsyncRead + AsyncWrite + Unpin,
    R: AsyncRead + AsyncWrite + Unpin,
{
    tokio::select! {
        result = tokio::io::copy_bidirectional(left, right) => {
            result.map(|(left_to_right, right_to_left)| RelayStats { left_to_right, right_to_left })
        }
        result = shutdown.changed() => {
            match result {
                Ok(()) | Err(_) => Err(io::Error::new(io::ErrorKind::Interrupted, "relay cancelled")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn relays_in_both_directions_until_eof() {
        let (mut app, mut bridge_left) = tokio::io::duplex(64);
        let (mut bridge_right, mut agent) = tokio::io::duplex(64);
        let (_shutdown_sender, mut shutdown) = watch::channel(false);
        let relay_task =
            tokio::spawn(
                async move { relay(&mut bridge_left, &mut bridge_right, &mut shutdown).await },
            );

        app.write_all(b"request").await.expect("write request");
        let mut request = [0; 7];
        agent.read_exact(&mut request).await.expect("read request");
        assert_eq!(&request, b"request");

        agent.write_all(b"reply").await.expect("write reply");
        let mut reply = [0; 5];
        app.read_exact(&mut reply).await.expect("read reply");
        assert_eq!(&reply, b"reply");
        drop(app);
        drop(agent);

        assert_eq!(
            relay_task
                .await
                .expect("relay task")
                .expect("relay succeeds"),
            RelayStats {
                left_to_right: 7,
                right_to_left: 5
            }
        );
    }

    #[tokio::test]
    async fn stops_on_cancellation() {
        let (_app, mut bridge_left) = tokio::io::duplex(64);
        let (mut bridge_right, _agent) = tokio::io::duplex(64);
        let (shutdown_sender, mut shutdown) = watch::channel(false);
        let relay_task =
            tokio::spawn(
                async move { relay(&mut bridge_left, &mut bridge_right, &mut shutdown).await },
            );
        shutdown_sender.send(true).expect("send cancellation");
        let error = relay_task
            .await
            .expect("relay task")
            .expect_err("is cancelled");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }
}
