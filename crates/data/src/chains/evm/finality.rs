use alloy::primitives::Address;
use alloy::providers::Provider;
use alloy::rpc::types::{Filter, Log};
use alloy::sol_types::SolEvent;

use super::contracts::StroemHTLCV1;
use crate::chains::net::retry_timed;

/// The default polling interval milliseconds
pub(crate) const DEFAULT_POLL_INTERVAL_MS: u64 = 10_000;

// Maximum blocks per rpc call
pub(crate) const DEFAULT_MAX_BLOCKS_PER_POLL: u64 = 1000;

#[derive(Debug, Clone, Copy)]
/// Tracks the last block that we have successfully polled, ensures
/// that we never miss a block
pub(super) struct PollState {
    pub cursor: u64,
}

impl PollState {
    /// Compute the next range of blocks to poll from the rpc
    pub(super) fn next_range(
        &self,
        current_block: u64,       // the current block number
        confirmations: u64,       // number of confirmations that we need
        max_blocks_per_poll: u64, // maximum blocks per fetch
    ) -> Option<(u64, u64)> {
        // We want to fetch until the current latest block back - confirmations +1 since we do up until but not
        // including
        let confirmed_end = current_block.checked_sub(confirmations)?.checked_add(1)?;
        let from = self.cursor;

        // If from is from is greater then the confirmed end it means we havent confirmed enough blocks
        if from >= confirmed_end {
            return None;
        }

        // At least one block per poll
        let max = max_blocks_per_poll.max(1);

        // The end is the smallest of the end and from + the amount of blocks we poll
        let end = confirmed_end.min(from.saturating_add(max));
        Some((from, end))
    }

    /// Advance the cursor to another block range end
    pub(super) fn advance(&mut self, end: u64) {
        debug_assert!(
            end >= self.cursor,
            "PollState cursor must never move backwards: {} -> {}",
            self.cursor,
            end
        );
        self.cursor = end;
    }

    /// Poll once in accorance with the block range
    pub(super) async fn poll_once<P: Provider>(
        &mut self,
        provider: &P,                     // the provider
        htlc_address: Address,            // the contract address
        minimum_block_confirmations: u64, // minimum amount of blocks to wait for conf
        max_blocks_per_poll: u64,         // maximum amount of blocks per poll
    ) -> Vec<Log> {
        // Get the block number and retry a few times but its timeout based
        let current = match retry_timed("eth_blockNumber", || provider.get_block_number()).await {
            Some(n) => n,
            None => {
                tracing::warn!("eth_blockNumber failed — cursor unchanged, will retry");
                return Vec::new();
            }
        };

        // Compute the next range to fetch from
        // or return an empty vec if we are not ready to fetch more
        let Some((from, end)) =
            self.next_range(current, minimum_block_confirmations, max_blocks_per_poll)
        else {
            return Vec::new();
        };

        // Create a simple evm filter
        let filter = Filter::new()
            .address(htlc_address)
            .events([
                StroemHTLCV1::Commitment::SIGNATURE.as_bytes(),
                StroemHTLCV1::Claim::SIGNATURE.as_bytes(),
                StroemHTLCV1::Refund::SIGNATURE.as_bytes(),
            ])
            .from_block(from) // the from block
            .to_block(end - 1); // this is inclusive, but our range is up and to  (non-inclusive) so we do -1

        // Retrieve the logs in accordance with the created filter
        let logs = match retry_timed("eth_getLogs", || provider.get_logs(&filter)).await {
            Some(l) => l,
            None => {
                tracing::warn!(
                    "eth_getLogs {from}..{end} timed out — cursor unchanged, will retry"
                );
                return Vec::new();
            }
        };

        tracing::debug!(
            "fetched {} log(s) from block {from}..{end} (head {current})",
            logs.len()
        );

        // After succesfully fetching we should advance
        self.advance(end);
        logs
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
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
