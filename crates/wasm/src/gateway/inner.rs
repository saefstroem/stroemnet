use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use stroemnet_node::{Node, NodeConfig, SwapStatusUpdate};
use tokio::sync::mpsc;
#[derive(Default)]
pub struct EventCallbacks {
    pub quote: Vec<js_sys::Function>,
    pub swap_status: Vec<js_sys::Function>,
    pub peer_count: Vec<js_sys::Function>,
}
pub struct Inner {
    pub node: OnceLock<Arc<Node>>,
    pub callbacks: Arc<Mutex<EventCallbacks>>,
    pub config: Mutex<Option<NodeConfig>>,
    pub quote_rx: Mutex<Option<mpsc::UnboundedReceiver<stroemnet_node::CheckedQuote>>>,
    pub swap_status_rx: Mutex<Option<mpsc::UnboundedReceiver<SwapStatusUpdate>>>,
}
