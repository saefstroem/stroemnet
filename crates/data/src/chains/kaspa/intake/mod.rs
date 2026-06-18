mod read;
use std::sync::Arc;

use indexmap::IndexMap;
use kaspa_hashes::Hash;
use kaspa_wrpc_client::{KaspaRpcClient, prelude::RpcBlock};
use stroemnet_protocol::ChannelId;
use tokio::sync::mpsc::Sender;

use crate::CursorStore;

/// The Intake component is responsible for receiving new blocks from the Kaspa network,
/// performing initial processing and validation, and forwarding them to the appropriate channels.
pub(super) struct Intake {
    /// The sender channel to forward validated blocks to the next stage of processing.
    sender: Sender<Arc<RpcBlock>>,
    /// The Kaspa RPC client used to fetch block data and subscribe to new block notifications.
    client: Arc<KaspaRpcClient>,
    /// The minimum number of confirmations required before a block is forwarded. This helps ensure
    /// that the block is unlikely to be reorged out of the chain before we process it.
    minimum_block_confirmations: u64,
    /// A buffer of recently seen blocks that are waiting for enough confirmations before being forwarded.
    pending_blocks: IndexMap<Hash, Arc<RpcBlock>>,
    /// The maximum DAA score seen so far.
    /// This is used to filter out old blocks that are too far behind the current chain tip.
    pub(crate) max_seen_daa: u64,
    /// The current cursor, representing the last processed block's hash.
    cursor: Option<Hash>,
    /// An optional cursor store for persisting the last processed block's hash across restarts.
    cursor_store: Option<Arc<dyn CursorStore>>,
    /// The channel ID associated with this intake instance, used for identifying the source of blocks.
    channel_id: ChannelId,
}

impl Intake {
    /// Creates a new Intake instance with the given Kaspa RPC client, sender channel, and minimum confirmations.
    pub(super) fn new(
        client: Arc<KaspaRpcClient>,
        sender: Sender<Arc<RpcBlock>>,
        minimum_block_confirmations: u64,
        channel_id: ChannelId,
        initial_cursor: Option<Hash>,
        cursor_store: Option<Arc<dyn CursorStore>>,
    ) -> Self {
        Self {
            sender,
            client,
            minimum_block_confirmations,
            pending_blocks: IndexMap::new(),
            max_seen_daa: 0,
            cursor: initial_cursor,
            cursor_store,
            channel_id,
        }
    }
}
