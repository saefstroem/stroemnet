use std::rc::Rc;

use futures::lock::Mutex;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::Message;
use gloo_net::websocket::futures::WebSocket;

use crate::Result;
use crate::error::StroemnetP2pError;

#[derive(Clone)]
pub struct WsTransport {
    sink: Rc<Mutex<SplitSink<WebSocket, Message>>>,
    stream: Rc<Mutex<SplitStream<WebSocket>>>,
}

impl WsTransport {
    pub async fn dial(url: &str) -> Result<Self> {
        let ws =
            WebSocket::open(url).map_err(|e| StroemnetP2pError::Io(format!("dial {url}: {e}")))?;
        let (sink, stream) = ws.split();
        Ok(Self {
            sink: Rc::new(Mutex::new(sink)),
            stream: Rc::new(Mutex::new(stream)),
        })
    }

    pub async fn send(&self, bytes: Vec<u8>) -> Result<()> {
        self.sink
            .lock()
            .await
            .send(Message::Bytes(bytes))
            .await
            .map_err(|e| StroemnetP2pError::Io(format!("ws send: {e}")))
    }

    pub async fn recv(&self) -> Result<Vec<u8>> {
        loop {
            let msg = self
                .stream
                .lock()
                .await
                .next()
                .await
                .ok_or(StroemnetP2pError::TransportClosed)?
                .map_err(|e| StroemnetP2pError::Io(format!("ws recv: {e}")))?;
            match msg {
                Message::Bytes(b) => return Ok(b),
                Message::Text(_) => continue,
            }
        }
    }

    pub async fn close(&self) -> Result<()> {
        Ok(())
    }
}
