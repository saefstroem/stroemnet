use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use stroemnet_data::{CursorStore, SwapStore};

use super::build::{build_handler_and_sink, build_network};
use super::driver::spawn_processing;
use super::state::{Node, StartOutput};
use crate::NodeConfig;
use crate::connection::spawn_bootstrap_with_counter;
use crate::coordinator::Coordinator;
use crate::result::Result;

#[cfg(not(target_arch = "wasm32"))]
use super::services::spawn_native_services;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

#[cfg(target_arch = "wasm32")]
use ahash::AHashMap;
#[cfg(target_arch = "wasm32")]
use tokio::sync::RwLock;
#[cfg(target_arch = "wasm32")]
use tokio::sync::mpsc;

impl Node {
    /// Starts the node
    pub async fn start(
        cfg: NodeConfig,
        cursor_store: Option<Arc<dyn CursorStore>>,
        swap_store: Option<Arc<dyn SwapStore>>,
    ) -> Result<StartOutput> {
        #[cfg(target_arch = "wasm32")]
        let (quote_tx, quote_rx) = mpsc::unbounded_channel();
        #[cfg(target_arch = "wasm32")]
        let swap_status_tx = cfg.swap_status_tx.clone();

        // Build the handler and sink which creates them
        let (handler, sink) =
            build_handler_and_sink(cfg.handler, cfg.channels, cursor_store, swap_store).await?;

        #[cfg(not(target_arch = "wasm32"))]
        let (dial_tx, dial_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        
        // build network and receiver for network events
        let (network, net_events) = build_network(
            cfg.bootstrap_peers,
            cfg.advertised_listen_addr.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            dial_tx,
        );
        let peer_count = Arc::new(AtomicUsize::new(0));

        #[cfg(target_arch = "wasm32")]
        let pending_claims = Arc::new(RwLock::new(AHashMap::new()));

        // Create a new coordinator
        let coordinator = Coordinator::new(
            handler.clone(),
            network.clone(),
            sink.clone(),
            cfg.role,
            #[cfg(target_arch = "wasm32")]
            quote_tx,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx.clone(),
        );
        #[cfg(not(target_arch = "wasm32"))]
        let mut tasks: Vec<JoinHandle<()>> = Vec::new();

        // Spawn the main loops
        spawn_processing(
            coordinator,
            sink.clone(),
            handler.clone(),
            network.clone(),
            net_events,
            #[cfg(target_arch = "wasm32")]
            pending_claims.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            &mut tasks,
        );

        #[cfg(not(target_arch = "wasm32"))]
        spawn_native_services(
            handler.clone(),
            network.clone(),
            peer_count.clone(),
            cfg.role,
            cfg.price_oracle_update_interval_secs,
            cfg.bind_addr,
            dial_rx,
            &mut tasks,
        )
        .await?;

        spawn_bootstrap_with_counter(network.clone(), peer_count.clone());
        network.clone().spawn_periodic_state_broadcast(60);

        // Create the node
        let node = Node::assemble(
            handler,
            network,
            peer_count,
            cfg.role,
            #[cfg(target_arch = "wasm32")]
            sink,
            #[cfg(target_arch = "wasm32")]
            pending_claims,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx,
            #[cfg(not(target_arch = "wasm32"))]
            tasks,
        );
        #[cfg(target_arch = "wasm32")]
        return Ok((node, quote_rx));
        #[cfg(not(target_arch = "wasm32"))]
        Ok(node)
    }
}
