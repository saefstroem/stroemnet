use alloy::primitives::Address;
use alloy::providers::Provider;
use alloy::rpc::types::{Filter, Log};
use alloy::sol_types::SolEvent;

use super::contracts::StroemHTLCV1;

/// Default interval and poll parameters for the EVM chain poller,
/// can be overridden by config and are tested in the PollState tests
pub(crate) const DEFAULT_POLL_INTERVAL_MS: u64 = 10_000;
pub(crate) const DEFAULT_MAX_BLOCKS_PER_POLL: u64 = 1000;

#[derive(Debug, Clone, Copy)]
/// A container for tracking the next
/// block to poll for events,
/// and computing the next block range to poll based on the current chain head
pub(super) struct PollState {
    pub cursor: u64,
}

impl PollState {
    /// Computes the next block range to poll based on the current chain head,
    /// the required number of confirmations, and the maximum blocks to poll at once.
    /// Returns None if there are no new blocks to poll yet.
    pub(super) fn next_range(
        &self,
        current_block: u64,
        confirmations: u64,
        max_blocks_per_poll: u64,
    ) -> Option<(u64, u64)> {
        // Exclusive upper bound: one past the deepest confirmed block.
        let confirmed_end = current_block.checked_sub(confirmations)?.checked_add(1)?;
        // The cursor is the next unread block, we start from here.
        let from = self.cursor;
        if from >= confirmed_end {
            return None;
        }

        // Cap the range to max_blocks_per_poll, or 1 if max_blocks_per_poll is zero
        let max = max_blocks_per_poll.max(1);

        // Half-open end: at most max blocks ahead, never past the confirmed end.
        let end = confirmed_end.min(from.saturating_add(max));
        Some((from, end))
    }

    pub(super) fn advance(&mut self, end: u64) {
        debug_assert!(
            end >= self.cursor,
            "PollState cursor must never move backwards: {} -> {}",
            self.cursor,
            end
        );
        // Update the cursor to the new block ensuring that
        // caller doesnt try to move it backwards, which would risk missing events
        self.cursor = end;
    }

    /// Polls the EVM chain for logs from the HTLC contract in the next block range,
    /// returning the logs or an empty vector if there are no new blocks to poll or if
    /// there was an error fetching the block number or logs (in which case the cursor is unchanged to allow retrying)
    pub(super) async fn poll_once<P: Provider>(
        &mut self,
        provider: &P,
        htlc_address: Address,
        minimum_block_confirmations: u64,
        max_blocks_per_poll: u64,
    ) -> Vec<Log> {
        // Get the current block number
        let current = match provider.get_block_number().await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("eth_blockNumber failed: {e} — cursor unchanged, will retry");
                return Vec::new();
            }
        };

        // Compute the next block range to poll, if any
        // If there are no new blocks to poll yet, return an empty vector
        let Some((from, end)) =
            self.next_range(current, minimum_block_confirmations, max_blocks_per_poll)
        else {
            return Vec::new();
        };

        // Create a filter which is by our address but also
        // the signatures of the events that we are looking for.
        let filter = Filter::new()
            .address(htlc_address)
            .events([
                StroemHTLCV1::Commitment::SIGNATURE.as_bytes(),
                StroemHTLCV1::Claim::SIGNATURE.as_bytes(),
                StroemHTLCV1::Refund::SIGNATURE.as_bytes(),
            ])
            .from_block(from)
            .to_block(end - 1); // end is exclusive

        // Retrieve logs from the provider in accordance with the filter
        let logs = match provider.get_logs(&filter).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    "eth_getLogs {from}..{end} failed: {e} — cursor unchanged, will retry"
                );
                return Vec::new();
            }
        };

        tracing::debug!(
            "fetched {} log(s) from block {from}..{end} (head {current})",
            logs.len()
        );

        // Advance the cursor to the end of the range
        self.advance(end);
        logs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(cursor: u64) -> PollState {
        PollState { cursor }
    }

    #[test]
    fn next_range_returns_none_at_head() {
        assert_eq!(state(101).next_range(110, 10, 1000), None);
    }

    #[test]
    fn next_range_returns_none_when_head_below_cursor() {
        assert_eq!(state(200).next_range(150, 10, 1000), None);
        assert_eq!(state(100).next_range(105, 10, 1000), None);
    }

    #[test]
    fn next_range_advances_one_block() {
        assert_eq!(state(101).next_range(111, 10, 1000), Some((101, 102)));
    }

    #[test]
    fn next_range_caps_to_max_blocks_per_poll() {
        let (from, end) = state(101).next_range(5110, 10, 1000).unwrap();
        assert_eq!(from, 101);
        assert_eq!(end, 1101);
        assert_eq!(end - from, 1000);
    }

    #[test]
    fn next_range_zero_confirmations_allowed() {
        assert_eq!(state(101).next_range(101, 0, 1000), Some((101, 102)));
    }

    #[test]
    fn next_range_max_blocks_of_one_still_advances() {
        let (from, end) = state(101).next_range(120, 10, 1).unwrap();
        assert_eq!(from, 101);
        assert_eq!(end, 102);
    }

    #[test]
    fn next_range_treats_max_blocks_of_zero_as_one() {
        let (from, end) = state(101).next_range(120, 10, 0).unwrap();
        assert_eq!(from, 101);
        assert_eq!(end, 102);
    }

    #[test]
    fn next_range_handles_cursor_at_u64_max() {
        assert_eq!(state(u64::MAX).next_range(u64::MAX, 0, 1000), None);
    }

    #[test]
    fn boundary_block_at_exact_confirmation_depth_is_included() {
        assert_eq!(state(100).next_range(110, 10, 1000), Some((100, 101)));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must never move backwards")]
    fn cursor_never_moves_backward() {
        let mut s = state(500);
        s.advance(400);
    }

    struct Xorshift {
        state: u64,
    }
    impl Xorshift {
        fn new(seed: u64) -> Self {
            Self { state: seed.max(1) }
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x
        }
        fn range(&mut self, lo: u64, hi_inclusive: u64) -> u64 {
            lo + (self.next_u64() % (hi_inclusive - lo + 1))
        }
        fn flip(&mut self, numerator: u64) -> bool {
            self.next_u64() % 100 < numerator
        }
    }

    fn run_simulation(
        seed: u64,
        growth_ticks: usize,
        confirmations: u64,
        max_blocks_per_poll: u64,
        getlogs_failure_rate: u64,
        blocknumber_failure_rate: u64,
    ) -> (Vec<bool>, u64, u64) {
        let mut rng = Xorshift::new(seed);
        let mut chain_head: u64 = 1_000;
        let initial_cursor = chain_head.saturating_sub(confirmations).saturating_add(1);
        let mut state = PollState {
            cursor: initial_cursor,
        };

        let mut visited = vec![false; ((chain_head as usize) + growth_ticks * 100).max(8192)];

        for _ in 0..growth_ticks {
            chain_head += rng.range(1, 50);
            if visited.len() <= (chain_head as usize) + 64 {
                visited.resize(visited.len() * 2, false);
            }
            tick_once(
                &mut state,
                chain_head,
                confirmations,
                max_blocks_per_poll,
                &mut rng,
                getlogs_failure_rate,
                blocknumber_failure_rate,
                &mut visited,
            );
        }

        let final_confirmed_end = chain_head.saturating_sub(confirmations).saturating_add(1);
        let drain_cap = (final_confirmed_end - state.cursor) as usize * 10 + 1000;
        let mut drained = 0usize;
        while state.cursor < final_confirmed_end {
            drained += 1;
            assert!(drained < drain_cap, "drain phase exceeded cap");
            tick_once(
                &mut state,
                chain_head,
                confirmations,
                max_blocks_per_poll,
                &mut rng,
                getlogs_failure_rate,
                blocknumber_failure_rate,
                &mut visited,
            );
        }

        (visited, initial_cursor, final_confirmed_end)
    }

    #[allow(clippy::too_many_arguments)]
    fn tick_once(
        state: &mut PollState,
        chain_head: u64,
        confirmations: u64,
        max_blocks_per_poll: u64,
        rng: &mut Xorshift,
        getlogs_failure_rate: u64,
        blocknumber_failure_rate: u64,
        visited: &mut [bool],
    ) {
        if rng.flip(blocknumber_failure_rate) {
            return;
        }
        let Some((from, end)) = state.next_range(chain_head, confirmations, max_blocks_per_poll)
        else {
            return;
        };

        if rng.flip(getlogs_failure_rate) {
            return;
        }
        for b in from..end {
            assert!(!visited[b as usize], "block {b} returned twice");
            visited[b as usize] = true;
        }
        state.advance(end);
    }

    fn assert_full_coverage(visited: &[bool], initial_cursor: u64, confirmed_end: u64) {
        for b in initial_cursor..confirmed_end {
            assert!(visited[b as usize], "block {b} was missed");
        }
        for (b, hit) in visited.iter().enumerate() {
            let b = b as u64;
            if *hit {
                assert!(
                    b >= initial_cursor && b < confirmed_end,
                    "block {b} out of range"
                );
            }
        }
    }

    #[test]
    fn never_misses_blocks_in_simulated_chain() {
        let (visited, cursor0, head_final) = run_simulation(0xDEAD_BEEF, 500, 10, 1000, 0, 0);
        assert!(head_final > cursor0);
        assert_full_coverage(&visited, cursor0, head_final);
    }

    #[test]
    fn never_misses_blocks_with_simulated_rpc_errors() {
        let (visited, cursor0, head_final) = run_simulation(0xC0FFEE, 800, 10, 1000, 30, 0);
        assert_full_coverage(&visited, cursor0, head_final);
    }

    #[test]
    fn never_misses_blocks_with_simulated_block_number_errors() {
        let (visited, cursor0, head_final) = run_simulation(0xBA5E_BA11, 800, 10, 1000, 0, 40);
        assert_full_coverage(&visited, cursor0, head_final);
    }

    #[test]
    fn never_misses_blocks_under_combined_error_pressure() {
        let (visited, cursor0, head_final) = run_simulation(0xFEED_FACE, 1000, 10, 1000, 25, 25);
        assert_full_coverage(&visited, cursor0, head_final);
    }
}
