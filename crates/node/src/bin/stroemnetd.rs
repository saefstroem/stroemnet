#[cfg(not(target_arch = "wasm32"))]
fn main() -> stroemnet_node::result::Result<()> {
    daemon::main()
}

// The daemon is native-only; wasm builds (the SDK) never run this binary.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod daemon {
    use std::path::Path;
    use std::sync::Arc;

    use stroemnet_node::Node;
    use stroemnet_node::config::DaemonConfig;
    use stroemnet_node::error::StroemnetError;
    use stroemnet_node::result::Result;
    use stroemnet_storage::{Peer, PeerDb};
    use url::Url;

    #[tokio::main]
    /// Main entry point for stroemnet node
    pub async fn main() -> Result<()> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        tracing_subscriber::fmt::init();

        // Config path: first positional argument, defaulting to `stroemnet.toml`.
        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "stroemnet.toml".to_string());
        let config = DaemonConfig::load(Path::new(&config_path))?;

        let lp_mode = config.lp;
        if lp_mode {
            tracing::info!("LP mode enabled — trade initiation disabled");
        }

        // Initialize the peer db which is used for peer persistence
        let peer_db_path = config.peer_db.clone();
        let peer_db = Arc::new(PeerDb::new(Path::new(&peer_db_path))?);

        // Get all saved peers
        let saved_peers: Vec<String> = peer_db
            .get_peers()?
            .into_iter()
            .map(|p| p.url.into())
            .collect();
        if !saved_peers.is_empty() {
            tracing::info!(
                "loaded {} peer(s) from {} for redial",
                saved_peers.len(),
                peer_db_path
            );
        }

        // Build the node config from the file, merging saved peers into the bootstrap set.
        let node_config = config.into_node_config(saved_peers)?;
        let cursor_store: Arc<dyn stroemnet_data::CursorStore> =
            Arc::new(stroemnet_storage::DbCursorStore::new(peer_db.clone()));
        let node = Node::start(node_config, Some(cursor_store)).await?;

        let network_clone = node.network.clone();
        let peer_db_clone = peer_db.clone();

        // Spawn a periodic task to save in-memory peers to the disk for persistence
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.tick().await;
            loop {
                tick.tick().await;
                // Get the currently connected peers from the network
                let urls: Vec<String> = network_clone
                    .connected_peers
                    .lock()
                    .await
                    .iter()
                    .filter_map(|p| p.advertised_listen.clone())
                    .collect();

                // Save all peers to the db, if they are not already present
                for url_s in urls {
                    let Ok(url) = Url::parse(&url_s) else {
                        continue;
                    };

                    // If there is no peer there already, add it, otherwise do nothing
                    if let Ok(None) = peer_db_clone.get_peer(&url)
                        && let Err(e) = peer_db_clone.add_peer(Peer { url }) {
                            tracing::warn!("peer persist failed for {url_s}: {e}");
                        }
                }
            }
        });

        // Finally wait for shutdown signal and shutdown the node gracefully
        wait_for_shutdown_signal().await?;

        // Stop the node and all its tasks
        node.shutdown();
        Ok(())
    }

    #[cfg(unix)]
    async fn wait_for_shutdown_signal() -> Result<()> {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| StroemnetError::Other(format!("Failed to setup SIGTERM handler: {e}")))?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => tracing::info!("Received SIGINT, shutting down"),
            _ = sigterm.recv() => tracing::info!("Received SIGTERM, shutting down"),
        }
        Ok(())
    }

    #[cfg(not(unix))]
    async fn wait_for_shutdown_signal() -> Result<()> {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for ctrl-c: {e}");
        }
        tracing::info!("Received SIGINT, shutting down");
        Ok(())
    }
}
