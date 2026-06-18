#[cfg(test)]
mod tests {
    use crate::chains::kaspa::contracts::contract_v1::{SOLVER_REWARD, create_htlc_script};

    use kaspa_consensus_core::tx::{
        Transaction, TransactionInput, TransactionOutpoint, TransactionOutput, UtxoEntry,
    };
    use kaspa_consensus_core::{
        hashing::{
            sighash::{SigHashReusedValuesUnsync, calc_schnorr_signature_hash},
            sighash_type::SIG_HASH_ALL,
        },
        subnets::SUBNETWORK_ID_NATIVE,
        tx::{MutableTransaction, VerifiableTransaction},
    };
    use kaspa_hashes::Hash;
    use kaspa_txscript::opcodes::codes::{OpFalse, OpTrue};
    use kaspa_txscript::{
        TxScriptEngine, caches::Cache, pay_to_script_hash_script, script_builder::ScriptBuilder,
    };
    use secp256k1::{Keypair, Secp256k1};
    use sha2::{Digest, Sha256};

    use crate::chains::kaspa::broadcast::spk_to_vec;
    use crate::chains::kaspa::test_helpers::p2pk_spk;

    fn build_ccr_tx(
        htlc_spk: &kaspa_consensus_core::tx::ScriptPublicKey,
        htlc_value: u64,
        solver_fee_value: u64,
        solver_fee_spk: &kaspa_consensus_core::tx::ScriptPublicKey,
        outputs: Vec<TransactionOutput>,
        lock_time: u64,
    ) -> (Transaction, Vec<UtxoEntry>) {
        let htlc_utxo = UtxoEntry::new(htlc_value, htlc_spk.clone(), 0, false);
        let fee_utxo = UtxoEntry::new(solver_fee_value, solver_fee_spk.clone(), 0, false);

        let input0 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        };
        let input1 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(2), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };

        let tx = Transaction::new(
            1,
            vec![input0, input1],
            outputs,
            lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        (tx, vec![htlc_utxo, fee_utxo])
    }

    fn build_refund_tx(
        htlc_spk: &kaspa_consensus_core::tx::ScriptPublicKey,
        htlc_value: u64,
        outputs: Vec<TransactionOutput>,
        lock_time: u64,
    ) -> (Transaction, Vec<UtxoEntry>) {
        let utxo = UtxoEntry::new(htlc_value, htlc_spk.clone(), 0, false);

        let input = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };

        let tx = Transaction::new(
            1,
            vec![input],
            outputs,
            lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        (tx, vec![utxo])
    }

    fn build_refund_tx_2in(
        htlc_spk: &kaspa_consensus_core::tx::ScriptPublicKey,
        htlc_value: u64,
        fee_spk: &kaspa_consensus_core::tx::ScriptPublicKey,
        fee_value: u64,
        outputs: Vec<TransactionOutput>,
        lock_time: u64,
    ) -> (Transaction, Vec<UtxoEntry>) {
        let input0 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        };
        let input1 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(2), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };

        let utxo0 = UtxoEntry::new(htlc_value, htlc_spk.clone(), 0, false);
        let utxo1 = UtxoEntry::new(fee_value, fee_spk.clone(), 0, false);

        let tx = Transaction::new(
            1,
            vec![input0, input1],
            outputs,
            lock_time,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        (tx, vec![utxo0, utxo1])
    }

    #[test]
    fn test_refund_succeeds_with_exact_amount_minus_solver_reward() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;
        let refund_amount = input_value - SOLVER_REWARD as u64;

        let output0 = TransactionOutput::new(refund_amount, sender_p2pk.clone());
        let output1 = TransactionOutput::new(
            SOLVER_REWARD as u64 + fee_input_value - fee,
            executor_p2pk.clone(),
        );

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let reused_values = SigHashReusedValuesUnsync::new();
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            1,
            SIG_HASH_ALL,
            &reused_values,
        );
        let msg = secp256k1::Message::from_digest(sig_hash.as_bytes());
        let sig = executor.sign_schnorr(msg.as_ref());
        let mut signature = Vec::new();
        signature.extend_from_slice(sig.as_ref());
        signature.push(SIG_HASH_ALL.to_u8());
        mutable_tx.tx.inputs[1].signature_script =
            ScriptBuilder::new().add_data(&signature).unwrap().drain();

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("Refund should succeed with exact amount minus solver reward");
    }

    #[test]
    fn test_refund_succeeds_with_more_than_minimum() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let reused_values = SigHashReusedValuesUnsync::new();
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            1,
            SIG_HASH_ALL,
            &reused_values,
        );
        let msg = secp256k1::Message::from_digest(sig_hash.as_bytes());
        let sig = executor.sign_schnorr(msg.as_ref());
        let mut signature = Vec::new();
        signature.extend_from_slice(sig.as_ref());
        signature.push(SIG_HASH_ALL.to_u8());
        mutable_tx.tx.inputs[1].signature_script =
            ScriptBuilder::new().add_data(&signature).unwrap().drain();

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("Refund should succeed when sender gets more than minimum");
    }

    #[test]
    fn test_refund_fails_when_sender_gets_one_sompi_less_than_minimum() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;

        let refund_amount = input_value - SOLVER_REWARD as u64 - 1;
        let output0 = TransactionOutput::new(refund_amount, sender_p2pk.clone());
        let output1 = TransactionOutput::new(
            SOLVER_REWARD as u64 + 1 + fee_input_value,
            executor_p2pk.clone(),
        );

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when sender gets 1 sompi less than minimum");
    }

    #[test]
    fn test_refund_fails_when_executor_takes_double_reward() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;

        let stolen = SOLVER_REWARD as u64 * 2;
        let refund_amount = input_value - stolen;
        let output0 = TransactionOutput::new(refund_amount, sender_p2pk.clone());
        let output1 = TransactionOutput::new(stolen + fee_input_value, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when executor takes double the solver reward");
    }

    #[test]
    fn test_refund_fails_when_executor_takes_entire_htlc() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;

        let output0 = TransactionOutput::new(0, sender_p2pk.clone());
        let output1 = TransactionOutput::new(input_value + fee_input_value, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when executor takes entire HTLC amount");
    }

    #[test]
    fn test_refund_fails_when_sender_gets_half() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;

        let half = input_value / 2;
        let output0 = TransactionOutput::new(half, sender_p2pk.clone());
        let output1 = TransactionOutput::new(half + fee_input_value, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when sender only gets half");
    }

    #[test]
    fn test_refund_succeeds_with_small_htlc_amount() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);

        let input_value = SOLVER_REWARD as u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(0, sender_p2pk.clone());
        let output1 =
            TransactionOutput::new(input_value + fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let reused_values = SigHashReusedValuesUnsync::new();
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            1,
            SIG_HASH_ALL,
            &reused_values,
        );
        let msg = secp256k1::Message::from_digest(sig_hash.as_bytes());
        let sig = executor.sign_schnorr(msg.as_ref());
        let mut signature = Vec::new();
        signature.extend_from_slice(sig.as_ref());
        signature.push(SIG_HASH_ALL.to_u8());
        mutable_tx.tx.inputs[1].signature_script =
            ScriptBuilder::new().add_data(&signature).unwrap().drain();

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("Refund should succeed when HTLC equals solver reward (sender gets 0)");
    }

    #[test]
    fn test_refund_path_success() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let reused_values = SigHashReusedValuesUnsync::new();
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            1,
            SIG_HASH_ALL,
            &reused_values,
        );
        let msg = secp256k1::Message::from_digest(sig_hash.as_bytes());
        let sig = executor.sign_schnorr(msg.as_ref());
        let mut signature = Vec::new();
        signature.extend_from_slice(sig.as_ref());
        signature.push(SIG_HASH_ALL.to_u8());
        let fee_sig_script = ScriptBuilder::new().add_data(&signature).unwrap().drain();
        mutable_tx.tx.inputs[1].signature_script = fee_sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect("Refund path should succeed");
    }

    #[test]
    fn test_refund_fails_before_timelock() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock - 1,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail before timelock");
    }

    #[test]
    fn test_refund_fails_when_receiver_redirects_funds() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, p2pk_spk(&receiver));
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Receiver should not be able to redirect refund");
    }

    #[test]
    fn test_refund_fails_when_attacker_redirects_funds() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let attacker = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let attacker_p2pk = p2pk_spk(&attacker);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, attacker_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, attacker_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &attacker_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Attacker should not be able to redirect refund");
    }

    #[test]
    fn test_refund_fails_with_finalized_sequence() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let utxo0 = UtxoEntry::new(input_value, spk.clone(), 0, false);
        let utxo1 = UtxoEntry::new(fee_input_value, executor_p2pk.clone(), 0, false);

        let input0 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: u64::MAX,
            sig_op_count: 0,
        };
        let input1 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(2), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };

        let tx = Transaction::new(
            1,
            vec![input0, input1],
            vec![output0, output1],
            timelock,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, vec![utxo0, utxo1]);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail with finalized sequence");
    }

    #[test]
    fn test_refund_succeeds_with_locktime_above_timelock() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock + 1000,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let reused_values = SigHashReusedValuesUnsync::new();
        let sig_hash = calc_schnorr_signature_hash(
            &mutable_tx.as_verifiable(),
            1,
            SIG_HASH_ALL,
            &reused_values,
        );
        let msg = secp256k1::Message::from_digest(sig_hash.as_bytes());
        let sig = executor.sign_schnorr(msg.as_ref());
        let mut signature = Vec::new();
        signature.extend_from_slice(sig.as_ref());
        signature.push(SIG_HASH_ALL.to_u8());
        let fee_sig_script = ScriptBuilder::new().add_data(&signature).unwrap().drain();
        mutable_tx.tx.inputs[1].signature_script = fee_sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("Refund should succeed when lock_time > timelock");
    }

    #[test]
    fn test_ccr_fails_with_extra_stack_data() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(SOLVER_REWARD as u64 + fee_value, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let junk: [u8; 32] = rand::random();
        let sig_script = ScriptBuilder::new()
            .add_data(&junk)
            .unwrap()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with extra stack data");
    }

    #[test]
    fn test_refund_fails_with_wrong_output_destination() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, p2pk_spk(&receiver));
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when output goes to wrong address");
    }

    #[test]
    fn test_refund_fails_with_extra_stack_data() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;
        let fee = 3_000u64;

        let output0 = TransactionOutput::new(input_value, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value - fee, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let junk: [u8; 32] = rand::random();
        let sig_script = ScriptBuilder::new()
            .add_data(&junk)
            .unwrap()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with extra stack data");
    }

    #[test]
    fn test_refund_fails_with_three_outputs() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;

        let third = input_value / 3;
        let output0 = TransactionOutput::new(third, sender_p2pk.clone());
        let output1 = TransactionOutput::new(third, executor_p2pk.clone());
        let output2 = TransactionOutput::new(third, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1, output2],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail with 3 outputs — script enforces exactly 2");
    }

    #[test]
    fn test_refund_fails_with_underpaid_sender() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let executor = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);
        let executor_p2pk = p2pk_spk(&executor);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_input_value = 100_000u64;

        let stolen = 500_000_000u64;
        let output0 = TransactionOutput::new(input_value - stolen, sender_p2pk.clone());
        let output1 = TransactionOutput::new(fee_input_value + stolen, executor_p2pk.clone());

        let (tx, entries) = build_refund_tx_2in(
            &spk,
            input_value,
            &executor_p2pk,
            fee_input_value,
            vec![output0, output1],
            timelock,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when sender gets less than full HTLC amount");
    }

    #[test]
    fn test_ccr_path_success() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_amount = SOLVER_REWARD as u64 + fee_value;

        let solver_p2pk = p2pk_spk(&solver);
        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(solver_amount, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect("CCR path should succeed");
    }

    #[test]
    fn test_ccr_fails_with_wrong_secret() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(SOLVER_REWARD as u64 + fee_value, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let wrong_secret: [u8; 32] = rand::random();
        let sig_script = ScriptBuilder::new()
            .add_data(&wrong_secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with wrong secret");
    }

    #[test]
    fn test_ccr_fails_with_wrong_receiver() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let wrong_recv = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, p2pk_spk(&wrong_recv));
        let output1 = TransactionOutput::new(SOLVER_REWARD as u64 + fee_value, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when output goes to wrong receiver");
    }

    #[test]
    fn test_ccr_fails_with_insufficient_receiver_amount() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let receiver_amount = input_value - SOLVER_REWARD as u64 - 1;
        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 =
            TransactionOutput::new(SOLVER_REWARD as u64 + 1 + fee_value, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when receiver gets insufficient amount");
    }

    #[test]
    fn test_ccr_fails_with_empty_secret() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(SOLVER_REWARD as u64 + fee_value, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let empty_secret: [u8; 0] = [];
        let sig_script = ScriptBuilder::new()
            .add_data(&empty_secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with empty secret");
    }

    #[test]
    fn test_ccr_succeeds_receiver_gets_more() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let receiver_amount = input_value - SOLVER_REWARD as u64 + 5_000_000;
        let solver_amount = SOLVER_REWARD as u64 - 5_000_000 + fee_value;

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(solver_amount, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("CCR should succeed when receiver gets more than minimum");
    }

    #[test]
    fn test_ccr_fails_with_one_input() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(SOLVER_REWARD as u64, solver_p2pk.clone());
        let utxo_entry = UtxoEntry::new(input_value, spk.clone(), 0, false);

        let input = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        };

        let tx = Transaction::new(
            1,
            vec![input],
            vec![output0, output1],
            0,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, vec![utxo_entry.clone()]);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with only 1 input");
    }

    #[test]
    fn test_ccr_fails_with_three_inputs() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 =
            TransactionOutput::new(SOLVER_REWARD as u64 + fee_value * 2, solver_p2pk.clone());

        let htlc_utxo = UtxoEntry::new(input_value, spk.clone(), 0, false);
        let fee_utxo1 = UtxoEntry::new(fee_value, solver_p2pk.clone(), 0, false);
        let fee_utxo2 = UtxoEntry::new(fee_value, solver_p2pk.clone(), 0, false);

        let input0 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        };
        let input1 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(2), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };
        let input2 = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(3), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 1,
        };

        let tx = Transaction::new(
            1,
            vec![input0, input1, input2],
            vec![output0, output1],
            0,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        let mut mutable_tx =
            MutableTransaction::with_entries(tx, vec![htlc_utxo.clone(), fee_utxo1, fee_utxo2]);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &htlc_utxo,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with 3 inputs");
    }

    #[test]
    fn test_ccr_second_input_high_value_cannot_inflate_check() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let attacker_fee_value = 50_000_000_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let receiver_amount = input_value - SOLVER_REWARD as u64 - 1;
        let solver_amount = SOLVER_REWARD as u64 + 1 + attacker_fee_value;

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(solver_amount, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            attacker_fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err(
            "Should fail: second input's high value cannot inflate the receiver amount check",
        );
    }

    #[test]
    fn test_ccr_second_input_high_value_correct_receiver_succeeds() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let large_fee_value = 50_000_000_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_amount = SOLVER_REWARD as u64 + large_fee_value;

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(solver_amount, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            large_fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("Should succeed: receiver correctly paid, solver uses own funds");
    }

    #[test]
    fn test_ccr_second_input_cannot_redirect_receiver_funds() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let receiver_amount = input_value / 2;
        let solver_amount = input_value / 2 + SOLVER_REWARD as u64 + fee_value;

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(solver_amount, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail: receiver gets less than minimum");
    }

    #[test]
    fn test_ccr_second_input_zero_value() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_amount = SOLVER_REWARD as u64;

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(solver_amount, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            0,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect("Should succeed with zero-value fee input");
    }

    #[test]
    fn test_ccr_fails_with_one_output() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(input_value + fee_value, receiver_p2pk.clone());

        let (tx, entries) =
            build_ccr_tx(&spk, input_value, fee_value, &solver_p2pk, vec![output0], 0);
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with only 1 output");
    }

    #[test]
    fn test_ccr_fails_with_three_outputs() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let attacker = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 =
            TransactionOutput::new(SOLVER_REWARD as u64 / 2 + fee_value, solver_p2pk.clone());
        let output2 = TransactionOutput::new(SOLVER_REWARD as u64 / 2, p2pk_spk(&attacker));

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1, output2],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with 3 outputs");
    }

    #[test]
    fn test_ccr_fails_with_zero_outputs() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let solver_p2pk = p2pk_spk(&solver);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;

        let (tx, entries) = build_ccr_tx(&spk, input_value, 100_000, &solver_p2pk, vec![], 0);
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_data(&secret)
            .unwrap()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with zero outputs");
    }

    #[test]
    fn test_ccr_fails_with_missing_secret() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let solver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_p2pk = p2pk_spk(&receiver);
        let receiver_spk_vec = spk_to_vec(&receiver_p2pk);

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let fee_value = 100_000u64;
        let receiver_amount = input_value - SOLVER_REWARD as u64;
        let solver_p2pk = p2pk_spk(&solver);

        let output0 = TransactionOutput::new(receiver_amount, receiver_p2pk.clone());
        let output1 = TransactionOutput::new(SOLVER_REWARD as u64 + fee_value, solver_p2pk.clone());

        let (tx, entries) = build_ccr_tx(
            &spk,
            input_value,
            fee_value,
            &solver_p2pk,
            vec![output0, output1],
            0,
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpTrue)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with missing secret");
    }

    #[test]
    fn test_refund_fails_with_excessive_fee() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));
        let sender_p2pk = p2pk_spk(&sender);
        let sender_spk_vec = spk_to_vec(&sender_p2pk);

        let htlc_script = create_htlc_script(
            &sender_spk_vec,
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;

        let stolen_amount = input_value - 10_000;
        let output = TransactionOutput::new(stolen_amount, sender_p2pk.clone());

        let (tx, entries) = build_refund_tx(&spk, input_value, vec![output], timelock);
        let mut mutable_tx = MutableTransaction::with_entries(tx, entries);

        let sig_script = ScriptBuilder::new()
            .add_op(OpFalse)
            .unwrap()
            .add_data(&htlc_script)
            .unwrap()
            .drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();
        let utxo_entry = tx.utxo(0).unwrap().clone();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute()
            .expect_err("Should fail when fee exceeds allowance");
    }

    #[test]
    fn test_fails_with_only_redeem_script() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret: [u8; 32] = rand::random();
        let secret_hash: [u8; 32] = Sha256::digest(&secret).into();
        let timelock = 1_700_000_000u64 + 7200;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));

        let htlc_script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        let spk = pay_to_script_hash_script(&htlc_script);
        let input_value = 1_000_000_000u64;
        let output = TransactionOutput::new(input_value, spk.clone());
        let utxo_entry = UtxoEntry::new(input_value, spk.clone(), 0, false);

        let input = TransactionInput {
            previous_outpoint: TransactionOutpoint::new(Hash::from_u64_word(1), 0),
            signature_script: vec![],
            sequence: 0,
            sig_op_count: 0,
        };

        let tx = Transaction::new(
            1,
            vec![input],
            vec![output],
            0,
            SUBNETWORK_ID_NATIVE,
            0,
            vec![],
        );
        let mut mutable_tx = MutableTransaction::with_entries(tx, vec![utxo_entry.clone()]);

        let sig_script = ScriptBuilder::new().add_data(&htlc_script).unwrap().drain();
        mutable_tx.tx.inputs[0].signature_script = sig_script;

        let tx = mutable_tx.as_verifiable();
        let sig_cache = Cache::new(10_000);
        let reused_values = SigHashReusedValuesUnsync::new();

        let mut vm = TxScriptEngine::from_transaction_input(
            &tx,
            &tx.inputs()[0],
            0,
            &utxo_entry,
            &reused_values,
            &sig_cache,
        );
        vm.execute().expect_err("Should fail with no arguments");
    }

    #[test]
    fn test_script_size() {
        let secp = Secp256k1::new();
        let mut rng = rand::rng();

        let sender = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);
        let receiver = Keypair::from_secret_key(&secp, &secp.generate_keypair(&mut rng).0);

        let secret_hash: [u8; 32] = Sha256::digest(&rand::random::<[u8; 32]>()).into();
        let timelock = 1_700_000_000u64;

        let receiver_spk_vec = spk_to_vec(&p2pk_spk(&receiver));

        let script = create_htlc_script(
            &sender.x_only_public_key().0.serialize(),
            &vec![],
            &receiver_spk_vec,
            &secret_hash,
            timelock,
            0,
            [0u8; 32],
        )
        .expect("Script creation");

        assert!(script.len() < 250, "Script should be under 250 bytes");
    }
}
