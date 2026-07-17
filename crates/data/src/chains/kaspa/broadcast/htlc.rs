use kaspa_addresses::Address;
use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_txscript::pay_to_address_script;
use stroemnet_protocol::v1::CommitmentV1;

use super::super::contracts::create_htlc_script;
use super::super::error::{Result, script_err};
use super::utxo::spk_to_vec;

/// An announcement of an address and its associated redeem script
pub(crate) struct Announce {
    pub address: String,
    pub redeem_script: Vec<u8>,
}

/// Converts a commitmentv1 into a kaspa canonical utxo script
pub(super) fn htlc_script_from_commitment(
    commitment: &CommitmentV1,
) -> Result<(Vec<u8>, ScriptPublicKey, ScriptPublicKey, u64)> {
    // Conver the unlock timestamp to milliseconds the OpDaaScore opcode uses millis
    let unlock_ts_ms = commitment.unlock_ts.saturating_mul(1000);

    // Conver the sender to spk
    let sender_spk =
        pay_to_address_script(&Address::try_from(commitment.addresses.sender.clone())?);

    // Conver the receiver to spk
    let receiver_spk =
        pay_to_address_script(&Address::try_from(commitment.addresses.receiver.clone())?);

    // Create the htlc script based on the arguments
    let htlc_script = create_htlc_script(
        &spk_to_vec(&sender_spk),
        commitment.addresses.sender_destination.as_bytes(),
        &spk_to_vec(&receiver_spk),
        &commitment.secret_hash,
        unlock_ts_ms,
        commitment.destination,
        commitment.swap_id,
    )
    .map_err(script_err)?;

    // Return the htlc script, sender,receiver spk and unlock timestamp in millis
    Ok((htlc_script, sender_spk, receiver_spk, unlock_ts_ms))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use kaspa_addresses::{Prefix, Version};
    use stroemnet_protocol::ChannelId;
    use stroemnet_protocol::v1::{AddressesV1, AmountV1};

    #[test]
    fn builds_non_empty_script_and_scales_timelock() {
        let sender = Address::new(Prefix::Testnet, Version::PubKey, &[1u8; 32]).to_string();
        let receiver = Address::new(Prefix::Testnet, Version::PubKey, &[2u8; 32]).to_string();
        let commitment = CommitmentV1 {
            swap_id: [3u8; 32],
            addresses: AddressesV1::new(sender, receiver, "0xdest".into()),
            amount: AmountV1::new("1".into(), 8),
            secret_hash: [4u8; 32],
            unlock_ts: 1000,
            source: ChannelId::KaspaTn10 as u8,
            destination: ChannelId::EthereumSepolia as u8,
        };
        let (script, _s, _r, unlock_ts_ms) = htlc_script_from_commitment(&commitment).unwrap();
        assert!(!script.is_empty());
        assert_eq!(unlock_ts_ms, 1_000_000);
    }
}
