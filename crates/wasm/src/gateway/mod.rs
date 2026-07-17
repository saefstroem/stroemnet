mod config;
mod connect;
mod event_listeners;
mod inner;
mod swaps;

use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

use tokio::sync::mpsc;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use crate::gateway::config::GatewayConfig;
use crate::gateway::inner::{EventCallbacks, Inner};
use stroemnet_node::Node;

#[wasm_bindgen]
pub struct StroemGateway {
    inner: Arc<Inner>,
}

#[wasm_bindgen]
impl StroemGateway {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<StroemGateway, JsError> {
        let cfg: GatewayConfig = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsError::new(&format!("config: {e}")))?;
        let (swap_status_tx, swap_status_rx) = mpsc::unbounded_channel();
        let node_cfg = cfg.into_node_config(swap_status_tx)?;
        Ok(StroemGateway {
            inner: Arc::new(Inner {
                node: OnceLock::new(),
                callbacks: Arc::new(Mutex::new(EventCallbacks::default())),
                config: Mutex::new(Some(node_cfg)),
                quote_rx: Mutex::new(None),
                swap_status_rx: Mutex::new(Some(swap_status_rx)),
            }),
        })
    }

    fn require_node(&self) -> Result<Arc<Node>, JsError> {
        self.inner
            .node
            .get()
            .cloned()
            .ok_or_else(|| JsError::new("gateway not connected — call connect() first"))
    }

    #[wasm_bindgen(js_name = peerCount)]
    pub fn peer_count(&self) -> Result<usize, JsError> {
        Ok(self.require_node()?.peer_count())
    }
}
