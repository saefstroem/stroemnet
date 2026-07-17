use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use stroemnet_handler::Handler;
use stroemnet_p2p::P2p;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

use crate::coordinator::Role;

#[cfg(target_arch = "wasm32")]
use ahash::AHashMap;
#[cfg(target_arch = "wasm32")]
use stroemnet_data::ChainDataSink;
#[cfg(target_arch = "wasm32")]
use tokio::sync::RwLock;
#[cfg(target_arch = "wasm32")]
use tokio::sync::mpsc;

#[cfg(target_arch = "wasm32")]
use crate::{CheckedQuote, PendingClaim, SwapStage, SwapStatusUpdate};

/// The node which collectively stores multiple major components
pub struct Node {
    pub handler: Arc<Handler>,
    pub network: Arc<P2p>,
    pub peer_count: Arc<AtomicUsize>,
    #[cfg(target_arch = "wasm32")]
    pub(super) sink: Arc<ChainDataSink>,
    #[cfg(target_arch = "wasm32")]
    pub(super) pending_claims: Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    #[cfg(target_arch = "wasm32")]
    pub(super) swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
    pub(super) role: Role,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) tasks: Vec<JoinHandle<()>>,
}

#[cfg(target_arch = "wasm32")]
pub(super) type StartOutput = (Node, mpsc::UnboundedReceiver<CheckedQuote>);
#[cfg(not(target_arch = "wasm32"))]
pub(super) type StartOutput = Node;

impl Node {
    /// Assemble multiple components into one
    pub(super) fn assemble(
        handler: Arc<Handler>,
        network: Arc<P2p>,
        peer_count: Arc<AtomicUsize>,
        role: Role,
        #[cfg(target_arch = "wasm32")] sink: Arc<ChainDataSink>,
        #[cfg(target_arch = "wasm32")] pending_claims: Arc<
            RwLock<AHashMap<[u8; 32], PendingClaim>>,
        >,
        #[cfg(target_arch = "wasm32")] swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
        #[cfg(not(target_arch = "wasm32"))] tasks: Vec<JoinHandle<()>>,
    ) -> Self {
        Self {
            handler,
            network,
            peer_count,
            #[cfg(target_arch = "wasm32")]
            sink,
            #[cfg(target_arch = "wasm32")]
            pending_claims,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx,
            role,
            #[cfg(not(target_arch = "wasm32"))]
            tasks,
        }
    }

    pub fn peer_count(&self) -> usize {
        self.peer_count.load(Ordering::SeqCst)
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn shutdown(self) {
        #[cfg(not(target_arch = "wasm32"))]
        for t in &self.tasks {
            t.abort();
        }
        tracing::info!("stroemnet node shut down");
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn emit_status(&self, swap_id: [u8; 32], stage: SwapStage) {
        let _ = self.swap_status_tx.send(SwapStatusUpdate {
            swap_id,
            stage,
            at: stroemnet_protocol::now_unix_secs(),
        });
    }
}
