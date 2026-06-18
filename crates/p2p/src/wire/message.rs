use borsh::{BorshDeserialize, BorshSerialize};
use stroemnet_protocol::v1::RevealV1;

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum P2pMsg {
    State(NodeState),
    ProposalRequest(ProposalRequest),
    ProposalResponse(ProposalResponse),

    Reveal(RevealV1),

    ScriptAnnounce(ScriptAnnounce),
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NodeState {
    pub node_id: [u8; 32],

    pub listen_addr: Option<String>,

    pub peers: Vec<PeerAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub struct PeerAddr {
    pub url: String,

    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProposalRequest {
    pub swap_id: [u8; 32],
    pub origin: u8,
    pub destination: u8,
    pub amount: String,
    pub extra_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ProposalResponse {
    pub swap_id: [u8; 32],
    pub origin: u8,
    pub destination: u8,
    pub amount_in: String,
    pub amount_out: String,
    pub sender_destination_address: String,
    pub commit_unlock_offset_secs: u64,
    pub lp_sender_address: String,
    pub lp_signature: Vec<u8>,
    pub lp_block_confirmations: u64,
    pub extra_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ScriptAnnounce {
    pub address: String,
    pub swap_id: [u8; 32],
    pub redeem_script: Vec<u8>,
    pub unlock_ts: u64,

    pub deposit_target: String,
}
