use super::{Coordinator, DynResult};
use stroemnet_p2p::wire::message::{P2pMsg, ProposalError};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, RevealV1};

#[cfg(target_arch = "wasm32")]
use crate::SwapStage;

impl Coordinator {
    /// Handle an incoming p2p message and route it to the appropriate place
    pub(crate) async fn handle_incoming(&self, from: &str, msg: P2pMsg) -> DynResult {
        match msg {
            P2pMsg::State(s) => self.network.process_peer_addrs(s.peers).await,
            P2pMsg::ProposalRequest(req) => self.on_proposal_request(from, req).await?,
            P2pMsg::ProposalResponse(r) => self.on_proposal_response(from, r).await?,
            P2pMsg::Reveal(r) => self.on_reveal(from, r).await?,
            P2pMsg::ScriptAnnounce(s) => self.on_script_announce(from, s).await?,
            P2pMsg::ProposalError(e) => self.on_proposal_error(from, e),
        }
        Ok(())
    }

    /// An event handler for when the coordinator receives a reveal message
    async fn on_reveal(&self, from: &str, r: RevealV1) -> DynResult {
        // Attempt to forward the reveal to other peers
        if !self
            .network
            .forward(from, &P2pMsg::Reveal(r.clone()))
            .await?
        {
            return Ok(());
        }
        // Handle this as an external reveal event
        if let Err(e) = self.handler.handle_external_reveal(r.clone()).await {
            tracing::error!("reveal state update for {}: {e}", hex::encode(r.swap_id));
        }
        let channels: Vec<ChannelId> = self.sink.channels().collect();
        for chan in channels {
            // For each channel broadcast the event to all channels,
            // todo: in the future we can restrict to only channels that are involved in this swap
            if let Err(e) = self
                .sink
                .broadcast_event(chan, &ChainEvent::Reveal(r.clone()))
                .await
            {
                tracing::error!(
                    "reveal claim on {chan} for swap {}: {e}",
                    hex::encode(r.swap_id)
                );
            }
        }
        Ok(())
    }

    /// For wasm this emits an error event so that we can see why a request
    /// was rejected
    fn on_proposal_error(&self, from: &str, rejection: ProposalError) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = from;
            self.emit_status(
                rejection.swap_id,
                SwapStage::Failed {
                    reason: rejection.reason,
                },
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        tracing::info!(
            "proposal rejected by {from} for swap {}: {}",
            hex::encode(rejection.swap_id),
            rejection.reason
        );
    }
}
