#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::tx::ScriptPublicKey;
use kaspa_txscript::pay_to_address_script;
use secp256k1::Keypair;

pub(crate) fn p2pk_spk(kp: &Keypair) -> ScriptPublicKey {
    pay_to_address_script(&Address::new(
        Prefix::Mainnet,
        Version::PubKey,
        kp.x_only_public_key().0.serialize().as_slice(),
    ))
}

pub(crate) fn vec_to_spk(v: &[u8]) -> ScriptPublicKey {
    let version = u16::from_be_bytes([v[0], v[1]]);
    let script = &v[2..];
    ScriptPublicKey::from_vec(version, script.to_vec())
}
