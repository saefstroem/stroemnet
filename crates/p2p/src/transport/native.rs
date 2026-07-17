use std::sync::Arc;

use futures::SinkExt;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::Result;
use crate::error::StroemnetP2pError;

pub(crate) fn ws_config() -> WebSocketConfig {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(crate::wire::codec::MAX_MESSAGE_BYTES);
    config.max_frame_size = Some(crate::wire::codec::MAX_MESSAGE_BYTES);
    config
}

#[derive(Clone)]
pub struct WsTransport {
    sink: Arc<Mutex<SinkKind>>,
    stream: Arc<Mutex<StreamKind>>,
}

type OutboundStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type InboundStream = WebSocketStream<TcpStream>;

enum SinkKind {
    Outbound(SplitSink<OutboundStream, Message>),
    Inbound(SplitSink<InboundStream, Message>),
}

enum StreamKind {
    Outbound(SplitStream<OutboundStream>),
    Inbound(SplitStream<InboundStream>),
}

impl WsTransport {
    pub async fn dial(url: &str) -> Result<Self> {
        let (ws, _resp) =
            tokio_tungstenite::connect_async_with_config(url, Some(ws_config()), false)
                .await
                .map_err(|e| StroemnetP2pError::Io(format!("dial {url}: {e}")))?;
        let (sink, stream) = ws.split();
        Ok(Self {
            sink: Arc::new(Mutex::new(SinkKind::Outbound(sink))),
            stream: Arc::new(Mutex::new(StreamKind::Outbound(stream))),
        })
    }

    pub fn from_inbound(ws: WebSocketStream<TcpStream>) -> Self {
        let (sink, stream) = ws.split();
        Self {
            sink: Arc::new(Mutex::new(SinkKind::Inbound(sink))),
            stream: Arc::new(Mutex::new(StreamKind::Inbound(stream))),
        }
    }

    pub async fn send(&self, bytes: Vec<u8>) -> Result<()> {
        let mut guard = self.sink.lock().await;
        match &mut *guard {
            SinkKind::Outbound(s) => s
                .send(Message::Binary(bytes.into()))
                .await
                .map_err(|e| StroemnetP2pError::Io(format!("ws send: {e}"))),
            SinkKind::Inbound(s) => s
                .send(Message::Binary(bytes.into()))
                .await
                .map_err(|e| StroemnetP2pError::Io(format!("ws send: {e}"))),
        }
    }

    pub async fn recv(&self) -> Result<Vec<u8>> {
        loop {
            let mut guard = self.stream.lock().await;
            let next = match &mut *guard {
                StreamKind::Outbound(s) => s.next().await,
                StreamKind::Inbound(s) => s.next().await,
            };
            let msg = next
                .ok_or(StroemnetP2pError::TransportClosed)?
                .map_err(|e| StroemnetP2pError::Io(format!("ws recv: {e}")))?;
            if let Some(b) = ws_msg_to_bytes(msg)? {
                return Ok(b);
            }
        }
    }

    pub async fn close(&self) -> Result<()> {
        let mut guard = self.sink.lock().await;
        let _ = match &mut *guard {
            SinkKind::Outbound(s) => s.close().await,
            SinkKind::Inbound(s) => s.close().await,
        };
        Ok(())
    }
}

fn ws_msg_to_bytes(msg: Message) -> Result<Option<Vec<u8>>> {
    Ok(match msg {
        Message::Binary(b) => Some(b.to_vec()),
        Message::Close(_) => return Err(StroemnetP2pError::TransportClosed),
        Message::Ping(_) | Message::Pong(_) | Message::Text(_) | Message::Frame(_) => None,
    })
}

#[cfg(any(test, feature = "test-helpers"))]
#[allow(clippy::expect_used)]
pub async fn loopback_pair() -> (WsTransport, WsTransport) {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local_addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let ws = tokio_tungstenite::accept_async_with_config(stream, Some(ws_config()))
            .await
            .expect("ws accept");
        WsTransport::from_inbound(ws)
    });
    let client = WsTransport::dial(&format!("ws://{addr}"))
        .await
        .expect("dial");
    (client, server.await.expect("server task"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[tokio::test]
    async fn loopback_send_recv() {
        let (a, b) = loopback_pair().await;
        a.send(vec![1, 2, 3]).await.unwrap();
        assert_eq!(b.recv().await.unwrap(), vec![1, 2, 3]);
        b.send(vec![4, 5]).await.unwrap();
        assert_eq!(a.recv().await.unwrap(), vec![4, 5]);
    }

    #[tokio::test]
    async fn send_does_not_block_on_pending_recv() {
        let (a, b) = loopback_pair().await;
        let b_clone = b.clone();

        let recv_task = tokio::spawn(async move { b_clone.recv().await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        b.send(vec![9, 9, 9])
            .await
            .expect("send while recv pending");

        a.send(vec![1]).await.unwrap();
        let got = recv_task.await.unwrap().unwrap();
        assert_eq!(got, vec![1]);

        assert_eq!(a.recv().await.unwrap(), vec![9, 9, 9]);
    }
}
