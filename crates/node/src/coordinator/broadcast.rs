use stroemnet_p2p::wire::message::P2pMsg;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::RevealV1;

use super::Coordinator;
use crate::{PendingClaim, SwapStage, SwapStatusUpdate};

impl Coordinator {
    /// Emit a status of a swap, this is mostly used in frontend and WASM
    pub(crate) fn emit_status(&self, swap_id: [u8; 32], stage: SwapStage) {
        let _ = self.swap_status_tx.send(SwapStatusUpdate {
            swap_id,
            stage,
            at: now_unix_secs(),
        });
    }

    /// Spawn the broadcast of a reveal, essentially wanting to fulfill the swap
    pub fn spawn_reveal_broadcast(&self, swap_id: [u8; 32], claim: PendingClaim) {
        let network = self.network.clone();
        let swap_status_tx = self.swap_status_tx.clone();

        // Create a future which will broadcast the reveal and return
        // a stage update for the swap
        let fut = async move {
            let reveal = RevealV1::new(swap_id, claim.secret);
            let stage = match network.broadcast(&P2pMsg::Reveal(reveal)).await {
                Ok(()) => {
                    tracing::info!(
                        "auto-claim: reveal broadcast for swap {}",
                        hex::encode(swap_id)
                    );
                    SwapStage::Completed
                }
                Err(e) => {
                    tracing::warn!(
                        "auto-claim: reveal broadcast failed for {}: {e}",
                        hex::encode(swap_id)
                    );
                    SwapStage::Failed {
                        reason: format!("reveal broadcast: {e}"),
                    }
                }
            };

            // Transmit the swap status
            let _ = swap_status_tx.send(SwapStatusUpdate {
                swap_id,
                stage,
                at: now_unix_secs(),
            });
        };
        stroemnet_protocol::spawn(fut);
    }
}
