use crate::Handler;
use crate::result::Result;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;

impl Handler {
    /// Get the counterparty channel id for a given swap and our channel.
    /// This is used to know which channel to monitor for the counterparty commitment and reveal/refund events
    pub async fn get_counterparty_channel_id(
        &self,
        swap_id: &[u8; 32],
        our_channel: ChannelId,
    ) -> Result<Option<ChannelId>> {
        let tracker_read = self.swap_tracker.read().await;
        // Try and retrieve the swap record for the given swap id. If it doesn't exist, return None
        if let Some(record) = tracker_read.get_swap(swap_id) {
            let init_source = ChannelId::try_from(record.init_commitment.source)?;

            // If there is no counter commitment, we are in the state where only the init commitment has been observed.
            if record.counter_commitment.is_none() {
                // This means we can simply return the destination of the init commitment as the counterparty channel,
                // since the init commitment is always sent by us and received by the counterparty
                let init_dest = ChannelId::try_from(record.init_commitment.destination)?;
                return Ok(Some(init_dest));
            }

            // Otherwise simply compute the counter channel id by reading it from the source
            // of the counter commitment
            let counter_source =
                ChannelId::try_from(record.counter_commitment.as_ref().unwrap().source)?;

            if init_source == our_channel {
                Ok(Some(counter_source))
            } else {
                Ok(Some(init_source))
            }
        } else {
            Ok(None)
        }
    }

    /// Retrieve a commitment for a given swap id and channel
    pub async fn get_commitment_for_channel(
        &self,
        swap_id: &[u8; 32],
        channel: ChannelId,
    ) -> Result<Option<CommitmentV1>> {
        let tracker_read = self.swap_tracker.read().await;
        // If we dont have this swap there is nothing to return
        let Some(record) = tracker_read.get_swap(swap_id) else {
            return Ok(None);
        };

        // Check if the init commitment belongs to the given channel, if yes return it
        if ChannelId::try_from(record.init_commitment.source)? == channel {
            return Ok(Some(record.init_commitment.clone()));
        }

        // Check if the counter commitment belongs to the given channel, if yes return it
        if let Some(counter) = record.counter_commitment.as_ref()
            && ChannelId::try_from(counter.source)? == channel {
                return Ok(Some(counter.clone()));
            }

        // Otherwise we dont have it
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_fixtures::{
        TEST_SECRET, create_test_handler, mock_counter_commitment, mock_init_commitment,
    };
    use stroemnet_protocol::ChannelId;

    #[tokio::test]
    async fn test_get_counterparty_channel_id_returns_none_when_no_swap() {
        let (handler, _tracker) = create_test_handler();
        let swap_id = [3u8; 32];

        let result = handler
            .get_counterparty_channel_id(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert!(result.is_none(), "Expected None for non-existent swap");
    }

    #[tokio::test]
    async fn test_get_counterparty_channel_id_for_init_lock() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [4u8; 32];

        let commitment = mock_init_commitment(swap_id);

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, commitment).unwrap();
        }

        let result = handler
            .get_counterparty_channel_id(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert_eq!(
            result,
            Some(ChannelId::KaspaTn10),
            "InitLock should return its destination as the counterparty channel"
        );
    }

    #[tokio::test]
    async fn test_get_commitment_for_channel_returns_none_when_no_swap() {
        let (handler, _tracker) = create_test_handler();
        let swap_id = [5u8; 32];

        let result = handler
            .get_commitment_for_channel(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert!(result.is_none(), "Expected None for non-existent swap");
    }

    #[tokio::test]
    async fn test_get_counterparty_channel_id_lock_returns_other_channel() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [10u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
        }

        let result = handler
            .get_counterparty_channel_id(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert_eq!(result, Some(ChannelId::EthereumSepolia));

        let result2 = handler
            .get_counterparty_channel_id(&swap_id, ChannelId::EthereumSepolia)
            .await
            .unwrap();
        assert_eq!(result2, Some(ChannelId::KaspaTn10));
    }

    #[tokio::test]
    async fn test_get_counterparty_channel_id_reveal_state() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [11u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
            t.set_revealed(swap_id, TEST_SECRET).unwrap();
        }

        let result = handler
            .get_counterparty_channel_id(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert_eq!(result, Some(ChannelId::EthereumSepolia));
    }

    #[tokio::test]
    async fn test_get_counterparty_channel_id_refunded_state() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [12u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
            t.set_refunded(swap_id).unwrap();
        }

        let result = handler
            .get_counterparty_channel_id(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert_eq!(result, Some(ChannelId::EthereumSepolia));
    }

    #[tokio::test]
    async fn test_get_commitment_for_channel_lock_kaspa() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [15u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
        }

        let result = handler
            .get_commitment_for_channel(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert!(result.is_some());

        assert_eq!(result.unwrap().amount.value, "2000");
    }

    #[tokio::test]
    async fn test_get_commitment_for_channel_lock_ethereum() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [16u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
        }

        let result = handler
            .get_commitment_for_channel(&swap_id, ChannelId::EthereumSepolia)
            .await
            .unwrap();
        assert!(result.is_some());

        assert_eq!(result.unwrap().amount.value, "1000");
    }

    #[tokio::test]
    async fn test_get_commitment_for_channel_reveal_state() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [17u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
            t.set_revealed(swap_id, TEST_SECRET).unwrap();
        }

        let result = handler
            .get_commitment_for_channel(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().amount.value, "2000");
    }

    #[tokio::test]
    async fn test_get_commitment_for_channel_refunded_state() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [18u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
            t.set_refunded(swap_id).unwrap();
        }

        let result = handler
            .get_commitment_for_channel(&swap_id, ChannelId::KaspaTn10)
            .await
            .unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().amount.value, "2000");
    }

    #[tokio::test]
    async fn test_get_commitment_for_channel_init_lock_returns_init() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [19u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
        }

        let result = handler
            .get_commitment_for_channel(&swap_id, ChannelId::EthereumSepolia)
            .await
            .unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().amount.value, "1000");
    }
}
