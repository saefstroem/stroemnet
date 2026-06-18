#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

use ahash::AHashMap;

use crate::ChannelId;

#[derive(Clone, Debug, Default)]
pub struct ChainClock {
    times: AHashMap<ChannelId, u64>,
}

impl ChainClock {
    pub fn new(times: AHashMap<ChannelId, u64>) -> Self {
        Self { times }
    }

    pub fn now(&self, channel: ChannelId) -> u64 {
        self.times
            .get(&channel)
            .copied()
            .unwrap_or_else(now_unix_secs)
    }

    pub fn now_checked(&self, channel: ChannelId) -> Option<u64> {
        match self.times.get(&channel).copied() {
            Some(ts) => Some(ts),
            None if channel.uses_synthetic_clock() => None,
            None => Some(now_unix_secs()),
        }
    }
}
