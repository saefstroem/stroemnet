use super::Kaspa;
use super::signing;
use crate::{BufFut, UtxoScript, UtxoScriptDetector};

impl UtxoScriptDetector for Kaspa {
    /// Registers a script in the system
    /// this usually comes in from a p2p message where
    fn register_script<'a>(
        &'a self,
        address: String,
        redeem_script: Vec<u8>,
        swap_id: [u8; 32],
        unlock_ts: u64,
        deposit_target: String,
    ) -> BufFut<'a, ()> {
        Box::pin(async move {
            // validate the script before storing it
            signing::validate_script_announce(
                &self.network_id,
                self.channel_id,
                &address,
                &redeem_script,
                swap_id,
                unlock_ts,
            )?;
            let script = UtxoScript {
                redeem_script,
                unlock_ts,
                deposit_target,
            };
            // insert the script in our storage and key it by swap id
            self.scripts.lock().insert(swap_id, script.clone());
            // register internally the script by its p2sh address
            self.register_internal(address, script).await;
            // prune scripts that have expired
            self.prune_scripts().await;
            // persist any kind of swap state for this swap id to disk
            self.persist_swap(swap_id);
            Ok(())
        })
    }
}
