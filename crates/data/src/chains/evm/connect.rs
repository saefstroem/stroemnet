use parking_lot::Mutex;
use std::sync::Arc;

use alloy::providers::Provider;
use serde_json::Value;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;

use super::config::EvmConfig;
use super::finality::PollState;
use super::provider::build_providers;
use super::{Evm, EvmState};
use crate::CursorStore;
use crate::SwapStore;
use crate::chains::evm::parse_address;
use crate::chains::net::retry_timed;
use crate::chains::record::restore;
use crate::chains::settlement::{SettlementMetrics, or_noop, seed_queue};
use crate::{DataError, Result};

impl Evm {
    /// Connects to the EVM channel, restores cursors and reconciles pending swap data
    pub(crate) async fn connect(
        channel_id: ChannelId,                       // channel id for the network
        cfg: &Value,                                 // the arbitrary value of the configuration
        private_key: Option<String>, // maybe a private key if this is an lp or participates in ccr
        cursor_store: Option<Arc<dyn CursorStore>>, // storage for storing cursors
        swap_store: Option<Arc<dyn SwapStore>>, // storing swaps
        metrics: Option<Arc<dyn SettlementMetrics>>, // metrics for statistics
    ) -> Result<Self> {
        // Try parse the evm config
        let cfg: EvmConfig = serde_json::from_value(cfg.clone())
            .map_err(|e| DataError::Config(format!("evm config: {e}")))?;

        // Parse the htlc address
        let htlc_address = parse_address("htlc_address", &cfg.htlc_address)?;

        // Build providers to connect to the EVM network
        let (read_provider, signed_provider) =
            build_providers(&cfg.rpc_url, private_key.as_deref()).await?;

        // Retrieve the current head of the evm chain
        let head = retry_timed("connect get_block_number", || {
            read_provider.get_block_number()
        })
        .await
        .ok_or_else(|| DataError::Connect("get_block_number: timed out".into()))?;

        // Compute the fallback cursor which is the minimum block confirmations + 1
        // this is essentially means that we start from the stable finalized head
        // from our perspective.
        let fallback_cursor = head
            .saturating_sub(cfg.minimum_block_confirmations)
            .saturating_add(1);

        // Retrieve the cursor for this channel id and convert it back to
        // a u64 of fallback to the fallback cursor
        let cursor = cursor_store
            .as_ref()
            .and_then(|s| s.load(channel_id))
            .and_then(|b| <[u8; 8]>::try_from(b.as_slice()).ok())
            .map(u64::from_le_bytes)
            .unwrap_or(fallback_cursor);

        tracing::info!(
            "EVM buffer {channel_id} connected to {} — polling from block {cursor} (confirmations {}, ccr {})",
            cfg.rpc_url,
            cfg.minimum_block_confirmations,
            cfg.participate_ccr,
        );

        // Restore old swaps based on the channel id
        let restored = restore(swap_store.as_ref(), channel_id);

        // Some of the restored swaps might need to be claimed or refunded
        // so lets seed the queue and try
        let queue = seed_queue(&restored, now_unix_secs());

        // Track the pending refunds and claims
        let pending_refunds = restored.pending_refunds;
        let pending_claims = restored.pending_claims;

        // Create the evm buffer
        let buffer = Self {
            channel_id,
            htlc_address,
            minimum_block_confirmations: cfg.minimum_block_confirmations,
            poll_interval_secs: (cfg.poll_interval_ms / 1000).max(1),
            max_blocks_per_poll: cfg.max_blocks_per_poll,
            participate_ccr: cfg.participate_ccr,
            gas_payment: cfg.gas_payment,
            private_key,
            read_provider,
            signed_provider,
            state: Mutex::new(EvmState {
                poll: PollState { cursor },
                pending_refunds,
                pending_claims,
                next_poll_secs: 0,
                last_block_ts: None,
            }),
            cursor_store,
            swap_store,
            queue,
            metrics: or_noop(metrics),
        };
        #[cfg(not(target_arch = "wasm32"))]
        // Check if any of the restored swaps have been finished during the outage or offline time
        crate::chains::settlement::reconcile_on_boot(&buffer, buffer.metrics.as_ref()).await;
        Ok(buffer)
    }
}
