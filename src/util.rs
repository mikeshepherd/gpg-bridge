use std::pin::Pin;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

// mod monad;
// pub use monad::*;

pub type PinAsyncRead<'a> = Pin<Box<dyn AsyncRead + Send + 'a>>;
pub type PinAsyncWrite<'a> = Pin<Box<dyn AsyncWrite + Send + 'a>>;

pub trait SplitStream {
    fn split_rw(&mut self) -> (PinAsyncRead<'_>, PinAsyncWrite<'_>);
}

impl SplitStream for TcpStream {
    #[inline]
    fn split_rw(&mut self) -> (PinAsyncRead<'_>, PinAsyncWrite<'_>) {
        let (read_half, write_half) = TcpStream::split(self);
        (Box::pin(read_half), Box::pin(write_half))
    }
}
