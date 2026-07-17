use super::{Coordinator, DynResult};
use stroemnet_handler::handle::proposal::SwapRequest;
use stroemnet_p2p::wire::message::{P2pMsg, ProposalError, ProposalRequest, ProposalResponse};

impl Coordinator {
    /// Handles an incomign proposal request
    pub(super) async fn handle_proposal(&self, from: &str, req: ProposalRequest) -> DynResult {
        // Create a swaprequest struct
        let swap_request = SwapRequest {
            origin: req.origin,
            destination: req.destination,
            amount: req.amount.clone(),
        };

        // Create a proposal for the users request
        let proposal = match self.handler.create_proposal(&swap_request).await {
            Ok(proposal) => proposal,
            Err(e) => {
                // If there was a rejection reason emit a proposal error
                // so that the user can see the reason
                if let Some(reason) = e.rejection_reason() {
                    let rejection = ProposalError {
                        swap_id: req.swap_id,
                        origin: req.origin,
                        destination: req.destination,
                        reason,
                    };
                    self.network
                        .send_to(from, &P2pMsg::ProposalError(rejection))
                        .await?;
                    return Ok(());
                }
                return Err(e.into());
            }
        };

        let origin = proposal.origin;
        let origin_u8 = origin as u8;
        let destination_u8 = proposal.destination as u8;
        let lp_block_confirmations = self
            .handler
            .block_confirmations
            .get(&proposal.destination)
            .copied()
            .unwrap_or(0);
        let lp_sender_address = self.sink.lp_address(origin)?;

        // Hash the proposal so we can guarantee authenticity
        let digest = stroemnet_p2p::proposal_digest(
            req.swap_id,
            origin_u8,
            destination_u8,
            &proposal.amount_in,
            &proposal.amount_out,
            &proposal.sender_destination_address,
            &lp_sender_address,
            proposal.commit_unlock_offset_secs,
            lp_block_confirmations,
        );

        // Sign the message attesting our balance as well that we at least have the
        // amount in for the proposal
        let (_addr, lp_signature) = self
            .sink
            .sign_message(origin, digest, &proposal.amount_in)
            .await?;

        let resp = ProposalResponse {
            swap_id: req.swap_id,
            origin: origin_u8,
            destination: destination_u8,
            amount_in: proposal.amount_in,
            amount_out: proposal.amount_out,
            sender_destination_address: proposal.sender_destination_address,
            commit_unlock_offset_secs: proposal.commit_unlock_offset_secs,
            lp_sender_address,
            lp_signature,
            lp_block_confirmations,
            extra_data: vec![],
        };

        // Create the proposal and emit it across the p2p network
        self.network
            .send_to(from, &P2pMsg::ProposalResponse(resp))
            .await?;
        Ok(())
    }
}
