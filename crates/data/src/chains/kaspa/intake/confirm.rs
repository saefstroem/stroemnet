use ahash::AHashSet;
use kaspa_hashes::Hash;

use crate::chains::kaspa::error::Result;
use crate::chains::kaspa::intake::Intake;

/// A block is not confirmed if removed contains its and if its below the required daa threshold
fn unconfirmed(
    removed: &AHashSet<Hash>,
    hash: &Hash,
    block_daa: u64,
    virtual_daa: u64,
    threshold: u64,
) -> bool {
    removed.contains(hash) || virtual_daa.saturating_sub(block_daa) < threshold
}

impl Intake {
    /// Computes which blocks are confirmed and transmits them to the next stage in the block processing pipeline
    pub(super) async fn flush_confirmed_blocks(
        &mut self,
        removed: &AHashSet<Hash>, // which block hashes have previously been reorged out
        chain_blocks: &AHashSet<Hash>, // the current chain blocks
    ) -> Result<()> {
        let virtual_daa = self.max_seen_daa;
        let threshold = self.minimum_block_confirmations;

        let mut new_cursor: Option<Hash> = None;
        let mut buffer = Vec::new();

        // For all the blocks that are pending
        for (hash, block) in std::mem::take(&mut self.pending_blocks) {
            if unconfirmed(
                removed,
                &hash,
                block.header.daa_score,
                virtual_daa,
                threshold,
            ) {
                // if we encountered a block that is unconfirmed we cannot proceed beyond it
                // it has been reorged out.
                break;
            }
            // Push the block to the buffer
            buffer.push(block);

            // If the chain blocks contains this block
            // it means its part of the canonical chain
            if chain_blocks.contains(&hash) {
                // we assume everything will be sent
                let mut all_sent = true;
                for block in buffer.drain(..) {
                    // drain all the blocks and send them to the next step
                    if self.sender.send(block).await.is_err() {
                        // if anything errored we fail here and exit
                        all_sent = false;
                        break;
                    }
                }
                // if not everything was sent we break
                if !all_sent {
                    break;
                }

                // only if everything was successful do we update the cursor
                new_cursor = Some(hash);
            }
        }

        // If the cursor was updated we update it on disk too
        if let Some(hash) = new_cursor {
            self.set_cursor(hash);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn h(b: u8) -> Hash {
        Hash::from_bytes([b; 32])
    }

    #[test]
    fn removed_block_is_unconfirmed() {
        let mut removed = AHashSet::new();
        removed.insert(h(1));
        assert!(unconfirmed(&removed, &h(1), 100, 200, 10));
    }

    #[test]
    fn shallow_block_is_unconfirmed() {
        let removed = AHashSet::new();
        assert!(unconfirmed(&removed, &h(2), 195, 200, 10));
    }

    #[test]
    fn deep_unremoved_block_is_confirmed() {
        let removed = AHashSet::new();
        assert!(!unconfirmed(&removed, &h(3), 100, 200, 10));
    }
}
