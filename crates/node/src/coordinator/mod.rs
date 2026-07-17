#[cfg(target_arch = "wasm32")]
mod broadcast;
mod dispatch;
mod proposal;
mod request;
mod response;

use std::sync::Arc;

use futures::StreamExt;
use stroemnet_data::ChainDataSink;
use stroemnet_handler::Handler;
use stroemnet_p2p::P2p;
use stroemnet_p2p::network::NetEvent;
#[cfg(target_arch = "wasm32")]
use tokio::sync::mpsc;

#[cfg(target_arch = "wasm32")]
use crate::{CheckedQuote, SwapStatusUpdate};

/// Maximum size of the redeem script, i.e. the swap script.
/// This gives a generous margin
pub const MAX_REDEEM_SCRIPT_BYTES: usize = 512;

pub(super) type DynResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The role of the node, either an LP node or an observer. CCR is gated by a flag
pub enum Role {
    Lp,
    Observer,
}

/// The junction that ties multiple components together at least at the message
/// level
pub struct Coordinator {
    pub handler: Arc<Handler>, // knows about swaps and their state as well as prices
    pub network: Arc<P2p>,     // the p2p  network connecting multiple nodes
    pub sink: Arc<ChainDataSink>, // a collection of all the channels
    pub role: Role,            // what role this coordinator has
    #[cfg(target_arch = "wasm32")]
    pub quote_tx: mpsc::UnboundedSender<CheckedQuote>, // for wasm only
    #[cfg(target_arch = "wasm32")]
    pub swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>, // wasm only
}

impl Coordinator {
    pub fn new(
        handler: Arc<Handler>,
        network: Arc<P2p>,
        sink: Arc<ChainDataSink>,
        role: Role,
        #[cfg(target_arch = "wasm32")] quote_tx: mpsc::UnboundedSender<CheckedQuote>,
        #[cfg(target_arch = "wasm32")] swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
    ) -> Arc<Self> {
        Arc::new(Self {
            handler,
            network,
            sink,
            role,
            #[cfg(target_arch = "wasm32")]
            quote_tx,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx,
        })
    }

    /// Spawns the loop that reads incomign p2p messages and routes them through the coordinator
    pub fn spawn_dispatch_loop(
        self: Arc<Self>,
        mut events: futures::channel::mpsc::Receiver<NetEvent>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let fut = async move {
            while let Some(NetEvent { from, msg }) = events.next().await {
                if let Err(e) = self.handle_incoming(&from, msg).await {
                    tracing::warn!("p2p incoming message error from {from}: {e}");
                }
            }
            tracing::info!("p2p coordinator: events channel closed; shutting down");
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            Some(tokio::spawn(fut))
        }
        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(fut);
            None
        }
    }
}
