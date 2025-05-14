#![allow(clippy::missing_panics_doc)]

use std::{
    fmt,
    io::Result,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite};
use tokio::{
    io::{AsyncRead as _, AsyncWrite as _, ReadBuf, Stdin, Stdout},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

use crate::result::{ServerError, ServerResult};

/**
    Transport implementation for sockets and stdio.
*/
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub enum Transport {
    Socket(u16),
    #[default]
    Stdio,
}

impl Transport {
    /**
        Creates the reader and writer for the transport.

        # Errors

        - If the `Socket` transport is used, and the port is not valid.
        - If the `Socket` transport is used, and an I/O error occurs.
    */
    pub async fn into_read_write(self) -> ServerResult<(LspTransportRead, LspTransportWrite)> {
        if let Self::Socket(port) = self {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));

            let stream = TcpStream::connect(addr)
                .await
                .map_err(|_| ServerError::TcpConnect(port))?;

            let (stream_read, stream_write) = stream.into_split();

            Ok((
                LspTransportRead::Socket(stream_read),
                LspTransportWrite::Socket(stream_write),
            ))
        } else if let Self::Stdio = self {
            Ok((
                LspTransportRead::Stdio(tokio::io::stdin()),
                LspTransportWrite::Stdio(tokio::io::stdout()),
            ))
        } else {
            unreachable!()
        }
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio => write!(f, "Stdio"),
            Self::Socket(p) => write!(f, "Socket({p})"),
        }
    }
}

/**
    The read half of an LSP transport.
*/
#[derive(Debug)]
pub enum LspTransportRead {
    Socket(OwnedReadHalf),
    Stdio(Stdin),
}

impl AsyncRead for LspTransportRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let mut read_buf = ReadBuf::new(buf);

        let poll_result = match self.get_mut() {
            Self::Socket(s) => Pin::new(s).poll_read(cx, &mut read_buf),
            Self::Stdio(s) => Pin::new(s).poll_read(cx, &mut read_buf),
        };

        match poll_result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(read_buf.filled().len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
        }
    }
}

/**
    The write half of an LSP transport.
*/
#[derive(Debug)]
pub enum LspTransportWrite {
    Socket(OwnedWriteHalf),
    Stdio(Stdout),
}

impl AsyncWrite for LspTransportWrite {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        match self.get_mut() {
            Self::Socket(s) => Pin::new(s).poll_write(cx, buf),
            Self::Stdio(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match self.get_mut() {
            Self::Socket(s) => Pin::new(s).poll_flush(cx),
            Self::Stdio(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match self.get_mut() {
            Self::Socket(s) => Pin::new(s).poll_shutdown(cx),
            Self::Stdio(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
