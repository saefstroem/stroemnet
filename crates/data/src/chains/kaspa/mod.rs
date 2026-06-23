mod broadcast;
mod contracts;
mod decode;
mod error;
mod intake;
mod signing;
#[cfg(test)]
mod test_helpers;

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::prelude::{NetworkId, RpcBlock};
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use serde::Deserialize;
use serde_json::Value;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1, RefundV1, RevealV1};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

use crate::{
    BufFut, ChainDataBuffer, CursorStore, DataError, ProposalVerification, Result,
    ScriptAnnouncement, SwapStore, UtxoScript, UtxoScriptDetector,
};

const DEFAULT_COINBASE_MATURITY: u64 = 100;
const DEFAULT_MINIMUM_BLOCK_CONFIRMATIONS: u64 = 10 * (60 * 10);
const DEFAULT_SCRIPT_TTL_SECS: u64 = 4 * 60 * 60;

#[derive(Deserialize)]
struct KaspaConfig {
    #[serde(default)]
    wrpc_url: Option<String>,
    network_id: String,
    #[serde(default = "default_min_confirmations")]
    minimum_block_confirmations: u64,
    #[serde(default = "default_coinbase_maturity")]
    coinbase_maturity: u64,
    #[serde(default = "default_script_ttl_secs")]
    script_ttl_secs: u64,
    #[serde(default)]
    participate_ccr: bool,
}

fn default_min_confirmations() -> u64 {
    DEFAULT_MINIMUM_BLOCK_CONFIRMATIONS
}
fn default_coinbase_maturity() -> u64 {
    DEFAULT_COINBASE_MATURITY
}
fn default_script_ttl_secs() -> u64 {
    DEFAULT_SCRIPT_TTL_SECS
}

type PersistedKaspaState = (
    AHashMap<[u8; 32], CommitmentV1>,
    AHashMap<[u8; 32], UtxoScript>,
    Vec<(RefundV1, u64)>,
    Vec<RevealV1>,
);

fn load_persisted_state(
    swap_store: Option<&Arc<dyn SwapStore>>,
    channel_id: ChannelId,
) -> PersistedKaspaState {
    let mut commitments = AHashMap::new();
    let mut scripts = AHashMap::new();
    let mut pending_refunds = Vec::new();
    let mut pending_claims = Vec::new();
    let Some(store) = swap_store else {
        return (commitments, scripts, pending_refunds, pending_claims);
    };
    for (swap_id, bytes) in store.load_channel(channel_id) {
        match borsh::from_slice::<crate::PersistedSwap>(&bytes) {
            Ok(rec) => {
                if let Some(c) = rec.commitment {
                    commitments.insert(swap_id, c);
                }
                if let Some(s) = rec.script {
                    scripts.insert(swap_id, s);
                }
                if let Some(pr) = rec.pending_refund {
                    pending_refunds.push(pr);
                }
                if let Some(pc) = rec.pending_claim {
                    pending_claims.push(pc);
                }
            }
            Err(e) => tracing::warn!("kaspa seed swap {}: {e}", hex::encode(swap_id)),
        }
    }
    (commitments, scripts, pending_refunds, pending_claims)
}

pub(crate) struct Kaspa {
    channel_id: ChannelId,
    network_id: String,
    prefix: Prefix,
    coinbase_maturity: u64,
    script_ttl_secs: u64,
    participate_ccr: bool,
    private_key: Option<String>,
    client: Arc<KaspaRpcClient>,
    utxo_scripts: Arc<RwLock<AHashMap<String, UtxoScript>>>,
    safe_blocks: Mutex<Receiver<Arc<RpcBlock>>>,
    commitments: Mutex<AHashMap<[u8; 32], CommitmentV1>>,
    pending_refunds: Mutex<Vec<(RefundV1, u64)>>,
    pending_claims: Mutex<Vec<RevealV1>>,
    announcements: Mutex<Vec<ScriptAnnouncement>>,
    scripts: Mutex<AHashMap<[u8; 32], UtxoScript>>,
    swap_store: Option<Arc<dyn SwapStore>>,
}

impl Kaspa {
    pub(crate) async fn connect(
        channel_id: ChannelId,
        cfg: &Value,
        private_key: Option<String>,
        cursor_store: Option<Arc<dyn CursorStore>>,
        swap_store: Option<Arc<dyn SwapStore>>,
    ) -> Result<Self> {
        let cfg: KaspaConfig = serde_json::from_value(cfg.clone())
            .map_err(|e| DataError::Config(format!("kaspa config: {e}")))?;
        let network_id = NetworkId::from_str(&cfg.network_id)
            .map_err(|e| DataError::Config(format!("network_id: {e:?}")))?;
        let prefix: Prefix = network_id.into();
        let resolver = match cfg.wrpc_url.as_deref() {
            Some(_) => None,
            None => Some(Resolver::default()),
        };
        let client = Arc::new(
            KaspaRpcClient::new(
                WrpcEncoding::Borsh,
                cfg.wrpc_url.as_deref(),
                resolver,
                Some(network_id),
                None,
            )
            .map_err(|e| DataError::Connect(format!("wrpc client: {e}")))?,
        );
        client
            .connect(None)
            .await
            .map_err(|e| DataError::Connect(format!("kaspa connect: {e}")))?;

        // Retrieve the initial cursor from the cursor store if available, and convert it to a Hash
        let initial_cursor = cursor_store
            .as_ref()
            .and_then(|s| s.load(channel_id))
            .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
            .map(Hash::from_bytes);

        let (tx, rx) = tokio::sync::mpsc::channel::<Arc<RpcBlock>>(1024);
        let mut reader = intake::Intake::new(
            client.clone(),
            tx,
            cfg.minimum_block_confirmations,
            channel_id,
            initial_cursor,
            cursor_store,
        );
        stroemnet_protocol::spawn(async move {
            if let Err(e) = reader.read().await {
                tracing::error!("kaspa intake loop terminated: {e}");
            }
        });

        tracing::info!(
            "Kaspa buffer {channel_id} connected to {:?} (confirmations {}, ccr {})",
            client.url(),
            cfg.minimum_block_confirmations,
            cfg.participate_ccr,
        );

        let (commitments, scripts, pending_refunds, pending_claims) =
            load_persisted_state(swap_store.as_ref(), channel_id);
        if !commitments.is_empty() || !pending_refunds.is_empty() || !pending_claims.is_empty() {
            tracing::info!(
                "Kaspa buffer {channel_id} restored {} commitment(s), {} script(s), {} refund(s), {} claim(s) from store",
                commitments.len(),
                scripts.len(),
                pending_refunds.len(),
                pending_claims.len(),
            );
        }

        Ok(Self {
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
            pending_refunds: Mutex::new(pending_refunds),
            pending_claims: Mutex::new(pending_claims),
            announcements: Mutex::new(Vec::new()),
            scripts: Mutex::new(scripts),
            swap_store,
        })
    }

    fn key(&self) -> Result<&str> {
        self.private_key
            .as_deref()
            .ok_or(DataError::MissingKey(self.channel_id))
    }

    fn cache_commitment(&self, commitment: &CommitmentV1) {
        self.commitments
            .lock()
            .unwrap()
            .insert(commitment.swap_id, commitment.clone());
    }

    fn commitment(&self, swap_id: &[u8; 32]) -> Option<CommitmentV1> {
        self.commitments.lock().unwrap().get(swap_id).cloned()
    }

    fn script(&self, swap_id: &[u8; 32]) -> Option<UtxoScript> {
        self.scripts.lock().unwrap().get(swap_id).cloned()
    }

    fn persist_swap(&self, swap_id: [u8; 32]) {
        let Some(store) = &self.swap_store else {
            return;
        };
        let commitment = self.commitments.lock().unwrap().get(&swap_id).cloned();
        let pending_refund = self
            .pending_refunds
            .lock()
            .unwrap()
            .iter()
            .find(|(r, _)| r.swap_id == swap_id)
            .map(|(r, ts)| (r.clone(), *ts));
        let pending_claim = self
            .pending_claims
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.swap_id == swap_id)
            .cloned();
        let script = self.scripts.lock().unwrap().get(&swap_id).cloned();
        let record = crate::PersistedSwap {
            commitment,
            script,
            pending_refund,
            pending_claim,
        };
        if record.is_empty() {
            store.delete(self.channel_id, swap_id);
        } else {
            match borsh::to_vec(&record) {
                Ok(bytes) => store.save(self.channel_id, swap_id, &bytes),
                Err(e) => tracing::warn!("kaspa persist swap {} encode: {e}", hex::encode(swap_id)),
            }
        }
    }

    fn track_actionable_event(&self, event: &ChainEvent) {
        let swap_id = match event {
            ChainEvent::Commitment(c) => c.swap_id,
            ChainEvent::Reveal(r) => r.swap_id,
            ChainEvent::Refund(r) => r.swap_id,
        };
        if let ChainEvent::Commitment(c) = event {
            self.cache_commitment(c);
        }
        super::queue_dequeue_refund_event(
            &mut self.pending_refunds.lock().unwrap(),
            event,
            self.participate_ccr,
        );
        match event {
            ChainEvent::Reveal(_) | ChainEvent::Refund(_) => {
                self.pending_claims
                    .lock()
                    .unwrap()
                    .retain(|c| c.swap_id != swap_id);
                self.commitments.lock().unwrap().remove(&swap_id);
                self.scripts.lock().unwrap().remove(&swap_id);
            }
            ChainEvent::Commitment(_) => {}
        }
        self.persist_swap(swap_id);
    }

    async fn prune_scripts(&self) {
        let now = now_unix_secs();
        let ttl = self.script_ttl_secs;
        let mut scripts = self.utxo_scripts.write().await;
        scripts.retain(|_, s| now <= s.unlock_ts.saturating_add(ttl));
    }

    async fn register_internal(&self, address: String, script: UtxoScript) {
        self.utxo_scripts.write().await.insert(address, script);
    }

    async fn run_refund_scheduler(&self) {
        if !self.participate_ccr || self.private_key.is_none() {
            return;
        }
        let has_pending = { !self.pending_refunds.lock().unwrap().is_empty() };
        if !has_pending {
            return;
        }
        let pmt = match self.client.get_block_dag_info().await {
            Ok(info) => info.past_median_time,
            Err(e) => {
                tracing::warn!("kaspa refund scheduler: get_block_dag_info: {e}");
                return;
            }
        };
        let ready: Vec<[u8; 32]> = {
            let pending = self.pending_refunds.lock().unwrap();
            pending
                .iter()
                .filter(|(_, unlock_ts)| pmt > unlock_ts.saturating_mul(1000))
                .map(|(r, _)| r.swap_id)
                .collect()
        };
        for swap_id in ready {
            let stored = self.script(&swap_id);
            let remove = match self.commitment(&swap_id) {
                None => true,
                Some(commitment) => match broadcast::submit_refund(
                    &self.client,
                    self.key().unwrap_or_default(),
                    self.coinbase_maturity,
                    &commitment,
                    stored.as_ref().map(|s| s.redeem_script.as_slice()),
                )
                .await
                {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::error!("kaspa scheduled refund {}: {e}", hex::encode(swap_id));
                        false
                    }
                },
            };
            if remove {
                self.pending_refunds
                    .lock()
                    .unwrap()
                    .retain(|(r, _)| r.swap_id != swap_id);
                self.persist_swap(swap_id);
            }
        }
    }

    async fn run_claim_scheduler(&self) {
        if !self.participate_ccr || self.private_key.is_none() {
            return;
        }
        let claims: Vec<RevealV1> = { self.pending_claims.lock().unwrap().clone() };
        for reveal in claims {
            let Some(commitment) = self.commitment(&reveal.swap_id) else {
                continue;
            };
            let stored = self.script(&reveal.swap_id);
            match broadcast::submit_reveal(
                &self.client,
                self.key().unwrap_or_default(),
                self.coinbase_maturity,
                &commitment,
                &reveal,
                stored.as_ref().map(|s| s.redeem_script.as_slice()),
            )
            .await
            {
                Ok(()) => {
                    self.pending_claims
                        .lock()
                        .unwrap()
                        .retain(|c| c.swap_id != reveal.swap_id);
                    self.persist_swap(reveal.swap_id);
                }
                Err(e) => {
                    tracing::error!("kaspa claim retry for {}: {e}", hex::encode(reveal.swap_id))
                }
            }
        }
    }
}

impl ChainDataBuffer for Kaspa {
    fn lp_address(&self) -> Result<String> {
        Ok(signing::lp_address_from_private_key(
            &self.network_id,
            self.key()?,
        )?)
    }

    fn derive_deposit(&self, commitment: &CommitmentV1) -> Result<(String, Vec<u8>)> {
        Ok(signing::p2sh_components(&self.network_id, commitment)?)
    }

    fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>> {
        Box::pin(async move {
            let blocks: Vec<Arc<RpcBlock>> = {
                let mut rx = self.safe_blocks.lock().unwrap();
                let mut v = Vec::new();
                while let Ok(block) = rx.try_recv() {
                    v.push(block);
                }
                v
            };

            let mut events = Vec::new();
            for block in blocks {
                let outcomes = decode::handle_block_added(
                    &block,
                    &self.utxo_scripts,
                    self.prefix,
                    self.channel_id,
                )
                .await?;
                if self.participate_ccr {
                    let mut pushed = Vec::new();
                    {
                        let mut pending = self.pending_refunds.lock().unwrap();
                        for (swap_id, unlock_ts) in outcomes.refunds {
                            if !pending.iter().any(|(r, _)| r.swap_id == swap_id) {
                                pending.push((RefundV1::new(swap_id), unlock_ts));
                                pushed.push(swap_id);
                            }
                        }
                    }
                    for swap_id in pushed {
                        self.persist_swap(swap_id);
                    }
                }
                for event in outcomes.events {
                    self.track_actionable_event(&event);
                    events.push((self.channel_id, event));
                }
            }

            self.prune_scripts().await;
            self.run_refund_scheduler().await;
            self.run_claim_scheduler().await;
            Ok(events)
        })
    }

    fn broadcast_event<'a>(&'a self, event: &'a ChainEvent) -> BufFut<'a, ()> {
        Box::pin(async move {
            match event {
                ChainEvent::Commitment(c) => {
                    self.cache_commitment(c);
                    let announce = broadcast::submit_commitment(
                        &self.client,
                        self.key()?,
                        self.coinbase_maturity,
                        c,
                    )
                    .await?;
                    let script = UtxoScript {
                        redeem_script: announce.redeem_script,
                        unlock_ts: c.unlock_ts,
                        deposit_target: c.amount.value.clone(),
                    };
                    self.scripts
                        .lock()
                        .unwrap()
                        .insert(c.swap_id, script.clone());
                    self.register_internal(announce.address.clone(), script.clone())
                        .await;
                    self.announcements.lock().unwrap().push(ScriptAnnouncement {
                        address: announce.address,
                        swap_id: c.swap_id,
                        script,
                    });
                    self.persist_swap(c.swap_id);
                    Ok(())
                }
                ChainEvent::Reveal(r) => {
                    if self.participate_ccr && self.commitment(&r.swap_id).is_some() {
                        {
                            let mut pending = self.pending_claims.lock().unwrap();
                            if !pending.iter().any(|c| c.swap_id == r.swap_id) {
                                pending.push(r.clone());
                            }
                        }
                        self.persist_swap(r.swap_id);
                    }
                    Ok(())
                }
                ChainEvent::Refund(r) => {
                    if !self.participate_ccr {
                        return Ok(());
                    }
                    let commitment =
                        self.commitment(&r.swap_id).ok_or(DataError::Other(format!(
                            "kaspa refund: unknown commitment for swap {}",
                            hex::encode(r.swap_id)
                        )))?;
                    let stored = self.script(&r.swap_id);
                    broadcast::submit_refund(
                        &self.client,
                        self.key()?,
                        self.coinbase_maturity,
                        &commitment,
                        stored.as_ref().map(|s| s.redeem_script.as_slice()),
                    )
                    .await
                    .map_err(DataError::from)
                }
            }
        })
    }

    fn sign_message<'a>(
        &'a self,
        digest: [u8; 32],
        required_balance: &'a str,
    ) -> BufFut<'a, (String, Vec<u8>)> {
        Box::pin(async move {
            let required: u64 = required_balance
                .parse()
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;
            signing::sign_message(
                &self.client,
                &self.network_id,
                self.key()?,
                digest,
                required,
            )
            .await
            .map_err(DataError::from)
        })
    }

    fn verify_message<'a>(
        &'a self,
        digest: [u8; 32],
        claimed_address: &'a str,
        signature: &'a [u8],
        required_balance: &'a str,
    ) -> BufFut<'a, ProposalVerification> {
        Box::pin(async move {
            let required: u64 = required_balance
                .parse()
                .map_err(|e| DataError::Sign(format!("required_balance: {e}")))?;
            signing::verify_message(&self.client, digest, claimed_address, signature, required)
                .await
                .map_err(DataError::from)
        })
    }

    fn utxo_script_detector(&self) -> Option<&dyn UtxoScriptDetector> {
        Some(self)
    }

    fn take_utxo_script_announcements(&self) -> Vec<ScriptAnnouncement> {
        std::mem::take(&mut self.announcements.lock().unwrap())
    }
}

impl UtxoScriptDetector for Kaspa {
    fn register_script<'a>(
        &'a self,
        address: String,
        redeem_script: Vec<u8>,
        swap_id: [u8; 32],
        unlock_ts: u64,
        deposit_target: String,
    ) -> BufFut<'a, ()> {
        Box::pin(async move {
            signing::validate_script_announce(
                &self.network_id,
                self.channel_id,
                &address,
                &redeem_script,
                swap_id,
                unlock_ts,
            )?;
            let script = UtxoScript {
                redeem_script,
                unlock_ts,
                deposit_target,
            };
            self.scripts.lock().unwrap().insert(swap_id, script.clone());
            self.register_internal(address, script).await;
            self.prune_scripts().await;
            self.persist_swap(swap_id);
            Ok(())
        })
    }
}
