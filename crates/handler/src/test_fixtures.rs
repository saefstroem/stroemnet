use std::sync::Arc;

use ahash::AHashMap;
use stroemnet_amounts::PriceStorage;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::swap_tracker::SwapTracker;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};
use tokio::sync::RwLock;

use crate::Handler;
use crate::HandlerConfig;

pub(crate) const TEST_SECRET: [u8; 32] = [0xAB; 32];

pub(crate) fn sha256(secret: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let out = Sha256::digest(secret);
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    a
}

pub(crate) fn test_secret_hash() -> [u8; 32] {
    sha256(&TEST_SECRET)
}

pub(crate) fn default_test_config() -> HandlerConfig {
    HandlerConfig {
        min_trade_usd: 0.01,
        max_trade_usd: 1_000_000.0,
        spread_percent: 0.01,
        commit_buffer_secs: 60,
    }
}

pub(crate) fn create_test_handler() -> (Handler, Arc<RwLock<SwapTracker>>) {
    create_test_handler_with(
        default_test_config(),
        &[
            (ChannelId::KaspaTn10, 0.0),
            (ChannelId::EthereumSepolia, 0.0),
        ],
        AHashMap::new(),
    )
}

pub(crate) fn create_test_handler_with(
    config: HandlerConfig,
    prices: &[(ChannelId, f64)],
    addresses: AHashMap<ChannelId, String>,
) -> (Handler, Arc<RwLock<SwapTracker>>) {
    let swap_tracker = Arc::new(RwLock::new(SwapTracker::new()));
    let channels: Vec<ChannelId> = prices.iter().map(|(c, _)| *c).collect();
    let storage = PriceStorage::new(channels);
    for (channel, price) in prices {
        storage.set(*channel, *price);
    }
    let address_lookup: Arc<AHashMap<ChannelId, String>> = Arc::new(addresses);
    let block_confirmations: Arc<AHashMap<ChannelId, u64>> =
        Arc::new(prices.iter().map(|(c, _)| (*c, 1u64)).collect());
    let handler = Handler::new(
        storage,
        Arc::clone(&swap_tracker),
        config,
        address_lookup,
        block_confirmations,
    );
    (handler, swap_tracker)
}

pub(crate) fn lp_addresses() -> AHashMap<ChannelId, String> {
    let mut m = AHashMap::new();
    m.insert(ChannelId::KaspaTn10, "kaspa:mm_kaspa_address".to_string());
    m.insert(
        ChannelId::EthereumSepolia,
        "0xMmEthereumAddress".to_string(),
    );
    m
}

pub(crate) fn mock_init_commitment(swap_id: [u8; 32]) -> CommitmentV1 {
    mock_init_commitment_with_secret(swap_id, test_secret_hash())
}

pub(crate) fn mock_init_commitment_with_secret(
    swap_id: [u8; 32],
    secret_hash: [u8; 32],
) -> CommitmentV1 {
    CommitmentV1 {
        swap_id,
        addresses: AddressesV1::new(
            "0xUserEthSender".to_string(),
            "0xUserEthReceiver".to_string(),
            "kaspa:user_dest_address".to_string(),
        ),
        amount: AmountV1::new("1000".to_string(), 8),
        secret_hash,
        unlock_ts: u64::MAX,
        source: ChannelId::EthereumSepolia as u8,
        destination: ChannelId::KaspaTn10 as u8,
    }
}

pub(crate) fn mock_counter_commitment(swap_id: [u8; 32]) -> CommitmentV1 {
    mock_counter_commitment_with_secret(swap_id, test_secret_hash())
}

pub(crate) fn mock_counter_commitment_with_secret(
    swap_id: [u8; 32],
    secret_hash: [u8; 32],
) -> CommitmentV1 {
    CommitmentV1 {
        swap_id,
        addresses: AddressesV1::new(
            "kaspa:mm_kaspa_sender".to_string(),
            "kaspa:user_dest_address".to_string(),
            "0xUserEthReceiver".to_string(),
        ),
        amount: AmountV1::new("2000".to_string(), 18),
        secret_hash,
        unlock_ts: u64::MAX,
        source: ChannelId::KaspaTn10 as u8,
        destination: ChannelId::EthereumSepolia as u8,
    }
}
