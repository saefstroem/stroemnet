use crate::StroemGateway;
use serde::Serialize;
use std::sync::Arc;
use stroemnet_node::Node;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use crate::gateway::inner::{EventCallbacks, Inner};
const PEER_COUNT_POLL_MS: u64 = 500;

fn spawn_drain<T: Serialize + 'static>(
    rx: Option<tokio::sync::mpsc::UnboundedReceiver<T>>,
    callbacks: Arc<parking_lot::Mutex<EventCallbacks>>,
    select: impl Fn(&EventCallbacks) -> Vec<js_sys::Function> + 'static,
    label: &'static str,
) {
    let Some(mut rx) = rx else {
        return;
    };
    stroemnet_protocol::spawn(async move {
        while let Some(item) = rx.recv().await {
            let js_value = match item.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("serialize {label}: {e}");
                    continue;
                }
            };
            let listeners: Vec<js_sys::Function> = select(&callbacks.lock());
            for f in listeners {
                let _ = f.call1(&JsValue::NULL, &js_value);
            }
        }
    });
}

fn spawn_peer_count_poll(inner: Arc<Inner>, node: Arc<Node>) {
    let callbacks = inner.callbacks.clone();
    stroemnet_protocol::spawn(async move {
        let mut last: Option<usize> = None;
        loop {
            let current = node.peer_count();
            if last != Some(current) {
                last = Some(current);
                let js_value = JsValue::from_f64(current as f64);
                let listeners: Vec<js_sys::Function> = callbacks.lock().peer_count.clone();
                for f in listeners {
                    let _ = f.call1(&JsValue::NULL, &js_value);
                }
            }
            stroemnet_protocol::sleep_ms(PEER_COUNT_POLL_MS).await;
        }
    });
}
#[wasm_bindgen]
impl StroemGateway {
    #[wasm_bindgen]
    pub async fn connect(&self) -> Result<(), JsError> {
        if self.inner.node.get().is_some() {
            return Err(JsError::new("gateway already connected"));
        }
        let cfg = self
            .inner
            .config
            .lock()
            .take()
            .ok_or_else(|| JsError::new("gateway already consumed"))?;

        let (node, quote_rx) = Node::start(cfg, None, None)
            .await
            .map_err(|e| JsError::new(&format!("Node::start: {e}")))?;
        let node = Arc::new(node);
        self.inner
            .node
            .set(node.clone())
            .map_err(|_| JsError::new("node already set"))?;
        *self.inner.quote_rx.lock() = Some(quote_rx);

        spawn_drain(
            self.inner.quote_rx.lock().take(),
            self.inner.callbacks.clone(),
            |c| c.quote.clone(),
            "CheckedQuote",
        );
        spawn_drain(
            self.inner.swap_status_rx.lock().take(),
            self.inner.callbacks.clone(),
            |c| c.swap_status.clone(),
            "SwapStatusUpdate",
        );
        spawn_peer_count_poll(self.inner.clone(), node);
        Ok(())
    }
}
