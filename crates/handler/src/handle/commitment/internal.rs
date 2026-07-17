use crate::result::Result;
use crate::{Handler, HandlerError, normalised_address_eq};
use stroemnet_protocol::v1::CommitmentV1;
use stroemnet_protocol::{ChainClock, ChannelId};

impl Handler {
    /// Handle an internval commitment that was sent to us via an internal channel from one
    /// of our channels
    pub async fn handle_internal_commitment(
        &self,
        commitment: &CommitmentV1,
        channel_id: ChannelId,
        clock: &ChainClock,
    ) -> Result<CommitmentV1> {
        // Try and retrieve the init commitment
        let init_commitment = {
            let tracker = self.swap_tracker.read().await;
            let record = tracker
                .get_swap(&commitment.swap_id)
                .ok_or(HandlerError::SwapNotFound(commitment.swap_id))?;
            if record.counter_commitment.is_some() || record.resolution.is_some() {
                return Err(HandlerError::InvalidState(commitment.swap_id));
            }
            record.init_commitment.clone()
        };

        // Parse the source and destination channels
        let source_channel = ChannelId::try_from(init_commitment.source)?;
        let destination_channel = ChannelId::try_from(init_commitment.destination)?;
        if destination_channel != channel_id {
            return Err(HandlerError::InvalidChannelId(destination_channel));
        }

        // Get our source address
        let our_source_address = self
            .address_lookup_table
            .get(&source_channel)
            .ok_or(HandlerError::MissingAddress(source_channel))?;

        // Check if this commitment receiver is our address
        if !normalised_address_eq(
            source_channel,
            &init_commitment.addresses.receiver,
            our_source_address,
        ) {
            return Err(HandlerError::NotAddressedToUs(commitment.swap_id));
        }

        // Its addressed to us we need to build a counter commitment
        self.build_counter_commitment(
            commitment.swap_id,
            &init_commitment,
            source_channel,
            destination_channel,
            clock,
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::test_fixtures::{create_test_handler_with, default_test_config, lp_addresses};
    use crate::{Handler, HandlerError};
    use std::sync::Arc;
    use stroemnet_protocol::swap_tracker::SwapTracker;
    use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};
    use stroemnet_protocol::{ChainClock, ChannelId};
    use tokio::sync::RwLock;

    pub(super) async fn handler() -> (Handler, Arc<RwLock<SwapTracker>>) {
        create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        )
    }

    pub(super) fn init(swap_id: [u8; 32], unlock_ts: u64) -> CommitmentV1 {
        CommitmentV1::new(
            swap_id,
            AddressesV1::new(
                "0xUserEthSender".into(),
                "0xMmEthereumAddress".into(),
                "kaspa:user_dest".into(),
            ),
            AmountV1::new("1000000000000000000".into(), 18),
            [0xEE; 32],
            unlock_ts,
            ChannelId::EthereumSepolia as u8,
            ChannelId::KaspaTn10 as u8,
        )
    }

    #[tokio::test]
    async fn swap_not_found() {
        let (handler, _t) = handler().await;
        let err = handler
            .handle_internal_commitment(
                &init([3u8; 32], u64::MAX),
                ChannelId::KaspaTn10,
                &ChainClock::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::SwapNotFound(_)));
    }

    #[tokio::test]
    async fn rejected_when_not_addressed_to_us() {
        let (handler, tracker) = handler().await;
        let swap_id = [42u8; 32];
        let mut c = init(swap_id, u64::MAX);
        c.addresses.receiver = "0xSomeOtherLp".into();
        tracker
            .write()
            .await
            .set_init_commitment(swap_id, c.clone())
            .unwrap();
        let err = handler
            .handle_internal_commitment(&c, ChannelId::KaspaTn10, &ChainClock::default())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::NotAddressedToUs(id) if id == swap_id));
    }
}
