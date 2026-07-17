use k256::schnorr::SigningKey;
use k256::schnorr::signature::hazmat::PrehashSigner;
use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::hashing::sighash::{
    SigHashReusedValuesUnsync, calc_schnorr_signature_hash,
};
use kaspa_consensus_core::hashing::sighash_type::SIG_HASH_ALL;
use kaspa_consensus_core::tx::{MutableTransaction, ScriptPublicKey, Transaction};
use kaspa_txscript::{pay_to_address_script, script_builder::ScriptBuilder};

use super::super::error::{KaspaError, Result, script_err};
use super::super::signing::{pubkey_bytes, signing_key};

/// A signer structure for the Kaspa channel
pub(super) struct Signer340 {
    key: SigningKey,
    pubkey: [u8; 32],
    prefix: Prefix,
}

impl Signer340 {
    /// Derive from a provided private key and prefix
    pub(super) fn derive(private_key: &str, prefix: Prefix) -> Result<Self> {
        let key = signing_key(private_key)?;
        let pubkey = pubkey_bytes(&key)?;
        Ok(Self {
            key,
            pubkey,
            prefix,
        })
    }

    /// Compute the address of the signer, taking into account the prefix
    pub(super) fn address(&self) -> Address {
        Address::new(self.prefix, Version::PubKey, &self.pubkey)
    }

    /// Convert the signer to a script public key
    pub(super) fn spk(&self) -> ScriptPublicKey {
        pay_to_address_script(&self.address())
    }

    /// Sign the input of some mutable transaction at a specified index
    pub(super) fn sign_input(
        &self,
        mutable_tx: &MutableTransaction<Transaction>,
        index: usize,
    ) -> Result<Vec<u8>> {
        let reused_values = SigHashReusedValuesUnsync::new();

        // Compute the signature hash
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            index,
            SIG_HASH_ALL,
            &reused_values,
        );
        // Sign the hash via k256
        let sig: k256::schnorr::Signature =
            self.key
                .sign_prehash(sig_hash.as_bytes().as_slice())
                .map_err(|e| KaspaError::Other(format!("schnorr sign: {e}")))?;
        let mut signature = Vec::with_capacity(65);
        // Extend the signature
        signature.extend_from_slice(&sig.to_bytes());
        // the signature commits to all inputs and outputs,
        // any change to them will invalidate the sig
        signature.push(SIG_HASH_ALL.to_u8());

        // Push the signature as a script and return it
        Ok(ScriptBuilder::new()
            .add_data(&signature)
            .map_err(script_err)?
            .drain())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;
    use kaspa_consensus_core::subnets::SUBNETWORK_ID_NATIVE;
    use kaspa_consensus_core::tx::{
        TransactionInput, TransactionOutpoint, TransactionOutput, UtxoEntry, VerifiableTransaction,
    };
    use kaspa_hashes::Hash;
    use kaspa_txscript::{TxScriptEngine, caches::Cache};

    #[test]
    fn production_signer_p2pk_input_verifies_on_engine() {
        let signer = Signer340::derive(
            "1111111111111111111111111111111111111111111111111111111111111111",
            Prefix::Testnet,
        )
        .unwrap();
        let spk = signer.spk();
        let input_value = 1_000_000u64;
        let input = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };
        let output = TransactionOutput::new(input_value - 1_000, spk.clone());
        let tx = Transaction::new(
            0,
            vec![input],
            vec![output],
            0,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        let entry = UtxoEntry::new(input_value, spk.clone(), 0, false);
        let mut mutable_tx = MutableTransaction::with_entries(tx, vec![entry]);
        mutable_tx.tx.inputs[0].signature_script = signer.sign_input(&mutable_tx, 0).unwrap();

        let reused = SigHashReusedValuesUnsync::new();
        let verifiable = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = verifiable.utxo(0).unwrap().clone();
        let mut vm = TxScriptEngine::from_transaction_input(
            &verifiable,
            &verifiable.inputs()[0],
            0,
            &utxo_entry,
            &reused,
            &sig_cache,
        );
        vm.execute()
            .expect("production-signed P2PK input must satisfy CHECKSIG");
    }
}
