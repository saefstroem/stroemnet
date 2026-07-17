use std::sync::Arc;

use kaspa_hashes::Hash;
use kaspa_wrpc_client::prelude::{NetworkId, RpcBlock};
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use stroemnet_protocol::ChannelId;
use tokio::sync::mpsc::Receiver;

use super::intake::Intake;
use crate::{CursorStore, DataError, Result};

/// Builds a kaspa rpc client
pub(super) async fn build_client(
    network_id: NetworkId,
    wrpc_url: Option<&str>,
) -> Result<Arc<KaspaRpcClient>> {
    // If we have an rpc url we wont use the resolver
    let resolver = match wrpc_url {
        Some(_) => None,
        None => Some(Resolver::default()),
    };
    // Create an arced kaspa rpc client
    let client = Arc::new(
        KaspaRpcClient::new(
            WrpcEncoding::Borsh,
            wrpc_url,
            resolver,
            Some(network_id),
            None,
        )
        .map_err(|e| DataError::Connect(format!("wrpc client: {e}")))?,
    );

    // Connect the client to the rpc
    client
        .connect(None)
        .await
        .map_err(|e| DataError::Connect(format!("kaspa connect: {e}")))?;
    Ok(client)
}

/// Spawns the kaspa rpc intake on another task
pub(super) fn spawn_intake(
    client: Arc<KaspaRpcClient>,
    minimum_block_confirmations: u64,
    channel_id: ChannelId,
    initial_cursor: Option<Hash>,
    cursor_store: Option<Arc<dyn CursorStore>>,
) -> Receiver<Arc<RpcBlock>> {
    // Create the tx and rx channels
    let (tx, rx) = tokio::sync::mpsc::channel::<Arc<RpcBlock>>(1024);

    // Create the reader
    let mut reader = Intake::new(
        client,
        tx,
        minimum_block_confirmations,
        channel_id,
        initial_cursor,
        cursor_store,
    );
    // Spawn the reader on a new task
    stroemnet_protocol::spawn(async move {
        if let Err(e) = reader.read().await {
            tracing::error!("kaspa intake loop terminated: {e}");
        }
    });

    // return the receiver so that another task can consumer confirmed blocks
    rx
}
