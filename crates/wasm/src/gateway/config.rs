use ahash::AHashMap;
use serde::Deserialize;
use stroemnet_handler::HandlerConfig;
use stroemnet_node::coordinator::Role;
use stroemnet_node::{ChannelSpec, NodeConfig, SwapStatusUpdate};
use stroemnet_protocol::ChannelId;
use tokio::sync::mpsc;
use wasm_bindgen::JsError;

use crate::defaults::{default_bootstrap_peers, default_observer_channels_json};

#[derive(Deserialize)]
pub(super) struct GatewayConfig {
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
    pub(super) fn into_node_config(
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
            let id = ChannelId::try_from(k.as_str()).map_err(|e| JsError::new(&e))?;
            channels.insert(
                id,
                ChannelSpec {
                    config: v.clone(),
                    lp_private_key: None,
                },
            );
        }

        let bootstrap_peers = self.bootstrap_peers.unwrap_or_else(default_bootstrap_peers);

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
