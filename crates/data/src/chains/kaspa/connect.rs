use parking_lot::Mutex;
use std::str::FromStr;
use std::sync::Arc;

use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_hashes::Hash;
use kaspa_wrpc_client::prelude::NetworkId;
use serde_json::Value;
use stroemnet_protocol::ChannelId;
use tokio::sync::RwLock;

use stroemnet_protocol::now_unix_secs;

use super::Kaspa;
use super::client::{build_client, spawn_intake};
use super::config::KaspaConfig;
use super::contracts::commitments_from_scripts;
use crate::chains::record::restore;
use crate::chains::settlement::{SettlementMetrics, or_noop, seed_queue};
use crate::{CursorStore, DataError, Result, SwapStore};

impl Kaspa {
    /// Connect to the kaspa rpc client and setup the channel fully for processing data.
    /// I.e. the main entrypoint for this channel
    pub(crate) async fn connect(
        channel_id: ChannelId,                       // the channel
        cfg: &Value,                                 // the configuration for the channel
        private_key: Option<String>,                 // private key
        cursor_store: Option<Arc<dyn CursorStore>>,  // cursor storage
        swap_store: Option<Arc<dyn SwapStore>>,      // swap storage
        metrics: Option<Arc<dyn SettlementMetrics>>, // general stats
    ) -> Result<Self> {
        // parse the config
        let cfg: KaspaConfig = serde_json::from_value(cfg.clone())
            .map_err(|e| DataError::Config(format!("kaspa config: {e}")))?;
        let network_id = NetworkId::from_str(&cfg.network_id)
            .map_err(|e| DataError::Config(format!("network_id: {e:?}")))?;
        let prefix: Prefix = network_id.into();

        // Build the kaspa rpc client
        let client = build_client(network_id, cfg.wrpc_url.as_deref()).await?;

        // Compute the initial cursor
        let initial_cursor = cursor_store
            .as_ref()
            .and_then(|s| s.load(channel_id))
            .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
            .map(Hash::from_bytes);

        // Spawn the intake
        let rx = spawn_intake(
            client.clone(),
            cfg.minimum_block_confirmations,
            channel_id,
            initial_cursor,
            cursor_store,
        );

        tracing::info!(
            "Kaspa buffer {channel_id} connected to {:?} (confirmations {}, ccr {})",
            client.url(),
            cfg.minimum_block_confirmations,
            cfg.participate_ccr,
        );

        // Restore old swaps
        let restored = restore(swap_store.as_ref(), channel_id);
        tracing::info!(
            "Kaspa buffer {channel_id} restored {} refund(s), {} claim(s) from store",
            restored.pending_refunds.len(),
            restored.pending_claims.len(),
        );

        // Seed the queue with swaps
        let queue = seed_queue(&restored, now_unix_secs());

        // Compute commitments from restored scripts
        let commitments = commitments_from_scripts(&restored.scripts, prefix, channel_id);

        // Create the kaspa channel data buffer
        let buffer = Self {
            channel_id,
            network_id: cfg.network_id,
            prefix,
            coinbase_maturity: cfg.coinbase_maturity,
            script_ttl_secs: cfg.script_ttl_secs,
            participate_ccr: cfg.participate_ccr,
            private_key,
            client,
            utxo_scripts: Arc::new(RwLock::new(AHashMap::new())),
            safe_blocks: Mutex::new(rx),
            commitments: Mutex::new(commitments),
            pending_refunds: Mutex::new(restored.pending_refunds),
            pending_claims: Mutex::new(restored.pending_claims),
            announcements: Mutex::new(Vec::new()),
            scripts: Mutex::new(restored.scripts),
            swap_store,
            queue,
            metrics: or_noop(metrics),
        };
        #[cfg(not(target_arch = "wasm32"))]
        // check if any of the pending stored swaps have been settled while we were away
        crate::chains::settlement::reconcile_on_boot(&buffer, buffer.metrics.as_ref()).await;
        Ok(buffer)
    }
}
