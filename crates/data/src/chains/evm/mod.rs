mod broadcast;
mod buffer;
mod config;
mod connect;
mod contracts;
mod decode;
mod emit;
mod finality;
mod persist;
mod poll;
mod provider;
#[cfg(not(target_arch = "wasm32"))]
mod reconcile;
#[cfg(not(target_arch = "wasm32"))]
mod replace;
#[cfg(not(target_arch = "wasm32"))]
mod settle;
#[cfg(not(target_arch = "wasm32"))]
mod settler;
mod signing;

use parking_lot::Mutex;
use std::sync::Arc;

use alloy::primitives::Address;
use alloy::providers::DynProvider;
use serde::Deserialize;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{RefundV1, RevealV1};

use crate::chains::settlement::{RetryQueue, SettlementMetrics};
use crate::{CursorStore, DataError, Result, SwapStore};
use finality::PollState;
#[derive(Deserialize, Clone, Copy, Default, Debug)]
#[serde(rename_all = "lowercase")]
/// Gas variant. Some networks use legacy, not all are eip1559 compatible
pub(crate) enum GasPayment {
    #[default]
    Eip1559,
    Legacy,
}

/// The state of the EVM channel
struct EvmState {
    /// The current cursor of the evm channel
    poll: PollState,
    /// Pending refunds that have the time for which they can be refunded
    pending_refunds: Vec<(RefundV1, u64)>,
    /// Pending claims for already revealed secrets
    pending_claims: Vec<RevealV1>,
    /// When is the next time for which we should poll the network
    next_poll_secs: u64,
    /// Last safe block timestamp for evm network
    last_block_ts: Option<(u64, u64)>,
}

/// The general struct for the EVM channel
pub(crate) struct Evm {
    /// Identifies which channel this is
    /// todo: currently non-evm channels can be represented here
    /// maybe channelids should be categorized to make invalid state
    /// non-representable
    channel_id: ChannelId,
    /// Contract address of the htlc contract
    htlc_address: Address,
    /// Minimum number of block confirmations
    minimum_block_confirmations: u64,
    /// How often to poll from the network
    poll_interval_secs: u64,
    /// Maximum amount of blocks per poll
    max_blocks_per_poll: u64,
    /// Whether to participate in ccr
    participate_ccr: bool,
    /// Which variant of gas payment that we should do
    gas_payment: GasPayment,
    /// If this is an LP then we also utilize a private key
    private_key: Option<String>,
    /// A read provider used for read only operations
    read_provider: DynProvider,
    /// Signed providers for LP's and CCR nodes
    signed_provider: Option<DynProvider>,
    /// The state of the channel, tracking current state
    state: Mutex<EvmState>,
    /// trait backed cursor store to support both wasm and native impls
    cursor_store: Option<Arc<dyn CursorStore>>,
    /// trait backed cursor store to support both wasm and native impls
    swap_store: Option<Arc<dyn SwapStore>>,
    /// A queue for actions that have been executed by the settler, and also retry attempts
    queue: RetryQueue,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    /// Statistics about swaps in general
    metrics: Arc<dyn SettlementMetrics>,
}

/// Function to parse a string EVM address into the alloy variant
fn parse_address(label: &str, value: &str) -> Result<Address> {
    value
        .parse()
        .map_err(|e| DataError::Broadcast(format!("{label} address {value}: {e}")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::chains::record::restore;
    use crate::{ChainDataBuffer, SwapStore};
    use alloy::providers::{Provider, ProviderBuilder};
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;
    use stroemnet_protocol::v1::ChainEvent;

    type Rows = StdMutex<HashMap<(u8, [u8; 32]), Vec<u8>>>;

    #[derive(Default)]
    struct MemSwapStore {
        rows: Rows,
    }

    impl crate::SwapStore for MemSwapStore {
        fn load_channel(&self, channel_id: ChannelId) -> Vec<([u8; 32], Vec<u8>)> {
            self.rows
                .lock()
                .unwrap()
                .iter()
                .filter(|((c, _), _)| *c == channel_id as u8)
                .map(|((_, id), v)| (*id, v.clone()))
                .collect()
        }
        fn save(&self, channel_id: ChannelId, swap_id: [u8; 32], record: &[u8]) {
            self.rows
                .lock()
                .unwrap()
                .insert((channel_id as u8, swap_id), record.to_vec());
        }
        fn delete(&self, channel_id: ChannelId, swap_id: [u8; 32]) {
            self.rows
                .lock()
                .unwrap()
                .remove(&(channel_id as u8, swap_id));
        }
    }

    async fn test_evm(swap_store: Option<Arc<dyn crate::SwapStore>>) -> Evm {
        let read_provider = ProviderBuilder::new()
            .connect("http://127.0.0.1:1")
            .await
            .unwrap()
            .erased();
        Evm {
            channel_id: ChannelId::IgraGalleon,
            htlc_address: Address::ZERO,
            minimum_block_confirmations: 0,
            poll_interval_secs: 1,
            max_blocks_per_poll: 1,
            participate_ccr: true,
            gas_payment: GasPayment::Legacy,
            private_key: None,
            read_provider,
            signed_provider: None,
            state: Mutex::new(EvmState {
                poll: PollState { cursor: 0 },
                pending_refunds: Vec::new(),
                pending_claims: Vec::new(),
                next_poll_secs: 0,
                last_block_ts: None,
            }),
            cursor_store: None,
            swap_store,
            queue: RetryQueue::default(),
            metrics: crate::chains::settlement::or_noop(None),
        }
    }

    #[tokio::test]
    async fn reveal_enqueues_and_persists_without_pending_refund() {
        let store = Arc::new(MemSwapStore::default());
        let store_dyn: Arc<dyn crate::SwapStore> = store.clone();
        let evm = test_evm(Some(store_dyn)).await;
        assert!(evm.state.lock().pending_refunds.is_empty());

        let reveal = RevealV1::new([5u8; 32], [6u8; 32]);
        evm.broadcast_event(&ChainEvent::Reveal(reveal.clone()))
            .await
            .unwrap();

        assert_eq!(evm.state.lock().pending_claims, vec![reveal.clone()]);
        let rows = store.load_channel(ChannelId::IgraGalleon);
        assert_eq!(rows.len(), 1);
        let store_dyn: Arc<dyn crate::SwapStore> = store.clone();
        let restored = restore(Some(&store_dyn), ChannelId::IgraGalleon);
        assert_eq!(restored.pending_claims, vec![reveal]);
    }

    #[test]
    fn seeds_pending_claims_from_store() {
        let store = Arc::new(MemSwapStore::default());
        let reveal = RevealV1::new([1u8; 32], [2u8; 32]);
        let rec = crate::PersistedSwap {
            script: None,
            pending_refund: None,
            pending_claim: Some(reveal.clone()),
            claim_attempt: None,
            refund_attempt: None,
        };
        store.save(
            ChannelId::IgraGalleon,
            reveal.swap_id,
            &crate::chains::record::encode(&rec).unwrap(),
        );
        let store_dyn: Arc<dyn crate::SwapStore> = store;
        let restored = restore(Some(&store_dyn), ChannelId::IgraGalleon);
        assert!(restored.pending_refunds.is_empty());
        assert_eq!(restored.pending_claims, vec![reveal]);
    }
}
