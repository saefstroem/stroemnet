use std::sync::Arc;

use ahash::AHashMap;
use stroemnet_amounts::PriceStorage;
use stroemnet_data::ChainDataSink;
use stroemnet_data::{CursorStore, Gauge, Metric, SettlementMetrics, SwapStore};
use stroemnet_handler::{Handler, HandlerConfig};
use stroemnet_p2p::network::NetEvent;
use stroemnet_p2p::{P2p, P2pConfig};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::swap_tracker::SwapTracker;
use tokio::sync::RwLock;

use crate::ChannelSpec;
use crate::error::StroemnetError;
use crate::result::Result;

/// Builds the p2p struct responsible for communicating with other ndoes
pub(super) fn build_network(
    bootstrap_peers: Vec<String>, // the bootstrap peers
    advertised: Option<String>,   // whether we listen to any address
    #[cfg(not(target_arch = "wasm32"))] dial_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> (Arc<P2p>, futures::channel::mpsc::Receiver<NetEvent>) {
    let net_config = P2pConfig {
        bootstrap_peers,
        advertised_listen_addr: advertised,
        #[cfg(not(target_arch = "wasm32"))]
        discovered_peer_dial_tx: Some(dial_tx),
        ..P2pConfig::default()
    };
    let (network, net_events) = P2p::new(net_config);
    (Arc::new(network), net_events)
}

/// Build the swap tracker and the element responsible
/// for tracking swaps and their state
pub(super) async fn build_handler_and_sink(
    handler_config: HandlerConfig, // configuration for the swap tracker
    channels: AHashMap<ChannelId, ChannelSpec>, // which channels we work on
    cursor_store: Option<Arc<dyn CursorStore>>, // where to store the cursor which we use to maintain chain synk
    swap_store: Option<Arc<dyn SwapStore>>, // where we store swaps after or during their completion to disk
) -> Result<(Arc<Handler>, Arc<ChainDataSink>)> {
    // Retrieve all active channels
    let channel_ids: Vec<ChannelId> = channels.keys().copied().collect();
    let price_storage = PriceStorage::new(channel_ids);
    let swap_tracker = Arc::new(RwLock::new(SwapTracker::new()));

    let mut block_confirmations_map = AHashMap::new();
    let mut sink_channels = AHashMap::new();
    let mut keyed_channels = Vec::new();

    // Go over all channels and insert their minimum block confirmations and
    // configuration, todo: the two DS can be merged into one.
    for (id, spec) in channels {
        if spec.lp_private_key.is_some() {
            keyed_channels.push(id);
        }
        block_confirmations_map.insert(id, spec.minimum_block_confirmations());
        sink_channels.insert(id, (spec.config, spec.lp_private_key));
    }

    let metrics: Arc<dyn SettlementMetrics> = Arc::new(TracingMetrics);

    // Create a new sink which is responsible
    // for giving us chain data
    let sink = ChainDataSink::new(sink_channels, cursor_store, swap_store, Some(metrics))
        .await
        .map_err(|e| StroemnetError::Other(format!("chain data sink: {e}")))?;

    let mut address_map = AHashMap::new();

    // Go over all channels and map the channel id to the LP address
    // so that we can tell users where they should lock funds for
    for id in keyed_channels {
        let address = sink
            .lp_address(id)
            .map_err(|e| StroemnetError::Other(format!("lp address for {id}: {e}")))?;
        tracing::info!("Derived LP address for {id}: {address}");
        address_map.insert(id, address);
    }

    // Create the swap handler
    let handler = Arc::new(Handler::new(
        price_storage,
        swap_tracker,
        handler_config,
        Arc::new(address_map),
        Arc::new(block_confirmations_map),
    ));
    Ok((handler, Arc::new(sink)))
}

struct TracingMetrics;

impl SettlementMetrics for TracingMetrics {
    fn incr(&self, metric: Metric) {
        match metric {
            Metric::Fatal | Metric::DeadlineExceeded => {
                tracing::warn!(target: "settlement", kind = "metric", ?metric);
            }
            _ => tracing::info!(target: "settlement", kind = "metric", ?metric),
        }
    }
    fn gauge(&self, gauge: Gauge, value: u64) {
        tracing::info!(target: "settlement", kind = "gauge", ?gauge, value);
    }
}
