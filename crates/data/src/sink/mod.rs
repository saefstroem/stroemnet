mod forward;
mod registry;

use ahash::AHashMap;
use serde_json::Value;
use std::sync::Arc;
use stroemnet_protocol::ChannelId;

use crate::chains::build_buffer;
use crate::{ChainDataBuffer, CursorStore, DataError, Result, SettlementMetrics, SwapStore};

/// Contains all chain data buffers by channel id
pub struct ChainDataSink {
    buffers: AHashMap<ChannelId, Arc<dyn ChainDataBuffer>>,
}

impl ChainDataSink {
    /// Create a new chain data sink that will gather data for different chains
    pub async fn new(
        channels: AHashMap<ChannelId, (Value, Option<String>)>,
        cursor_store: Option<Arc<dyn CursorStore>>,
        swap_store: Option<Arc<dyn SwapStore>>,
        metrics: Option<Arc<dyn SettlementMetrics>>,
    ) -> Result<Self> {
        let mut buffers: AHashMap<ChannelId, Arc<dyn ChainDataBuffer>> = AHashMap::new();
        // Go over all channels
        for (channel_id, (cfg, lp_key)) in channels {
            let buffer: Arc<dyn ChainDataBuffer> = Arc::from(
                // build the channel buffer
                build_buffer(
                    channel_id,
                    &cfg,
                    lp_key,
                    cursor_store.clone(),
                    swap_store.clone(),
                    metrics.clone(),
                )
                .await?,
            );
            // Get the settler task and spawn it
            if let Some(task) = buffer.clone().settler_task() {
                stroemnet_protocol::spawn(task);
            }
            buffers.insert(channel_id, buffer);
        }
        Ok(Self { buffers })
    }

    /// Returns the chain data buffer
    fn buffer(&self, channel_id: ChannelId) -> Result<&dyn ChainDataBuffer> {
        self.buffers
            .get(&channel_id)
            .map(|b| b.as_ref())
            .ok_or(DataError::UnknownChannel(channel_id))
    }
}
