use super::{Coordinator, DynResult, MAX_REDEEM_SCRIPT_BYTES, Role};
use stroemnet_p2p::wire::message::{P2pMsg, ProposalRequest, ScriptAnnounce};
use stroemnet_protocol::ChannelId;

impl Coordinator {
    /// An event handler when we get a raw proposal request
    pub(super) async fn on_proposal_request(&self, from: &str, req: ProposalRequest) -> DynResult {
        let origin = ChannelId::try_from(req.origin);
        let dest = ChannelId::try_from(req.destination);

        // Ensure the channels that are requested are ones that we can handle
        match (origin, dest) {
            (Ok(o), Ok(d)) if self.handler.knows_channel(o) && self.handler.knows_channel(d) => {}
            _ => {
                tracing::info!(
                    "dropping ProposalRequest from {from} (unknown chains origin={} dest={})",
                    req.origin,
                    req.destination
                );
                return Ok(());
            }
        }

        // Forward the proposal request to other peers too
        if !self
            .network
            .forward(from, &P2pMsg::ProposalRequest(req.clone()))
            .await?
        {
            return Ok(());
        }
        // Only if this node is an lp node do we handle it
        if self.role == Role::Lp {
            self.handle_proposal(from, req).await?;
        }
        Ok(())
    }

    /// Event handler for handling script announcements
    /// these are announcements that utxo scripts have been received and that we
    /// might get them in the future
    pub(super) async fn on_script_announce(&self, from: &str, s: ScriptAnnounce) -> DynResult {
        // We only allow redeem scripts that are within limits
        if s.redeem_script.len() > MAX_REDEEM_SCRIPT_BYTES {
            tracing::warn!(
                "script-announce: rejected from {from} for swap {} — redeem_script too large",
                hex::encode(s.swap_id)
            );
            return Ok(());
        }
        // Forward the script to other peers
        if !self
            .network
            .forward(from, &P2pMsg::ScriptAnnounce(s.clone()))
            .await?
        {
            return Ok(());
        }

        // Todo: in the future, to support multiple scripts channels
        // we will need to distinguish where we send and notify of the script
        let Some(channel_id) = self.sink.script_channel() else {
            return Ok(());
        };

        // Register the script with that channel
        if let Err(e) = self
            .sink
            .register_script(
                channel_id,
                s.address,
                s.redeem_script,
                s.swap_id,
                s.unlock_ts,
                s.deposit_target,
            )
            .await
        {
            tracing::warn!(
                "script-announce: rejected from {from} for swap {} — {e}",
                hex::encode(s.swap_id)
            );
        }
        Ok(())
    }
}
