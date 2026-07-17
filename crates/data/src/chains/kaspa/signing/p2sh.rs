use kaspa_addresses::Address;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_txscript::opcodes::{OpCodeImplementation, deserialize_next_opcode};
use kaspa_txscript::{
    extract_script_pub_key_address, pay_to_address_script, pay_to_script_hash_script,
};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;

use super::super::broadcast::spk_to_vec;
use super::super::contracts::{VerifiableTransactionMock, create_htlc_script, extract_commitment};
use super::super::error::{KaspaError, Result, script_err};
use super::keys::parse_network_prefix;

/// Compute the p2sh address and redeem script from a network id and commitment v1
pub(crate) fn p2sh_components(
    network_id: &str,
    commitment: &CommitmentV1,
) -> Result<(String, Vec<u8>)> {
    // compute htlc prefix
    let prefix = parse_network_prefix(network_id)?;

    // Compute sender receiver spk
    let sender_addr = Address::try_from(commitment.addresses.sender.as_str())
        .map_err(|e| KaspaError::Other(format!("sender address: {e}")))?;
    let receiver_addr = Address::try_from(commitment.addresses.receiver.as_str())
        .map_err(|e| KaspaError::Other(format!("receiver address: {e}")))?;
    let sender_spk = pay_to_address_script(&sender_addr);
    let receiver_spk = pay_to_address_script(&receiver_addr);

    // Conver unlock timestamp to milliseconds
    let unlock_ts_ms = commitment.unlock_ts.saturating_mul(1000);

    // Create the redeem script
    let redeem_script = create_htlc_script(
        &spk_to_vec(&sender_spk),
        commitment.addresses.sender_destination.as_bytes(),
        &spk_to_vec(&receiver_spk),
        &commitment.secret_hash,
        unlock_ts_ms,
        commitment.destination,
        commitment.swap_id,
    )
    .map_err(script_err)?;

    // Compute the p2sh address
    let p2sh_spk = pay_to_script_hash_script(&redeem_script);
    let p2sh_addr = extract_script_pub_key_address(&p2sh_spk, prefix)
        .map_err(|e| KaspaError::Other(format!("p2sh address derive: {e:?}")))?;

    // Return it
    Ok((p2sh_addr.to_string(), redeem_script))
}

/// Validate that an announced script matches the redeem script
pub(crate) fn validate_script_announce(
    network_id: &str,
    channel_id: ChannelId,
    announced_address: &str,
    redeem_script: &[u8],
    expected_swap_id: [u8; 32],
    expected_expiration_secs: u64,
) -> Result<()> {
    // Parse the network prefix
    let prefix = parse_network_prefix(network_id)?;
    let mut iter = redeem_script.iter();
    let mut opcodes: Vec<
        Box<dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>>,
    > = Vec::new();

    // Parse all the opcodes
    while let Some(parsed) = deserialize_next_opcode(&mut iter) {
        opcodes.push(parsed.map_err(KaspaError::TxScript)?);
    }

    // Attempt to extract the commitment
    let extracted = extract_commitment(&opcodes, "0".to_string(), prefix, channel_id)?;

    // Ensure the swap id
    if extracted.swap_id != expected_swap_id {
        return Err(KaspaError::ScriptAnnounceSwapIdMismatch(
            extracted.swap_id,
            expected_swap_id,
        ));
    }

    // Ensure the unlock timestamp
    if extracted.unlock_ts != expected_expiration_secs {
        return Err(KaspaError::ScriptAnnounceTimelockMismatch {
            script_secs: extracted.unlock_ts,
            announced_secs: expected_expiration_secs,
        });
    }

    // Compute the p2sh address
    let p2sh_spk = pay_to_script_hash_script(redeem_script);
    let derived = extract_script_pub_key_address(&p2sh_spk, prefix)
        .map_err(|e| KaspaError::Other(format!("p2sh derive: {e:?}")))?;
    let derived_str = derived.to_string();

    // Ensure that the script derived address matches the one in the announcement
    if derived_str != announced_address {
        return Err(KaspaError::ScriptAnnounceAddressMismatch {
            announced: announced_address.to_string(),
            derived: derived_str,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use kaspa_addresses::{Prefix, Version};
    use stroemnet_protocol::v1::{AddressesV1, AmountV1};

    fn commitment() -> CommitmentV1 {
        let sender = Address::new(Prefix::Testnet, Version::PubKey, &[1u8; 32]).to_string();
        let receiver = Address::new(Prefix::Testnet, Version::PubKey, &[2u8; 32]).to_string();
        CommitmentV1 {
            swap_id: [3u8; 32],
            addresses: AddressesV1::new(sender, receiver, "0xdest".into()),
            amount: AmountV1::new("0".into(), 8),
            secret_hash: [4u8; 32],
            unlock_ts: 1000,
            source: ChannelId::KaspaTn10 as u8,
            destination: ChannelId::EthereumSepolia as u8,
        }
    }

    #[test]
    fn p2sh_components_roundtrip_validates() {
        let (addr, redeem) = p2sh_components("testnet-10", &commitment()).unwrap();
        assert!(!redeem.is_empty());
        validate_script_announce(
            "testnet-10",
            ChannelId::KaspaTn10,
            &addr,
            &redeem,
            [3u8; 32],
            1000,
        )
        .unwrap();
    }

    #[test]
    fn validate_rejects_swap_id_mismatch() {
        let (addr, redeem) = p2sh_components("testnet-10", &commitment()).unwrap();
        assert!(
            validate_script_announce(
                "testnet-10",
                ChannelId::KaspaTn10,
                &addr,
                &redeem,
                [9u8; 32],
                1000,
            )
            .is_err()
        );
    }
}
