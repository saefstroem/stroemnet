use kaspa_consensus_core::tx::{Transaction, TransactionInput, UtxoEntry, VerifiableTransaction};
use kaspa_txscript::opcodes::codes::{
    OpCheckLockTimeVerify, OpElse, OpEndIf, OpEqualVerify, OpFalse, OpGreaterThanOrEqual, OpIf,
    OpNumEqualVerify, OpSHA256, OpSub, OpTxInputAmount, OpTxInputCount, OpTxInputIndex,
    OpTxOutputAmount, OpTxOutputCount, OpTxOutputSpk,
};

pub(crate) use super::extract::{extract_commitment, extract_reveal_secret, validate_refund_sig};
pub(crate) use super::script::{SOLVER_REWARD, create_htlc_script};

/// Mock struct to satisfy trait bounds in order to decode scripts.
pub(crate) struct VerifiableTransactionMock;
impl VerifiableTransaction for VerifiableTransactionMock {
    fn tx(&self) -> &Transaction {
        unimplemented!()
    }
    fn populated_input(&self, _index: usize) -> (&TransactionInput, &UtxoEntry) {
        unimplemented!()
    }
    fn utxo(&self, _index: usize) -> Option<&UtxoEntry> {
        unimplemented!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Represents the expected opcode or data at a given position in the HTLC script
pub(crate) enum ExpectedOpCode {
    OpCode(u8),
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// The different data types that we expect when we operate
/// on the HTLC script.
pub enum DataType {
    Opcode,                // A raw opcode byte, e.g. 0x63
    SecretHash,            // The 32-byte hash of the secret,
    SwapId,                // The 32-byte swap ID,
    ReceiverSpk,           // The receiver's script public key
    SenderSpk,             // The sender's script public key
    Timelock,              // The timelock value (u64) encoded as 8 bytes in little-endian
    SenderReceiverAddress, // The senders receiver address on the destination chain
    Destination,           // The destination chain id (u8) encoded as 1 byte
}

/// All the expected opcodes, data and their order in the HTLC script for our version 1 contract.
pub(crate) const EXPECTED_OPCODES: &[(ExpectedOpCode, DataType)] = &[
    (ExpectedOpCode::OpCode(OpIf), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpSHA256), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::SecretHash),
    (ExpectedOpCode::OpCode(OpEqualVerify), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxInputCount), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpNumEqualVerify), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxOutputCount), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpNumEqualVerify), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::ReceiverSpk),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxOutputSpk), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpEqualVerify), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxOutputAmount), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxInputIndex), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxInputAmount), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpSub), DataType::Opcode),
    (
        ExpectedOpCode::OpCode(OpGreaterThanOrEqual),
        DataType::Opcode,
    ),
    (ExpectedOpCode::OpCode(OpElse), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Timelock),
    (
        ExpectedOpCode::OpCode(OpCheckLockTimeVerify),
        DataType::Opcode,
    ),
    (ExpectedOpCode::OpCode(OpTxInputCount), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpNumEqualVerify), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxOutputCount), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpNumEqualVerify), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::SenderSpk),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxOutputSpk), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpEqualVerify), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxOutputAmount), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxInputIndex), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpTxInputAmount), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::Opcode),
    (ExpectedOpCode::OpCode(OpSub), DataType::Opcode),
    (
        ExpectedOpCode::OpCode(OpGreaterThanOrEqual),
        DataType::Opcode,
    ),
    (ExpectedOpCode::OpCode(OpEndIf), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpFalse), DataType::Opcode),
    (ExpectedOpCode::OpCode(OpIf), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::SwapId),
    (ExpectedOpCode::Data, DataType::SenderReceiverAddress),
    (ExpectedOpCode::Data, DataType::Destination),
    (ExpectedOpCode::OpCode(OpEndIf), DataType::Opcode),
];
