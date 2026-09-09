use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::time::{Instant, sleep, timeout};
use zeroize::Zeroizing;

pub const MAX_REDIRECT_FILE_SIZE: u64 = 128;
pub const AGENT_STARTUP_WINDOW: Duration = Duration::from_secs(15);
const AGENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const AGENT_RETRY_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("failed to inspect Gpg4win redirect file {path}: {source}")]
    MetadataIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Gpg4win redirect file {path} is too large")]
    MetadataTooLarge { path: PathBuf },
    #[error("Gpg4win redirect file is shorter than its 16-byte nonce")]
    MetadataTooShort,
    #[error("Gpg4win redirect file port prefix is not valid UTF-8")]
    InvalidPortEncoding,
    #[error("Gpg4win redirect file port prefix is empty")]
    EmptyPort,
    #[error("Gpg4win redirect file contains an invalid TCP port")]
    InvalidPort,
    #[error("Gpg4win redirect file contains TCP port zero")]
    ZeroPort,
    #[error("failed to connect to the local Gpg4win agent: {source}")]
    Connect {
        #[source]
        source: io::Error,
    },
    #[error("timed out connecting to the local Gpg4win agent")]
    ConnectTimeout,
    #[cfg(unix)]
    #[error("failed to connect to the local Unix GnuPG agent socket {path}: {source}")]
    UnixConnect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to start gpg-agent: {source}")]
    Start {
        #[source]
        source: io::Error,
    },
    #[error("gpg-connect-agent did not complete within the startup timeout")]
    StartTimeout,
    #[error("gpg-connect-agent exited unsuccessfully")]
    StartFailed,
    #[error("local Gpg4win agent did not become ready within the startup window: {last_error}")]
    StartupWindowExpired { last_error: String },
}

struct AgentMetadata {
    port: u16,
    nonce: Zeroizing<[u8; 16]>,
}

fn parse_metadata(contents: &[u8]) -> Result<AgentMetadata, AgentError> {
    if contents.len() < 16 {
        return Err(AgentError::MetadataTooShort);
    }
    let port_text = std::str::from_utf8(&contents[..contents.len() - 16])
        .map_err(|_| AgentError::InvalidPortEncoding)?
        .trim();
    if port_text.is_empty() {
        return Err(AgentError::EmptyPort);
    }
    let port = port_text
        .parse::<u16>()
        .map_err(|_| AgentError::InvalidPort)?;
    if port == 0 {
        return Err(AgentError::ZeroPort);
    }
    let mut nonce = [0_u8; 16];
    nonce.copy_from_slice(&contents[contents.len() - 16..]);
    Ok(AgentMetadata {
        port,
        nonce: Zeroizing::new(nonce),
    })
}

async fn load_metadata(path: &Path) -> Result<AgentMetadata, AgentError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|source| AgentError::MetadataIo {
            path: path.to_owned(),
            source,
        })?;
    if metadata.len() > MAX_REDIRECT_FILE_SIZE {
        return Err(AgentError::MetadataTooLarge {
            path: path.to_owned(),
        });
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|source| AgentError::MetadataIo {
            path: path.to_owned(),
            source,
        })?;
    let capacity = usize::try_from(metadata.len()).unwrap_or(0);
    let mut contents = Vec::with_capacity(capacity);
    file.take(MAX_REDIRECT_FILE_SIZE + 1)
        .read_to_end(&mut contents)
        .await
        .map_err(|source| AgentError::MetadataIo {
            path: path.to_owned(),
            source,
        })?;
    if contents.len() as u64 > MAX_REDIRECT_FILE_SIZE {
        return Err(AgentError::MetadataTooLarge {
            path: path.to_owned(),
        });
    }
    parse_metadata(&contents)
}

async fn start_agent() -> Result<(), AgentError> {
    let mut command = Command::new("gpg-connect-agent");
    command.arg("/bye").kill_on_drop(true);
    let output = timeout(AGENT_CONNECT_TIMEOUT, command.output())
        .await
        .map_err(|_| AgentError::StartTimeout)?
        .map_err(|source| AgentError::Start { source })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AgentError::StartFailed)
    }
}

/// A connected local agent stream.
pub enum AgentStream {
    /// A Gpg4win loopback connection after its nonce preface.
    Gpg4win(TcpStream),
    /// A direct Unix `GnuPG` extra-socket connection.
    #[cfg(unix)]
    Unix(UnixStream),
}

/// Connects a newly accepted bridge session to a configured local agent backend.
#[derive(Clone, Debug)]
pub enum AgentConnector {
    /// The Gpg4win redirect-file backend.
    Gpg4winRedirect { redirect_file: PathBuf },
    /// A direct Unix `GnuPG` extra socket backend.
    #[cfg(unix)]
    UnixSocket { socket: PathBuf },
}

impl AgentConnector {
    #[must_use]
    pub fn gpg4win(redirect_file: PathBuf) -> Self {
        Self::Gpg4winRedirect { redirect_file }
    }

    /// Connects after refreshing the redirect metadata for this session.
    ///
    /// # Errors
    ///
    /// Returns metadata, startup, connection, or nonce-write failures.
    pub async fn connect(&self) -> Result<AgentStream, AgentError> {
        match self {
            Self::Gpg4winRedirect { redirect_file } => Self::connect_gpg4win(redirect_file).await,
            #[cfg(unix)]
            Self::UnixSocket { socket } => UnixStream::connect(socket)
                .await
                .map(AgentStream::Unix)
                .map_err(|source| AgentError::UnixConnect {
                    path: socket.clone(),
                    source,
                }),
        }
    }

    async fn connect_gpg4win(redirect_file: &Path) -> Result<AgentStream, AgentError> {
        let deadline = Instant::now() + AGENT_STARTUP_WINDOW;
        let mut last_error;
        loop {
            match load_metadata(redirect_file).await {
                Ok(metadata) => match timeout(
                    AGENT_CONNECT_TIMEOUT,
                    TcpStream::connect(("127.0.0.1", metadata.port)),
                )
                .await
                {
                    Ok(Ok(mut stream)) => {
                        stream
                            .write_all(metadata.nonce.as_ref())
                            .await
                            .map_err(|source| AgentError::Connect { source })?;
                        stream
                            .flush()
                            .await
                            .map_err(|source| AgentError::Connect { source })?;
                        return Ok(AgentStream::Gpg4win(stream));
                    }
                    Ok(Err(source)) => last_error = AgentError::Connect { source },
                    Err(_) => last_error = AgentError::ConnectTimeout,
                },
                Err(error) => last_error = error,
            }
            if Instant::now() >= deadline {
                break;
            }
            let _ = start_agent().await;
            sleep(AGENT_RETRY_DELAY).await;
        }
        Err(AgentError::StartupWindowExpired {
            last_error: last_error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn write_redirect(path: &Path, port: u16, nonce: [u8; 16]) {
        let mut contents = port.to_string().into_bytes();
        contents.extend(nonce);
        tokio::fs::write(path, contents)
            .await
            .expect("write redirect file");
    }
    #[test]
    fn parses_port_and_exact_nonce() {
        let mut contents = b"4321\n".to_vec();
        contents.extend(0_u8..16);
        let metadata = parse_metadata(&contents).expect("metadata parses");
        assert_eq!(metadata.port, 4321);
        assert_eq!(metadata.nonce.as_ref(), &(0_u8..16).collect::<Vec<_>>()[..]);
    }
    #[test]
    fn malformed_metadata_never_panics() {
        assert!(matches!(
            parse_metadata(b""),
            Err(AgentError::MetadataTooShort)
        ));
        assert!(matches!(
            parse_metadata(b"123"),
            Err(AgentError::MetadataTooShort)
        ));
        let mut invalid_utf8 = vec![0xff];
        invalid_utf8.extend([0; 16]);
        assert!(matches!(
            parse_metadata(&invalid_utf8),
            Err(AgentError::InvalidPortEncoding)
        ));
        let mut zero = b"0".to_vec();
        zero.extend([0; 16]);
        assert!(matches!(parse_metadata(&zero), Err(AgentError::ZeroPort)));
    }

    #[tokio::test]
    async fn connects_to_fake_agent_and_sends_nonce_first() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let redirect = directory.path().join("S.gpg-agent.extra");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let port = listener.local_addr().expect("address").port();
        let nonce = [42; 16];
        write_redirect(&redirect, port, nonce).await;
        let agent = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("agent connection");
            let mut received = [0; 16];
            stream.read_exact(&mut received).await.expect("nonce");
            received
        });
        let _stream = AgentConnector::gpg4win(redirect)
            .connect()
            .await
            .expect("connect agent");
        assert_eq!(agent.await.expect("agent task"), nonce);
    }

    #[tokio::test]
    async fn refreshes_redirect_metadata_for_every_session() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let redirect = directory.path().join("S.gpg-agent.extra");
        let first = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("first listener");
        write_redirect(
            &redirect,
            first.local_addr().expect("address").port(),
            [1; 16],
        )
        .await;
        let first_task = tokio::spawn(async move {
            let (mut stream, _) = first.accept().await.expect("first connection");
            let mut nonce = [0; 16];
            stream.read_exact(&mut nonce).await.expect("first nonce");
            nonce
        });
        let connector = AgentConnector::gpg4win(redirect.clone());
        drop(connector.connect().await.expect("first session"));
        assert_eq!(first_task.await.expect("first task"), [1; 16]);

        let second = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("second listener");
        write_redirect(
            &redirect,
            second.local_addr().expect("address").port(),
            [2; 16],
        )
        .await;
        let second_task = tokio::spawn(async move {
            let (mut stream, _) = second.accept().await.expect("second connection");
            let mut nonce = [0; 16];
            stream.read_exact(&mut nonce).await.expect("second nonce");
            nonce
        });
        drop(connector.connect().await.expect("second session"));
        assert_eq!(second_task.await.expect("second task"), [2; 16]);
    }
}
