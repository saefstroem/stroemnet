use std::sync::Arc;

use ahash::AHashMap;
use stroemnet_amounts::PriceStorage;
use stroemnet_handler::{Handler, HandlerConfig};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::swap_tracker::SwapTracker;
use tokio::sync::RwLock;

pub fn test_handler() -> Arc<Handler> {
    let tracker = Arc::new(RwLock::new(SwapTracker::new()));
    let storage = PriceStorage::new(vec![ChannelId::KaspaTn10, ChannelId::EthereumSepolia]);
    let config = HandlerConfig {
        min_trade_usd: 0.01,
        max_trade_usd: 1_000_000.0,
        spread_percent: 0.01,
        commit_buffer_secs: 60,
    };
    let mut addresses = AHashMap::new();
    addresses.insert(ChannelId::KaspaTn10, "kaspa:test_address".to_string());
    addresses.insert(ChannelId::EthereumSepolia, "0xTestEthAddress".to_string());
    let mut confirmations = AHashMap::new();
    confirmations.insert(ChannelId::KaspaTn10, 1u64);
    confirmations.insert(ChannelId::EthereumSepolia, 1u64);
    Arc::new(Handler::new(
        storage,
        tracker,
        config,
        Arc::new(addresses),
        Arc::new(confirmations),
    ))
}
