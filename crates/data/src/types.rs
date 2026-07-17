use std::future::Future;
use std::pin::Pin;

use crate::Result;

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type BufFut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub(crate) type BufFut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + 'a>>;

#[derive(Debug, Clone)]
pub struct ProposalVerification {
    pub address_matches: bool,
    pub balance_sufficient: bool,
}

#[derive(Debug, Clone, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct UtxoScript {
    pub redeem_script: Vec<u8>,
    pub unlock_ts: u64,
    pub deposit_target: String,
}

#[derive(Debug, Clone)]
pub struct ScriptAnnouncement {
    pub address: String,
    pub swap_id: [u8; 32],
    pub script: UtxoScript,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _requires_maybe_send<M: MaybeSend>() {}

    #[test]
    fn send_sync_types_are_maybe_send() {
        _requires_maybe_send::<u8>();
        let s = UtxoScript {
            redeem_script: vec![1, 2],
            unlock_ts: 5,
            deposit_target: "1".into(),
        };
        assert_eq!(s.unlock_ts, 5);
    }
}
