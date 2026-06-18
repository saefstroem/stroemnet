use kaspa_txscript::{
    opcodes::codes::{
        OpCheckLockTimeVerify, OpElse, OpEndIf, OpEqualVerify, OpFalse, OpGreaterThanOrEqual, OpIf,
        OpNumEqualVerify, OpSHA256, OpSub, OpTxInputAmount, OpTxInputCount, OpTxInputIndex,
        OpTxOutputAmount, OpTxOutputCount, OpTxOutputSpk,
    },
    script_builder::{ScriptBuilder, ScriptBuilderResult},
};

/// The reward amount for the solver in the HTLC script.
pub(crate) const SOLVER_REWARD: i64 = 10_000_000;

/// Creates an HTLC script for a swap with the given parameters. The script will allow the receiver to claim the funds
/// if they can provide the correct secret before the timelock expires, or allow the sender
/// to refund the funds after the timelock expires. The script also includes a branch for solvers
/// to claim a reward for helping to execute the swap, which requires providing the swap ID and sender's receiver address.
pub(crate) fn create_htlc_script(
    sender_spk: &[u8],
    sender_receiver_address: &[u8],
    receiver_spk: &[u8],
    secret_hash: &[u8],
    timelock: u64,
    destination: u8,
    swap_id: [u8; 32],
) -> ScriptBuilderResult<Vec<u8>> {
    let mut builder = ScriptBuilder::new();
    tracing::info!("Creating HTLC script with parameters:");
    tracing::info!("  sender_spk: {:02x?}", sender_spk);
    tracing::info!(
        "  sender_receiver_address: {:02x?}",
        sender_receiver_address
    );
    tracing::info!("  receiver_spk: {:02x?}", receiver_spk);
    tracing::info!("  secret_hash: {:02x?}", secret_hash);
    tracing::info!("  timelock: {}", timelock);
    tracing::info!("  destination: {}", destination);
    tracing::info!("  swap_id: {:02x?}", swap_id);
    builder
        .add_op(OpIf)?
        .add_op(OpSHA256)?
        .add_data(secret_hash)?
        .add_op(OpEqualVerify)?
        .add_op(OpTxInputCount)?
        .add_i64(2)?
        .add_op(OpNumEqualVerify)?
        .add_op(OpTxOutputCount)?
        .add_i64(2)?
        .add_op(OpNumEqualVerify)?
        .add_data(receiver_spk)?
        .add_i64(0)?
        .add_op(OpTxOutputSpk)?
        .add_op(OpEqualVerify)?
        .add_i64(0)?
        .add_op(OpTxOutputAmount)?
        .add_op(OpTxInputIndex)?
        .add_op(OpTxInputAmount)?
        .add_i64(SOLVER_REWARD)?
        .add_op(OpSub)?
        .add_op(OpGreaterThanOrEqual)?
        .add_op(OpElse)?
        .add_i64(timelock as i64)?
        .add_op(OpCheckLockTimeVerify)?
        .add_op(OpTxInputCount)?
        .add_i64(2)?
        .add_op(OpNumEqualVerify)?
        .add_op(OpTxOutputCount)?
        .add_i64(2)?
        .add_op(OpNumEqualVerify)?
        .add_data(sender_spk)?
        .add_i64(0)?
        .add_op(OpTxOutputSpk)?
        .add_op(OpEqualVerify)?
        .add_i64(0)?
        .add_op(OpTxOutputAmount)?
        .add_op(OpTxInputIndex)?
        .add_op(OpTxInputAmount)?
        .add_i64(SOLVER_REWARD)?
        .add_op(OpSub)?
        .add_op(OpGreaterThanOrEqual)?
        .add_op(OpEndIf)?
        .add_op(OpFalse)?
        .add_op(OpIf)?
        .add_data(swap_id.as_slice())?
        .add_data(sender_receiver_address)?
        .add_data(&[destination])?
        .add_op(OpEndIf)?;

    Ok(builder.drain())
}

pub(crate) fn decode_u64_from_script(bytes: &[u8]) -> u64 {
    let mut padded = [0u8; 8];
    let copy_len = bytes.len().min(8);
    padded[..copy_len].copy_from_slice(&bytes[..copy_len]);
    u64::from_le_bytes(padded)
}
