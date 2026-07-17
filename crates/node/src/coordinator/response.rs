use super::{Coordinator, DynResult};
use stroemnet_p2p::wire::message::ProposalResponse;

#[cfg(target_arch = "wasm32")]
use crate::CheckedQuote;
#[cfg(target_arch = "wasm32")]
use stroemnet_protocol::ChannelId;

impl Coordinator {
    /// Event handler for reacting to proposal response
    /// only relevant for wasm who needs to react to a proposal response
    pub(super) async fn on_proposal_response(&self, from: &str, r: ProposalResponse) -> DynResult {
        #[cfg(target_arch = "wasm32")]
        {
            // Ensure these are channels that we know how ot handle
            let origin = match (
                ChannelId::try_from(r.origin),
                ChannelId::try_from(r.destination),
            ) {
                (Ok(o), Ok(d))
                    if self.handler.knows_channel(o) && self.handler.knows_channel(d) =>
                {
                    o
                }
                _ => {
                    tracing::info!("dropping ProposalResponse from {from} for unknown chains");
                    return Ok(());
                }
            };

            // Recompute the hash
            let digest = stroemnet_p2p::proposal_digest(
                r.swap_id,
                r.origin,
                r.destination,
                &r.amount_in,
                &r.amount_out,
                &r.sender_destination_address,
                &r.lp_sender_address,
                r.commit_unlock_offset_secs,
                r.lp_block_confirmations,
            );

            // Verify the message that it is indeed signed by the 
            // claimed address and also that there is enough balance
            // to cover the swap
            let (signature_valid, balance_sufficient) = match self
                .sink
                .verify_message(
                    origin,
                    digest,
                    &r.lp_sender_address,
                    &r.lp_signature,
                    &r.amount_in,
                )
                .await
            {
                Ok(v) => (v.address_matches, v.balance_sufficient),
                Err(e) => {
                    tracing::error!("verify_message failed: {e}");
                    (false, false)
                }
            };

            // Transmit the quote to the rest of the system
            // in wasm this will yield changes in sdk/fe state.
            let _ = self.quote_tx.send(CheckedQuote {
                swap_id: r.swap_id,
                origin: r.destination,
                destination: r.origin,
                amount_in: r.amount_out,
                amount_out: r.amount_in,
                sender_destination_address: r.sender_destination_address,
                commit_unlock_offset_secs: r.commit_unlock_offset_secs,
                lp_sender_address: r.lp_sender_address,
                lp_signature: r.lp_signature,
                lp_block_confirmations: r.lp_block_confirmations,
                signature_valid,
                balance_sufficient,
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (from, r);
        Ok(())
    }
}
