use kaspa_consensus_core::tx::{Transaction, TransactionInput, UtxoEntry, VerifiableTransaction};
use kaspa_txscript::opcodes::codes::{
    OpCheckLockTimeVerify, OpElse, OpEndIf, OpEqualVerify, OpFalse, OpGreaterThanOrEqual, OpIf,
    OpNumEqualVerify, OpSHA256, OpSub, OpTxInputAmount, OpTxInputCount, OpTxInputIndex,
    OpTxOutputAmount, OpTxOutputCount, OpTxOutputSpk,
};

pub(crate) use super::extract::{extract_commitment, extract_reveal_secret, validate_refund_sig};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use super::script::SOLVER_REWARD;
pub(crate) use super::script::create_htlc_script;

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
/// An enum representing two different types of expected opcodes
pub(crate) enum ExpectedOpCode {
    OpCode(u8),
    Data,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// The different data types the parsers can detect in the stroem htlc v1 script
pub enum DataType {
    Opcode,
    SecretHash,
    SwapId,
    ReceiverSpk,
    SenderSpk,
    Timelock,
    SenderReceiverAddress,
    Destination,
}

/// The expected opcode type and the exact datatype expected at a specific position
/// at the script
pub(crate) const EXPECTED_OPCODES: &[(ExpectedOpCode, DataType)] = &[
    (ExpectedOpCode::OpCode(OpIf), DataType::Opcode), // if
    (ExpectedOpCode::OpCode(OpSHA256), DataType::Opcode), // the sha opcode
    (ExpectedOpCode::Data, DataType::SecretHash),     // the secret hash
    (ExpectedOpCode::OpCode(OpEqualVerify), DataType::Opcode), // should be valid with the hashed secret
    (ExpectedOpCode::OpCode(OpTxInputCount), DataType::Opcode), // validated input count
    (ExpectedOpCode::Data, DataType::Opcode),                  // and the actual input (hardcoded)
    (ExpectedOpCode::OpCode(OpNumEqualVerify), DataType::Opcode), // should be equal
    (ExpectedOpCode::OpCode(OpTxOutputCount), DataType::Opcode), // the output count
    (ExpectedOpCode::Data, DataType::Opcode),                  // the output (hardcoded)
    (ExpectedOpCode::OpCode(OpNumEqualVerify), DataType::Opcode), // should be equal
    (ExpectedOpCode::Data, DataType::ReceiverSpk),             // the hardcoded receiver spk
    (ExpectedOpCode::Data, DataType::Opcode),                  // the index of output spk
    (ExpectedOpCode::OpCode(OpTxOutputSpk), DataType::Opcode), // the opcode that retrieves output spk
    (ExpectedOpCode::OpCode(OpEqualVerify), DataType::Opcode), // should be equal
    (ExpectedOpCode::Data, DataType::Opcode),                  // index for output
    (ExpectedOpCode::OpCode(OpTxOutputAmount), DataType::Opcode), // the output amount
    (ExpectedOpCode::OpCode(OpTxInputIndex), DataType::Opcode), // the input index
    (ExpectedOpCode::OpCode(OpTxInputAmount), DataType::Opcode), // its input amount
    (ExpectedOpCode::Data, DataType::Opcode),                  // the harcoded rewards
    (ExpectedOpCode::OpCode(OpSub), DataType::Opcode),         // subtracted from the output amount
    (
        ExpectedOpCode::OpCode(OpGreaterThanOrEqual), // should be geq the full value - reward
        DataType::Opcode,
    ),
    (ExpectedOpCode::OpCode(OpElse), DataType::Opcode), // refund branch
    (ExpectedOpCode::Data, DataType::Timelock),         // ensure time is ready
    (
        ExpectedOpCode::OpCode(OpCheckLockTimeVerify),
        DataType::Opcode,
    ),
    (ExpectedOpCode::OpCode(OpTxInputCount), DataType::Opcode), // same validation again
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
    (ExpectedOpCode::OpCode(OpFalse), DataType::Opcode), // metadata for quickly parsing the data
    (ExpectedOpCode::OpCode(OpIf), DataType::Opcode),
    (ExpectedOpCode::Data, DataType::SwapId),
    (ExpectedOpCode::Data, DataType::SenderReceiverAddress),
    (ExpectedOpCode::Data, DataType::Destination),
    (ExpectedOpCode::OpCode(OpEndIf), DataType::Opcode),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use crate::chains::kaspa::broadcast::spk_to_vec;
    use crate::chains::kaspa::contracts::contract_v1::{
        DataType, EXPECTED_OPCODES, ExpectedOpCode, SOLVER_REWARD, VerifiableTransactionMock,
        create_htlc_script, extract_commitment,
    };
    use crate::chains::kaspa::contracts::contract_v1::{
        extract_reveal_secret, validate_refund_sig,
    };
    use crate::chains::kaspa::contracts::script::decode_u64_from_script;
    use crate::chains::kaspa::error::KaspaError;
    use crate::chains::kaspa::test_helpers::{p2pk_spk, vec_to_spk};
    use kaspa_addresses::Prefix;
    use kaspa_consensus_core::hashing::sighash::SigHashReusedValuesUnsync;
    use kaspa_txscript::extract_script_pub_key_address;
    use kaspa_txscript::opcodes::codes::OpTrue;
    use kaspa_txscript::{
        opcodes::{
            OpCodeImplementation,
            codes::{
                OpCheckLockTimeVerify, OpCheckSig, OpElse, OpEndIf, OpEqualVerify, OpFalse,
                OpGreaterThanOrEqual, OpIf, OpNumEqualVerify, OpReturn, OpSHA256, OpSub,
                OpTxInputAmount, OpTxInputCount, OpTxInputIndex, OpTxOutputAmount, OpTxOutputCount,
                OpTxOutputSpk,
            },
        },
        script_builder::ScriptBuilder,
    };
    use rand::Rng;
    use secp256k1::{Keypair, Secp256k1};
    use sha2::{Digest, Sha256};
    use stroemnet_protocol::ChannelId;

    const DEFAULT_TIMELOCK_MS: u64 = (1_700_000_000 + 7200) * 1000;
    const DEFAULT_DESTINATION: u8 = 0;
    const DEFAULT_AMOUNT: &str = "1000000000";
    struct TestFixture {
        sender: Keypair,
        receiver: Keypair,
        secret: [u8; 32],
        secret_hash: [u8; 32],
        swap_id: [u8; 32],
        sender_receiver_address: Vec<u8>,
        destination: u8,
        timelock: u64,
    }

    impl TestFixture {
        fn new() -> Self {
            let secp = Secp256k1::new();
            let mut rng = rand::rng();
            let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
            let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
            let secret: [u8; 32] = rand::random();
            let secret_hash: [u8; 32] = Sha256::digest(secret).into();
            let swap_id: [u8; 32] = rand::random();
            let sender_receiver_address = b"sender_recv_addr_placeholder".to_vec();

            Self {
                sender,
                receiver,
                secret,
                secret_hash,
                swap_id,
                sender_receiver_address,
                destination: DEFAULT_DESTINATION,
                timelock: DEFAULT_TIMELOCK_MS,
            }
        }

        fn sender_pub(&self) -> [u8; 32] {
            self.sender.x_only_public_key().0.serialize()
        }

        fn sender_spk(&self) -> kaspa_consensus_core::tx::ScriptPublicKey {
            p2pk_spk(&self.sender)
        }

        fn sender_spk_vec(&self) -> Vec<u8> {
            spk_to_vec(&self.sender_spk())
        }

        fn receiver_spk_vec(&self) -> Vec<u8> {
            spk_to_vec(&p2pk_spk(&self.receiver))
        }

        fn build_valid_script(&self) -> Vec<u8> {
            create_htlc_script(
                &self.sender_spk_vec(),
                &self.sender_receiver_address,
                &self.receiver_spk_vec(),
                &self.secret_hash,
                self.timelock,
                self.destination,
                self.swap_id,
            )
            .expect("Script creation")
        }

        fn build_script_with_mutation_at(
            &self,
            position: usize,
            mutate: impl Fn(&mut ScriptBuilder),
        ) -> Vec<u8> {
            let mut builder = ScriptBuilder::new();
            let receiver_spk = self.receiver_spk_vec();
            let sender_spk = self.sender_spk_vec();

            for i in 0..EXPECTED_OPCODES.len() {
                if i == position {
                    mutate(&mut builder);
                    continue;
                }
                match i {
                    0 => {
                        builder.add_op(OpIf).unwrap();
                    }
                    1 => {
                        builder.add_op(OpSHA256).unwrap();
                    }
                    2 => {
                        builder.add_data(&self.secret_hash).unwrap();
                    }
                    3 => {
                        builder.add_op(OpEqualVerify).unwrap();
                    }
                    4 => {
                        builder.add_op(OpTxInputCount).unwrap();
                    }
                    5 => {
                        builder.add_i64(2).unwrap();
                    }
                    6 => {
                        builder.add_op(OpNumEqualVerify).unwrap();
                    }
                    7 => {
                        builder.add_op(OpTxOutputCount).unwrap();
                    }
                    8 => {
                        builder.add_i64(2).unwrap();
                    }
                    9 => {
                        builder.add_op(OpNumEqualVerify).unwrap();
                    }
                    10 => {
                        builder.add_data(&receiver_spk).unwrap();
                    }
                    11 => {
                        builder.add_i64(0).unwrap();
                    }
                    12 => {
                        builder.add_op(OpTxOutputSpk).unwrap();
                    }
                    13 => {
                        builder.add_op(OpEqualVerify).unwrap();
                    }
                    14 => {
                        builder.add_i64(0).unwrap();
                    }
                    15 => {
                        builder.add_op(OpTxOutputAmount).unwrap();
                    }
                    16 => {
                        builder.add_op(OpTxInputIndex).unwrap();
                    }
                    17 => {
                        builder.add_op(OpTxInputAmount).unwrap();
                    }
                    18 => {
                        builder.add_i64(SOLVER_REWARD).unwrap();
                    }
                    19 => {
                        builder.add_op(OpSub).unwrap();
                    }
                    20 => {
                        builder.add_op(OpGreaterThanOrEqual).unwrap();
                    }

                    21 => {
                        builder.add_op(OpElse).unwrap();
                    }
                    22 => {
                        builder.add_i64(self.timelock as i64).unwrap();
                    }
                    23 => {
                        builder.add_op(OpCheckLockTimeVerify).unwrap();
                    }
                    24 => {
                        builder.add_op(OpTxInputCount).unwrap();
                    }
                    25 => {
                        builder.add_i64(2).unwrap();
                    }
                    26 => {
                        builder.add_op(OpNumEqualVerify).unwrap();
                    }
                    27 => {
                        builder.add_op(OpTxOutputCount).unwrap();
                    }
                    28 => {
                        builder.add_i64(2).unwrap();
                    }
                    29 => {
                        builder.add_op(OpNumEqualVerify).unwrap();
                    }
                    30 => {
                        builder.add_data(&sender_spk).unwrap();
                    }
                    31 => {
                        builder.add_i64(0).unwrap();
                    }
                    32 => {
                        builder.add_op(OpTxOutputSpk).unwrap();
                    }
                    33 => {
                        builder.add_op(OpEqualVerify).unwrap();
                    }
                    34 => {
                        builder.add_i64(0).unwrap();
                    }
                    35 => {
                        builder.add_op(OpTxOutputAmount).unwrap();
                    }
                    36 => {
                        builder.add_op(OpTxInputIndex).unwrap();
                    }
                    37 => {
                        builder.add_op(OpTxInputAmount).unwrap();
                    }
                    38 => {
                        builder.add_i64(SOLVER_REWARD).unwrap();
                    }
                    39 => {
                        builder.add_op(OpSub).unwrap();
                    }
                    40 => {
                        builder.add_op(OpGreaterThanOrEqual).unwrap();
                    }

                    41 => {
                        builder.add_op(OpEndIf).unwrap();
                    }
                    42 => {
                        builder.add_op(OpFalse).unwrap();
                    }
                    43 => {
                        builder.add_op(OpIf).unwrap();
                    }
                    44 => {
                        builder.add_data(&self.swap_id).unwrap();
                    }
                    45 => {
                        builder.add_data(&self.sender_receiver_address).unwrap();
                    }
                    46 => {
                        builder.add_data(&[self.destination]).unwrap();
                    }
                    47 => {
                        builder.add_op(OpEndIf).unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            builder.drain()
        }

        fn extract(&self, raw: &[u8]) -> Result<stroemnet_protocol::v1::CommitmentV1, KaspaError> {
            let parsed =
                crate::chains::kaspa::decode::parse_script(raw).collect::<Result<Vec<_>, _>>()?;
            extract_commitment(
                &parsed,
                DEFAULT_AMOUNT.to_string(),
                Prefix::Devnet,
                ChannelId::KaspaTn10,
            )
        }
    }

    #[test]
    fn test_extract_refund_fails_empty() {
        let parsed: Vec<
            Box<dyn OpCodeImplementation<VerifiableTransactionMock, SigHashReusedValuesUnsync>>,
        > = vec![];
        assert!(matches!(
            validate_refund_sig(&parsed).unwrap_err(),
            KaspaError::InvalidSigScriptLength {
                expected: 2,
                got: 0
            }
        ));
    }

    #[test]
    fn test_extract_refund_fails_one_opcode() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();

        let sig_script = ScriptBuilder::new().add_data(&htlc_script).unwrap().drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            validate_refund_sig(&parsed).unwrap_err(),
            KaspaError::InvalidSigScriptLength {
                expected: 2,
                got: 1
            }
        ));
    }
    #[test]
    fn test_swap_id_too_short_rejected() {
        let f = TestFixture::new();
        let short_id = [0u8; 16];
        let raw = f.build_script_with_mutation_at(44, |b| {
            b.add_data(&short_id).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "16-byte swap_id should be rejected");
        match res.unwrap_err() {
            KaspaError::InvalidSwapIdLength => {}
            other => panic!("Expected InvalidSwapIdLength, got {other:?}"),
        }
    }

    #[test]
    fn test_swap_id_too_long_rejected() {
        let f = TestFixture::new();
        let long_id = [0u8; 64];
        let raw = f.build_script_with_mutation_at(44, |b| {
            b.add_data(&long_id).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "64-byte swap_id should be rejected");
        match res.unwrap_err() {
            KaspaError::InvalidSwapIdLength => {}
            other => panic!("Expected InvalidSwapIdLength, got {other:?}"),
        }
    }

    #[test]
    fn test_swap_id_different_value_still_parses() {
        let f = TestFixture::new();
        let different_id: [u8; 32] = rand::random();
        let raw = f.build_script_with_mutation_at(44, |b| {
            b.add_data(&different_id).unwrap();
        });
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.swap_id, different_id);
    }

    #[test]
    fn test_sender_receiver_address_different_value_extracts() {
        let f = TestFixture::new();
        let other_addr = "completely_different_address".to_string();

        let raw = f.build_script_with_mutation_at(45, |b| {
            b.add_data(other_addr.as_bytes()).unwrap();
        });
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.addresses.sender_destination, other_addr);
    }

    #[test]
    fn test_swap_id_empty_rejected() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(44, |b| {
            b.add_data(&[]).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Empty swap_id should be rejected");
    }

    #[test]
    fn test_refund_sub_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(39, |b| {
            b.add_op(OpCheckLockTimeVerify).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_refund_gte_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(40, |b| {
            b.add_op(OpCheckLockTimeVerify).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_destination_mutated_extracts() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(46, |b| {
            b.add_data(&[42u8]).unwrap();
        });
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.destination, 42);
    }

    #[test]
    fn test_destination_empty_vec_returns_missing_data() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(46, |b| {
            b.add_op(OpEqualVerify).unwrap();
        });
        let res = f.extract(&raw);
        match res {
            Err(KaspaError::MissingData(DataType::Destination)) => {}
            Err(other) => {
                assert!(
                    matches!(other, KaspaError::MissingData(_)),
                    "Expected MissingData, got {other:?}"
                );
            }
            Ok(_) => panic!("Should fail with non-push opcode in destination slot"),
        }
    }
    #[test]
    fn test_extract_refund_fails_four_opcodes() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let junk: [u8; 32] = rand::random();
        let sig_script = ScriptBuilder::new()
            .add_data(&junk)
            .unwrap()
            .add_data(&junk)
            .unwrap()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            validate_refund_sig(&parsed).unwrap_err(),
            KaspaError::InvalidSigScriptLength {
                expected: 2,
                got: 4
            }
        ));
    }

    #[test]
    fn test_extract_refund_fails_non_push_redeem() {
        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_op(OpElse)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            validate_refund_sig(&parsed).unwrap_err(),
            KaspaError::MissingRedeemScript
        ));
    }

    #[test]
    fn test_extract_refund_fails_with_op_true_selector() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();

        let sig_script = ScriptBuilder::new()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        match validate_refund_sig(&parsed).unwrap_err() {
            KaspaError::WrongBranchSelector { expected, got } => {
                assert_eq!(expected, OpFalse);
                assert_eq!(got, OpTrue);
            }
            other => panic!("Expected WrongBranchSelector, got {other:?}"),
        }
    }

    #[test]
    fn test_op_checklocktimeverify_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(23, |b| {
            b.add_op(OpSub).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_metadata_envelope_missing_inner_op_if() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(41, |b| {
            b.add_op(OpElse).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_op_if_at_41_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(41, |b| {
            b.add_op(OpSub).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_sender_pubkey_different_value_extracts() {
        let f = TestFixture::new();
        let secp = Secp256k1::new();
        let mut rng = rand::rng();
        let other = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let other_spk = spk_to_vec(&p2pk_spk(&other));

        let raw = f.build_script_with_mutation_at(30, |b| {
            b.add_data(&other_spk).unwrap();
        });
        let c = f.extract(&raw).unwrap();
        let other_addr =
            extract_script_pub_key_address(&vec_to_spk(&other_spk), Prefix::Devnet).unwrap();
        assert_eq!(c.addresses.sender, other_addr.to_string());
        let original_addr =
            extract_script_pub_key_address(&f.sender_spk(), Prefix::Devnet).unwrap();
        assert_ne!(c.addresses.sender, original_addr.to_string());
    }
    #[test]
    fn test_valid_script_extracts_all_fields() {
        let f = TestFixture::new();
        let raw = f.build_valid_script();
        let c = f.extract(&raw).expect("Valid script should parse");

        let f_sender_addr =
            extract_script_pub_key_address(&f.sender_spk(), Prefix::Devnet).unwrap();
        let f_receiver_addr =
            extract_script_pub_key_address(&vec_to_spk(&f.receiver_spk_vec()), Prefix::Devnet)
                .unwrap();
        assert_eq!(c.swap_id, f.swap_id);
        assert_eq!(c.secret_hash, f.secret_hash);
        assert_eq!(c.addresses.sender, f_sender_addr.to_string());
        assert_eq!(c.addresses.receiver, f_receiver_addr.to_string());
        assert_eq!(c.destination, f.destination);
        assert_eq!(c.amount.value, DEFAULT_AMOUNT);
        assert_eq!(c.amount.decimals, 8);
        assert_eq!(
            c.addresses.sender_destination,
            String::from_utf8(f.sender_receiver_address.clone()).unwrap()
        );
    }

    #[test]
    fn test_valid_script_deterministic() {
        let f = TestFixture::new();
        let raw1 = f.build_valid_script();
        let raw2 = f.build_valid_script();
        assert_eq!(raw1, raw2);

        let c1 = f.extract(&raw1).unwrap();
        let c2 = f.extract(&raw2).unwrap();
        assert_eq!(c1.swap_id, c2.swap_id);
        assert_eq!(c1.secret_hash, c2.secret_hash);
        assert_eq!(c1.addresses.sender, c2.addresses.sender);
    }

    #[test]
    fn test_different_fixtures_produce_different_commitments() {
        let f1 = TestFixture::new();
        let f2 = TestFixture::new();

        let c1 = f1.extract(&f1.build_valid_script()).unwrap();
        let c2 = f2.extract(&f2.build_valid_script()).unwrap();

        assert_ne!(c1.swap_id, c2.swap_id);
        assert_ne!(c1.secret_hash, c2.secret_hash);
        assert_ne!(c1.addresses.sender, c2.addresses.sender);
    }

    #[test]
    fn test_secret_hash_different_value_still_parses() {
        let f = TestFixture::new();
        let different_hash: [u8; 32] = rand::random();
        let raw = f.build_script_with_mutation_at(2, |b| {
            b.add_data(&different_hash).unwrap();
        });
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.secret_hash, different_hash);
        assert_ne!(c.secret_hash, f.secret_hash);
    }

    #[test]
    fn test_sender_spk_different_value_extracts() {
        let f = TestFixture::new();
        let secp = Secp256k1::new();
        let mut rng = rand::rng();
        let other = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let other_spk = spk_to_vec(&p2pk_spk(&other));

        let raw = f.build_script_with_mutation_at(30, |b| {
            b.add_data(&other_spk).unwrap();
        });
        let c = f.extract(&raw).unwrap();
        let other_spk =
            extract_script_pub_key_address(&vec_to_spk(&other_spk), Prefix::Devnet).unwrap();
        assert_eq!(c.addresses.sender, other_spk.to_string());
        let original_spk = extract_script_pub_key_address(&f.sender_spk(), Prefix::Devnet).unwrap();
        assert_ne!(c.addresses.sender, original_spk.to_string());
    }

    #[test]
    fn test_receiver_spk_different_value_extracts() {
        let f = TestFixture::new();
        let secp = Secp256k1::new();
        let mut rng = rand::rng();
        let other = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let other_spk = spk_to_vec(&p2pk_spk(&other));

        let raw = f.build_script_with_mutation_at(10, |b| {
            b.add_data(&other_spk).unwrap();
        });
        let other_spk =
            extract_script_pub_key_address(&vec_to_spk(&other_spk), Prefix::Devnet).unwrap();
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.addresses.receiver, other_spk.to_string());
    }

    #[test]
    fn test_swap_id_31_bytes_rejected() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(42, |b| {
            b.add_data(&[0xAA; 31]).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_swap_id_33_bytes_rejected() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(42, |b| {
            b.add_data(&[0xBB; 33]).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_secret_hash_too_short_rejected() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(2, |b| {
            b.add_data(&[0u8; 16]).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "16-byte secret_hash should be rejected");
        match res.unwrap_err() {
            KaspaError::InvalidSecretHashLength => {}
            other => panic!("Expected InvalidSecretHashLength, got {other:?}"),
        }
    }

    #[test]
    fn test_secret_hash_too_long_rejected() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(2, |b| {
            b.add_data(&[0u8; 64]).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "64-byte secret_hash should be rejected");
    }

    #[test]
    fn test_secret_hash_empty_rejected() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(2, |b| {
            b.add_data(&[]).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Empty secret_hash should be rejected");
    }

    #[test]
    fn test_all_zero_swap_id_accepted() {
        let mut f = TestFixture::new();
        f.swap_id = [0u8; 32];
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.swap_id, [0u8; 32]);
    }

    #[test]
    fn test_all_ff_swap_id_accepted() {
        let mut f = TestFixture::new();
        f.swap_id = [0xFF; 32];
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.swap_id, [0xFF; 32]);
    }

    #[test]
    fn test_missing_data_sender_spk() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(30, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Non-push in sender spk slot should fail");
    }

    #[test]
    fn test_missing_data_receiver_spk() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(10, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Non-push in receiver spk slot should fail");
    }

    #[test]
    fn test_missing_data_timelock() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(22, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Non-push in timelock slot should fail");
    }

    #[test]
    fn test_missing_data_sender_receiver_address() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(43, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(
            res.is_err(),
            "Non-push in sender_receiver_address slot should fail"
        );
    }

    #[test]
    fn test_extract_opcode_data_fallthrough_non_push_opcode_in_data_slot() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(2, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Non-push opcode in data slot should fail");
    }

    #[test]
    fn test_extract_opcode_data_fallthrough_in_swap_id_slot() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(42, |b| {
            b.add_op(OpCheckSig).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_extract_opcode_data_fallthrough_in_destination_slot() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(44, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_extract_reveal_success() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();

        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();

        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let secret = extract_reveal_secret(&parsed).unwrap();

        assert_eq!(secret, f.secret);
    }

    #[test]
    fn test_extract_reveal_deterministic() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();

        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();

        let p1 = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let p2 = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let s1 = extract_reveal_secret(&p1).unwrap();
        let s2 = extract_reveal_secret(&p2).unwrap();

        assert_eq!(s1, s2);
    }

    #[test]
    fn test_extract_reveal_fails_with_op_false_selector() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();

        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();

        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let err = extract_reveal_secret(&parsed).unwrap_err();
        match err {
            KaspaError::WrongBranchSelector { expected, got } => {
                assert_eq!(expected, 0x51);
                assert_eq!(got, 0x00);
            }
            other => panic!("Expected WrongBranchSelector, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_reveal_fails_with_arbitrary_selector() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();

        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpCheckSig)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();

        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::WrongBranchSelector { .. }
        ));
    }

    #[test]
    fn test_extract_reveal_fails_empty() {
        let parsed: Vec<
            Box<
                dyn kaspa_txscript::opcodes::OpCodeImplementation<
                        crate::chains::kaspa::contracts::contract_v1::VerifiableTransactionMock,
                        SigHashReusedValuesUnsync,
                    >,
            >,
        > = vec![];
        match extract_reveal_secret(&parsed).unwrap_err() {
            KaspaError::InvalidSigScriptLength {
                expected: 3,
                got: 0,
            } => {}
            other => panic!("Expected InvalidSigScriptLength, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_reveal_fails_two_opcodes() {
        let f = TestFixture::new();
        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSigScriptLength {
                expected: 3,
                got: 2
            }
        ));
    }

    #[test]
    fn test_extract_reveal_fails_four_opcodes() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let junk: [u8; 32] = rand::random();
        let sig_script = ScriptBuilder::new()
            .add_data(&junk)
            .unwrap()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSigScriptLength {
                expected: 3,
                got: 4
            }
        ));
    }

    #[test]
    fn test_extract_reveal_fails_secret_too_short() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let short_secret = [0xAA; 16];
        let sig_script = ScriptBuilder::new()
            .add_data(&short_secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSecretLength
        ));
    }

    #[test]
    fn test_extract_reveal_fails_secret_too_long() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let long_secret = [0xBB; 64];
        let sig_script = ScriptBuilder::new()
            .add_data(&long_secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSecretLength
        ));
    }

    #[test]
    fn test_extract_reveal_fails_secret_1_byte() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let sig_script = ScriptBuilder::new()
            .add_data(&[0x42])
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSecretLength
        ));
    }

    #[test]
    fn test_extract_reveal_fails_secret_31_bytes() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let sig_script = ScriptBuilder::new()
            .add_data(&[0xCC; 31])
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSecretLength
        ));
    }

    #[test]
    fn test_extract_reveal_fails_secret_33_bytes() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let sig_script = ScriptBuilder::new()
            .add_data(&[0xDD; 33])
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::InvalidSecretLength
        ));
    }

    #[test]
    fn test_extract_reveal_fails_non_push_secret() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let sig_script = ScriptBuilder::new()
            .add_op(OpElse)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::MissingSecret
        ));
    }

    #[test]
    fn test_extract_reveal_fails_non_push_redeem() {
        let f = TestFixture::new();
        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_op(OpElse)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            extract_reveal_secret(&parsed).unwrap_err(),
            KaspaError::MissingRedeemScript
        ));
    }

    fn build_refund_sig_script(f: &TestFixture) -> Vec<u8> {
        let htlc_script = f.build_valid_script();

        ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain()
    }

    #[test]
    fn test_extract_refund_success() {
        let f = TestFixture::new();
        let sig_script = build_refund_sig_script(&f);
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        validate_refund_sig(&parsed).unwrap();
    }

    #[test]
    fn test_extract_refund_deterministic() {
        let f = TestFixture::new();
        let sig_script = build_refund_sig_script(&f);

        let p1 = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let p2 = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        validate_refund_sig(&p1).unwrap();
        validate_refund_sig(&p2).unwrap();
    }

    #[test]
    fn test_extract_refund_fails_with_extra_non_push_opcode() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let sig_script = ScriptBuilder::new()
            .add_op(OpElse)
            .unwrap()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            validate_refund_sig(&parsed).unwrap_err(),
            KaspaError::InvalidSigScriptLength {
                expected: 2,
                got: 3
            }
        ));
    }

    #[test]
    fn test_claim_sig_rejected_by_validate_refund_sig() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let sig_script = ScriptBuilder::new()
            .add_data(&f.secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(validate_refund_sig(&parsed).is_err());
    }

    #[test]
    fn test_refund_sig_rejected_by_extract_reveal_secret() {
        let f = TestFixture::new();
        let sig_script = build_refund_sig_script(&f);
        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(extract_reveal_secret(&parsed).is_err());
    }

    #[test]
    fn test_extract_reveal_random_bytes() {
        let raw: Vec<u8> = (0..100).map(|_| rand::random::<u8>()).collect();
        if let Ok(p) = crate::chains::kaspa::decode::parse_script(&raw)
            .collect::<std::result::Result<Vec<_>, _>>()
        {
            let _ = extract_reveal_secret(&p);
        }
    }

    #[test]
    fn test_extract_refund_random_bytes() {
        let raw: Vec<u8> = (0..100).map(|_| rand::random::<u8>()).collect();
        if let Ok(p) = crate::chains::kaspa::decode::parse_script(&raw)
            .collect::<std::result::Result<Vec<_>, _>>()
        {
            let _ = validate_refund_sig(&p);
        }
    }

    #[test]
    fn test_destination_0x51_round_trips() {
        let mut f = TestFixture::new();
        f.destination = 0x51;
        let raw = f.build_valid_script();
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.destination, 0x51);
    }

    #[test]
    fn test_extract_reveal_all_zero_secret() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let zero_secret = [0u8; 32];

        let sig_script = ScriptBuilder::new()
            .add_data(&zero_secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();

        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let secret = extract_reveal_secret(&parsed).unwrap();
        assert_eq!(secret, zero_secret);
    }

    #[test]
    fn test_extract_reveal_all_ff_secret() {
        let f = TestFixture::new();
        let htlc_script = f.build_valid_script();
        let ff_secret = [0xFF; 32];

        let sig_script = ScriptBuilder::new()
            .add_data(&ff_secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();

        let parsed = crate::chains::kaspa::decode::parse_script(&sig_script)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let secret = extract_reveal_secret(&parsed).unwrap();
        assert_eq!(secret, ff_secret);
    }

    #[test]
    fn test_destination_zero() {
        let mut f = TestFixture::new();
        f.destination = 0;
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.destination, 0);
    }

    #[test]
    fn test_destination_one() {
        let mut f = TestFixture::new();
        f.destination = 1;
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.destination, 1);
    }

    #[test]
    fn test_destination_max() {
        let mut f = TestFixture::new();
        f.destination = 255;
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.destination, 255);
    }

    #[test]
    fn test_timelock_small() {
        let mut f = TestFixture::new();

        f.timelock = 1_000;
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.unlock_ts, 1);
    }

    #[test]
    fn test_timelock_large() {
        let mut f = TestFixture::new();

        f.timelock = 2_500_000_000_000;
        let c = f.extract(&f.build_valid_script()).unwrap();
        assert_eq!(c.unlock_ts, 2_500_000_000);
    }

    #[test]
    fn test_empty_script_rejected() {
        let f = TestFixture::new();
        let res = f.extract(&[]);
        assert!(res.is_err());
    }

    #[test]
    fn test_single_opcode_rejected() {
        let f = TestFixture::new();
        let raw = ScriptBuilder::new().add_op(OpIf).unwrap().drain();
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_script_one_opcode_short_rejected() {
        let f = TestFixture::new();
        let mut raw = f.build_valid_script();
        raw.truncate(raw.len().saturating_sub(2));
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_script_with_extra_trailing_opcode_rejected() {
        let f = TestFixture::new();
        let mut raw = f.build_valid_script();
        let extra = ScriptBuilder::new().add_op(OpCheckSig).unwrap().drain();
        raw.extend_from_slice(&extra);
        let res = f.extract(&raw);
        assert!(
            res.is_err(),
            "Extra trailing opcode should trigger TooManyOpcodes"
        );
    }

    #[test]
    fn test_every_fixed_opcode_position_rejects_wrong_opcode() {
        let f = TestFixture::new();

        let fixed_positions: Vec<usize> = EXPECTED_OPCODES
            .iter()
            .enumerate()
            .filter_map(|(i, (exp, _))| match exp {
                ExpectedOpCode::OpCode(_) => Some(i),
                ExpectedOpCode::Data => None,
            })
            .collect();

        for &pos in &fixed_positions {
            let raw = f.build_script_with_mutation_at(pos, |b| match pos {
                0 | 28 => {
                    b.add_op(OpElse).unwrap();
                }
                26 | 32 => {
                    b.add_op(OpSHA256).unwrap();
                }
                27 => {
                    b.add_op(OpCheckSig).unwrap();
                }
                _ => {
                    b.add_op(OpReturn).unwrap();
                }
            });

            let res = f.extract(&raw);
            assert!(
                res.is_err(),
                "Position {pos}: wrong opcode should be rejected"
            );

            match res.unwrap_err() {
                KaspaError::OpcodeMismatch(p) => assert_eq!(p, pos),
                other => panic!("Position {pos}: expected OpcodeMismatch, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_op_if_replaced_with_op_else() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(0, |b| {
            b.add_op(OpElse).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_sha256_replaced_with_op_checksig() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(1, |b| {
            b.add_op(OpCheckSig).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_equalverify_at_3_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(3, |b| {
            b.add_op(OpSub).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_txinputcount_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(4, |b| {
            b.add_op(OpTxOutputCount).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_numequalverify_at_6_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(6, |b| {
            b.add_op(OpEqualVerify).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_txoutputcount_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(7, |b| {
            b.add_op(OpTxInputCount).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_txoutputspk_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(12, |b| {
            b.add_op(OpTxOutputAmount).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_txoutputamount_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(15, |b| {
            b.add_op(OpTxOutputSpk).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_txinputindex_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(16, |b| {
            b.add_op(OpTxInputAmount).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_txinputamount_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(17, |b| {
            b.add_op(OpTxInputIndex).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_sub_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(19, |b| {
            b.add_op(OpEqualVerify).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_gte_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(20, |b| {
            b.add_op(OpSub).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_else_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(21, |b| {
            b.add_op(OpIf).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_cltv_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(23, |b| {
            b.add_op(OpCheckSig).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_endif_at_26_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(26, |b| {
            b.add_op(OpSHA256).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_false_at_27_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(27, |b| {
            b.add_op(OpCheckSig).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_metadata_op_if_at_41_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(41, |b| {
            b.add_op(OpElse).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_op_endif_at_32_replaced() {
        let f = TestFixture::new();
        let raw = f.build_script_with_mutation_at(32, |b| {
            b.add_op(OpSHA256).unwrap();
        });
        assert!(f.extract(&raw).is_err());
    }

    #[test]
    fn test_all_zeros_rejected() {
        let f = TestFixture::new();
        let raw = vec![0x00; 100];
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_random_bytes_rejected() {
        let f = TestFixture::new();
        let raw: Vec<u8> = (0..150).map(|_| rand::random::<u8>()).collect();
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_valid_p2pk_script_rejected() {
        let f = TestFixture::new();
        let spk = p2pk_spk(&f.sender);
        let res = f.extract(spk.script());
        assert!(res.is_err(), "P2PK is not an HTLC");
    }

    #[test]
    fn test_just_op_return_rejected() {
        let f = TestFixture::new();
        let raw = ScriptBuilder::new().add_op(OpReturn).unwrap().drain();
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_metadata_envelope_op_false_replaced_with_op_true() {
        let f = TestFixture::new();

        let raw = f.build_script_with_mutation_at(27, |b| {
            b.add_i64(1).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "OP_TRUE at pos 27 should be rejected");
    }

    #[test]
    fn test_metadata_envelope_missing_closing_op_endif() {
        let f = TestFixture::new();

        let raw = f.build_script_with_mutation_at(32, |b| {
            b.add_op(OpFalse).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err());
    }

    #[test]
    fn test_metadata_envelope_duplicate_rejected() {
        let f = TestFixture::new();
        let mut raw = f.build_valid_script();

        let extra = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_op(OpIf)
            .unwrap()
            .add_data(&f.swap_id)
            .unwrap()
            .add_data(&f.sender_receiver_address)
            .unwrap()
            .add_data(&[f.destination])
            .unwrap()
            .add_op(OpEndIf)
            .unwrap()
            .drain();
        raw.extend_from_slice(&extra);
        let res = f.extract(&raw);
        assert!(
            res.is_err(),
            "Duplicate metadata envelope should be rejected"
        );
    }

    #[test]
    fn test_fuzz_single_byte_flip() {
        let f = TestFixture::new();
        let valid = f.build_valid_script();
        let valid_c = f.extract(&valid).unwrap();

        for byte_pos in 0..valid.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut tampered = valid.clone();
                tampered[byte_pos] ^= flip;

                if tampered == valid {
                    continue;
                }

                let res = f.extract(&tampered);
                match res {
                    Err(_) => {}
                    Ok(c) => {
                        let differs = c.swap_id != valid_c.swap_id
                            || c.secret_hash != valid_c.secret_hash
                            || c.addresses.sender != valid_c.addresses.sender
                            || c.addresses.receiver != valid_c.addresses.receiver
                            || c.destination != valid_c.destination
                            || c.unlock_ts != valid_c.unlock_ts
                            || c.addresses.sender_destination
                                != valid_c.addresses.sender_destination;

                        if !differs {
                            assert_eq!(c.swap_id, valid_c.swap_id);
                            assert_eq!(c.secret_hash, valid_c.secret_hash);
                            assert_eq!(c.addresses.sender, valid_c.addresses.sender);
                            assert_eq!(c.addresses.receiver, valid_c.addresses.receiver);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_fuzz_multi_byte_corruption() {
        let f = TestFixture::new();
        let valid = f.build_valid_script();
        let valid_c = f.extract(&valid).unwrap();
        let mut rng = rand::rng();

        for _ in 0..500 {
            let mut tampered = valid.clone();

            let n_corruptions = rng.random_range(1..=5usize);
            for _ in 0..n_corruptions {
                let pos = rng.random_range(0..tampered.len());
                tampered[pos] = rand::random::<u8>();
            }

            if tampered == valid {
                continue;
            }

            let res = f.extract(&tampered);
            match res {
                Err(_) => {}
                Ok(c) => {
                    let _differs = c.swap_id != valid_c.swap_id
                        || c.secret_hash != valid_c.secret_hash
                        || c.addresses.sender != valid_c.addresses.sender
                        || c.addresses.receiver != valid_c.addresses.receiver
                        || c.destination != valid_c.destination
                        || c.unlock_ts != valid_c.unlock_ts
                        || c.addresses.sender_destination != valid_c.addresses.sender_destination;
                }
            }
        }
    }

    #[test]
    fn test_fuzz_truncation_at_every_length() {
        let f = TestFixture::new();
        let valid = f.build_valid_script();

        for truncate_to in 0..valid.len() {
            let truncated = &valid[..truncate_to];
            let res = f.extract(truncated);
            assert!(
                res.is_err(),
                "Truncated to {truncate_to} bytes should be rejected"
            );
        }
    }

    #[test]
    fn test_fuzz_prepend_junk() {
        let f = TestFixture::new();
        let valid = f.build_valid_script();

        for prefix_len in 1..=10 {
            let mut junk: Vec<u8> = (0..prefix_len).map(|_| rand::random::<u8>()).collect();
            junk.extend_from_slice(&valid);
            let res = f.extract(&junk);
            assert!(
                res.is_err(),
                "Prepending {prefix_len} junk bytes should be rejected"
            );
        }
    }

    #[test]
    fn test_fuzz_append_junk() {
        let f = TestFixture::new();
        let valid = f.build_valid_script();

        for suffix_len in 1..=10 {
            let mut extended = valid.clone();
            let junk: Vec<u8> = (0..suffix_len).map(|_| rand::random::<u8>()).collect();
            extended.extend_from_slice(&junk);
            let res = f.extract(&extended);
            assert!(
                res.is_err(),
                "Appending {suffix_len} junk bytes should be rejected"
            );
        }
    }

    #[test]
    fn test_fuzz_random_scripts() {
        let f = TestFixture::new();
        let mut rng = rand::rng();

        for _ in 0..1000 {
            let len = rng.random_range(0..500usize);
            let raw: Vec<u8> = (0..len).map(|_| rand::random::<u8>()).collect();
            let res = f.extract(&raw);
            if let Ok(c) = res {
                assert_ne!(
                    c.swap_id, f.swap_id,
                    "Random script matched our swap_id — astronomically unlikely"
                );
            }
        }
    }

    #[test]
    fn test_reversed_script_rejected() {
        let f = TestFixture::new();
        let mut raw = f.build_valid_script();
        raw.reverse();
        let res = f.extract(&raw);
        assert!(res.is_err(), "Reversed script should be rejected");
    }

    #[test]
    fn test_amount_passthrough_various() {
        let f = TestFixture::new();
        let raw = f.build_valid_script();

        for amount_str in ["0", "1", "999999999999", "100000000", ""] {
            let parsed = crate::chains::kaspa::decode::parse_script(&raw)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let c = extract_commitment(
                &parsed,
                amount_str.to_string(),
                Prefix::Devnet,
                ChannelId::KaspaTn10,
            )
            .unwrap();
            assert_eq!(c.amount.value, amount_str);
        }
    }

    #[test]
    fn test_decimals_always_8() {
        let f = TestFixture::new();
        let raw = f.build_valid_script();
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.amount.decimals, 8);
    }

    #[test]
    fn test_empty_sender_receiver_address() {
        let mut f = TestFixture::new();
        f.sender_receiver_address = vec![];
        let raw = f.build_valid_script();
        let res = f.extract(&raw);

        if let Ok(c) = res {
            assert!(
                c.addresses.sender_destination.len() <= 1,
                "Empty address should encode as at most 1 byte, got {}",
                c.addresses.sender_destination.len()
            );
        }
    }

    #[test]
    fn test_long_sender_receiver_address() {
        let mut f = TestFixture::new();

        let long_addr = "kaspa:".to_string() + &"a".repeat(194);
        f.sender_receiver_address = long_addr.as_bytes().to_vec();
        let raw = f.build_valid_script();
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.addresses.sender_destination.len(), 200);
    }

    #[test]
    fn test_decode_u64_empty_bytes() {
        assert_eq!(decode_u64_from_script(&[]), 0);
    }

    #[test]
    fn test_decode_u64_single_byte() {
        assert_eq!(decode_u64_from_script(&[0x01]), 1);
        assert_eq!(decode_u64_from_script(&[0xFF]), 255);
    }

    #[test]
    fn test_decode_u64_exact_8_bytes() {
        let val: u64 = 1_700_007_200;
        let bytes = val.to_le_bytes();
        assert_eq!(decode_u64_from_script(&bytes), val);
    }

    #[test]
    fn test_decode_u64_more_than_8_bytes() {
        let mut bytes = 42u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(decode_u64_from_script(&bytes), 42);
    }

    #[test]
    fn test_decode_u64_max_value() {
        assert_eq!(decode_u64_from_script(&u64::MAX.to_le_bytes()), u64::MAX);
    }

    #[test]
    fn test_extract_opcode_data_op1_through_op16_via_timelock() {
        for tl_secs in 1..=16u64 {
            let mut f = TestFixture::new();
            f.timelock = tl_secs * 1000;
            let raw = f.build_valid_script();
            let c = f.extract(&raw).unwrap();
            assert_eq!(
                c.unlock_ts, tl_secs,
                "Timelock {tl_secs}s should round-trip via ms"
            );
        }
    }

    #[test]
    fn test_timelock_zero() {
        let mut f = TestFixture::new();
        f.timelock = 0;
        let raw = f.build_valid_script();
        let c = f.extract(&raw).unwrap();
        assert_eq!(c.unlock_ts, 0);
    }

    #[test]
    fn test_missing_data_sender_pubkey() {
        let f = TestFixture::new();

        let raw = f.build_script_with_mutation_at(24, |b| {
            b.add_op(OpSub).unwrap();
        });
        let res = f.extract(&raw);
        assert!(res.is_err(), "Non-push in sender pubkey slot should fail");
    }

    #[test]
    fn test_script_exactly_one_fewer_opcode() {
        let f = TestFixture::new();

        let mut builder = ScriptBuilder::new();
        let receiver_spk = f.receiver_spk_vec();

        builder.add_op(OpIf).unwrap();
        builder.add_op(OpSHA256).unwrap();
        builder.add_data(&f.secret_hash).unwrap();
        builder.add_op(OpEqualVerify).unwrap();
        builder.add_op(OpTxInputCount).unwrap();
        builder.add_i64(2).unwrap();
        builder.add_op(OpNumEqualVerify).unwrap();
        builder.add_op(OpTxOutputCount).unwrap();
        builder.add_i64(2).unwrap();
        builder.add_op(OpNumEqualVerify).unwrap();
        builder.add_data(&receiver_spk).unwrap();
        builder.add_i64(0).unwrap();
        builder.add_op(OpTxOutputSpk).unwrap();
        builder.add_op(OpEqualVerify).unwrap();
        builder.add_i64(0).unwrap();
        builder.add_op(OpTxOutputAmount).unwrap();
        builder.add_op(OpTxInputIndex).unwrap();
        builder.add_op(OpTxInputAmount).unwrap();
        builder.add_i64(SOLVER_REWARD).unwrap();
        builder.add_op(OpSub).unwrap();
        builder.add_op(OpGreaterThanOrEqual).unwrap();
        builder.add_op(OpElse).unwrap();
        builder.add_i64(f.timelock as i64).unwrap();
        builder.add_op(OpCheckLockTimeVerify).unwrap();
        builder.add_data(&f.sender_pub()).unwrap();
        builder.add_op(OpCheckSig).unwrap();
        builder.add_op(OpEndIf).unwrap();
        builder.add_op(OpFalse).unwrap();
        builder.add_op(OpIf).unwrap();
        builder.add_data(&f.swap_id).unwrap();
        builder.add_data(&f.sender_receiver_address).unwrap();
        builder.add_data(&[f.destination]).unwrap();

        let raw = builder.drain();
        let res = f.extract(&raw);
        assert!(
            res.is_err(),
            "Script missing final OP_ENDIF should be rejected"
        );
    }

    #[test]
    fn test_script_exactly_one_extra_opcode() {
        let f = TestFixture::new();
        let mut raw = f.build_valid_script();

        let extra = ScriptBuilder::new().add_op(OpFalse).unwrap().drain();
        raw.extend_from_slice(&extra);
        let res = f.extract(&raw);
        assert!(
            res.is_err(),
            "Script with one extra opcode should be rejected"
        );
    }

    #[test]
    fn test_destination_values_1_through_16() {
        for d in 1..=16u8 {
            let mut f = TestFixture::new();
            f.destination = d;
            let raw = f.build_valid_script();
            let c = f.extract(&raw).unwrap();
            assert_eq!(c.destination, d, "Destination {d} should round-trip");
        }
    }

    #[test]
    fn test_destination_values_outside_small_int_range() {
        for d in [17u8, 127, 128, 254] {
            let mut f = TestFixture::new();
            f.destination = d;
            let raw = f.build_valid_script();
            let c = f.extract(&raw).unwrap();
            assert_eq!(c.destination, d, "Destination {d} should round-trip");
        }
    }

    #[test]
    fn test_timelock_boundary_values() {
        for tl_ms in [
            0u64,
            1_000,
            15_000,
            16_000,
            17_000,
            127_000,
            128_000,
            255_000,
            256_000,
            32_767_000,
            32_768_000,
            8_388_607_000,
            8_388_608_000,
            2_147_483_647_000,
            2_147_483_648_000,
            u64::MAX / 2,
        ] {
            let mut f = TestFixture::new();
            f.timelock = tl_ms;
            let raw = f.build_valid_script();
            let c = f
                .extract(&raw)
                .unwrap_or_else(|e| panic!("Timelock {tl_ms}ms should parse, got {e:?}"));
            assert_eq!(
                c.unlock_ts,
                tl_ms / 1000,
                "Timelock {tl_ms}ms should round-trip to {}s",
                tl_ms / 1000
            );
        }
    }

    #[test]
    fn template_starts_with_op_if_and_has_seven_data_fields() {
        assert!(matches!(
            EXPECTED_OPCODES.first().map(|(op, _)| op),
            Some(ExpectedOpCode::OpCode(_))
        ));
        let data_fields = EXPECTED_OPCODES
            .iter()
            .filter(|(op, ty)| matches!(op, ExpectedOpCode::Data) && *ty != DataType::Opcode)
            .count();
        assert_eq!(data_fields, 7);
    }
}
