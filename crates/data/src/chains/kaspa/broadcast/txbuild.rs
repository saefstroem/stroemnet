use kaspa_consensus_core::tx::{
    ScriptPublicKey, TransactionInput, TransactionOutpoint, TransactionOutput,
};
use kaspa_rpc_core::RpcUtxosByAddressesEntry;

/// Convert rpc utxo entries into transaction inputs
pub(super) fn spend_inputs(
    htlc_utxo: &RpcUtxosByAddressesEntry,
    fee_utxo: &RpcUtxosByAddressesEntry,
) -> Vec<TransactionInput> {
    vec![
        TransactionInput {
            previous_outpoint: TransactionOutpoint::new(
                htlc_utxo.outpoint.transaction_id,
                htlc_utxo.outpoint.index,
            ),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        },
        TransactionInput {
            previous_outpoint: TransactionOutpoint::new(
                fee_utxo.outpoint.transaction_id,
                fee_utxo.outpoint.index,
            ),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        },
    ]
}

/// Create outputs based on the destination amount to the receiver of the swap
/// and the reward which is to us as CCR fulfillers.
pub(super) fn spend_outputs(
    dest_amount: u64,
    reward: u64,
    dest_spk: &ScriptPublicKey,
    our_spk: &ScriptPublicKey,
) -> Vec<TransactionOutput> {
    vec![
        TransactionOutput::new(dest_amount, dest_spk.clone()),
        TransactionOutput::new(reward, our_spk.clone()),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::*;
    use kaspa_hashes::Hash;
    use kaspa_rpc_core::{RpcTransactionOutpoint, RpcUtxoEntry};

    fn entry() -> RpcUtxosByAddressesEntry {
        RpcUtxosByAddressesEntry {
            address: None,
            outpoint: RpcTransactionOutpoint {
                transaction_id: Hash::from_u64_word(1),
                index: 0,
            },
            utxo_entry: RpcUtxoEntry {
                amount: 1,
                script_public_key: ScriptPublicKey::new(0, vec![].into()),
                block_daa_score: 0,
                is_coinbase: false,
            },
        }
    }

    #[test]
    fn spend_inputs_assigns_sequences_and_sigops() {
        let inputs = spend_inputs(&entry(), &entry());
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].sequence, 0);
        assert_eq!(inputs[0].sig_op_count, 0);
        assert_eq!(inputs[1].sequence, 0);
        assert_eq!(inputs[1].sig_op_count, 1);
    }
}
