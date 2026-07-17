mod accessors;
mod broadcast;
mod buffer;
mod client;
mod config;
mod connect;
mod contracts;
mod decode;
mod detector;
mod emit;
mod error;
mod intake;
mod persist;
mod poll;
#[cfg(not(target_arch = "wasm32"))]
mod reconcile;
#[cfg(not(target_arch = "wasm32"))]
mod settle;
#[cfg(not(target_arch = "wasm32"))]
mod settler;
mod signing;
#[cfg(test)]
mod test_helpers;

use parking_lot::Mutex;
use std::sync::Arc;

use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_wrpc_client::KaspaRpcClient;
use kaspa_wrpc_client::prelude::RpcBlock;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{CommitmentV1, RefundV1, RevealV1};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

use crate::chains::settlement::{RetryQueue, SettlementMetrics};
use crate::{ScriptAnnouncement, SwapStore, UtxoScript};

/// The kaspa channel
pub(crate) struct Kaspa {
    /// Channel id of the kaspa channel
    channel_id: ChannelId,
    /// Kaspa specific network id
    network_id: String,
    /// Network prefix
    prefix: Prefix,
    /// How many daa to wait before coinbase utxo is valid
    coinbase_maturity: u64,
    /// Time to live for an announced script utxo
    script_ttl_secs: u64,
    /// Whether to participate in CCR
    participate_ccr: bool,
    /// Optional private key for lp/ccr nodes
    private_key: Option<String>,
    /// Kaspa rpc client
    client: Arc<KaspaRpcClient>,
    /// Storage of utxo scripts
    utxo_scripts: Arc<RwLock<AHashMap<String, UtxoScript>>>,
    /// A reciver for safe confirmed finalized blocks
    safe_blocks: Mutex<Receiver<Arc<RpcBlock>>>,
    /// Tracking all commitments by swap id
    commitments: Mutex<AHashMap<[u8; 32], CommitmentV1>>,
    /// Queue for pending refunds
    pending_refunds: Mutex<Vec<(RefundV1, u64)>>,
    /// Queue for pending claims
    pending_claims: Mutex<Vec<RevealV1>>,
    /// UTXO script annoucements
    announcements: Mutex<Vec<ScriptAnnouncement>>,
    /// Scripts by swap id
    scripts: Mutex<AHashMap<[u8; 32], UtxoScript>>,
    /// Permanent disk storage for swaps
    swap_store: Option<Arc<dyn SwapStore>>,
    /// A retry queue for both claims and refunds
    queue: RetryQueue,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    /// General stats
    metrics: Arc<dyn SettlementMetrics>,
}
