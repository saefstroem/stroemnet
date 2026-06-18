use ahash::AHashMap;
use kaspa_addresses::Prefix;
use kaspa_consensus_core::{hashing::sighash::SigHashReusedValuesUnsync, tx::ScriptPublicKey};
use kaspa_txscript::{
    extract_script_pub_key_address,
    opcodes::{
        OpCodeImplementation,
        codes::{OpFalse, OpTrue},
    },
};

use crate::chains::kaspa::error::{KaspaError, Result};
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::{AddressesV1, AmountV1, CommitmentV1};

use super::contract_v1::{DataType, EXPECTED_OPCODES, ExpectedOpCode, VerifiableTransactionMock};
use super::script::decode_u64_from_script;

/// Extracts the relevant data from the HTLC script
fn extract_opcode_data(
    opcode: &dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
) -> Vec<u8> {
    // If the opcode has associated data, return it. This is the case for pushdata opcodes.
    let d = opcode.get_data();
    if !d.is_empty() {
        return d.to_vec();
    }

    let val = opcode.value();
    match val {
        0x00 => vec![0u8], // OP_FALSE pushes an empty vector,
        0x51..=0x60 => {
            // OP_1 to OP_16 push the numbers 1 to 16, encoded as a single byte with value 0x51 to 0x60. We convert this to the corresponding number.
            vec![(val - 0x50)]
        }
        _ => vec![], // For other opcodes, we return an empty vector
    }
}

/// Compute a script public key from som arbitrary bytes
fn spk_from_bytes(bytes: &[u8]) -> Result<ScriptPublicKey> {
    // A valid script public key must be at least 2 bytes long to contain the version, plus some script data.
    if bytes.len() < 2 {
        return Err(KaspaError::InvalidSigScriptLength {
            expected: 2,
            got: bytes.len(),
        });
    }

    // Parse the first 2 bytes as the version and the rest as script.
    let version = u16::from_be_bytes([bytes[0], bytes[1]]);
    let script = bytes[2..].to_vec();

    Ok(ScriptPublicKey::from_vec(version, script))
}

/// Extracts a CommitmentV1 from a given HTLC script,
pub(crate) fn extract_commitment(
    script: &Vec<
        Box<dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>>,
    >,
    amount: String,
    prefix: Prefix,
    source_chain_id: ChannelId,
) -> Result<CommitmentV1> {
    // If the script has more or less opcodes than we expect for our HTLC contract, it's not valid.
    if script.len() != EXPECTED_OPCODES.len() {
        return Err(KaspaError::TooManyOpcodes);
    }

    let mut data: AHashMap<DataType, Vec<u8>> = AHashMap::new();

    // Go over all opcodes in the script
    for (i, opcode) in script.iter().enumerate() {
        // Try and get the opcode, otherwise we have an opcode count mismatch.
        let (expected, label) = EXPECTED_OPCODES.get(i).ok_or(KaspaError::TooManyOpcodes)?;

        // Match the opcode against the expected opcode or data type for this position in the script.
        match expected {
            ExpectedOpCode::OpCode(expected_opcode) => {
                // If the opcode value doesn't match the expected opcode, we have an opcode mismatch.
                if opcode.value() != *expected_opcode {
                    tracing::error!(
                        "Opcode mismatch at position {i}: expected {label:?} (0x{expected_opcode:02x}), got 0x{:02x}",
                        opcode.value()
                    );
                    return Err(KaspaError::OpcodeMismatch(i));
                }
            }
            ExpectedOpCode::Data => {
                // If we expect data at this position,
                // we extract it from the opcode and store it in our data map under the corresponding label.
                if *label != DataType::Opcode {
                    data.insert(label.clone(), extract_opcode_data(opcode.as_ref()));
                }
            }
        }
    }

    // Now we have validated all the operations and extracted all data from the script.

    // Retrieve the swap id from the data map, ensuring it's present and has the correct length.
    let swap_id: [u8; 32] = data
        .get(&DataType::SwapId)
        .ok_or(KaspaError::MissingData(DataType::SwapId))?
        .clone()
        .try_into()
        .map_err(|_| KaspaError::InvalidSwapIdLength)?;

    // Retrieve the sender from the data map, ensuring it's present and not empty
    let sender = data
        .get(&DataType::SenderSpk)
        .filter(|v| !v.is_empty())
        .ok_or(KaspaError::MissingData(DataType::SenderSpk))?
        .clone();

    // Retrieve the receiver from the data map, ensuring it's present and not empty
    let receiver = data
        .get(&DataType::ReceiverSpk)
        .filter(|v| !v.is_empty())
        .ok_or(KaspaError::MissingData(DataType::ReceiverSpk))?
        .clone();

    // Retrieve the secret hash from the data map, ensuring it's present and has the correct length.
    let secret_hash: [u8; 32] = data
        .get(&DataType::SecretHash)
        .ok_or(KaspaError::MissingData(DataType::SecretHash))?
        .clone()
        .try_into()
        .map_err(|_| KaspaError::InvalidSecretHashLength)?;

    // Retrieve the timelock from the data map, ensuring it's present and not empty,
    // then decode it from bytes to a u64 timestamp in seconds.
    let unlock_ts_ms = decode_u64_from_script(
        data.get(&DataType::Timelock)
            .filter(|v| !v.is_empty())
            .ok_or(KaspaError::MissingData(DataType::Timelock))?
            .as_slice(),
    );

    // Convert the unlock timestamp from milliseconds to seconds,
    // as we want to work with second precision for timelocks.
    let unlock_ts = unlock_ts_ms / 1000;

    // Retrieve the sender's destination address on the target chain from the data map, ensuring it's present and not empty.
    let sender_destination_address = data
        .get(&DataType::SenderReceiverAddress)
        .filter(|v| !v.is_empty())
        .ok_or(KaspaError::MissingData(DataType::SenderReceiverAddress))?
        .clone();

    // Retrieve the destination channel id from the data map, ensuring it's present.
    let destination = *data
        .get(&DataType::Destination)
        .ok_or(KaspaError::MissingData(DataType::Destination))?
        .first()
        .ok_or(KaspaError::MissingData(DataType::Destination))?;

    // If we've reached this point, it means we've successfully
    // validated the script and extracted all necessary data to construct a CommitmentV1, which we do and return.
    // Now we just need to decode some of the fields successfully in order to guarantee that this is a valid
    // commitment.
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
        source: source_chain_id as u8,
        destination,
    })
}

/// Extracts the secret from a reveal transaction sig script
pub(crate) fn extract_reveal_secret(
    sig_script: &[Box<
        dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
    >],
) -> Result<[u8; 32]> {
    // If the sig script doesn have exactly 3 opcodes (secret, selector and redeem script), it's not valid.
    if sig_script.len() != 3 {
        return Err(KaspaError::InvalidSigScriptLength {
            expected: 3,
            got: sig_script.len(),
        });
    }

    // Ensure we are in the reveal branch of the script by checking the selector opcode.
    // If it's not OP_TRUE, it's not valid.
    let selector = sig_script[1].value();
    if selector != OpTrue {
        return Err(KaspaError::WrongBranchSelector {
            expected: OpTrue,
            got: selector,
        });
    }

    // Extract the secret from the first opcode, ensuring it's present and has the correct length.
    let secret = extract_opcode_data(sig_script[0].as_ref());
    if secret.is_empty() {
        return Err(KaspaError::MissingSecret);
    }

    // Convert the secret to a fixed-size array, ensuring it has the correct length.
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| KaspaError::InvalidSecretLength)?;

    // Ensure the redeem script is present in the third opcode. We don't actually need to parse it here,
    // but its presence is required for a valid reveal transaction.
    let redeem_script = extract_opcode_data(sig_script[2].as_ref());
    if redeem_script.is_empty() {
        return Err(KaspaError::MissingRedeemScript);
    }

    Ok(secret)
}

/// Validates that a refund transaction sig script is correctly formed, meaning it has the right number of opcodes,
/// the correct selector for the refund branch and includes a redeem script.
/// We don't need to extract any data from the sig script for refunds,
pub(crate) fn validate_refund_sig(
    sig_script: &[Box<
        dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
    >],
) -> Result<()> {
    // If the sig script doesn have exactly 2 opcodes (selector and redeem script), it's not valid.
    if sig_script.len() != 2 {
        return Err(KaspaError::InvalidSigScriptLength {
            expected: 2,
            got: sig_script.len(),
        });
    }

    // Ensure we are in the refund branch of the script by checking the selector opcode.
    // If it's not OP_FALSE, it's not valid.
    let selector = sig_script[0].value();
    if selector != OpFalse {
        return Err(KaspaError::WrongBranchSelector {
            expected: OpFalse,
            got: selector,
        });
    }

    // Ensure the redeem script is present in the second opcode. We don't actually need to parse it here,
    // but its presence is required for a valid refund transaction.
    let redeem_script = extract_opcode_data(sig_script[1].as_ref());
    if redeem_script.is_empty() {
        return Err(KaspaError::MissingRedeemScript);
    }

    Ok(())
}
