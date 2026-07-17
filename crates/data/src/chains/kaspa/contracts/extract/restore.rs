use ahash::AHashMap;
use kaspa_addresses::Prefix;
use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;

use super::extract_commitment;
use crate::UtxoScript;
use crate::chains::kaspa::decode::parse_script;
use crate::chains::kaspa::error::Result;

/// Convert a hashmap of scripts to a hashmap of commitments
pub(crate) fn commitments_from_scripts(
    scripts: &AHashMap<[u8; 32], UtxoScript>,
    prefix: Prefix,
    source_channel_id: ChannelId,
) -> AHashMap<[u8; 32], CommitmentV1> {
    let mut out = AHashMap::new();
    for (swap_id, script) in scripts {
        // Reconstruct the commitment from each script and insert it into the map
        match reconstruct(script, prefix, source_channel_id) {
            Ok(c) => {
                out.insert(*swap_id, c);
            }
            Err(e) => tracing::error!(
                target: "settlement",
                "reconstruct commitment {} failed: {e} — not settleable until investigated",
                hex::encode(swap_id)
            ),
        }
    }
    out
}

/// Convert a script into a commitment
fn reconstruct(
    script: &UtxoScript,
    prefix: Prefix,
    source_channel_id: ChannelId,
) -> Result<CommitmentV1> {
    // Parse the script into individual opcodes
    let ops = parse_script(&script.redeem_script).collect::<std::result::Result<Vec<_>, _>>()?;
    // Extract the commitment from the vector of opcodes
    extract_commitment(
        &ops,
        script.deposit_target.clone(),
        prefix,
        source_channel_id,
    )
}
