#[cfg(not(target_arch = "wasm32"))]
fn main() -> stroemnet_node::result::Result<()> {
    daemon::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod daemon {
    use std::path::Path;
    use std::sync::Arc;

    use stroemnet_data::{CursorStore, SwapStore};
use stroemnet_node::Node;
    use stroemnet_node::config::DaemonConfig;
    use stroemnet_node::error::StroemnetError;
    use stroemnet_node::result::Result;
    use stroemnet_storage::{DbCursorStore, DbSwapStore, Peer, PeerDb};
    use url::Url;

    #[tokio::main]
    /// The main entrypoint for the stroemnet daemon
    pub async fn main() -> Result<()> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        tracing_subscriber::fmt::init();

        /// Read the confguration for the node
        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "stroemnet.toml".to_string());
        let config = DaemonConfig::load(Path::new(&config_path))?;

        let lp_mode = config.lp;
        if lp_mode {
            tracing::info!("LP mode enabled — trade initiation disabled");
        }

        // Load or create the peer database
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

        // Compute node configuration and also configure the peers
        let node_config = config.into_node_config(saved_peers)?;
        let cursor_store: Arc<dyn CursorStore> =
            Arc::new(DbCursorStore::new(peer_db.clone()));
        let swap_store: Arc<dyn SwapStore> =
            Arc::new(DbSwapStore::new(peer_db.clone()));

        // Start the node on a separate tokio task
        let node = Node::start(node_config, Some(cursor_store), Some(swap_store)).await?;

        let network_clone = node.network.clone();
        let peer_db_clone = peer_db.clone();

        // Create a periodic task to store connected peers to disk
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.tick().await;
            loop {
                tick.tick().await;
                let urls: Vec<String> = network_clone
                    .connected_peers
                    .lock()
                    .await
                    .iter()
                    .filter_map(|p| p.advertised_listen.clone())
                    .collect();

                for url_s in urls {
                    let Ok(url) = Url::parse(&url_s) else {
                        continue;
                    };

                    if let Ok(None) = peer_db_clone.get_peer(&url)
                        && let Err(e) = peer_db_clone.add_peer(Peer { url })
                    {
                        tracing::warn!("peer persist failed for {url_s}: {e}");
                    }
                }
            }
        });

        wait_for_shutdown_signal().await?;

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
