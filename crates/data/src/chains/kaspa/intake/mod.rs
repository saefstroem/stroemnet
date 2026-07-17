mod confirm;
mod read;
use std::sync::Arc;

use indexmap::IndexMap;
use kaspa_hashes::Hash;
use kaspa_wrpc_client::{KaspaRpcClient, prelude::RpcBlock};
use stroemnet_protocol::ChannelId;
use tokio::sync::mpsc::Sender;

use crate::CursorStore;

/// The intake primitive responsible for accepting new blocks and handling reorgs
pub(super) struct Intake {
    /// Sender of confirmed blocks
    sender: Sender<Arc<RpcBlock>>,
    /// Kaspa rpc client
    client: Arc<KaspaRpcClient>,
    /// Minimum amount of block confirmations required before considered safe to act on
    minimum_block_confirmations: u64,
    /// Pending blocks that are not yet confirmed
    pending_blocks: IndexMap<Hash, Arc<RpcBlock>>,
    /// The maximum seen daa
    pub(crate) max_seen_daa: u64,
    /// The cursor that we are tracking
    cursor: Option<Hash>,
    /// Cursor storage
    cursor_store: Option<Arc<dyn CursorStore>>,
    /// the channel id of the intake (kaspa tn10,mainnet and so forth)
    channel_id: ChannelId,
}

impl Intake {
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
