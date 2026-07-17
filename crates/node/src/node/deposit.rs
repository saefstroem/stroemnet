use stroemnet_p2p::wire::message::{P2pMsg, ScriptAnnounce};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;

use super::state::Node;
use crate::SwapStage;
use crate::error::StroemnetError;
use crate::result::Result;

impl Node {
    /// Registers a utxo script and computes a p2sh address
    /// which is where the user should send funds to
    pub(super) async fn register_kaspa_deposit(
        &self,
        source: ChannelId,
        swap_id: [u8; 32],
        commitment: &CommitmentV1,
    ) -> Result<String> {
        // Compute the deposit address based on the commitment
        let (p2sh, redeem) = self
            .sink
            .derive_deposit(source, commitment)
            .map_err(|e| StroemnetError::Other(format!("kaspa deposit derive: {e}")))?;
        let target = commitment.amount.value.clone();
        
        // Register the script
        self.sink
            .register_script(
                source,
                p2sh.clone(),
                redeem.clone(),
                swap_id,
                commitment.unlock_ts,
                target.clone(),
            )
            .await
            .map_err(|e| StroemnetError::Other(format!("register script: {e}")))?;

        // Create a script announcement so that many nodes are ready
        let announce = ScriptAnnounce {
            address: p2sh.clone(),
            swap_id,
            redeem_script: redeem,
            unlock_ts: commitment.unlock_ts,
            deposit_target: target.clone(),
        };

        // Broadcast the script announcement over p2p
        if let Err(e) = self
            .network
            .broadcast(&P2pMsg::ScriptAnnounce(announce))
            .await
        {
            tracing::warn!("kas-source script announce failed: {e}");
        }

        // Emit the status to potential wasm worker
        self.emit_status(
            swap_id,
            SwapStage::AwaitingDeposit {
                chain: source,
                address: p2sh.clone(),
                deposit_target: Some(target),
            },
        );
        Ok(p2sh)
    }
}
