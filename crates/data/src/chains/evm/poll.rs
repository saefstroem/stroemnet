use stroemnet_protocol::ChannelId;
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;

use super::Evm;
use super::{decode, provider};
use crate::Result;

impl Evm {
    /// A wrapper function to poll the finalized blocks from the evm state
    pub(super) async fn poll_finalized(&self) -> Result<Vec<(ChannelId, ChainEvent)>> {
        // Compute the unix timestamp
        let now = now_unix_secs();

        // Retrieve the poll state, but only if its truly time to poll
        let mut poll = {
            let mut st = self.state.lock();
            if now < st.next_poll_secs {
                None
            } else {
                st.next_poll_secs = now + self.poll_interval_secs;
                Some(st.poll)
            }
        };

        // Create a container to store events that we have found
        let mut events = Vec::new();

        // We only poll if we have a poll state (i.e. its time to poll again)
        if let Some(poll) = poll.as_mut() {
            // Retrieve logs from the poll
            let logs = poll
                .poll_once(
                    &self.read_provider,
                    self.htlc_address,
                    self.minimum_block_confirmations,
                    self.max_blocks_per_poll,
                )
                .await;
            {
                // Update the poll
                self.state.lock().poll = *poll;
            }

            // Update the cursor to be stored in the cursor storage
            if let Some(store) = &self.cursor_store {
                store.save(self.channel_id, &poll.cursor.to_le_bytes());
            }

            // Go over all logs, decode the log and then maybe queue a refund
            // via track actionable event
            for log in &logs {
                if let Some(event) = decode::decode_log(log, self.channel_id) {
                    self.track_actionable_event(&event);
                    // Then push the event to out container
                    events.push((self.channel_id, event));
                }
            }

            // Retrieve the current block timestamp and update it in our state
            if let Some(ts) = provider::current_block_timestamp(&self.read_provider).await {
                self.state.lock().last_block_ts = Some((ts, now_unix_secs()));
            }
        }

        Ok(events)
    }
}
