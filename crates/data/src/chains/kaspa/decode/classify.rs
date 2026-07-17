use kaspa_addresses::Prefix;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_txscript::{extract_script_pub_key_address, pay_to_script_hash_script};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{ChainEvent, RefundV1, RevealV1};

use super::super::contracts::{
    VerifiableTransactionMock, extract_commitment, extract_reveal_secret, validate_refund_sig,
};
use super::parse::parse_script;
use crate::UtxoScript;
use crate::chains::kaspa::error::Result;

// Check if the last sig script at the last position has a redeem script
pub(super) fn last_redeem(sig_script: &[u8]) -> Option<Vec<u8>> {
    let opcodes = parse_script::<VerifiableTransactionMock, SigHashReusedValuesUnsync>(sig_script)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    let op = opcodes.last()?;
    let d = op.get_data();
    if d.is_empty() { None } else { Some(d.to_vec()) }
}

// Compute the p2sh address for some redeem script
pub(super) fn derive_p2sh_addr(redeem_script: &[u8], prefix: Prefix) -> Option<String> {
    extract_script_pub_key_address(&pay_to_script_hash_script(redeem_script), prefix)
        .ok()
        .map(|a| a.to_string())
}

/// Classify a utxo script its sig script and what it is trying to do
pub(super) fn classify_spend(
    sig_script: &[u8],
    utxo_script: &UtxoScript,
    prefix: Prefix,
    channel_id: ChannelId,
) -> Result<(Option<ChainEvent>, bool)> {
    // Retrieve all the signature opcodes from the sig script
    let sig_opcodes = parse_script(sig_script).collect::<std::result::Result<Vec<_>, _>>()?;

    // Retrieve all the opcodes from the redeem script
    let redeem_opcodes =
        parse_script(&utxo_script.redeem_script).collect::<std::result::Result<Vec<_>, _>>()?;

    // Extract the swap id
    let swap_id = extract_commitment(
        &redeem_opcodes,
        utxo_script.deposit_target.clone(),
        prefix,
        channel_id,
    )
    .ok()
    .map(|c| c.swap_id);

    // Attempt to extract reveal secret in which case it is a reveal
    if let Ok(secret) = extract_reveal_secret(&sig_opcodes) {
        let Some(id) = swap_id else {
            return Ok((None, false));
        };
        return Ok((
            Some(ChainEvent::Reveal(RevealV1 {
                swap_id: id,
                secret,
            })),
            true,
        ));
    }

    // Or validate the refund signature in which case it is a refund
    if validate_refund_sig(&sig_opcodes).is_ok() {
        let Some(id) = swap_id else {
            return Ok((None, false));
        };
        return Ok((Some(ChainEvent::Refund(RefundV1 { swap_id: id })), true));
    }

    // If both attempts failed then we cannot classify this spend
    Ok((None, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_redeem_empty_is_none() {
        assert!(last_redeem(&[]).is_none());
    }

    #[test]
    fn derive_p2sh_addr_is_some_for_any_script() {
        assert!(derive_p2sh_addr(&[1, 2, 3], Prefix::Testnet).is_some());
    }
}
