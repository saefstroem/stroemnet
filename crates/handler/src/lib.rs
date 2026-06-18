mod address;
pub mod error;
pub mod get;
pub mod handle;
pub mod result;

#[cfg(test)]
mod test_fixtures;

use std::sync::Arc;

use ahash::AHashMap;
use sha2::{Digest, Sha256};
use stroemnet_amounts::PriceStorage;
use stroemnet_protocol::swap_tracker::SwapTracker;
use stroemnet_protocol::{ChainClock, ChannelId};
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1};
use tokio::sync::RwLock;

pub use address::normalised_address_eq;
pub use error::HandlerError;

pub fn required_init_lock_secs(destination: ChannelId, commit_buffer_secs: u64) -> u64 {
    2 * destination.finality_secs() + commit_buffer_secs
}

#[derive(Debug, Clone)]
pub struct HandlerConfig {
    /// Minimum trade size in USD, which is used to prevent spam and uneconomic trades
    pub min_trade_usd: f64,
    /// Maximum trade size in USD, which is used to prevent large trades that may be too risky for the LP
    pub max_trade_usd: f64,
    /// The percentage spread that the LP applies to the trade, which is how the LP makes money on each swap
    pub spread_percent: f64,
    /// The number of seconds before the required init lock time that we require for a trade,
    /// this is not related to chain finality but rather an additional buffer,
    /// based on network delay etc.
    pub commit_buffer_secs: u64,
}

#[derive(Debug)]
pub struct Handler {
    pub price_storage: PriceStorage,
    pub swap_tracker: Arc<RwLock<SwapTracker>>,
    pub config: HandlerConfig,

    pub address_lookup_table: Arc<AHashMap<ChannelId, String>>,
    pub block_confirmations: Arc<AHashMap<ChannelId, u64>>,
}

#[derive(Debug, Clone)]
pub struct DetectedCommitment {
    pub commitment: CommitmentV1,
}

#[derive(Debug, Clone)]
pub enum Effect {
    Broadcast(ChannelId, ChainEvent),
    TransmitReveal(DetectedCommitment),
}

impl Handler {
    pub fn knows_channel(&self, id: ChannelId) -> bool {
        self.block_confirmations.contains_key(&id)
    }

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

    pub async fn on_chain_event(
        &self,
        source: ChannelId,
        event: ChainEvent,
        clock: &ChainClock,
    ) -> Result<Vec<Effect>, HandlerError> {
        let mut effects = Vec::new();
        match event {
            ChainEvent::Commitment(commitment) => {
                self.handle_external_commitment(commitment.clone()).await?;
                effects.push(Effect::TransmitReveal(DetectedCommitment {
                    commitment: commitment.clone(),
                }));
                let destination = ChannelId::try_from(commitment.destination)?;
                if destination != source {
                    match self
                        .handle_internal_commitment(&commitment, destination, clock)
                        .await
                    {
                        Ok(counter) => {
                            effects.push(Effect::Broadcast(
                                destination,
                                ChainEvent::Commitment(counter),
                            ));
                        }
                        Err(HandlerError::NotAddressedToUs(_))
                        | Err(HandlerError::InvalidState(_)) => {}
                        Err(e) => tracing::warn!("internal commitment: {e}"),
                    }
                }
            }
            ChainEvent::Reveal(reveal) => {
                self.handle_external_reveal(reveal.clone()).await?;
                if let Some(counterparty) = self
                    .get_counterparty_channel_id(&reveal.swap_id, source)
                    .await?
                    && let Some(commitment) = self
                        .get_commitment_for_channel(&reveal.swap_id, counterparty)
                        .await?
                    && Sha256::digest(reveal.secret).as_slice() == commitment.secret_hash
                {
                    effects.push(Effect::Broadcast(counterparty, ChainEvent::Reveal(reveal)));
                }
            }
            ChainEvent::Refund(refund) => {
                self.handle_external_refund(refund).await?;
            }
        }
        Ok(effects)
    }
}

#[cfg(test)]
mod handler_tests {
    use super::*;

    #[test]
    fn required_init_lock_includes_propagation_margin() {
        let v = required_init_lock_secs(ChannelId::KaspaTn10, 60);
        assert_eq!(v, 2 * ChannelId::KaspaTn10.finality_secs() + 60);

        let v = required_init_lock_secs(ChannelId::EthereumSepolia, 30);
        assert_eq!(v, 2 * ChannelId::EthereumSepolia.finality_secs() + 30);
    }
}
