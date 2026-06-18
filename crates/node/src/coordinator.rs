use std::sync::Arc;

use futures::StreamExt;
use stroemnet_data::ChainDataSink;
use stroemnet_handler::Handler;
use stroemnet_p2p::P2p;
use stroemnet_p2p::network::NetEvent;
use stroemnet_p2p::wire::message::{P2pMsg, ProposalRequest, ProposalResponse};
#[cfg(target_arch = "wasm32")]
use stroemnet_protocol::now_unix_secs;
use stroemnet_protocol::v1::ChainEvent;
#[cfg(target_arch = "wasm32")]
use stroemnet_protocol::v1::RevealV1;
#[cfg(target_arch = "wasm32")]
use tokio::sync::mpsc;

#[cfg(target_arch = "wasm32")]
use crate::{CheckedQuote, PendingClaim, SwapStage, SwapStatusUpdate};

/// A DoS cap on how much external data we accept,
/// to prevent unbounded memory growth
pub const MAX_REDEEM_SCRIPT_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Lp,
    Observer,
}

/// The coordinator is responsible for binding together the p2p network
/// data sink and the handler
pub struct Coordinator {
    /// The handler is responsible for tracking swaps and enforcing
    /// that they follow correct core protocol logic
    pub handler: Arc<Handler>,
    /// The network is responsible for p2p communication with other nodes
    /// which includes receiving proposals, sending proposals, broadcasting reveals, etc.
    pub network: Arc<P2p>,
    /// The chain data sink is the unit that polls information from the chains
    /// and can also broadcast events to the chains
    pub sink: Arc<ChainDataSink>,
    /// If this node is an LP, it will respond to proposal requests and generate proposals,
    /// if its an observer, it will not respond to proposal requests but will still track swaps and broadcast reveals.
    pub role: Role,
    #[cfg(target_arch = "wasm32")]
    pub quote_tx: mpsc::UnboundedSender<CheckedQuote>,
    #[cfg(target_arch = "wasm32")]
    pub swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
}

impl Coordinator {
    pub fn new(
        handler: Arc<Handler>,
        network: Arc<P2p>,
        sink: Arc<ChainDataSink>,
        role: Role,
        #[cfg(target_arch = "wasm32")] quote_tx: mpsc::UnboundedSender<CheckedQuote>,
        #[cfg(target_arch = "wasm32")] swap_status_tx: mpsc::UnboundedSender<SwapStatusUpdate>,
    ) -> Arc<Self> {
        Arc::new(Self {
            handler,
            network,
            sink,
            role,
            #[cfg(target_arch = "wasm32")]
            quote_tx,
            #[cfg(target_arch = "wasm32")]
            swap_status_tx,
        })
    }

    /// Spawns a background task that listens for incoming network events and dispatches them to the appropriate handlers.
    pub fn spawn_dispatch_loop(
        self: Arc<Self>,
        mut events: futures::channel::mpsc::Receiver<NetEvent>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let fut = async move {
            while let Some(NetEvent { from, msg }) = events.next().await {
                if let Err(e) = self.handle_incoming(&from, msg).await {
                    tracing::warn!("p2p incoming message error from {from}: {e}");
                }
            }
            tracing::info!("p2p coordinator: events channel closed; shutting down");
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            Some(tokio::spawn(fut))
        }
        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(fut);
            None
        }
    }

    /// Handles an incoming P2P message, dispatching it to the appropriate handler based on its type.
    pub(crate) async fn handle_incoming(
        &self,
        from: &str,
        msg: P2pMsg,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let handler = &self.handler;
        let network = &self.network;
        match msg {
            // The State message contains information about the peer's known addresses and is used for peer discovery.
            P2pMsg::State(s) => {
                // process the peer's reported state
                network.process_peer_addrs(s.peers).await;
            }

            // The ProposalRequest message is sent by a peer to request a swap proposal from this node.
            P2pMsg::ProposalRequest(req) => {
                // Attempt to parse the origin and destination
                let origin = stroemnet_protocol::ChannelId::try_from(req.origin);
                let dest = stroemnet_protocol::ChannelId::try_from(req.destination);
                match (origin, dest) {
                    // If both the origin and destination are known channels, proceed with handling the proposal request.
                    (Ok(o), Ok(d)) if handler.knows_channel(o) && handler.knows_channel(d) => {}
                    _ => {
                        tracing::info!(
                            "dropping ProposalRequest from {from} for unknown/mismatched chains origin={} dest={}",
                            req.origin,
                            req.destination
                        );
                        return Ok(());
                    }
                }

                // Forward the proposal request to other peers in the network, if applicable.
                if !network
                    .forward(from, &P2pMsg::ProposalRequest(req.clone()))
                    .await?
                {
                    return Ok(());
                }

                // If we are an LP, handle the proposal request and generate a response.
                if self.role == Role::Lp {
                    self.handle_proposal(from, req).await?;
                }
            }

            // The ProposalResponse message is sent by a peer in response to a ProposalRequest,
            // containing the proposed swap details.
            P2pMsg::ProposalResponse(r) => {
                #[cfg(target_arch = "wasm32")]
                {
                    // LP nodes by definition dont act on proposal responses, and also wasm nodes
                    // are not allowed to be LPs so this is an efficient gate.

                    // Attempt to parse origin and destination
                    let origin = stroemnet_protocol::ChannelId::try_from(r.origin);
                    let dest = stroemnet_protocol::ChannelId::try_from(r.destination);
                    let origin = match (origin, dest) {
                        (Ok(o), Ok(d)) if handler.knows_channel(o) && handler.knows_channel(d) => o,
                        _ => {
                            tracing::info!(
                                "dropping ProposalResponse from {from} for unknown/mismatched chains origin={} dest={}",
                                r.origin,
                                r.destination
                            );
                            return Ok(());
                        }
                    };

                    // Compute the proposal digest for signature verification
                    let digest = stroemnet_p2p::proposal_digest(
                        r.swap_id,
                        r.origin,
                        r.destination,
                        &r.amount_in,
                        &r.amount_out,
                        &r.sender_destination_address,
                        &r.lp_sender_address,
                        r.commit_unlock_offset_secs,
                        r.lp_block_confirmations,
                    );

                    // Verify the LP's signature and check if the LP has sufficient balance for the proposed swap.
                    let (signature_valid, balance_sufficient) = match self
                        .sink
                        .verify_message(
                            origin,
                            digest,
                            &r.lp_sender_address,
                            &r.lp_signature,
                            &r.amount_in,
                        )
                        .await
                    {
                        Ok(v) => (v.address_matches, v.balance_sufficient),
                        Err(e) => {
                            tracing::error!("verify_message failed: {e}");
                            (false, false)
                        }
                    };

                    // Send the checked quote to the quote_tx channel for further processing.
                    // Even if the signature is invalid or the balance is insufficient,
                    // we still send the quote for transparency and logging.
                    // So that the user can see it in the UI.
                    let _ = self.quote_tx.send(CheckedQuote {
                        swap_id: r.swap_id,
                        origin: r.destination,
                        destination: r.origin,
                        amount_in: r.amount_out,
                        amount_out: r.amount_in,
                        sender_destination_address: r.sender_destination_address,
                        commit_unlock_offset_secs: r.commit_unlock_offset_secs,
                        lp_sender_address: r.lp_sender_address,
                        lp_signature: r.lp_signature,
                        lp_block_confirmations: r.lp_block_confirmations,
                        signature_valid,
                        balance_sufficient,
                    });
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = r;
            }

            // The Reveal message is sent by a peer to reveal the secret for a swap, allowing the swap to be completed.
            P2pMsg::Reveal(r) => {
                // Directly forward the reveal to other peers in the network so that everyone can
                // act on it and finalize the swap.
                if !network.forward(from, &P2pMsg::Reveal(r.clone())).await? {
                    return Ok(());
                }
                // Handle the reveal by updating the internal state
                if let Err(e) = handler.handle_external_reveal(r.clone()).await {
                    tracing::error!("reveal state update for {}: {e}", hex::encode(r.swap_id));
                }
                let channels: Vec<stroemnet_protocol::ChannelId> = self.sink.channels().collect();
                // Go over all channels todo: can technically restrict this to only channels that are relevant to the swap
                // but gas wastage is not applicable, the chains check the swap id before acting on it.
                for chan in channels {
                    // Broadcast the reveal event to each channel so 
                    // that the chain data sink can act on it and finalize the swap.
                    if let Err(e) = self
                        .sink
                        .broadcast_event(chan, &ChainEvent::Reveal(r.clone()))
                        .await
                    {
                        tracing::error!(
                            "reveal claim on {chan} for swap {}: {e}",
                            hex::encode(r.swap_id)
                        );
                    }
                }
            }
            // The ScriptAnnounce message is sent by a peer to announce a new redeem script for a swap, 
            // allowing the swap to be identified when we process incoming blocks.
            P2pMsg::ScriptAnnounce(s) => {
                // Reject scripts that are too large to prevent DoS attacks
                if s.redeem_script.len() > MAX_REDEEM_SCRIPT_BYTES {
                    tracing::warn!(
                        "script-announce: rejected from {from} for swap {} — redeem_script too large ({} bytes, max {})",
                        hex::encode(s.swap_id),
                        s.redeem_script.len(),
                        MAX_REDEEM_SCRIPT_BYTES
                    );
                    return Ok(());
                }
                // Forward the script announcement to other peers in the network
                if !network
                    .forward(from, &P2pMsg::ScriptAnnounce(s.clone()))
                    .await?
                {
                    return Ok(());
                }

                // If we have a script channel, register the script with 
                // the chain data sink so that it can be monitored and acted upon.
                let Some(channel_id) = self.sink.script_channel() else {
                    return Ok(());
                };

                // Register the script with the chain data sink, so that chains who rely
                // on scripts will see this script.
                if let Err(e) = self
                    .sink
                    .register_script(
                        channel_id,
                        s.address,
                        s.redeem_script,
                        s.swap_id,
                        s.unlock_ts,
                        s.deposit_target,
                    )
                    .await
                {
                    tracing::warn!(
                        "script-announce: rejected from {from} for swap {} — {e}",
                        hex::encode(s.swap_id)
                    );
                }
            }
        }
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    /// Emits a swap status update to the swap_status_tx channel, indicating the current stage of the swap.
    pub(crate) fn emit_status(&self, swap_id: [u8; 32], stage: SwapStage) {
        let _ = self.swap_status_tx.send(SwapStatusUpdate {
            swap_id,
            stage,
            at: now_unix_secs(),
        });
    }

    #[cfg(target_arch = "wasm32")]
    /// Creates a new reveal broadcast across the network for the given swap ID and pending claim, and emits a status update.
    pub fn spawn_reveal_broadcast(&self, swap_id: [u8; 32], claim: PendingClaim) {
        let network = self.network.clone();
        let swap_status_tx = self.swap_status_tx.clone();

        // Create a new future that will broadcast the reveal and emit a status update, and spawn it as a background task.
        let fut = async move {
            // Create a new revealv1 message
            let reveal = RevealV1::new(swap_id, claim.secret);

            // Broadcast the reveal across the network and emit a status update based on the result.
            let stage = match network.broadcast(&P2pMsg::Reveal(reveal)).await {
                Ok(()) => {
                    tracing::info!(
                        "auto-claim: reveal broadcast for swap {}",
                        hex::encode(swap_id)
                    );
                    SwapStage::Completed
                }
                Err(e) => {
                    tracing::warn!(
                        "auto-claim: reveal broadcast failed for swap {}: {e}",
                        hex::encode(swap_id)
                    );
                    SwapStage::Failed {
                        reason: format!("reveal broadcast: {e}"),
                    }
                }
            };

            // Now emit the swap status update to the swap_status_tx channel, indicating the current stage of the swap.
            let _ = swap_status_tx.send(SwapStatusUpdate {
                swap_id,
                stage,
                at: now_unix_secs(),
            });
        };
        stroemnet_protocol::spawn(fut);
    }

    /// Handles an incoming proposal request from a peer, generating a proposal response if this node is an LP.
    async fn handle_proposal(
        &self,
        from: &str,
        req: ProposalRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use stroemnet_handler::handle::proposal::SwapRequest;

        // Parse the swap request
        let swap_request = SwapRequest {
            origin: req.origin,
            destination: req.destination,
            amount: req.amount.clone(),
        };

        // Create a proposal for this request.
        let proposal = self.handler.create_proposal(&swap_request).await?;

        // Compute the origin and destination as u8's
        let origin = proposal.origin;
        let origin_u8 = origin as u8;
        let destination_u8 = proposal.destination as u8;

        // Retrieve the number of block confirmations 
        // that we require so that the user can be aware of how long this swap will take.
        let lp_block_confirmations = self
            .handler
            .block_confirmations
            .get(&proposal.destination)
            .copied()
            .unwrap_or(0);

        // Retrieve the LP's address for the origin channel, which will be used in the proposal response.
        let lp_sender_address = self.sink.lp_address(origin)?;

        // Compute a hash of the proposal details, which we will sign
        // in order to prove our identity.
        let digest = stroemnet_p2p::proposal_digest(
            req.swap_id,
            origin_u8,
            destination_u8,
            &proposal.amount_in,
            &proposal.amount_out,
            &proposal.sender_destination_address,
            &lp_sender_address,
            proposal.commit_unlock_offset_secs,
            lp_block_confirmations,
        );
        // Sign the message with our LP's private key for the origin channel, 
        // which will be included in the proposal response.
        let (_addr, lp_signature) = self
            .sink
            .sign_message(origin, digest, &proposal.amount_in)
            .await?;

        // Generate the proposal response and send it back to the requesting peer.
        let resp = ProposalResponse {
            swap_id: req.swap_id,
            origin: origin_u8,
            destination: destination_u8,
            amount_in: proposal.amount_in,
            amount_out: proposal.amount_out,
            sender_destination_address: proposal.sender_destination_address,
            commit_unlock_offset_secs: proposal.commit_unlock_offset_secs,
            lp_sender_address,
            lp_signature,
            lp_block_confirmations,
            extra_data: vec![],
        };

        // Broadcast the proposal response back to the requesting peer, completing the proposal handling process.
        self.network
            .send_to(from, &P2pMsg::ProposalResponse(resp))
            .await?;
        Ok(())
    }
}
