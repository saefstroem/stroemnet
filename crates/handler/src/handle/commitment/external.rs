use crate::Handler;
use crate::result::Result;
use stroemnet_protocol::v1::CommitmentV1;

impl Handler {
    /// Handle a commitment that came from an external chain
    pub async fn handle_external_commitment(&self, commitment: CommitmentV1) -> Result<()> {
        let mut tracker = self.swap_tracker.write().await;

        // Either retrieve the commitment or set it and return
        let Some(record) = tracker.get_swap(&commitment.swap_id) else {
            tracker.set_init_commitment(commitment.swap_id, commitment)?;
            return Ok(());
        };

        // If we already have a commitment and its eq to the init commitment
        // or if we already have a counter commitment
        if record.init_commitment == commitment
            || record.counter_commitment.as_ref() == Some(&commitment)
            || record.init_commitment.source == commitment.source
        {
            return Ok(());
        }

        // Then we can attempt to set a counter commitment
        tracker.set_counter_commitment(commitment.swap_id, commitment)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::test_fixtures::{create_test_handler_with, default_test_config, lp_addresses};
    use stroemnet_protocol::ChannelId;
    use stroemnet_protocol::swap_tracker::{SwapStage, SwapTracker};
    use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};

    fn init(swap_id: [u8; 32]) -> CommitmentV1 {
        CommitmentV1::new(
            swap_id,
            AddressesV1::new("0xUser".into(), "0xMm".into(), "kaspa:dest".into()),
            AmountV1::new("1000000000000000000".into(), 18),
            [0xEE; 32],
            u64::MAX,
            ChannelId::EthereumSepolia as u8,
            ChannelId::KaspaTn10 as u8,
        )
    }

    fn counter(swap_id: [u8; 32]) -> CommitmentV1 {
        CommitmentV1::new(
            swap_id,
            AddressesV1::new("kaspa:mm".into(), "kaspa:dest".into(), "0xMm".into()),
            AmountV1::new("50000000".into(), 18),
            [0xEE; 32],
            u64::MAX,
            ChannelId::KaspaTn10 as u8,
            ChannelId::EthereumSepolia as u8,
        )
    }

    #[tokio::test]
    async fn new_swap_creates_init_lock_then_counter_locks() {
        let (handler, tracker) = create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        );
        let swap_id = [1u8; 32];
        handler
            .handle_external_commitment(init(swap_id))
            .await
            .unwrap();
        {
            let t = tracker.read().await;
            assert_eq!(
                SwapTracker::stage(t.get_swap(&swap_id).unwrap()),
                SwapStage::Initialized
            );
        }
        handler
            .handle_external_commitment(counter(swap_id))
            .await
            .unwrap();
        let t = tracker.read().await;
        assert_eq!(
            SwapTracker::stage(t.get_swap(&swap_id).unwrap()),
            SwapStage::Locked
        );
    }

    #[tokio::test]
    async fn same_source_rearrival_is_ignored() {
        let (handler, tracker) = create_test_handler_with(
            default_test_config(),
            &[
                (ChannelId::KaspaTn10, 0.15),
                (ChannelId::EthereumSepolia, 3000.0),
            ],
            lp_addresses(),
        );
        let swap_id = [2u8; 32];
        handler
            .handle_external_commitment(init(swap_id))
            .await
            .unwrap();
        let mut dup = init(swap_id);
        dup.amount = AmountV1::new("999".into(), 18);
        handler.handle_external_commitment(dup).await.unwrap();
        let t = tracker.read().await;
        assert!(t.get_swap(&swap_id).unwrap().counter_commitment.is_none());
    }
}
