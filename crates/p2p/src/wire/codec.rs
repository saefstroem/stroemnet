use borsh::BorshDeserialize;

use super::message::P2pMsg;
use crate::Result;
use crate::error::StroemnetP2pError;

pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

pub fn encode(msg: &P2pMsg) -> Result<Vec<u8>> {
    borsh::to_vec(msg).map_err(StroemnetP2pError::Codec)
}

pub fn decode(bytes: &[u8]) -> Result<P2pMsg> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(StroemnetP2pError::MessageTooLarge {
            size: bytes.len(),
            max: MAX_MESSAGE_BYTES,
        });
    }
    P2pMsg::try_from_slice(bytes).map_err(StroemnetP2pError::Codec)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::wire::message::{
        NodeState, PeerAddr, ProposalError, ProposalRequest, ProposalResponse, ScriptAnnounce,
    };
    use stroemnet_protocol::v1::RevealV1;

    fn rt(msg: P2pMsg) {
        let bytes = encode(&msg).unwrap();
        assert_eq!(decode(&bytes).unwrap(), msg);
    }

    #[test]
    fn state_roundtrip() {
        rt(P2pMsg::State(NodeState {
            node_id: [42; 32],
            listen_addr: None,
            peers: Vec::new(),
        }));

        rt(P2pMsg::State(NodeState {
            node_id: [43; 32],
            listen_addr: Some("ws://node.example:3000".into()),
            peers: vec![PeerAddr {
                url: "wss://other.example".into(),
                last_seen: 1700000000,
            }],
        }));
    }

    #[test]
    fn proposal_roundtrip() {
        rt(P2pMsg::ProposalRequest(ProposalRequest {
            swap_id: [1; 32],
            origin: 0,
            destination: 1,
            amount: "1000".into(),
            extra_data: vec![],
        }));
        rt(P2pMsg::ProposalResponse(ProposalResponse {
            swap_id: [2; 32],
            origin: 1,
            destination: 0,
            amount_in: "1000".into(),
            amount_out: "999".into(),
            sender_destination_address: "0xabc".into(),
            commit_unlock_offset_secs: 1500,
            lp_sender_address: "0xLpSender".into(),
            lp_signature: vec![0x22; 65],
            lp_block_confirmations: 12,
            extra_data: vec![],
        }));
    }

    #[test]
    fn reveal_roundtrip() {
        rt(P2pMsg::Reveal(RevealV1::new([3; 32], [4; 32])));
    }

    #[test]
    fn proposal_error_roundtrip() {
        rt(P2pMsg::ProposalError(ProposalError {
            swap_id: [9; 32],
            origin: 1,
            destination: 0,
            reason: "Trade amount 1 USD value 0.5 is below minimum of 1 USD".into(),
        }));
    }

    #[test]
    fn script_announce_roundtrip() {
        rt(P2pMsg::ScriptAnnounce(ScriptAnnounce {
            address: "kaspatest:qpzry9x8gf2tvdw0s3jn54khce6mua7l".into(),
            swap_id: [11; 32],
            redeem_script: vec![0xab, 0xcd, 0xef],
            unlock_ts: 1700000000,
            deposit_target: "100000000".into(),
        }));
    }

    #[test]
    fn decode_rejects_oversized_message() {
        let oversize = vec![0u8; MAX_MESSAGE_BYTES + 1];
        match decode(&oversize) {
            Err(StroemnetP2pError::MessageTooLarge { size, max }) => {
                assert_eq!(size, MAX_MESSAGE_BYTES + 1);
                assert_eq!(max, MAX_MESSAGE_BYTES);
            }
            other => panic!("expected MessageTooLarge, got {other:?}"),
        }
    }
}
