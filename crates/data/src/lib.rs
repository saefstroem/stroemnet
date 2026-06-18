#![warn(unreachable_pub)]

mod chains;
pub mod error;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ahash::AHashMap;
use serde_json::Value;
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1};
use stroemnet_protocol::{ChainClock, ChannelId};

use chains::build_buffer;
pub use error::{DataError, Result};

#[cfg(not(target_arch = "wasm32"))]
/// A trait alias for Send + Sync, which is required for buffers that may be used across threads.
/// At least for native code
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
/// For wasm we dont require Send + Sync, as everything is single-threaded, so this trait is just a marker with no bounds.
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// A trait for persisting the last processed block's hash (cursor) across restarts.
pub trait CursorStore: MaybeSend {
    fn load(&self, channel_id: ChannelId) -> Option<Vec<u8>>;
    fn save(&self, channel_id: ChannelId, cursor: &[u8]);
}

#[cfg(not(target_arch = "wasm32"))]
/// A boxed future that is Send, which is required for buffers that may be used across threads.
/// At least for native code
pub(crate) type BufFut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
/// For wasm we dont require Send, as everything is single-threaded, so this is just a boxed future with no Send bound.
pub(crate) type BufFut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + 'a>>;

#[derive(Debug, Clone)]
/// The result of verifying a signature proposal,
/// indicating whether the claimed address matches the signature
/// and whether the balance is sufficient for the required amount.
pub struct ProposalVerification {
    pub address_matches: bool,
    pub balance_sufficient: bool,
}

#[derive(Debug, Clone)]
/// The data needed to detect and handle a UTXO script on the chain,
/// including the redeem script, its expiration time, and the target address for deposits.
pub struct UtxoScript {
    /// The redeem script that we are monitoring
    pub redeem_script: Vec<u8>,
    /// The unlock time for this swap (its a commitment)
    pub unlock_ts: u64,
    /// The minimum amount that must be deposited to the script for it to be considered valid
    pub deposit_target: String,
}

#[derive(Debug, Clone)]
/// A script announcement, including the address, swap ID, redeem script, expiration time, and deposit target.
pub struct ScriptAnnouncement {
    /// The address associated with the script, which may be used for monitoring or deposits.
    pub address: String,
    /// The unique identifier for the swap, which can be used to correlate on-chain events with off-chain state.
    pub swap_id: [u8; 32],
    /// A utxo script
    pub script: UtxoScript,
}

/// A trait used for onchain data which buffer confirmed onchain data
/// and give us the ability to broadcast events and sign messages on demand
pub(crate) trait ChainDataBuffer: MaybeSend {
    /// Returns the LP address if its configured, will throw error if not configured
    fn lp_address(&self) -> Result<String>;

    /// Computes the deposit address if this channel supports p2sh-like deposits
    fn derive_deposit(&self, _commitment: &CommitmentV1) -> Result<(String, Vec<u8>)> {
        Err(DataError::Other(
            "channel does not support P2SH deposits".into(),
        ))
    }

    /// Retrieves finalized chunk of on-chain events, which are considered irreversible and safe to act upon.
    /// that is of course as long as the operator has used a safe block confirmation threshold
    /// as some operators might take on more risk than others.
    fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>>;

    fn chain_now(&self) -> Option<u64> {
        None
    }

    /// Broadcasts a chain event to the associated chain,
    /// which will be picked up by other nodes monitoring the chain.
    fn broadcast_event<'a>(&'a self, event: &'a ChainEvent) -> BufFut<'a, ()>;

    /// Signs a message digest with the configured private key for this channel, after
    /// verifying that the associated address has sufficient balance to meet the required balance threshold.
    /// This is used for signing CCR proposals to prove on-chain ownership of the LP address and
    /// to ensure that the address has sufficient funds to fulfill the swap in case the proposal is accepted.
    fn sign_message<'a>(
        &'a self,
        digest: [u8; 32],
        required_balance: &'a str,
    ) -> BufFut<'a, (String, Vec<u8>)>;

    /// Verifies a message signature against a claimed address and required balance,
    /// returning whether the signature is valid,   whether the recovered address matches the claimed address,
    /// and whether the balance is sufficient.
    fn verify_message<'a>(
        &'a self,
        digest: [u8; 32],
        claimed_address: &'a str,
        signature: &'a [u8],
        required_balance: &'a str,
    ) -> BufFut<'a, ProposalVerification>;

    /// Returns an optional reference to a UTXO script detector if this channel supports UTXO scripts,
    /// which can be used to register scripts for monitoring and handling.
    fn utxo_script_detector(&self) -> Option<&dyn UtxoScriptDetector> {
        None
    }

    /// Takes all pending UTXO script announcements from the buffer and returns them as a vector.
    fn take_utxo_script_announcements(&self) -> Vec<ScriptAnnouncement> {
        Vec::new()
    }
}

pub(crate) trait UtxoScriptDetector: MaybeSend {
    fn register_script<'a>(
        &'a self,
        address: String,
        redeem_script: Vec<u8>,
        swap_id: [u8; 32],
        unlock_ts: u64,
        deposit_target: String,
    ) -> BufFut<'a, ()>;
}

/// A sink for all chain data across all blockchains
/// The internal buffers implement the same trait as this high level struct
/// and effectively abstract the dfferent chain logic from the rest of the system
pub struct ChainDataSink {
    buffers: AHashMap<ChannelId, Box<dyn ChainDataBuffer>>,
}

impl ChainDataSink {
    pub async fn new(
        channels: AHashMap<ChannelId, (Value, Option<String>)>,
        cursor_store: Option<Arc<dyn CursorStore>>,
    ) -> Result<Self> {
        let mut buffers: AHashMap<ChannelId, Box<dyn ChainDataBuffer>> = AHashMap::new();
        for (channel_id, (cfg, lp_key)) in channels {
            buffers.insert(
                channel_id,
                build_buffer(channel_id, &cfg, lp_key, cursor_store.clone()).await?,
            );
        }
        Ok(Self { buffers })
    }

    pub fn channels(&self) -> impl Iterator<Item = ChannelId> + '_ {
        self.buffers.keys().copied()
    }

    pub fn chain_clock(&self) -> ChainClock {
        let mut times = AHashMap::new();
        for (channel, buffer) in &self.buffers {
            if let Some(ts) = buffer.chain_now() {
                times.insert(*channel, ts);
            }
        }
        ChainClock::new(times)
    }

    pub fn knows_channel(&self, channel_id: ChannelId) -> bool {
        self.buffers.contains_key(&channel_id)
    }

    pub fn script_channel(&self) -> Option<ChannelId> {
        self.buffers
            .iter()
            .find(|(_, b)| b.utxo_script_detector().is_some())
            .map(|(id, _)| *id)
    }

    fn buffer(&self, channel_id: ChannelId) -> Result<&dyn ChainDataBuffer> {
        self.buffers
            .get(&channel_id)
            .map(|b| b.as_ref())
            .ok_or(DataError::UnknownChannel(channel_id))
    }

    pub async fn register_script(
        &self,
        channel_id: ChannelId,
        address: String,
        redeem_script: Vec<u8>,
        swap_id: [u8; 32],
        unlock_ts: u64,
        deposit_target: String,
    ) -> Result<()> {
        match self.utxo_script_detector(channel_id) {
            Some(detector) => {
                detector
                    .register_script(address, redeem_script, swap_id, unlock_ts, deposit_target)
                    .await
            }
            None => Err(DataError::UnknownChannel(channel_id)),
        }
    }
}

impl ChainDataSink {
    pub fn lp_address(&self, channel_id: ChannelId) -> Result<String> {
        self.buffer(channel_id)?.lp_address()
    }

    pub fn derive_deposit(
        &self,
        channel_id: ChannelId,
        commitment: &CommitmentV1,
    ) -> Result<(String, Vec<u8>)> {
        self.buffer(channel_id)?.derive_deposit(commitment)
    }

    pub async fn finalized_chunk(&self) -> Result<Vec<(ChannelId, ChainEvent)>> {
        let mut all = Vec::new();
        for buffer in self.buffers.values() {
            all.extend(buffer.finalized_chunk().await?);
        }
        Ok(all)
    }

    pub async fn broadcast_event(
        &self,
        destination_channel_id: ChannelId,
        event: &ChainEvent,
    ) -> Result<()> {
        self.buffer(destination_channel_id)?
            .broadcast_event(event)
            .await
    }

    pub async fn sign_message(
        &self,
        channel_id: ChannelId,
        digest: [u8; 32],
        required_balance: &str,
    ) -> Result<(String, Vec<u8>)> {
        self.buffer(channel_id)?
            .sign_message(digest, required_balance)
            .await
    }

    pub async fn verify_message(
        &self,
        channel_id: ChannelId,
        digest: [u8; 32],
        claimed_address: &str,
        signature: &[u8],
        required_balance: &str,
    ) -> Result<ProposalVerification> {
        self.buffer(channel_id)?
            .verify_message(digest, claimed_address, signature, required_balance)
            .await
    }

    fn utxo_script_detector(&self, channel_id: ChannelId) -> Option<&dyn UtxoScriptDetector> {
        self.buffers
            .get(&channel_id)
            .and_then(|b| b.utxo_script_detector())
    }

    pub fn take_utxo_script_announcements(&self) -> Vec<ScriptAnnouncement> {
        self.buffers
            .values()
            .flat_map(|b| b.take_utxo_script_announcements())
            .collect()
    }
}
