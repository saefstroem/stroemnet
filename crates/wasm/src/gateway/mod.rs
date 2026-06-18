mod connect;
mod event_listeners;
mod inner;
mod swaps;
use std::sync::{Arc, Mutex, OnceLock};

use ahash::AHashMap;
use serde::Deserialize;
use stroemnet_handler::HandlerConfig;
use stroemnet_node::coordinator::Role;
use stroemnet_node::{ChannelSpec, Node, NodeConfig, SwapStatusUpdate};
use stroemnet_protocol::ChannelId;
use tokio::sync::mpsc;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use crate::defaults::default_observer_channels_json;
use crate::gateway::inner::{EventCallbacks, Inner};

fn canonical_chain_id(name: &str) -> Result<ChannelId, String> {
    match name {
        "kaspa-tn10" => Ok(ChannelId::KaspaTn10),
        "ethereum-sepolia" => Ok(ChannelId::EthereumSepolia),
        "igra-galleon" => Ok(ChannelId::IgraGalleon),
        other => Err(format!("unknown chain '{other}'")),
    }
}

#[derive(Deserialize)]
struct GatewayConfig {
    #[serde(rename = "observerChannels", default)]
    observer_channels: Option<serde_json::Value>,
    #[serde(rename = "bootstrapPeers", default)]
    bootstrap_peers: Option<Vec<String>>,
    handler: HandlerConfigJs,
}

#[derive(Deserialize)]
struct HandlerConfigJs {
    #[serde(rename = "minTradeUsd")]
    min_trade_usd: f64,
    #[serde(rename = "maxTradeUsd")]
    max_trade_usd: f64,
    #[serde(rename = "spreadPercent")]
    spread_percent: f64,
    #[serde(rename = "commitBufferSecs")]
    commit_buffer_secs: u64,
}

impl GatewayConfig {
    fn into_node_config(
        self,
        swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
    ) -> Result<NodeConfig, JsError> {
        let raw_channels = self
            .observer_channels
            .unwrap_or_else(default_observer_channels_json);
        let obj = raw_channels
            .as_object()
            .ok_or_else(|| JsError::new("observerChannels must be a JSON object"))?;
        let mut channels: AHashMap<ChannelId, ChannelSpec> = AHashMap::new();
        for (k, v) in obj {
            let id = canonical_chain_id(k).map_err(|e| JsError::new(&e))?;
            channels.insert(
                id,
                ChannelSpec {
                    config: v.clone(),
                    lp_private_key: None,
                },
            );
        }

        let bootstrap_peers = self.bootstrap_peers.unwrap_or_else(|| {
            stroemnet_p2p::SEED_NODES
                .iter()
                .map(|u| (*u).to_string())
                .collect()
        });

        Ok(NodeConfig {
            handler: HandlerConfig {
                min_trade_usd: self.handler.min_trade_usd,
                max_trade_usd: self.handler.max_trade_usd,
                spread_percent: self.handler.spread_percent,
                commit_buffer_secs: self.handler.commit_buffer_secs,
            },
            channels,
            bootstrap_peers,
            role: Role::Observer,
            advertised_listen_addr: None,
            swap_status_tx,
        })
    }
}

#[wasm_bindgen]
/// A gateway for interacting with the stroemnet P2P Atomic Swap RFQ network,
/// allowing you to request quotes and submit commitments for cross-chain swaps.
///
/// The gateway manages an observer-only stroemnet node which connects to the
/// stroemnet network. It observes all chain activity monitoring both for swaps
/// and requests.
pub struct StroemGateway {
    inner: Arc<Inner>,
}

#[wasm_bindgen]
impl StroemGateway {
    #[wasm_bindgen(constructor)]
    /// Creates a new StroemGateway instance with the provided configuration.
    /// You can get the default configuration by calling `getDefaultConfig()`, and then modify it as needed.
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
    /// Returns the current number of connected peers in the stroemnet network. This can be useful for monitoring the connectivity status of the gateway.
    pub fn peer_count(&self) -> Result<usize, JsError> {
        Ok(self.require_node()?.peer_count())
    }
}
