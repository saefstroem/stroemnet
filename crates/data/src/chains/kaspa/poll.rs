use std::sync::Arc;

use kaspa_wrpc_client::prelude::RpcBlock;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;

use super::Kaspa;
use super::decode;
use crate::Result;
use crate::chains::push_pending_refund;
use crate::chains::settlement::ActionKey;

impl Kaspa {
    /// Poll finalized blocks from the rpc
    pub(super) async fn poll_finalized(&self) -> Result<Vec<(ChannelId, ChainEvent)>> {
        let blocks: Vec<Arc<RpcBlock>> = {
            // Get a lock on safe blocks, and receive them
            let mut rx = self.safe_blocks.lock();
            let mut v = Vec::new();
            while let Ok(block) = rx.try_recv() {
                v.push(block);
            }
            v
        };

        let mut events = Vec::new();
        // Go over all blocks
        for block in blocks {
            // Handle all blocks and store their outcomes
            let outcomes = decode::handle_block_added(
                &block,
                &self.utxo_scripts,
                self.prefix,
                self.channel_id,
            )
            .await?;
            if self.participate_ccr {
                // If we participate in CCR
                let mut pushed = Vec::new();
                {
                    // If there are any refunds we track them and will try to refund them once they expire
                    let mut pending = self.pending_refunds.lock();
                    for (swap_id, unlock_ts) in outcomes.refunds {
                        if push_pending_refund(&mut pending, swap_id, unlock_ts) {
                            pushed.push(swap_id);
                        }
                    }
                }
                // We populate the refunds in the retry queue and then also sync to disk
                for swap_id in pushed {
                    self.queue
                        .ensure(ActionKey::refund(swap_id), now_unix_secs());
                    self.persist_swap(swap_id);
                }
            }
            // For all other events we track them depending on what they are then add to the DS
            for event in outcomes.events {
                self.track_actionable_event(&event);
                events.push((self.channel_id, event));
            }
        }

        // After this we prune scripts that are expired
        self.prune_scripts().await;
        // Return events
        Ok(events)
    }
}
