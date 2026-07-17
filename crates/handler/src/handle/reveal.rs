use crate::Handler;
use crate::result::Result;
use stroemnet_protocol::v1::RevealV1;

impl Handler {
    /// Handle an external reveal event which sets the swap to be revealed by the swap id
    pub async fn handle_external_reveal(&self, reveal: RevealV1) -> Result<()> {
        tracing::info!("Handling reveal: {:?}", reveal);
        let mut tracker_write = self.swap_tracker.write().await;

        tracker_write.set_revealed(reveal.swap_id, reveal.secret)?;
        Ok(())
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
        create_test_handler, mock_counter_commitment_with_secret, mock_init_commitment_with_secret,
        sha256,
    };
    use stroemnet_protocol::swap_tracker::SwapStage;
    use stroemnet_protocol::v1::RevealV1;

    #[tokio::test]
    async fn reveal_valid_lock_transitions_to_reveal() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [1u8; 32];
        let secret = [0xAB; 32];
        let secret_hash = sha256(&secret);

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(
                swap_id,
                mock_init_commitment_with_secret(swap_id, secret_hash),
            )
            .unwrap();
            t.set_counter_commitment(
                swap_id,
                mock_counter_commitment_with_secret(swap_id, secret_hash),
            )
            .unwrap();
        }

        let reveal = RevealV1 { swap_id, secret };
        handler.handle_external_reveal(reveal).await.unwrap();

        let t = tracker.read().await;
        let record = t.get_swap(&swap_id).unwrap();
        assert_eq!(
            stroemnet_protocol::swap_tracker::SwapTracker::stage(record),
            SwapStage::Completed
        );
    }

    #[tokio::test]
    async fn reveal_with_wrong_secret_is_rejected() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [3u8; 32];
        let real_secret = [0x11; 32];
        let secret_hash = sha256(&real_secret);

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(
                swap_id,
                mock_init_commitment_with_secret(swap_id, secret_hash),
            )
            .unwrap();
            t.set_counter_commitment(
                swap_id,
                mock_counter_commitment_with_secret(swap_id, secret_hash),
            )
            .unwrap();
        }

        let bogus_reveal = RevealV1 {
            swap_id,
            secret: [0x99; 32],
        };
        let err = handler.handle_external_reveal(bogus_reveal).await;
        assert!(err.is_err(), "bogus reveal should be rejected");

        let real_reveal = RevealV1 {
            swap_id,
            secret: real_secret,
        };
        handler.handle_external_reveal(real_reveal).await.unwrap();
        let t = tracker.read().await;
        let record = t.get_swap(&swap_id).unwrap();
        assert_eq!(
            stroemnet_protocol::swap_tracker::SwapTracker::stage(record),
            SwapStage::Completed
        );
    }

    #[tokio::test]
    async fn reveal_swap_not_found() {
        let (handler, _tracker) = create_test_handler();
        let reveal = RevealV1 {
            swap_id: [99u8; 32],
            secret: [0xAB; 32],
        };
        let err = handler.handle_external_reveal(reveal).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn reveal_wrong_state_init_lock() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [2u8; 32];
        let secret = [0xAB; 32];
        let secret_hash = sha256(&secret);

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(
                swap_id,
                mock_init_commitment_with_secret(swap_id, secret_hash),
            )
            .unwrap();
        }

        let reveal = RevealV1 { swap_id, secret };
        let err = handler.handle_external_reveal(reveal).await;
        assert!(err.is_err());
    }
}
