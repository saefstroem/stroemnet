use kaspa_addresses::Prefix;
use kaspa_consensus_core::{hashing::sighash::SigHashReusedValuesUnsync, tx::ScriptPublicKey};
use kaspa_txscript::{extract_script_pub_key_address, opcodes::OpCodeImplementation};

use super::super::contract_v1::{DataType, VerifiableTransactionMock};
use super::super::script::decode_u64_from_script;
use super::opdata::collect_data;
use crate::chains::kaspa::error::{KaspaError, Result};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};

/// Convert some bytes into a script public key
/// using the serialization format
fn spk_from_bytes(bytes: &[u8]) -> Result<ScriptPublicKey> {
    let [v0, v1, script @ ..] = bytes else {
        return Err(KaspaError::InvalidSigScriptLength {
            expected: 2,
            got: bytes.len(),
        });
    };

    let version = u16::from_be_bytes([*v0, *v1]);

    Ok(ScriptPublicKey::from_vec(version, script.to_vec()))
}

/// Extract a htlc v1 commitment from the script
pub(crate) fn extract_commitment(
    script: &Vec<
        Box<dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>>,
    >,
    amount: String,               // the value of the utxo
    prefix: Prefix,               // chain prefix
    source_channel_id: ChannelId, // which channel id it came from
) -> Result<CommitmentV1> {
    // Compute all the data inside this script
    let data = collect_data(script)?;

    // Retrieve the detected swap id
    let swap_id: [u8; 32] = data
        .get(&DataType::SwapId)
        .ok_or(KaspaError::MissingData(DataType::SwapId))?
        .clone()
        .try_into()
        .map_err(|_| KaspaError::InvalidSwapIdLength)?;

    // Retrieve the detected sender
    let sender = data
        .get(&DataType::SenderSpk)
        .filter(|v| !v.is_empty())
        .ok_or(KaspaError::MissingData(DataType::SenderSpk))?
        .clone();

    // Retrieve the detected receiver
    let receiver = data
        .get(&DataType::ReceiverSpk)
        .filter(|v| !v.is_empty())
        .ok_or(KaspaError::MissingData(DataType::ReceiverSpk))?
        .clone();

    // Retrieve the detected secret hash
    let secret_hash: [u8; 32] = data
        .get(&DataType::SecretHash)
        .ok_or(KaspaError::MissingData(DataType::SecretHash))?
        .clone()
        .try_into()
        .map_err(|_| KaspaError::InvalidSecretHashLength)?;

    // Retrieve the detected unlock ts in millis
    let unlock_ts_ms = decode_u64_from_script(
        data.get(&DataType::Timelock)
            .filter(|v| !v.is_empty())
            .ok_or(KaspaError::MissingData(DataType::Timelock))?
            .as_slice(),
    );

    // Compute the unlock ts in seconds
    let unlock_ts = unlock_ts_ms / 1000;

    // Retrieve the senders destination address
    let sender_destination_address = data
        .get(&DataType::SenderReceiverAddress)
        .filter(|v| !v.is_empty())
        .ok_or(KaspaError::MissingData(DataType::SenderReceiverAddress))?
        .clone();

    // Retrieve the destination address
    let destination = *data
        .get(&DataType::Destination)
        .ok_or(KaspaError::MissingData(DataType::Destination))?
        .first()
        .ok_or(KaspaError::MissingData(DataType::Destination))?;

    // Create the commitment v1
    Ok(CommitmentV1 {
        swap_id,
        addresses: AddressesV1::new(
            extract_script_pub_key_address(&spk_from_bytes(&sender)?, prefix)?.to_string(),
            extract_script_pub_key_address(&spk_from_bytes(&receiver)?, prefix)?.to_string(),
            String::from_utf8(sender_destination_address)?,
        ),
        amount: AmountV1::new(amount, 8),
        secret_hash,
        unlock_ts,
        source: source_channel_id as u8,
        destination,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spk_from_bytes_rejects_short() {
        assert!(spk_from_bytes(&[0u8]).is_err());
    }

    #[test]
    fn spk_from_bytes_parses_version_and_script() {
        assert!(spk_from_bytes(&[0, 1, 0xaa, 0xbb]).is_ok());
    }
}
