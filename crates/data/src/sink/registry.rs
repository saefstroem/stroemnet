use ahash::AHashMap;
use stroemnet_protocol::{ChainClock, ChannelId};

use super::ChainDataSink;
use crate::{DataError, Result, UtxoScriptDetector};

impl ChainDataSink {
    /// Retrieves all channels that are registered
    pub fn channels(&self) -> impl Iterator<Item = ChannelId> + '_ {
        self.buffers.keys().copied()
    }

    /// Returns a world clock for all registered chains
    pub fn chain_clock(&self) -> ChainClock {
        let mut times = AHashMap::new();
        for (channel, buffer) in &self.buffers {
            if let Some(ts) = buffer.chain_now() {
                times.insert(*channel, ts);
            }
        }
        ChainClock::new(times)
    }

    /// Whether we have a particular channel id registered as a valid chain data buffer
    pub fn knows_channel(&self, channel_id: ChannelId) -> bool {
        self.buffers.contains_key(&channel_id)
    }

    /// Returns the first channel that has a utxo script detector
    pub fn script_channel(&self) -> Option<ChannelId> {
        self.buffers
            .iter()
            .find(|(_, b)| b.utxo_script_detector().is_some())
            .map(|(id, _)| *id)
    }

    /// Gets the utxo script detector based on a channel id
    fn utxo_script_detector(&self, channel_id: ChannelId) -> Option<&dyn UtxoScriptDetector> {
        self.buffers
            .get(&channel_id)
            .and_then(|b| b.utxo_script_detector())
    }

    /// Used to register a script, particular useful for utxo based systems
    pub async fn register_script(
        &self,
        channel_id: ChannelId,
        address: String,
        redeem_script: Vec<u8>,
        swap_id: [u8; 32],
        unlock_ts: u64,
        deposit_target: String,
    ) -> Result<()> {
        // it only works if the channel has a utxo script detector
        match self.utxo_script_detector(channel_id) {
            Some(detector) => {
                // register the script with the detector
                detector
                    .register_script(address, redeem_script, swap_id, unlock_ts, deposit_target)
                    .await
            }
            None => Err(DataError::UnknownChannel(channel_id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sink_knows_nothing() {
        let sink = ChainDataSink {
            buffers: AHashMap::new(),
        };
        assert_eq!(sink.channels().count(), 0);
        assert!(sink.script_channel().is_none());
        assert!(!sink.knows_channel(ChannelId::KaspaTn10));
    }
}
