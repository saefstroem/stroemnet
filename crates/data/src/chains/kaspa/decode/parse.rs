use itertools::Itertools;
use kaspa_consensus_core::hashing::sighash::SigHashReusedValues;
use kaspa_consensus_core::tx::VerifiableTransaction;
use kaspa_txscript::opcodes::{OpCodeImplementation, deserialize_next_opcode};
use kaspa_txscript_errors::TxScriptError;
use stroemnet_protocol::v1::ChainEvent;

pub(crate) type DynOpcodeImplementation<Tx, Reused> = Box<dyn OpCodeImplementation<Tx, Reused>>;

/// Parses a raw script into an opcode iterator
pub(crate) fn parse_script<T: VerifiableTransaction, Reused: SigHashReusedValues>(
    script: &[u8],
) -> impl Iterator<Item = std::result::Result<DynOpcodeImplementation<T, Reused>, TxScriptError>> + '_
{
    script.iter().batching(|it| deserialize_next_opcode(it))
}

#[derive(Debug)]
/// Computes outcomes in a certain block, such as events or refunds
pub(crate) struct BlockOutcomes {
    pub events: Vec<ChainEvent>,
    pub refunds: Vec<([u8; 32], u64)>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::super::super::contracts::VerifiableTransactionMock;
    use super::*;
    use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;

    #[test]
    fn parse_script_yields_opcodes() {
        let ops = parse_script::<VerifiableTransactionMock, SigHashReusedValuesUnsync>(&[0x51])
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn parse_script_empty_is_empty() {
        let ops = parse_script::<VerifiableTransactionMock, SigHashReusedValuesUnsync>(&[])
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(ops.is_empty());
    }
}
