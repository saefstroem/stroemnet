use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, CommitmentV1};

use crate::{BufFut, DataError, MaybeSend, ProposalVerification, Result, ScriptAnnouncement};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type TaskFut = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
#[cfg(target_arch = "wasm32")]
pub(crate) type TaskFut = Pin<Box<dyn Future<Output = ()> + 'static>>;

pub(crate) trait ChainDataBuffer: MaybeSend {
    /// Used to retrieve the lp address for the nodes that are LPs or do CCR
    fn lp_address(&self) -> Result<String>;

    /// The settler task that drives settlement of detected swaps/claims/refunds
    fn settler_task(self: Arc<Self>) -> Option<TaskFut> {
        None
    }

    /// Compute the deposit address based on some commitment
    fn derive_deposit(&self, _commitment: &CommitmentV1) -> Result<(String, Vec<u8>)> {
        Err(DataError::Other(
            "channel does not support P2SH deposits".into(),
        ))
    }

    /// Retrieves the next finalized chunk of data from all registered channels
    fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>>;

    /// Compute the timestamp for some chaindatabuffer
    fn chain_now(&self) -> Option<u64> {
        None
    }

    /// Broadcasts an event
    fn broadcast_event<'a>(&'a self, event: &'a ChainEvent) -> BufFut<'a, ()>;

    /// Signs a message whilst on the requirement that a minimumm balance is maintained
    fn sign_message<'a>(
        &'a self,
        digest: [u8; 32],
        required_balance: &'a str,
    ) -> BufFut<'a, (String, Vec<u8>)>;

    /// Verifies the signature of a message whilst also guaranteeing that the claimed address
    /// has the require balance specified
    fn verify_message<'a>(
        &'a self,
        digest: [u8; 32],
        claimed_address: &'a str,
        signature: &'a [u8],
        required_balance: &'a str,
    ) -> BufFut<'a, ProposalVerification>;

    /// An element that detects utxo scripts and matches them to swaps based on their p2sh signature
    fn utxo_script_detector(&self) -> Option<&dyn UtxoScriptDetector> {
        None
    }

    /// Extract and remove all the current utxo script announcements
    fn take_utxo_script_announcements(&self) -> Vec<ScriptAnnouncement> {
        Vec::new()
    }
}

pub(crate) trait UtxoScriptDetector: MaybeSend {
    fn register_script<'a>(
        &'a self,
        address: String,
        redeem_script: Vec<u8>,
        swap_id: [u8; 32],
        unlock_ts: u64,
        deposit_target: String,
    ) -> BufFut<'a, ()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;

    impl ChainDataBuffer for Dummy {
        fn lp_address(&self) -> Result<String> {
            Ok("lp".into())
        }
        fn finalized_chunk(&self) -> BufFut<'_, Vec<(ChannelId, ChainEvent)>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn broadcast_event<'a>(&'a self, _event: &'a ChainEvent) -> BufFut<'a, ()> {
            Box::pin(async { Ok(()) })
        }
        fn sign_message<'a>(&'a self, _d: [u8; 32], _r: &'a str) -> BufFut<'a, (String, Vec<u8>)> {
            Box::pin(async { Ok((String::new(), Vec::new())) })
        }
        fn verify_message<'a>(
            &'a self,
            _d: [u8; 32],
            _c: &'a str,
            _s: &'a [u8],
            _r: &'a str,
        ) -> BufFut<'a, ProposalVerification> {
            Box::pin(async {
                Ok(ProposalVerification {
                    address_matches: false,
                    balance_sufficient: false,
                })
            })
        }
    }

    #[test]
    fn trait_defaults_are_inert() {
        assert!(Dummy.chain_now().is_none());
        assert!(Dummy.utxo_script_detector().is_none());
        assert!(Dummy.take_utxo_script_announcements().is_empty());
        assert!(Arc::new(Dummy).settler_task().is_none());
    }
}
