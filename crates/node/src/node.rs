use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ahash::AHashMap;
use stroemnet_amounts::PriceStorage;
use stroemnet_data::ChainDataSink;
use stroemnet_data::CursorStore;
use stroemnet_handler::{Effect, Handler, HandlerConfig};
use stroemnet_p2p::wire::message::{P2pMsg, ScriptAnnounce};
use stroemnet_p2p::{P2p, P2pConfig};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::swap_tracker::SwapTracker;
use stroemnet_protocol::v1::ChainEvent;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

use crate::connection::spawn_bootstrap_with_counter;
#[cfg(not(target_arch = "wasm32"))]
use crate::connection::{spawn_accept, spawn_addr_dial_driver};
#[cfg(not(target_arch = "wasm32"))]
use crate::oracle::Oracle;

use crate::coordinator::{Coordinator, Role};
use crate::error::StroemnetError;
use crate::result::Result;
use crate::{ChannelSpec, NodeConfig};

#[cfg(target_arch = "wasm32")]
use crate::{CheckedQuote, PendingClaim, SwapStage, SwapStatusUpdate, pending_claim_matches};
#[cfg(target_arch = "wasm32")]
use sha2::{Digest, Sha256};
#[cfg(target_arch = "wasm32")]
use stroemnet_protocol::now_unix_secs;
#[cfg(target_arch = "wasm32")]
use stroemnet_protocol::v1::{AmountV1, CommitmentV1};

/// The main driver interval in milliseconds for the node
/// which represents how often the node checks for new chain events and processes them.
const DRIVER_TICK_MS: u64 = 1000;

/// The core struct encapsulating the main functionality
/// of the stroemnet node, including the handler, p2p network, chain data sink, and pending claims management.
pub struct Node {
    pub handler: Arc<Handler>,
    pub network: Arc<P2p>,
    pub peer_count: Arc<AtomicUsize>,

    #[cfg(target_arch = "wasm32")]
    sink: Arc<ChainDataSink>,
    #[cfg(target_arch = "wasm32")]
    pending_claims: Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    #[cfg(target_arch = "wasm32")]
    swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
    role: Role,

    #[cfg(not(target_arch = "wasm32"))]
    tasks: Vec<JoinHandle<()>>,
}

#[cfg(target_arch = "wasm32")]
type StartOutput = (Node, mpsc::UnboundedReceiver<CheckedQuote>);
#[cfg(not(target_arch = "wasm32"))]
type StartOutput = Node;

impl Node {
    /// Starts the stroemnet node with the given configuration, initializing all components and spawning necessary tasks.
    pub async fn start(
        cfg: NodeConfig,
        cursor_store: Option<Arc<dyn CursorStore>>,
    ) -> Result<StartOutput> {
        tracing::info!("Starting stroemnet node...");

        // Channel for receiving quote updates from the oracle to be exposed to the user
        #[cfg(target_arch = "wasm32")]
        let (quote_tx, quote_rx) = mpsc::unbounded_channel();

        // Channel for emitting swap status updates to the user, if configured
        #[cfg(target_arch = "wasm32")]
        let swap_status_tx = cfg.swap_status_tx.clone();
        let role = cfg.role;
        #[cfg(not(target_arch = "wasm32"))]
        let oracle_interval_secs = cfg.price_oracle_update_interval_secs;

        // Build the handler and chain data sink based on the provided configuration,
        // which includes channel specifications and other parameters.
        let (handler, sink) =
            build_handler_and_sink(cfg.handler, cfg.channels, cursor_store).await?;

        #[cfg(not(target_arch = "wasm32"))]
        // only native nodes participate in discovering and dialing peers, since they
        // cannot directly accept incoming connections
        let (discovered_peer_dial_tx, mut discovered_peer_dial_rx) =
            mpsc::unbounded_channel::<String>();

        // Prepare the p2p network configuration
        // which contains the bootstrap peers,
        // the advertised listen address,
        // and the channel for discovered peers to be dialed.
        // but on wasm we do not provide the discovered_peer_dial_tx
        // since we dont do peer discovery or dialing on wasm
        let net_config = P2pConfig {
            bootstrap_peers: cfg.bootstrap_peers,
            advertised_listen_addr: cfg.advertised_listen_addr.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            discovered_peer_dial_tx: Some(discovered_peer_dial_tx.clone()),
            ..P2pConfig::default()
        };

        // Instantiate p2p instance with the network configuration
        let (network, net_events) = P2p::new(net_config);

        // Create an Arc for the network to be shared across tasks
        let network = Arc::new(network);

        // Initialize peer count as 0
        let peer_count = Arc::new(AtomicUsize::new(0));

        // Create a container for tracking pending claims
        #[cfg(target_arch = "wasm32")]
        let pending_claims: Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>> =
            Arc::new(RwLock::new(AHashMap::new()));

        // Create a new p2p coordinator
        let coordinator = Coordinator::new(
            handler.clone(),
            network.clone(),
            sink.clone(),
            role,
            #[cfg(target_arch = "wasm32")]
            quote_tx,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx.clone(),
        );

        // Spawn the p2p coordinator's dispatch loop to handle incoming messages and dispatch them to the handler.
        let _coordinator_task = coordinator.clone().spawn_dispatch_loop(net_events);

        #[cfg(not(target_arch = "wasm32"))]
        // collect all tasks so that we can manage their lifetimes and shutdown properly
        let mut tasks: Vec<JoinHandle<()>> = _coordinator_task.into_iter().collect();

        // Spawn the main driver loop which periodically checks for new
        // chain events from the sink and processes them through the handler.
        // This happens for both wasm and native.
        spawn_driver_loop(
            sink.clone(),
            handler.clone(),
            network.clone(),
            #[cfg(target_arch = "wasm32")]
            coordinator.clone(), // coordinator on native is ran above
            #[cfg(target_arch = "wasm32")]
            pending_claims.clone(), // pending claims are only used in wasm since they are from user perspective.
            #[cfg(not(target_arch = "wasm32"))]
            &mut tasks,
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            // For native only, a future that listens for discovered peers from the discovery mechanism and dials them.
            let net = network.clone();
            let counter = peer_count.clone();
            let drainer = async move {
                use std::collections::HashSet;
                use std::sync::Mutex;
                // Contains the set of peer URLs that are alredy being dialed
                // to avoid duplicate tasks for the same peer.
                let in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

                // Drain discovered peers and attempt to dial them
                while let Some(url) = discovered_peer_dial_rx.recv().await {
                    let url_norm = url.trim_end_matches('/').to_ascii_lowercase();
                    if net.is_connected_peer(&url_norm).await {
                        continue;
                    }
                    {
                        let mut set = in_flight.lock().unwrap();
                        if !set.insert(url_norm.clone()) {
                            continue;
                        }
                    }
                    let net = net.clone();
                    let counter = counter.clone();
                    let in_flight = in_flight.clone();
                    let url_norm_clone = url_norm.clone();
                    // Spawn the dial task so we can check if this peer
                    // is real
                    spawn_addr_dial_driver(net, url, counter, in_flight, url_norm_clone);
                }
                tracing::info!("discovery: dial channel closed, dialer exiting");
            };
            // Spawn the drainer task to handle discovered peers
            tasks.push(tokio::spawn(drainer));
        }

        #[cfg(not(target_arch = "wasm32"))]
        if role == Role::Lp {
            // If this is an LP then we need to create the price oracle
            let oracle = Oracle::new(handler.price_storage.clone(), oracle_interval_secs)?;
            // and run the price fetching
            tasks.push(oracle.run_loop());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let tracker = handler.swap_tracker.clone();
            // Spawn a periodic cleanup task to remove old swaps from the tracker every hour
            // removes completed swaps that are older than 24 hours to prevent unbounded growth of the tracker state
            tasks.push(tokio::spawn(async move {
                loop {
                    stroemnet_protocol::sleep_secs(3600).await;
                    tracker.write().await.cleanup_old_swaps(86400);
                }
            }));
        }

        // Spawns a task to continously attempt to connect to bootstrap peers and open
        // connections with them
        spawn_bootstrap_with_counter(network.clone(), peer_count.clone());

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(bind_addr) = cfg.bind_addr {
            // If we are on native we open the accept loop to allow incoming p2p connections on the bind address
            spawn_accept(bind_addr, network.clone(), peer_count.clone(), &mut tasks).await;
        }

        // Spawn a task to periodically broadcast the node's state to its peers,
        // This shares information about the nodes current state such as its peers
        network.clone().spawn_periodic_state_broadcast(60);

        tracing::info!("stroemnet node started successfully");

        let node = Node {
            handler,
            network,
            peer_count,
            #[cfg(target_arch = "wasm32")]
            sink,
            #[cfg(target_arch = "wasm32")]
            pending_claims,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx,
            role,
            #[cfg(not(target_arch = "wasm32"))]
            tasks,
        };

        #[cfg(target_arch = "wasm32")]
        {
            Ok((node, quote_rx))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(node)
        }
    }

    /// Returns the current number of connected peers as tracked by the node.
    pub fn peer_count(&self) -> usize {
        self.peer_count.load(Ordering::SeqCst)
    }

    #[cfg(target_arch = "wasm32")]
    /// Initiates a quote request for a swap by broadcasting a ProposalRequest message to the network.
    pub async fn request_quote(
        &self,
        swap_id: [u8; 32],
        origin: ChannelId,
        destination: ChannelId,
        amount: String,
    ) -> Result<()> {
        use stroemnet_p2p::wire::message::ProposalRequest;
        if self.role == Role::Lp {
            return Err(StroemnetError::LpModeForbidsInitiation);
        }
        // create a proposal request and broadcast it to the network
        let req = ProposalRequest {
            swap_id,
            origin: origin as u8,
            destination: destination as u8,
            amount,
            extra_data: vec![],
        };
        self.network
            .broadcast(&P2pMsg::ProposalRequest(req))
            .await
            .map_err(|e| StroemnetError::Other(format!("broadcast: {e}")))?;
        Ok(())
    }

    pub fn role(&self) -> Role {
        self.role
    }

    #[cfg(target_arch = "wasm32")]
    /// Emits a swap status update through the swap status tx
    fn emit_status(&self, swap_id: [u8; 32], stage: SwapStage) {
        let _ = self.swap_status_tx.send(SwapStatusUpdate {
            swap_id,
            stage,
            at: now_unix_secs(),
        });
    }

    #[cfg(target_arch = "wasm32")]
    /// Registers a commitment and its secret with the node, verifying the secret matches the
    /// commitment hash and returning the source-chain deposit target. Does not submit the
    /// commitment on-chain — the caller performs the deposit separately.
    pub async fn register_commitment(
        &self,
        commitment: CommitmentV1,
        secret: [u8; 32],
        expected_amount_out: String,
    ) -> Result<String> {
        // Only if the node is not an lp,
        // however wasm32 should never even be LP nodes so this is technically
        // unreachable
        if self.role == Role::Lp {
            return Err(StroemnetError::LpModeForbidsInitiation);
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&Sha256::digest(secret));

        // We only allow the actual hash to be submitted
        if hash != commitment.secret_hash {
            return Err(StroemnetError::SecretHashMismatch);
        }

        // Parse the source and destination channel ids
        let source = ChannelId::try_from(commitment.source)
            .map_err(|e| StroemnetError::Other(format!("source channel id: {e}")))?;
        let destination = ChannelId::try_from(commitment.destination)
            .map_err(|e| StroemnetError::Other(format!("destination channel id: {e}")))?;
        let swap_id = commitment.swap_id;

        let expected_value = expected_amount_out.parse::<u128>().map_err(|_| {
            StroemnetError::Other(format!(
                "expected_amount_out must be a base-unit integer: {expected_amount_out}"
            ))
        })?;
        if expected_value == 0 {
            return Err(StroemnetError::Other(
                "expected_amount_out must be greater than zero".into(),
            ));
        }
        let expected_amount_out = AmountV1::new(expected_amount_out, destination.decimals());

        // Store the pending claim information in the pending claims
        // as we are waiting for this claim to eventually be claimable
        // once the swap is ready to "finalize".
        self.pending_claims.write().await.insert(
            swap_id,
            PendingClaim {
                secret,
                expected_counter_chain: destination,
                expected_secret_hash: commitment.secret_hash,
                expected_destination_address: commitment.addresses.sender_destination.clone(),
                expected_amount_out,
            },
        );

        // Return the "deposit address", for ethereum
        // its just information about the swap itself but the caller already has this info
        // only for kaspa rn do we return unique information in the way that we compute the p2sh
        // address and redeem script.
        match source {
            ChannelId::EthereumSepolia | ChannelId::IgraGalleon => {
                serde_json::to_string(&commitment)
                    .map_err(|e| StroemnetError::Other(format!("commitment params: {e}")))
            }
            ChannelId::KaspaTn10 => {
                // derive p2sh address and redeem scrit
                let (p2sh, redeem) = self
                    .sink
                    .derive_deposit(source, &commitment)
                    .map_err(|e| StroemnetError::Other(format!("kaspa deposit derive: {e}")))?;
                let target = commitment.amount.value.clone();

                // register this script so that we can detect the deposit when it happens
                self.sink
                    .register_script(
                        source,
                        p2sh.clone(),
                        redeem.clone(),
                        swap_id,
                        commitment.unlock_ts,
                        target.clone(),
                    )
                    .await
                    .map_err(|e| StroemnetError::Other(format!("register script: {e}")))?;

                // Other lp nodes need to be able to see this script announcement so we broadcast it to the network.
                let announce = ScriptAnnounce {
                    address: p2sh.clone(),
                    swap_id,
                    redeem_script: redeem,
                    unlock_ts: commitment.unlock_ts,
                    deposit_target: target.clone(),
                };

                // Broadcast it over p2p
                if let Err(e) = self
                    .network
                    .broadcast(&P2pMsg::ScriptAnnounce(announce))
                    .await
                {
                    tracing::warn!("kas-source script announce failed: {e}");
                }
                self.emit_status(
                    swap_id,
                    SwapStage::AwaitingDeposit {
                        chain: source,
                        address: p2sh.clone(),
                        deposit_target: Some(target),
                    },
                );
                Ok(p2sh)
            }
        }
    }

    pub fn shutdown(self) {
        #[cfg(not(target_arch = "wasm32"))]
        // in wasm we dont have multithreaded env
        // so there is nothing to abort
        for t in &self.tasks {
            t.abort();
        }
        tracing::info!("stroemnet node shut down");
    }
}

/// Helper function to build the configured handler and chain
/// data sink which provides the system with onchain data
async fn build_handler_and_sink(
    handler_config: HandlerConfig,
    channels: AHashMap<ChannelId, ChannelSpec>,
    cursor_store: Option<Arc<dyn CursorStore>>,
) -> Result<(Arc<Handler>, Arc<ChainDataSink>)> {
    // We extract the channel ids from the configuration to initialize the price storage and swap tracker,
    let channel_ids: Vec<ChannelId> = channels.keys().copied().collect();
    let price_storage = PriceStorage::new(channel_ids);
    let swap_tracker = Arc::new(RwLock::new(SwapTracker::new()));

    let mut block_confirmations_map = AHashMap::new();
    let mut sink_channels = AHashMap::new();
    let mut keyed_channels = Vec::new();
    // Build the channels into their respective collections
    for (id, spec) in channels {
        if spec.lp_private_key.is_some() {
            keyed_channels.push(id);
        }
        block_confirmations_map.insert(id, spec.minimum_block_confirmations());
        sink_channels.insert(id, (spec.config, spec.lp_private_key));
    }

    // Create a new chain data sink
    let sink = ChainDataSink::new(sink_channels, cursor_store)
        .await
        .map_err(|e| StroemnetError::Other(format!("chain data sink: {e}")))?;

    let mut address_map = AHashMap::new();

    // Build the lp address lookup table which maps
    // a channel id to our lp address on that channel/chain
    for id in keyed_channels {
        let address = sink
            .lp_address(id)
            .map_err(|e| StroemnetError::Other(format!("lp address for {id}: {e}")))?;
        tracing::info!("Derived LP address for {id}: {address}");
        address_map.insert(id, address);
    }

    // Create the handler
    let handler = Arc::new(Handler::new(
        price_storage,
        swap_tracker,
        handler_config,
        Arc::new(address_map),
        Arc::new(block_confirmations_map),
    ));

    Ok((handler, Arc::new(sink)))
}

/// The main driver loop of the node which continuously checks
/// for new finalized chain events from the sink and processes
/// them through the handler, as well as broadcasting any new script announcements to the network.
fn spawn_driver_loop(
    sink: Arc<ChainDataSink>,
    handler: Arc<Handler>,
    network: Arc<P2p>,
    #[cfg(target_arch = "wasm32")] coordinator: Arc<Coordinator>, // coordinator
    #[cfg(target_arch = "wasm32")] pending_claims: Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    #[cfg(not(target_arch = "wasm32"))] tasks: &mut Vec<JoinHandle<()>>,
) {
    let fut = async move {
        loop {
            // get a new finalized chunk of chain events from the sink and apply them through the handler
            match sink.finalized_chunk().await {
                Ok(events) => {
                    for (source, event) in events {
                        // apply events
                        apply_event(
                            &sink,
                            &handler,
                            #[cfg(target_arch = "wasm32")]
                            &coordinator,
                            #[cfg(target_arch = "wasm32")]
                            &pending_claims,
                            source,
                            event,
                        )
                        .await;
                    }
                }
                Err(e) => tracing::warn!("finalized_chunk: {e}"),
            }
            // Check if there are any new script announcements from the sink
            // and broadcast them to the network so that other nodes can detect deposits to these scripts.
            for a in sink.take_utxo_script_announcements() {
                let msg = P2pMsg::ScriptAnnounce(ScriptAnnounce {
                    address: a.address,
                    swap_id: a.swap_id,
                    redeem_script: a.script.redeem_script,
                    unlock_ts: a.script.unlock_ts,
                    deposit_target: a.script.deposit_target,
                });
                if let Err(e) = network.broadcast(&msg).await {
                    tracing::warn!("script-announce broadcast failed: {e}");
                }
            }
            stroemnet_protocol::sleep_ms(DRIVER_TICK_MS).await;
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    tasks.push(tokio::spawn(fut));
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(fut);
}

/// Computes a side effect from the result of processing an event
/// and forward the effect to the appropriate channel
async fn apply_event(
    sink: &Arc<ChainDataSink>,
    handler: &Arc<Handler>,
    #[cfg(target_arch = "wasm32")] coordinator: &Arc<Coordinator>,
    #[cfg(target_arch = "wasm32")] pending_claims: &Arc<RwLock<AHashMap<[u8; 32], PendingClaim>>>,
    source: ChannelId,
    event: ChainEvent,
) {
    #[cfg(target_arch = "wasm32")]
    if let ChainEvent::Commitment(c) = &event {
        let own_deposit = pending_claims
            .read()
            .await
            .get(&c.swap_id)
            .map(|claim| source != claim.expected_counter_chain)
            .unwrap_or(false);
        if own_deposit {
            coordinator.emit_status(c.swap_id, SwapStage::Locked);
        }
    }

    // Compute the effects from the event through the handler, if there is an error we log it and skip to the next event
    let clock = sink.chain_clock();
    let effects = match handler.on_chain_event(source, event, &clock).await {
        Ok(effects) => effects,
        Err(e) => {
            tracing::warn!("on_chain_event: {e}");
            return;
        }
    };
    for effect in effects {
        match effect {
            // Broadcast the event to the network so that other nodes can update their state accordingly
            Effect::Broadcast(channel_id, ev) => {
                if let Err(e) = sink.broadcast_event(channel_id, &ev).await {
                    tracing::error!("broadcast_event to {channel_id}: {e}");
                }
            }
            // If it is the case that we should transmit the reveal of the secret
            // then we do it here
            Effect::TransmitReveal(detected) => {
                #[cfg(target_arch = "wasm32")]
                {
                    let c = &detected.commitment;
                    let claim_to_fire = {
                        // retrieve the claim that we are supposed to reveal
                        let mut map = pending_claims.write().await;
                        if let Some(claim) = map.remove(&c.swap_id) {
                            if pending_claim_matches(&claim, c) {
                                Some(claim)
                            } else {
                                map.insert(c.swap_id, claim);
                                None
                            }
                        } else {
                            None
                        }
                    };

                    // if the claim is found spawn a new reveal broadcast
                    // so that the secret is broadcasted over the p2p network
                    if let Some(claim) = claim_to_fire {
                        coordinator.spawn_reveal_broadcast(c.swap_id, claim);
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = detected;
            }
        }
    }
}
