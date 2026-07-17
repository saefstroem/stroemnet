#![allow(clippy::result_large_err)]

mod address;
mod dispatch;
pub mod error;
pub mod get;
pub mod handle;
pub mod result;

#[cfg(test)]
mod test_fixtures;

use std::sync::Arc;

use ahash::AHashMap;
use stroemnet_amounts::PriceStorage;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::swap_tracker::SwapTracker;
use tokio::sync::RwLock;

pub use address::normalised_address_eq;
pub use dispatch::Effect;
pub use error::HandlerError;

pub fn required_init_lock_secs(
    destination: ChannelId,
    commit_buffer_secs: u64,
    with_buffer: bool,
) -> u64 {
    // The user needs to always lock for 2x the LP + the commit buffer
    if !with_buffer {
        return destination.lock_time_secs() * 2;
    }
    destination.lock_time_secs() * 2 + commit_buffer_secs
}

#[derive(Debug, Clone)]
/// Configuration for the handler on trade requirements
pub struct HandlerConfig {
    /// Minimum usd amount for trading
    pub min_trade_usd: f64,
    /// Maximum usd amount for trading
    pub max_trade_usd: f64,
    /// Spread percent for trading
    pub spread_percent: f64,
    /// Buffer in seconds to allow trade to propagate across P2P network.
    pub commit_buffer_secs: u64,
}

#[derive(Debug)]
pub struct Handler {
    /// Price storage tracking prices for all channels
    pub price_storage: PriceStorage,
    /// Tracking all swaps
    pub swap_tracker: Arc<RwLock<SwapTracker>>,
    /// Configuration for the handler
    pub config: HandlerConfig,
    /// Lookup table for us as an LP going from a channel id to that address
    pub address_lookup_table: Arc<AHashMap<ChannelId, String>>,
    /// Required block confirmations
    pub block_confirmations: Arc<AHashMap<ChannelId, u64>>,
}

impl Handler {
    /// Whether this channel is activated, a bit hacky but works
    pub fn knows_channel(&self, id: ChannelId) -> bool {
        self.block_confirmations.contains_key(&id)
    }

    /// Create the handler
    pub fn new(
        price_storage: PriceStorage,
        swap_tracker: Arc<RwLock<SwapTracker>>,
        config: HandlerConfig,
        address_lookup_table: Arc<AHashMap<ChannelId, String>>,
        block_confirmations: Arc<AHashMap<ChannelId, u64>>,
    ) -> Self {
        Self {
            price_storage,
            swap_tracker,
            config,
            address_lookup_table,
            block_confirmations,
        }
    }
}
