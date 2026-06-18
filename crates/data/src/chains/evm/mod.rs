mod broadcast;
mod contracts;
mod decode;
mod finality;
mod signing;

use std::sync::{Arc, Mutex};

use alloy::primitives::{Address, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use serde::Deserialize;
use serde_json::Value;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::{ChainEvent, RefundV1, RevealV1};

use crate::{BufFut, ChainDataBuffer, DataError, ProposalVerification, Result};
use finality::{DEFAULT_MAX_BLOCKS_PER_POLL, DEFAULT_POLL_INTERVAL_MS, PollState};

#[derive(Deserialize, Clone, Copy, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GasPayment {
    #[default]
    Eip1559,
    Legacy,
}

#[derive(Deserialize)]
/// Configuration for the EVM chain data buffer
struct EvmConfig {
    /// The RPC URL of the EVM node to connect to
    rpc_url: String,
    /// The address of the HTLC contract to monitor and interact with
    htlc_address: String,
    #[serde(default)]
    /// The number of block confirmations required before considering an event final
    minimum_block_confirmations: u64,
    #[serde(default = "default_poll_interval_ms")]
    /// The interval in milliseconds between polling the chain for new events
    poll_interval_ms: u64,
    #[serde(default = "default_max_blocks_per_poll")]
    /// The maximum number of blocks to query in each poll
    max_blocks_per_poll: u64,
    #[serde(default)]
    /// Whether to participate in CCR by submitting claims
    /// and refunds on the destination chain
    participate_ccr: bool,
    #[serde(default)]
    gas_payment: GasPayment,
}

fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

fn default_max_blocks_per_poll() -> u64 {
    DEFAULT_MAX_BLOCKS_PER_POLL
}

/// The state of the evm from the perspective of polling
struct EvmState {
    poll: PollState,
    pending_refunds: Vec<(RefundV1, u64)>,
    pending_claims: Vec<RevealV1>,
    next_poll_secs: u64,
    last_block_ts: Option<(u64, u64)>,
}

/// The main Evm struct that polls and emits confirmed data
pub(crate) struct Evm {
    channel_id: ChannelId,                // the channel that it operates on
    htlc_address: Address,                // the address of the htlc contract
    minimum_block_confirmations: u64, // the number of confirmations required before considering an event final
    poll_interval_secs: u64, // the interval in seconds between polling the chain for new events
    max_blocks_per_poll: u64, // the maximum number of blocks to query in each poll
    participate_ccr: bool,   // whether to participate in CCR
    gas_payment: GasPayment, // how to price transactions (eip1559 default, or legacy)
    private_key: Option<String>, // private key (only for LP)
    read_provider: DynProvider, // provider for reading from the chain
    signed_provider: Option<DynProvider>, // provider for signing and broadcasting transactions (only for LP)
    state: Mutex<EvmState>, // the state of the evm buffer, including the poll state and pending refunds
    cursor_store: Option<Arc<dyn crate::CursorStore>>, // optional cursor store for persisting the polling state (for native)
}

impl Evm {
    /// Connects to the EVM chain using the provided configuration
    /// and optional private key for signing transactions
    /// Returns an instance of the EVM buffer ready to poll for
    /// events and broadcast transactions
    pub(crate) async fn connect(
        channel_id: ChannelId,
        cfg: &Value,
        private_key: Option<String>,
        cursor_store: Option<Arc<dyn crate::CursorStore>>,
    ) -> Result<Self> {
        // Parse the configuration from the provided JSON value
        let cfg: EvmConfig = serde_json::from_value(cfg.clone())
            .map_err(|e| DataError::Config(format!("evm config: {e}")))?;

        // Parse the HTLC contract address from the configuration
        let htlc_address: Address = cfg
            .htlc_address
            .parse()
            .map_err(|e| DataError::Config(format!("htlc_address: {e}")))?;

        // Instantiate a provider for reading from the chain and another for signing transactions if a private key is provided
        let read_provider = ProviderBuilder::new()
            .connect(&cfg.rpc_url)
            .await
            .map_err(|e| DataError::Connect(format!("evm provider: {e}")))?
            .erased();

        // If a private key is provided, create a signed provider for broadcasting transactions
        let signed_provider = match &private_key {
            Some(pk) => {
                let signer: PrivateKeySigner = pk
                    .parse()
                    .map_err(|e| DataError::Config(format!("private_key: {e}")))?;
                Some(
                    ProviderBuilder::new()
                        .wallet(signer)
                        .connect(&cfg.rpc_url)
                        .await
                        .map_err(|e| DataError::Connect(format!("evm signed provider: {e}")))?
                        .erased(),
                )
            }
            None => None,
        };

        // Calculate the initial cursor for polling based on the current chain head and the required number of confirmations
        let head = read_provider
            .get_block_number()
            .await
            .map_err(|e| DataError::Connect(format!("get_block_number: {e}")))?;

        // If a cursor store is provided, attempt to load the last saved cursor for this channel
        let cursor = match cursor_store
            .as_ref()
            .and_then(|s| s.load(channel_id))
            .filter(|b| b.len() == 8)
        {
            // convert the cursor from bytes to u64
            Some(bytes) => u64::from_le_bytes(bytes.try_into().unwrap()),
            None => {
                head // otherwise take the head-minimum_block_confirmations+1 as the starting cursor
                    .saturating_sub(cfg.minimum_block_confirmations)
                    .saturating_add(1)
            }
        };

        tracing::info!(
            "EVM buffer {channel_id} connected to {} — polling from block {cursor} (confirmations {}, ccr {})",
            cfg.rpc_url,
            cfg.minimum_block_confirmations,
            cfg.participate_ccr,
        );

        // Return a new instance of the EVM buffer with
        // the initialized state and providers
        Ok(Self {
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
                pending_refunds: Vec::new(),
                pending_claims: Vec::new(),
                next_poll_secs: 0,
                last_block_ts: None,
            }),
            cursor_store,
        })
    }

    /// Notify that an event should be tracked
    fn track_actionable_event(&self, event: &ChainEvent) {
        let mut st = self.state.lock().unwrap();
        super::queue_dequeue_refund_event(&mut st.pending_refunds, event, self.participate_ccr);
        match event {
            ChainEvent::Reveal(r) => st.pending_claims.retain(|c| c.swap_id != r.swap_id),
            ChainEvent::Refund(r) => st.pending_claims.retain(|c| c.swap_id != r.swap_id),
            ChainEvent::Commitment(_) => {}
        }
    }

    /// Retrieve the signer provider
    fn signed(&self) -> Result<&DynProvider> {
        self.signed_provider
            .as_ref()
            .ok_or(DataError::MissingKey(self.channel_id))
    }

    /// Check if any pending refunds are ready to be submitted and submit them if so
    /// This is used to automatically submit refunds for swaps that have passed
    /// their unlock timestamp without a reveal
    async fn run_refund_scheduler(&self) {
        // If CCR participation is disabled or there is no signing provider,
        // skip the refund scheduler
        if !self.participate_ccr {
            return;
        }

        // Retrieve the signed provider and return early if its not configured
        let Some(signed) = self.signed_provider.as_ref() else {
            return;
        };

        // Check if there are any pending refunds, and if not, skip the rest of the function
        let has_pending = { !self.state.lock().unwrap().pending_refunds.is_empty() };
        if !has_pending {
            return;
        }

        // Retrieve the current block timestamp to use for checking which refunds
        // are ready to be submitted.
        let Some(block_ts) = broadcast::current_block_timestamp(&self.read_provider).await else {
            return;
        };

        // Compute the ready swap ids
        let ready: Vec<[u8; 32]> = {
            let st = self.state.lock().unwrap();
            // Collect the swap ids of all pending refunds whose unlock timestamp has passed
            st.pending_refunds
                .iter()
                .filter(|(_, unlock_ts)| block_ts >= *unlock_ts)
                .map(|(r, _)| r.swap_id)
                .collect()
        };

        // Loop over each swap id and attempt to submit a refund transaction for it,
        // logging any errors that occur
        for swap_id in ready {
            match broadcast::submit_refund(signed, self.htlc_address, swap_id, self.gas_payment).await
            {
                Ok(_) => {
                    self.state
                        .lock()
                        .unwrap()
                        .pending_refunds
                        .retain(|(r, _)| r.swap_id != swap_id);
                }
                Err(e) => {
                    tracing::error!("EVM scheduled refund {}: {e}", hex::encode(swap_id));
                }
            }
        }
    }

    async fn run_claim_scheduler(&self) {
        if !self.participate_ccr {
            return;
        }
        let Some(signed) = self.signed_provider.as_ref() else {
            return;
        };
        let claims: Vec<RevealV1> = { self.state.lock().unwrap().pending_claims.clone() };
        for reveal in claims {
            match broadcast::submit_claim(signed, self.htlc_address, &reveal, self.gas_payment).await
            {
                Ok(()) => {
                    self.state
                        .lock()
                        .unwrap()
                        .pending_claims
                        .retain(|c| c.swap_id != reveal.swap_id);
                }
                Err(e) => {
                    tracing::error!("EVM claim retry for {}: {e}", hex::encode(reveal.swap_id));
                }
            }
        }
    }
}

impl ChainDataBuffer for Evm {
    /// Returns the LP address derived from the configured private key, or an error if no private key is configured
    fn lp_address(&self) -> Result<String> {
        let pk = self
            .private_key
            .as_deref()
            .ok_or(DataError::MissingKey(self.channel_id))?;
        signing::address_from_private_key(pk)
    }

    /// Finalizes a chunk by polling the chain for new events since the last cursor,
    /// decoding them, and returning them as a vector of (ChannelId, ChainEvent) tuples
    /// For ethereum this method is already reorg safe as we only poll blocks behind
    /// the required confirmation threshold
    fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>> {
        Box::pin(async move {
            let now = now_unix_secs();
            // Check if it's time to poll the chain for new events based on the configured polling interval
            let mut poll = {
                let mut st = self.state.lock().unwrap();
                if now < st.next_poll_secs {
                    None
                } else {
                    st.next_poll_secs = now + self.poll_interval_secs;
                    Some(st.poll)
                }
            };

            // Create a container for events
            let mut events = Vec::new();

            // If its time to poll, lets poll.
            if let Some(poll) = poll.as_mut() {
                // Pull all logs according to the pollstate
                let logs = poll
                    .poll_once(
                        &self.read_provider,
                        self.htlc_address,
                        self.minimum_block_confirmations,
                        self.max_blocks_per_poll,
                    )
                    .await;
                {
                    self.state.lock().unwrap().poll = *poll;
                }

                // If we have a cursor store, save the current cursor to it for persistence
                if let Some(store) = &self.cursor_store {
                    store.save(self.channel_id, &poll.cursor.to_le_bytes());
                }

                // For each log
                for log in &logs {
                    if let Some(event) = decode::decode_log(log, self.channel_id) {
                        // If we could decode it as a chain event, we queue or dequeue any relevant refunds
                        self.track_actionable_event(&event);
                        // Push it as an event
                        events.push((self.channel_id, event));
                    }
                }

                if let Some(ts) = broadcast::current_block_timestamp(&self.read_provider).await {
                    self.state.lock().unwrap().last_block_ts = Some((ts, now_unix_secs()));
                }
            }

            // After processing the logs we run the refund scheduler to submit any refunds that are ready to be submitted
            self.run_refund_scheduler().await;
            self.run_claim_scheduler().await;
            Ok(events)
        })
    }

    fn chain_now(&self) -> Option<u64> {
        let max_age = self.poll_interval_secs.saturating_mul(3).max(30);
        let (ts, observed) = self.state.lock().unwrap().last_block_ts?;
        if now_unix_secs().saturating_sub(observed) > max_age {
            return None;
        }
        Some(ts)
    }

    /// Broadcasts an incoming event by routing it based on the type of chainevent
    fn broadcast_event<'a>(&'a self, event: &'a ChainEvent) -> BufFut<'a, ()> {
        Box::pin(async move {
            match event {
                ChainEvent::Commitment(c) => {
                    broadcast::submit_commitment(self.signed()?, self.htlc_address, c, self.gas_payment).await
                }
                ChainEvent::Reveal(r) => {
                    if self.participate_ccr {
                        let mut st = self.state.lock().unwrap();
                        let known = st.pending_refunds.iter().any(|(p, _)| p.swap_id == r.swap_id);
                        let queued = st.pending_claims.iter().any(|c| c.swap_id == r.swap_id);
                        if known && !queued {
                            st.pending_claims.push(r.clone());
                        }
                    }
                    Ok(())
                }
                ChainEvent::Refund(r) => {
                    if self.participate_ccr {
                        // we only submit refunds if we participate in CCR
                        broadcast::submit_refund(self.signed()?, self.htlc_address, r.swap_id, self.gas_payment).await
                    } else {
                        Ok(())
                    }
                }
            }
        })
    }

    /// Signs a message digest using the configured private key and returns the signature bytes
    /// but also ensures that the signer has the required balance
    fn sign_message<'a>(
        &'a self,
        digest: [u8; 32],
        required_balance: &'a str,
    ) -> BufFut<'a, (String, Vec<u8>)> {
        Box::pin(async move {
            let pk = self
                .private_key
                .as_deref()
                .ok_or(DataError::MissingKey(self.channel_id))?;
            let required = U256::from_str_radix(required_balance, 10)
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;
            signing::sign_message(&self.read_provider, pk, digest, required).await
        })
    }

    /// Verifies a message signature by recovering the signer address and comparing it to the claimed address,
    /// and also checks that the signer has the required balance
    fn verify_message<'a>(
        &'a self,
        digest: [u8; 32],
        claimed_address: &'a str,
        signature: &'a [u8],
        required_balance: &'a str,
    ) -> BufFut<'a, ProposalVerification> {
        Box::pin(async move {
            let required = U256::from_str_radix(required_balance, 10)
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;
            // Verify the message signature and
            // return whether the recovered address matches the claimed address, and whether the required balance is met
            signing::verify_message(
                &self.read_provider,
                digest,
                claimed_address,
                signature,
                required,
            )
            .await
        })
    }
}
