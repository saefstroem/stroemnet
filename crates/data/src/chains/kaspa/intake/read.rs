use std::sync::Arc;

use ahash::AHashSet;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;

use crate::chains::kaspa::error::{KaspaError, Result};
use crate::chains::kaspa::intake::Intake;
use crate::chains::net::retry_timed;

/// Whether we should continue to poll for new blocks and advance to next page or stop
enum PageStep {
    Stop,
    Advance(Hash),
}

/// Compute the next page which is either the lowest hash at that page or if we should stop
fn next_page(last: Option<Hash>, page_low: Hash, reached_sink: bool) -> PageStep {
    if reached_sink {
        return PageStep::Stop;
    }
    // If the last hash is not eq to the page low it means there are still more unchecked blocks
    match last {
        Some(h) if h != page_low => PageStep::Advance(h),
        _ => PageStep::Stop,
    }
}

impl Intake {
    /// Starts a continuous loop to poll blocks every second from the rpc
    pub(crate) async fn read(&mut self) -> Result<()> {
        loop {
            if let Err(e) = self.poll_once().await {
                tracing::warn!("kaspa intake poll failed for {}: {e}", self.channel_id);
            }
            stroemnet_protocol::sleep_secs(1).await;
        }
    }

    /// Poll for blocks once
    async fn poll_once(&mut self) -> Result<()> {
        // Get the kaspa rpc
        let client = self.client.clone();

        // Get dag data
        let dag = retry_timed("get_block_dag_info", || client.get_block_dag_info())
            .await
            .ok_or_else(|| KaspaError::Other("get_block_dag_info: timed out".into()))?;

        // Update the max seen daa
        self.max_seen_daa = dag.virtual_daa_score;

        // Get the block hash of the sink
        let sink = dag.sink;

        // Get the cursor or set the sink to be the new cursor
        let low = match self.cursor {
            Some(h) => h,
            None => {
                self.set_cursor(sink);
                return Ok(());
            }
        };

        // Clear all pending blocks
        // We will get new ones in this iteration
        self.pending_blocks.clear();

        // Compute the start of the page
        let mut page_low = low;

        // To prevent any kind of rpc rate limit we limit how many pages we advance at once
        let mut iterations: u32 = 0;
        loop {
            iterations += 1;
            // Retrieve the blocks and transactions
            let resp = retry_timed("get_blocks", || {
                client.get_blocks(Some(page_low), true, true)
            })
            .await
            .ok_or_else(|| KaspaError::Other("get_blocks: timed out".into()))?;

            // Did we manage to reach the sink?
            let reached_sink = resp.block_hashes.contains(&sink);

            // Do we have the last block hash?
            let last = resp.block_hashes.last().copied();

            if resp.blocks.len() != resp.block_hashes.len() {
                return Err(KaspaError::Other(
                    "get_blocks: blocks/hashes length mismatch".into(),
                ));
            }

            // Go over all blocks
            for (block, hash) in resp
                .blocks
                .into_iter()
                .zip(resp.block_hashes.iter().copied())
            {
                if hash == page_low {
                    // if the hash is equal to the lower page it is a block that we have already processed
                    continue;
                }
                // Add it to pending blocks
                self.pending_blocks
                    .entry(hash)
                    .or_insert_with(|| Arc::new(block));
            }

            // Compute whether we should advance o the next page
            match next_page(last, page_low, reached_sink) {
                PageStep::Stop => break,
                PageStep::Advance(h) => page_low = h, // if we should advance we update the lower page bound
            }

            // break if we have too many pagings at once
            if iterations >= 10_000 {
                tracing::warn!(
                    "kaspa intake for {}: get_blocks paging exceeded bound — continuing next poll",
                    self.channel_id
                );
                break;
            }
        }

        // Retrieve the virtual chain from the lower bound hash
        let vc = retry_timed("get_virtual_chain_from_block", || {
            client.get_virtual_chain_from_block(low, false, None)
        })
        .await
        .ok_or_else(|| KaspaError::Other("get_virtual_chain_from_block: timed out".into()))?;

        // Retrieve the removed blocks due to reorg and all the chain blocks
        let removed: AHashSet<Hash> = vc.removed_chain_block_hashes.iter().copied().collect();
        let chain_blocks: AHashSet<Hash> = vc.added_chain_block_hashes.iter().copied().collect();

        // Based on the above data flush the confirmed blocks to the next stage of the pipeline
        self.flush_confirmed_blocks(&removed, &chain_blocks).await
    }

    // Update the cursor in memory and save it to disk if there is such storage
    pub(super) fn set_cursor(&mut self, hash: Hash) {
        self.cursor = Some(hash);
        if let Some(store) = &self.cursor_store {
            store.save(self.channel_id, &hash.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> Hash {
        Hash::from_bytes([b; 32])
    }

    #[test]
    fn stops_at_sink() {
        assert!(matches!(next_page(Some(h(2)), h(1), true), PageStep::Stop));
    }

    #[test]
    fn advances_to_last() {
        assert!(matches!(
            next_page(Some(h(2)), h(1), false),
            PageStep::Advance(_)
        ));
    }

    #[test]
    fn stops_when_no_progress() {
        assert!(matches!(next_page(Some(h(1)), h(1), false), PageStep::Stop));
        assert!(matches!(next_page(None, h(1), false), PageStep::Stop));
    }
}
