use sha2::{Digest, Sha256};
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1};
use stroemnet_protocol::{ChainClock, ChannelId};

use crate::{Handler, HandlerError};

#[derive(Debug, Clone)]
/// An onchain event yields an effect in the handler
/// 
/// Currently we have two types of effects, a general broadcast
/// and also the transmit reveal effect which is an effect that is commonly
/// used by users, as a signal that they can now transmit the reveal because
/// the LP has counter locked.
pub enum Effect {
    Broadcast(ChannelId, ChainEvent),
    TransmitReveal(CommitmentV1),
}

impl Handler {
    pub async fn on_chain_event(
        &self,
        source: ChannelId,
        event: ChainEvent,
        clock: &ChainClock,
    ) -> Result<Vec<Effect>, HandlerError> {
        let mut effects = Vec::new();
        match event {
            ChainEvent::Commitment(commitment) => {
                // Handle the external commitment
                self.handle_external_commitment(commitment.clone()).await?;
                // Signal to transmit reveal is ok
                effects.push(Effect::TransmitReveal(commitment.clone()));
                let destination = ChannelId::try_from(commitment.destination)?;
                
                // If the destination is not the source we also need to forward this to
                // to the destination channel as an internal commitment
                if destination != source {
                    match self
                        .handle_internal_commitment(&commitment, destination, clock)
                        .await
                    {
                        // In many cases we get a counter event, so we need to broadcast this onchain
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
            // It was a reveal event
            ChainEvent::Reveal(reveal) => {
                // Handle the external reveal evet
                self.handle_external_reveal(reveal.clone()).await?;


                // Verify the secret and then add it as a broadcast on the other chain
                // so that we can finalize the swap
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
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::test_fixtures::{create_test_handler_with, default_test_config, lp_addresses};
    use stroemnet_protocol::v1::{AddressesV1, AmountV1};

    #[tokio::test]
    async fn commitment_event_emits_transmit_reveal() {
        let (handler, _t) = create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let commitment = CommitmentV1::new(
            [1u8; 32],
            AddressesV1::new("0xUser".into(), "0xMm".into(), "kaspa:dest".into()),
            AmountV1::new("1000000000000000000".into(), 18),
            [0xEE; 32],
            now + 3600,
            ChannelId::EthereumSepolia as u8,
            ChannelId::KaspaTn10 as u8,
        );
        let effects = handler
            .on_chain_event(
                ChannelId::EthereumSepolia,
                ChainEvent::Commitment(commitment),
                &ChainClock::default(),
            )
            .await
            .unwrap();
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::TransmitReveal(_)))
        );
    }
}
