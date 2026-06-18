use std::sync::Arc;

use ahash::AHashSet;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;

use crate::chains::kaspa::error::{KaspaError, Result};
use crate::chains::kaspa::intake::Intake;

impl Intake {
    /// Starts the intake process, continuously polling for new blocks and processing them.
    pub(crate) async fn read(&mut self) -> Result<()> {
        loop {
            // Poll for new blocks and process them
            match self.poll_once().await {
                // if its ok reset the consecutive failures counter
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!("kaspa intake poll failed for {}: {e}", self.channel_id);
                }
            }
            // Sleep for a short duration before the next poll to avoid overwhelming the RPC server
            stroemnet_protocol::sleep_secs(1).await;
        }
    }

    async fn poll_once(&mut self) -> Result<()> {
        // Fetch the current block DAG info to get the virtual DAA score and sink block hash
        let dag = self
            .client
            .get_block_dag_info()
            .await
            .map_err(|e| KaspaError::Other(format!("get_block_dag_info: {e}")))?;
        self.max_seen_daa = dag.virtual_daa_score;
        let sink = dag.sink;

        // Determine the starting point for fetching blocks based on the current cursor
        // if we dont have a cursor, we set the sink to be the cursor
        let low = match self.cursor {
            Some(h) => h,
            None => {
                self.set_cursor(sink);
                return Ok(());
            }
        };

        // clear the pending blocks buffer before fetching new blocks
        self.pending_blocks.clear();
        let mut page_low = low;
        let mut iterations: u32 = 0;
        loop {
            iterations += 1;
            // Get a page of blocks starting from the current page_low hash, including the sink block
            let resp = self
                .client
                .get_blocks(Some(page_low), true, true)
                .await
                .map_err(|e| KaspaError::Other(format!("get_blocks: {e}")))?;
            // whether we have reached the sink block in this page of results
            let reached_sink = resp.block_hashes.contains(&sink);

            // Go over all blocks
            for (i, block) in resp.blocks.into_iter().enumerate() {
                let hash = resp.block_hashes[i];
                // If the hash is equal to the page_low, we skip it to avoid processing the same block again
                if hash == page_low {
                    continue;
                }
                self.pending_blocks
                    .entry(hash)
                    .or_insert_with(|| Arc::new(block));
            }

            // If we have reached the sink block, we can break out of the
            // loop as we have fetched all new blocks
            if reached_sink {
                break;
            }
            // Otherwise we update the page low as long as the last block hash
            // in the response is different from the current page low, otherwise we break to avoid infinite loops
            match resp.block_hashes.last().copied() {
                Some(last) if last != page_low => page_low = last,
                _ => break,
            }

            // If we have iterated too many times, we log a warning and continue processing on the
            // next poll
            if iterations >= 10_000 {
                tracing::warn!(
                    "kaspa intake for {}: get_blocks paging exceeded bound — continuing next poll",
                    self.channel_id
                );
                break;
            }
        }

        // Retrieve the consensus chain from the node.
        let vc = self
            .client
            .get_virtual_chain_from_block(low, false, None)
            .await
            .map_err(|e| KaspaError::Other(format!("get_virtual_chain_from_block: {e}")))?;

        // Compute all blocks that were removed from the chain and should not be forwarded
        let removed: AHashSet<Hash> = vc.removed_chain_block_hashes.iter().copied().collect();

        // Compute all blocks that are part of the current chain and should be forwarded
        let chain_blocks: AHashSet<Hash> = vc.added_chain_block_hashes.iter().copied().collect();

        // Attempt to flush all confirmed chain blocks.
        self.flush_confirmed_blocks(&removed, &chain_blocks).await
    }

    async fn flush_confirmed_blocks(
        &mut self,
        removed: &AHashSet<Hash>,
        chain_blocks: &AHashSet<Hash>,
    ) -> Result<()> {
        // Retrieve our current virtual DAA score
        let virtual_daa = self.max_seen_daa;
        // Retrieve the threshold for forwarding.
        let threshold = self.minimum_block_confirmations;

        // Create a new cursor
        let mut new_cursor: Option<Hash> = None;
        let mut buffer = Vec::new();

        // Take all pending blocks and iterate over them in order of insertion
        for (hash, block) in std::mem::take(&mut self.pending_blocks) {
            // If the block is in the removed set or its DAA score is below the threshold, we break out of the loop
            if removed.contains(&hash)
                || virtual_daa.saturating_sub(block.header.daa_score) < threshold
            {
                break;
            }
            // Otherwise, we add the block to the buffer and check if it is part of the current chain
            buffer.push(block);

            // Ensure this hash is a chain block
            if chain_blocks.contains(&hash) {
                let mut all_sent = true;
                // Attempt to send all buffered blocks to the next stage of processing
                for block in buffer.drain(..) {
                    if self.sender.send(block).await.is_err() {
                        all_sent = false;
                        break;
                    }
                }
                // If we failed to send all blocks, we break out of the loop and will retry on the next poll
                // I.e. we dont update the cursor until we have successfully sent all blocks
                // The rest of the system is idempotent
                if !all_sent {
                    break;
                }
                // If we successfully sent all blocks, we update the new cursor to the current hash
                new_cursor = Some(hash);
            }
        }

        // Now if we have a new cursor, we update the current cursor and persist it if a cursor store is available
        if let Some(hash) = new_cursor {
            self.set_cursor(hash);
        }
        Ok(())
    }

    /// Helper function to set the current cursor and persist it if a cursor store is available.
    fn set_cursor(&mut self, hash: Hash) {
        self.cursor = Some(hash);
        if let Some(store) = &self.cursor_store {
            // save the cursor to the store for persistence across restarts
            store.save(self.channel_id, &hash.as_bytes());
        }
    }
}
