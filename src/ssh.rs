use crate::NamedPipePath;
use crate::protocol::{Decode, Decoder, Request, Response};
use std::fmt::Debug;
#[cfg(unix)]
use std::io::Error;
use tokio::sync::mpsc::*;

use tokio::io::{self, BufReader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::bytes::BytesMut;

use log::{debug, error, trace};

#[cfg(windows)]
use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode, tokio::*};
use std::path::Path;

#[cfg(windows)]
async fn copy<'a, Output>(
    tag: &str,
    from: &mut &PipeStream<pipe_mode::Bytes, pipe_mode::Bytes>,
    to: &mut &PipeStream<pipe_mode::Bytes, pipe_mode::Bytes>,
    log_sender: UnboundedSender<String>,
) -> io::Result<usize>
where
    Output: Decode<Item = Output> + Debug,
{
    let mut total = 0;
    let mut buf_reader = BufReader::new(from);
    loop {
        let raw_size = match buf_reader.read_u32().await {
            Ok(number) => number,
            Err(_) => return Ok(total),
        };
        let size = (raw_size - 1) as usize;
        let id = buf_reader.read_u8().await?;
        let mut buf = vec![0u8; size];
        buf_reader.read_exact(&mut buf).await?;
        let mut full_bytes = BytesMut::with_capacity(size);
        full_bytes.extend(
            [
                raw_size.to_be_bytes().as_slice(),
                [id].as_slice(),
                &buf.as_slice(),
            ]
            .concat(),
        );
        total += size;
        let custom = Decoder::<Output>::decode(&mut full_bytes.clone());
        let _ = log_sender.send(format!(
            "{} {:?} {:?}\n{} Decoded: {:?}\n",
            tag, id, raw_size, tag, custom
        ));
        to.write_all(&full_bytes[..]).await?;
    }
}

#[cfg(windows)]
async fn delegate(
    from: DuplexPipeStream<pipe_mode::Bytes>,
    ssh_socket: NamedPipePath,
    log_sender: UnboundedSender<String>,
) -> io::Result<()> {
    let delegate = DuplexPipeStream::<pipe_mode::Bytes>::connect_by_path(ssh_socket.0).await?;

    let [mut source_read, mut source_write] = [&from; 2];
    let [mut target_read, mut target_write] = [&delegate; 2];

    let s2t = copy::<Request>(
        "Server-->Agent",
        &mut source_read,
        &mut target_write,
        log_sender.clone(),
    );
    let t2s = copy::<Response>(
        "Agent-->Server",
        &mut target_read,
        &mut source_write,
        log_sender.clone(),
    );
    let (received, replied) = tokio::join!(s2t, t2s);
    debug!(
        "connection finished, received {}, replied {}",
        received?, replied?
    );
    Ok(())
}

#[cfg(windows)]
async fn bridge_to_stream(
    listener: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    local_ssh_socket_path: NamedPipePath,
    log_sender: UnboundedSender<String>,
) -> io::Result<()> {
    loop {
        let conn = listener.accept().await?;
        let socket = local_ssh_socket_path.clone();
        let sender = log_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = delegate(conn, socket, sender).await {
                error!("failed to delegate stream: {e:?}");
            }
        });
    }
}

/// A bridge that forwards all requests from certain stream to gpg-agent on Windows.
///
/// `local_gpg_socket_path` should point to the path of gnupg UDS.
/// `listen_address` is a TCP address that will be forwarded over ssh
pub async fn ssh_bridge(
    ssh_socket: NamedPipePath,
    listening_socket: NamedPipePath,
) -> io::Result<()> {
    // Attempt to setup gpg-agent if it's not up yet.
    // let _ = ping_gpg_agent().await;
    // Create listener on tcp socket
    #[cfg(unix)]
    return Err(Error::other("error"));

    let (sender, mut receiver) = unbounded_channel::<String>();

    tokio::spawn(async move {
        loop {
            let message = receiver.recv().await.unwrap();
            trace!("{}", message);
        }
    });

    #[cfg(windows)]
    let listener = PipeListenerOptions::new()
        .path(Path::new(listening_socket.0.as_str()))
        .create_tokio_duplex::<pipe_mode::Bytes>()?;
    #[cfg(windows)]
    bridge_to_stream(listener, ssh_socket, sender).await?;
    Ok(())
}
