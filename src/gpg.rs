use crate::AgentMeta;
use crate::util::SplitStream;
use log::{debug, error, trace};
use std::path::Path;
use std::pin::Pin;
use std::ptr;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use tokio::{io, process::Command};

pub async fn ping_gpg_agent() -> io::Result<()> {
    let output = Command::new("gpg-connect-agent")
        .arg("/bye")
        .output()
        .await?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "failed to start gpg-agent: {:?}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

// On connect we get a port number and a nonce
// We connect to the port on localhost and the first thing we must write to it is the nonce
async fn load_port_nonce(local_gpg_socket_path: &str) -> io::Result<AgentMeta> {
    if !Path::new(&local_gpg_socket_path).exists() {
        ping_gpg_agent().await?;
    }
    let mut f = File::open(&local_gpg_socket_path.replace('\\', "/")).await?;
    let mut buffer = Vec::with_capacity(50);
    f.read_to_end(&mut buffer).await?;
    let (left, right) = buffer.split_at(buffer.len() - 16);
    let to_port: u16 = str::from_utf8(left).unwrap().trim().parse().unwrap();
    let mut nonce = [0; 16];
    unsafe {
        ptr::copy_nonoverlapping(right.as_ptr(), nonce.as_mut_ptr(), 16);
    }
    Ok(AgentMeta {
        port: to_port,
        nonce,
    })
}

async fn copy<'a>(
    from: &mut Pin<Box<dyn AsyncRead + Send + 'a>>,
    to: &mut Pin<Box<dyn AsyncWrite + Send + 'a>>,
) -> io::Result<u64> {
    let mut buf = vec![0; 4096];
    let mut total = 0;
    loop {
        let cnt = from.read(&mut buf).await?;
        if cnt == 0 {
            to.shutdown().await?;
            unsafe {
                ptr::write_bytes(buf.as_mut_ptr(), 0, 4096);
            }
            return Ok(total);
        }
        total += cnt as u64;
        to.write_all(&buf[..cnt]).await?;
    }
}

async fn delegate(mut from: impl SplitStream, to_port: u16, nonce: [u8; 16]) -> io::Result<()> {
    // connect to the local tcp socket that is emulating a domain socket
    let mut delegate = match TcpStream::connect(("127.0.0.1", to_port)).await {
        Ok(s) => s,
        Err(e) => {
            // It's possible that gpg-client was killed and leave stale meta untouched.
            // Reping agent to make it startup.
            let _ = ping_gpg_agent().await;
            return Err(e);
        }
    };
    // write the nonce to prove we are allowed to talk
    trace!("--> {:?}", String::from_utf8_lossy(&nonce));
    delegate.write_all(&nonce).await?;
    delegate.flush().await?;

    let (mut source_read, mut source_write) = from.split_rw();
    let (mut target_read, mut target_write) = delegate.split_rw();
    let s2t = copy(&mut source_read, &mut target_write);
    let t2s = copy(&mut target_read, &mut source_write);
    let (received, replied) = tokio::join!(s2t, t2s);
    debug!(
        "connection finished, received {}, replied {}",
        received?, replied?
    );
    Ok(())
}

async fn bridge_to_stream(listener: TcpListener, local_gpg_socket_path: &str) -> io::Result<()>
// where
    // L::Connection: SplitStream + Send + 'static,
{
    let meta = Arc::new(Mutex::new(None::<AgentMeta>));
    loop {
        let (conn, _) = TcpListener::accept(&listener).await?;

        let meta = meta.clone();
        let AgentMeta { port, nonce } = {
            let mut m = meta.lock().await;
            if m.is_none() {
                m.replace(load_port_nonce(local_gpg_socket_path).await?);
            }
            m.unwrap()
        };

        tokio::spawn(async move {
            if let Err(e) = delegate(conn, port, nonce).await {
                error!("failed to delegate stream: {e:?}");
                meta.lock().await.take();
            }
        });
    }
}

/// A bridge that forwards all requests from certain stream to gpg-agent on Windows.
///
/// `local_gpg_socket_path` should point to the path of gnupg UDS.
/// `listen_address` is a TCP address that will be forwarded over ssh
pub async fn gpg_bridge(listen_address: String, local_gpg_socket_path: String) -> io::Result<()> {
    // Attempt to setup gpg-agent if it's not up yet.
    let _ = ping_gpg_agent().await;
    // Create listener on tcp socket
    let listener = TcpListener::bind(&listen_address).await?;
    bridge_to_stream(listener, local_gpg_socket_path.as_str()).await?;
    Ok(())
}
