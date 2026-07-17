use crate::Handler;
use crate::result::Result;
use stroemnet_protocol::v1::RefundV1;

impl Handler {
    /// Handle an external refund event by setting the swap id as refunded
    pub async fn handle_external_refund(&self, refund: RefundV1) -> Result<()> {
        tracing::info!("Handling refund: {:?}", refund);
        let mut tracker_write = self.swap_tracker.write().await;

        tracker_write.set_refunded(refund.swap_id)?;
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
        create_test_handler, mock_counter_commitment, mock_init_commitment,
    };
    use stroemnet_protocol::swap_tracker::{SwapStage, SwapTracker};
    use stroemnet_protocol::v1::RefundV1;

    #[tokio::test]
    async fn refund_valid_lock_transitions_to_refunded() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [1u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
            t.set_counter_commitment(swap_id, mock_counter_commitment(swap_id))
                .unwrap();
        }

        let refund = RefundV1 { swap_id };
        handler.handle_external_refund(refund).await.unwrap();

        let t = tracker.read().await;
        let record = t.get_swap(&swap_id).unwrap();
        assert_eq!(SwapTracker::stage(record), SwapStage::Refunded);
    }

    #[tokio::test]
    async fn refund_swap_not_found() {
        let (handler, _tracker) = create_test_handler();
        let refund = RefundV1 {
            swap_id: [99u8; 32],
        };
        let err = handler.handle_external_refund(refund).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn refund_wrong_state_init_lock() {
        let (handler, tracker) = create_test_handler();
        let swap_id = [2u8; 32];

        {
            let mut t = tracker.write().await;
            t.set_init_commitment(swap_id, mock_init_commitment(swap_id))
                .unwrap();
        }

        let refund = RefundV1 { swap_id };
        let err = handler.handle_external_refund(refund).await;
        assert!(err.is_err());
    }
}
