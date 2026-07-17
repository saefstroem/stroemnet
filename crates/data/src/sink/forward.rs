use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1};

use super::ChainDataSink;
use crate::{ProposalVerification, Result, ScriptAnnouncement};

impl ChainDataSink {
    /// Retrieves the lp address based on the provided channel id
    pub fn lp_address(&self, channel_id: ChannelId) -> Result<String> {
        self.buffer(channel_id)?.lp_address()
    }

    /// Compute the deposit address based on the channel id and commitment
    pub fn derive_deposit(
        &self,
        channel_id: ChannelId,
        commitment: &CommitmentV1,
    ) -> Result<(String, Vec<u8>)> {
        self.buffer(channel_id)?.derive_deposit(commitment)
    }

    /// Retrieve the next finalized cunk from all channels
    pub async fn finalized_chunk(&self) -> Result<Vec<(ChannelId, ChainEvent)>> {
        // Get all finalized chunks from all registered channels
        let polled =
            futures::future::join_all(self.buffers.iter().map(|(channel, buffer)| async move {
                (*channel, buffer.finalized_chunk().await)
            }))
            .await;
        let mut all = Vec::new();
        // Go over all channel data and add it to DS
        for (channel, result) in polled {
            match result {
                Ok(events) => all.extend(events),
                Err(e) => {
                    tracing::warn!(target: "settlement", "finalized_chunk for {channel}: {e}")
                }
            }
        }

        // return data
        Ok(all)
    }

    /// Broadcast the event to a destination channel
    pub async fn broadcast_event(
        &self,
        destination_channel_id: ChannelId,
        event: &ChainEvent,
    ) -> Result<()> {
        self.buffer(destination_channel_id)?
            .broadcast_event(event)
            .await
    }

    /// Sign a message and require that the signer has some required balance
    pub async fn sign_message(
        &self,
        channel_id: ChannelId,
        digest: [u8; 32],
        required_balance: &str,
    ) -> Result<(String, Vec<u8>)> {
        self.buffer(channel_id)?
            .sign_message(digest, required_balance)
            .await
    }

    /// Verify the signature and authenticity of a message whilst also verifying
    /// that the signer has the required amount of balance
    pub async fn verify_message(
        &self,
        channel_id: ChannelId,
        digest: [u8; 32],
        claimed_address: &str,
        signature: &[u8],
        required_balance: &str,
    ) -> Result<ProposalVerification> {
        self.buffer(channel_id)?
            .verify_message(digest, claimed_address, signature, required_balance)
            .await
    }

    /// Extract all utxo script announcements
    pub fn take_utxo_script_announcements(&self) -> Vec<ScriptAnnouncement> {
        self.buffers
            .values()
            .flat_map(|b| b.take_utxo_script_announcements())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashMap;

    #[test]
    fn empty_sink_forwards_are_inert() {
        let sink = ChainDataSink {
            buffers: AHashMap::new(),
        };
        assert!(sink.take_utxo_script_announcements().is_empty());
        assert!(sink.lp_address(ChannelId::KaspaTn10).is_err());
    }
}
