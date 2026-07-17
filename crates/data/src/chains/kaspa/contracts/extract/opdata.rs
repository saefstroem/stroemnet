use ahash::AHashMap;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_txscript::opcodes::OpCodeImplementation;

use super::super::contract_v1::{
    DataType, EXPECTED_OPCODES, ExpectedOpCode, VerifiableTransactionMock,
};
use crate::chains::kaspa::error::{KaspaError, Result};

/// Small data pushed are converted into opcodes,
/// therefore we must do a little subtraction to extract their data
fn value_to_bytes(val: u8) -> Vec<u8> {
    match val {
        0x00 => vec![0u8],
        0x51..=0x60 => vec![val - 0x50],
        _ => vec![],
    }
}

/// Extract opcode data based on the opcode
pub(super) fn extract_opcode_data(
    opcode: &dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
) -> Vec<u8> {
    // Attempt to return data from this opcode
    let d = opcode.get_data();
    if !d.is_empty() {
        // If this is not empty it was a data push so we can just convert it to vec
        return d.to_vec();
    }
    // Convert the value of the opcode to bytes
    value_to_bytes(opcode.value())
}

/// Collect the data from the entire htlc script
pub(super) fn collect_data(
    script: &[Box<
        dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
    >],
) -> Result<AHashMap<DataType, Vec<u8>>> {
    // if the script doesnt contain the exact amount of opcodes expected in a htlc script reject it
    if script.len() != EXPECTED_OPCODES.len() {
        return Err(KaspaError::TooManyOpcodes);
    }

    let mut data: AHashMap<DataType, Vec<u8>> = AHashMap::new();

    // Go over each opcode
    for (i, opcode) in script.iter().enumerate() {
        // Ensure the opcodes is of the expected type
        let (expected, label) = EXPECTED_OPCODES.get(i).ok_or(KaspaError::TooManyOpcodes)?;

        match expected {
            ExpectedOpCode::OpCode(expected_opcode) => {
                // Validate the opcode type
                if opcode.value() != *expected_opcode {
                    tracing::error!(
                        "Opcode mismatch at position {i}: expected {label:?} (0x{expected_opcode:02x}), got 0x{:02x}",
                        opcode.value()
                    );
                    return Err(KaspaError::OpcodeMismatch(i));
                }
            }
            ExpectedOpCode::Data => {
                // validate the opcode and then push the data to our container
                if *label != DataType::Opcode {
                    data.insert(label.clone(), extract_opcode_data(opcode.as_ref()));
                }
            }
        }
    }

    // Return the data
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_to_bytes_maps_push_opcodes() {
        assert_eq!(value_to_bytes(0x00), vec![0u8]);
        assert_eq!(value_to_bytes(0x51), vec![1u8]);
        assert_eq!(value_to_bytes(0x60), vec![16u8]);
        assert!(value_to_bytes(0x99).is_empty());
    }
}
