use sha2::{Digest, Sha256};
use stroemnet_p2p::wire::message::{P2pMsg, ProposalRequest};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AmountV1, CommitmentV1};

use super::state::Node;
use crate::PendingClaim;
use crate::coordinator::Role;
use crate::error::StroemnetError;
use crate::result::Result;

impl Node {
    /// Request a quote across the p2p network
    /// Only used in wasm
    pub async fn request_quote(
        &self,
        swap_id: [u8; 32],
        origin: ChannelId,
        destination: ChannelId,
        amount: String,
    ) -> Result<()> {
        if self.role == Role::Lp {
            return Err(StroemnetError::LpModeForbidsInitiation);
        }

        // Create the proposal request
        let req = ProposalRequest {
            swap_id,
            origin: origin as u8,
            destination: destination as u8,
            amount,
            extra_data: vec![],
        };

        // broadcast the request across the p2p network
        self.network
            .broadcast(&P2pMsg::ProposalRequest(req))
            .await
            .map_err(|e| StroemnetError::Other(format!("broadcast: {e}")))
    }

    /// Register the commitment with ourselves
    /// Used in wasm in order to highlight the fact that
    /// we soon expect our own deposit to come into the chain.
    /// Example: when a kaspa deposit comes in.
    /// Returns the deposit address
    pub async fn register_commitment(
        &self,
        commitment: CommitmentV1,
        secret: [u8; 32],
        expected_amount_out: String,
    ) -> Result<String> {
        if self.role == Role::Lp {
            return Err(StroemnetError::LpModeForbidsInitiation);
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&Sha256::digest(secret));
        if hash != commitment.secret_hash {
            return Err(StroemnetError::SecretHashMismatch);
        }

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

        // this automatically becomes a pending claim which needs
        // to be refunded or fulfilled
        self.pending_claims.write().await.insert(
            swap_id,
            PendingClaim {
                secret,
                expected_counter_chain: destination,
                expected_secret_hash: commitment.secret_hash,
                expected_destination_address: commitment.addresses.sender_destination.clone(),
                expected_amount_out: AmountV1::new(expected_amount_out, destination.decimals()),
            },
        );

        // its only relevant to get the deposit address for utxo based systems
        match source {
            ChannelId::EthereumSepolia | ChannelId::IgraGalleon => {
                serde_json::to_string(&commitment)
                    .map_err(|e| StroemnetError::Other(format!("commitment params: {e}")))
            }
            ChannelId::KaspaTn10 => {
                self.register_kaspa_deposit(source, swap_id, &commitment)
                    .await
            }
        }
    }
}
