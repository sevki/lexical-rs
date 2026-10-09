//! Carry jetstream frames over iroh QUIC streams.
//!
//! This is the same glue as the `jetstream_iroh` crate (client transport over one
//! bidirectional stream, server loop over accepted streams), written against current iroh.

use futures::{Sink, SinkExt, Stream, StreamExt};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use jetstream_rpc::client::ClientCodec;
use jetstream_rpc::context::Context;
use jetstream_rpc::server::{Server, ServerCodec};
use jetstream_rpc::{Error, Frame, IntoError, Protocol};
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, FramedWrite};

/// Client side: a [`ClientTransport`](jetstream_rpc::client::ClientTransport) over one stream.
pub struct IrohTransport<P: Protocol> {
    send: FramedWrite<SendStream, ClientCodec<P>>,
    recv: FramedRead<RecvStream, ClientCodec<P>>,
}

impl<P: Protocol> From<(SendStream, RecvStream)> for IrohTransport<P> {
    fn from((send, recv): (SendStream, RecvStream)) -> Self {
        Self {
            send: FramedWrite::new(send, ClientCodec::default()),
            recv: FramedRead::new(recv, ClientCodec::default()),
        }
    }
}

impl<P: Protocol> Sink<Frame<P::Request>> for IrohTransport<P>
where
    Self: Unpin,
{
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Result<(), Error>> {
        self.get_mut().send.poll_ready_unpin(cx)
    }
    fn start_send(self: Pin<&mut Self>, item: Frame<P::Request>) -> Result<(), Error> {
        self.get_mut().send.start_send_unpin(item)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Result<(), Error>> {
        self.get_mut().send.poll_flush_unpin(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Result<(), Error>> {
        self.get_mut().send.poll_close_unpin(cx)
    }
}

impl<P: Protocol> Stream for IrohTransport<P>
where
    Self: Unpin,
{
    type Item = Result<Frame<P::Response>, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().recv.poll_next_unpin(cx)
    }
}

/// Server side: serve every bidirectional stream of a connection with `P`.
#[derive(Debug)]
pub struct IrohServer<P: Protocol + Server + Debug + Clone + 'static> {
    inner: P,
}

impl<P: Protocol + Server + Debug + Clone + 'static> IrohServer<P> {
    pub fn new(inner: P) -> Self {
        Self { inner }
    }
}

impl<P: Protocol + Server + Debug + Clone + 'static> ProtocolHandler for IrohServer<P> {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        while let Ok((send, recv)) = connection.accept_bi().await {
            let handler = self.inner.clone();
            tokio::spawn(async move {
                let mut reader = FramedRead::new(recv, ServerCodec::<P>::new());
                let mut writer = FramedWrite::new(send, ServerCodec::<P>::new());
                let (responses, mut outgoing) = mpsc::channel::<Frame<P::Response>>(256);
                let write = tokio::spawn(async move {
                    while let Some(response) = outgoing.recv().await {
                        if writer.send(response).await.is_err() {
                            break;
                        }
                    }
                });
                // Requests run concurrently; responses are matched to them by tag.
                while let Some(request) = reader.next().await {
                    let Ok(request) = request else { continue };
                    let mut handler = handler.clone();
                    let responses = responses.clone();
                    tokio::spawn(async move {
                        match handler.rpc(Context::default(), request).await {
                            Ok(response) => {
                                let _ = responses.send(response).await;
                            }
                            Err(err) => tracing::debug!("request failed: {}", err.into_error()),
                        }
                    });
                }
                drop(responses);
                let _ = write.await;
            });
        }
        Ok(())
    }
}
