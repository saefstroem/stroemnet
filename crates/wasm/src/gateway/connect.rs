use crate::StroemGateway;
use serde::Serialize;
use std::sync::Arc;
use stroemnet_node::Node;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use crate::gateway::inner::Inner;
const PEER_COUNT_POLL_MS: u64 = 500;

fn spawn_quote_drain(inner: Arc<Inner>) {
    let Some(mut rx) = inner.quote_rx.lock().unwrap().take() else {
        return;
    };
    let callbacks = inner.callbacks.clone();
    stroemnet_protocol::spawn(async move {
        while let Some(row) = rx.recv().await {
            let js_value = match row.serialize(&serde_wasm_bindgen::Serializer::json_compatible()) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("serialize CheckedQuote: {e}");
                    continue;
                }
            };
            let listeners: Vec<js_sys::Function> = callbacks.lock().unwrap().quote.clone();
            for f in listeners {
                let _ = f.call1(&JsValue::NULL, &js_value);
            }
        }
    });
}

fn spawn_swap_status_drain(inner: Arc<Inner>) {
    let Some(mut rx) = inner.swap_status_rx.lock().unwrap().take() else {
        return;
    };
    let callbacks = inner.callbacks.clone();
    stroemnet_protocol::spawn(async move {
        while let Some(update) = rx.recv().await {
            let js_value =
                match update.serialize(&serde_wasm_bindgen::Serializer::json_compatible()) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("serialize SwapStatusUpdate: {e}");
                        continue;
                    }
                };
            let listeners: Vec<js_sys::Function> = callbacks.lock().unwrap().swap_status.clone();
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
                let listeners: Vec<js_sys::Function> = callbacks.lock().unwrap().peer_count.clone();
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
    /// Connects the gateway to the stroemnet network using the provided configuration.
    ///
    /// Errors with:
    /// - `gateway already connected` if the gateway is already connected.
    /// - `gateway already consumed` if the gateway was connected and then disconnected (not currently
    /// - `Node::start: {e}` if the underlying node failed to start for some reason (e.g. invalid config).
    /// - `node already set` if the node was somehow set by another concurrent call to connect (should be impossible).
    pub async fn connect(&self) -> Result<(), JsError> {
        if self.inner.node.get().is_some() {
            return Err(JsError::new("gateway already connected"));
        }
        let cfg = self
            .inner
            .config
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| JsError::new("gateway already consumed"))?;

        let (node, quote_rx) = Node::start(cfg, None)
            .await
            .map_err(|e| JsError::new(&format!("Node::start: {e}")))?;
        let node = Arc::new(node);
        self.inner
            .node
            .set(node.clone())
            .map_err(|_| JsError::new("node already set"))?;
        *self.inner.quote_rx.lock().unwrap() = Some(quote_rx);

        spawn_quote_drain(self.inner.clone());
        spawn_swap_status_drain(self.inner.clone());
        spawn_peer_count_poll(self.inner.clone(), node);
        Ok(())
    }
}
