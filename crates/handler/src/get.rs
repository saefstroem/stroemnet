use crate::Handler;
use crate::result::Result;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;

impl Handler {
    /// Retrieves the channel id for the counterparty provided some existing channel id
    pub async fn get_counterparty_channel_id(
        &self,
        swap_id: &[u8; 32],
        our_channel: ChannelId,
    ) -> Result<Option<ChannelId>> {
        let tracker_read = self.swap_tracker.read().await;
        if let Some(record) = tracker_read.get_swap(swap_id) {
            // get the swap

            // compute init source
            let init_source = ChannelId::try_from(record.init_commitment.source)?;

            let Some(counter) = record.counter_commitment.as_ref() else {
                // if there is no counter we will simply return none
                return Ok(None);
            };

            // compute counter
            let counter_source = ChannelId::try_from(counter.source)?;

            // depending on what we passed as our channel we will get the other side
            if init_source == our_channel {
                Ok(Some(counter_source))
            } else {
                Ok(Some(init_source))
            }
        } else {
            Ok(None)
        }
    }

    pub async fn get_commitment_for_channel(
        &self,
        swap_id: &[u8; 32],
        channel: ChannelId,
    ) -> Result<Option<CommitmentV1>> {
        let tracker_read = self.swap_tracker.read().await;
        let Some(record) = tracker_read.get_swap(swap_id) else {
            return Ok(None);
        };

        if ChannelId::try_from(record.init_commitment.source)? == channel {
            return Ok(Some(record.init_commitment.clone()));
        }

        if let Some(counter) = record.counter_commitment.as_ref()
            && ChannelId::try_from(counter.source)? == channel
        {
            return Ok(Some(counter.clone()));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
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
        assert!(
            result.is_none(),
            "no counterparty channel until the counter commitment locks"
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
