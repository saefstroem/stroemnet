use std::sync::Arc;

use kaspa_addresses::Prefix;
use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionInput, TransactionOutpoint, UtxoEntry};
use kaspa_rpc_core::RpcUtxosByAddressesEntry;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;

use super::super::error::{KaspaError, Result};
use crate::chains::net::retry_timed;

/// Compute the prefix for the connected kaspa network
pub(super) async fn prefix_for(client: &Arc<KaspaRpcClient>) -> Result<Prefix> {
    let info = retry_timed("get_server_info", || client.get_server_info())
        .await
        .ok_or_else(|| KaspaError::Other("get_server_info: timed out".into()))?;
    Ok(info.network_id.network_type.into())
}

/// Conver the spk into vector serialized format
pub(crate) fn spk_to_vec(spk: &ScriptPublicKey) -> Vec<u8> {
    let mut v = Vec::with_capacity(2 + spk.script().len());
    v.extend_from_slice(&spk.version.to_be_bytes());
    v.extend_from_slice(spk.script());
    v
}

/// Convert rpc utxo entry to a consensus utxo entry
pub(super) fn rpc_utxo_to_entry(u: &RpcUtxosByAddressesEntry) -> UtxoEntry {
    UtxoEntry::new(
        u.utxo_entry.amount,
        ScriptPublicKey::new(
            u.utxo_entry.script_public_key.version,
            u.utxo_entry.script_public_key.script().into(),
        ),
        u.utxo_entry.block_daa_score,
        u.utxo_entry.is_coinbase,
    )
}

/// Compute whether a utxo is mature to be spent
pub(super) fn utxo_is_mature(
    utxo: &RpcUtxosByAddressesEntry,
    coinbase_maturity: u64,
    current_daa: u64,
) -> bool {
    !utxo.utxo_entry.is_coinbase
        || utxo.utxo_entry.block_daa_score + coinbase_maturity <= current_daa
}

/// Select which utxos can be used for subsidizing the transaction fee
pub(super) fn select_funding_utxos(
    utxos: Vec<RpcUtxosByAddressesEntry>, // the candidate utxos
    amount: u64,                          // amount needed to cover
    coinbase_maturity: u64,               // how many daa to wait
    current_daa: u64,                     // the current daa score
) -> Result<(Vec<RpcUtxosByAddressesEntry>, u64)> {
    let mut selected = Vec::new();
    let mut total: u64 = 0;

    // Go over all the utxos
    for utxo in utxos {
        // If the utxo is not mature due to being a fresh miner utxo we cant use it
        if !utxo_is_mature(&utxo, coinbase_maturity, current_daa) {
            continue;
        }

        // Add the utxos value the total
        total += utxo.utxo_entry.amount;

        // Push this utxo as selected
        selected.push(utxo);

        // If the total exceeds the required amount we can break
        if total >= amount {
            break;
        }
    }

    // If total is not enough then we error
    if total < amount {
        return Err(KaspaError::InsufficientFunds {
            needed: amount,
            available: total,
        });
    }
    Ok((selected, total))
}

pub(super) fn to_inputs(utxos: &[RpcUtxosByAddressesEntry]) -> Vec<TransactionInput> {
    utxos
        .iter()
        .enumerate()
        .map(|(seq, utxo)| TransactionInput {
            previous_outpoint: TransactionOutpoint::new(
                utxo.outpoint.transaction_id,
                utxo.outpoint.index,
            ),
            signature_script: vec![],
            sequence: seq as u64,
            sig_op_count: 1,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use kaspa_hashes::Hash;
    use kaspa_rpc_core::{RpcTransactionOutpoint, RpcUtxoEntry};

    fn entry(amount: u64, daa: u64, coinbase: bool) -> RpcUtxosByAddressesEntry {
        RpcUtxosByAddressesEntry {
            address: None,
            outpoint: RpcTransactionOutpoint {
                transaction_id: Hash::from_u64_word(1),
                index: 0,
            },
            utxo_entry: RpcUtxoEntry {
                amount,
                script_public_key: ScriptPublicKey::new(0, vec![].into()),
                block_daa_score: daa,
                is_coinbase: coinbase,
            },
        }
    }

    #[test]
    fn coinbase_maturity_respected() {
        assert!(!utxo_is_mature(&entry(1, 100, true), 50, 120));
        assert!(utxo_is_mature(&entry(1, 100, true), 50, 150));
        assert!(utxo_is_mature(&entry(1, 100, false), 50, 0));
    }

    #[test]
    fn spk_to_vec_prepends_version() {
        let spk = ScriptPublicKey::new(0, vec![0xaa, 0xbb].into());
        assert_eq!(spk_to_vec(&spk), vec![0, 0, 0xaa, 0xbb]);
    }

    #[test]
    fn selection_accumulates_until_target() {
        let (sel, total) =
            select_funding_utxos(vec![entry(40, 0, false), entry(70, 0, false)], 100, 0, 0)
                .unwrap();
        assert_eq!(sel.len(), 2);
        assert_eq!(total, 110);
    }

    #[test]
    fn selection_fails_when_insufficient() {
        assert!(select_funding_utxos(vec![entry(10, 0, false)], 100, 0, 0).is_err());
    }
}
