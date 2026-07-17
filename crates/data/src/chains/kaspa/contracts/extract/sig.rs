use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
use kaspa_txscript::opcodes::{
    OpCodeImplementation,
    codes::{OpFalse, OpTrue},
};

use super::super::contract_v1::VerifiableTransactionMock;
use super::opdata::extract_opcode_data;
use crate::chains::kaspa::error::{KaspaError, Result};

/// From a signature script extract the secret
pub(crate) fn extract_reveal_secret(
    sig_script: &[Box<
        dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
    >],
) -> Result<[u8; 32]> {
    // Extract the signature script exact opcodes
    let [secret_op, selector_op, redeem_op] = sig_script else {
        return Err(KaspaError::InvalidSigScriptLength {
            expected: 3,
            got: sig_script.len(),
        });
    };

    // Ensure the selector is true
    let selector = selector_op.value();
    if selector != OpTrue {
        return Err(KaspaError::WrongBranchSelector {
            expected: OpTrue,
            got: selector,
        });
    }

    // Extract the secret at the secret opcode position
    let secret = extract_opcode_data(secret_op.as_ref());
    if secret.is_empty() {
        return Err(KaspaError::MissingSecret);
    }

    // Convert the secret into expected length
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| KaspaError::InvalidSecretLength)?;

    // Extract the redeem script
    // otherwise it is still not a canonical spend
    let redeem_script = extract_opcode_data(redeem_op.as_ref());
    if redeem_script.is_empty() {
        return Err(KaspaError::MissingRedeemScript);
    }

    Ok(secret)
}

/// Validate that some signature script indeed tries to execute the refund branch of some swap
pub(crate) fn validate_refund_sig(
    sig_script: &[Box<
        dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>,
    >],
) -> Result<()> {
    // Decode the expected sig script layout
    let [selector_op, redeem_op] = sig_script else {
        return Err(KaspaError::InvalidSigScriptLength {
            expected: 2,
            got: sig_script.len(),
        });
    };

    // Ensure the branch selector is false
    let selector = selector_op.value();
    if selector != OpFalse {
        return Err(KaspaError::WrongBranchSelector {
            expected: OpFalse,
            got: selector,
        });
    }

    // Ensure the redeemscript is present
    let redeem_script = extract_opcode_data(redeem_op.as_ref());
    if redeem_script.is_empty() {
        return Err(KaspaError::MissingRedeemScript);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type Ops =
        [Box<dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>>];

    #[test]
    fn reveal_rejects_wrong_length() {
        let empty: &Ops = &[];
        assert!(matches!(
            extract_reveal_secret(empty),
            Err(KaspaError::InvalidSigScriptLength { .. })
        ));
    }

    #[test]
    fn refund_rejects_wrong_length() {
        let empty: &Ops = &[];
        assert!(matches!(
            validate_refund_sig(empty),
            Err(KaspaError::InvalidSigScriptLength { .. })
        ));
    }
}
