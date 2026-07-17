use alloy_primitives::Address;
use std::str::FromStr;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = validateEthAddress)]
pub fn validate_eth_address(input: &str) -> Result<(), JsError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(JsError::new("address is empty"));
    }
    let lower = trimmed.starts_with("0x") && trimmed[2..].chars().all(|c| !c.is_ascii_uppercase());
    if lower {
        Address::from_str(trimmed)
            .map_err(|e| JsError::new(&format!("malformed EVM address: {e}")))?;
    } else {
        Address::parse_checksummed(trimmed, None).map_err(|e| {
            JsError::new(&format!(
                "EVM address has invalid EIP-55 checksum (likely typo): {e}"
            ))
        })?;
    }
    Ok(())
}

#[wasm_bindgen(js_name = validateKasAddress)]
pub fn validate_kas_address(input: &str, network_id: &str) -> Result<(), JsError> {
    use kaspa_addresses::{Address as KasAddress, Prefix};
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(JsError::new("address is empty"));
    }
    let addr = KasAddress::try_from(trimmed)
        .map_err(|e| JsError::new(&format!("malformed Kaspa address: {e}")))?;
    let expected_prefix = match network_id {
        s if s.starts_with("mainnet") => Prefix::Mainnet,
        s if s.starts_with("testnet") => Prefix::Testnet,
        s if s.starts_with("simnet") => Prefix::Simnet,
        s if s.starts_with("devnet") => Prefix::Devnet,
        other => return Err(JsError::new(&format!("unknown network id: {other}"))),
    };
    if addr.prefix != expected_prefix {
        return Err(JsError::new(&format!(
            "address is on wrong network — expected {expected_prefix:?}, got {:?}",
            addr.prefix
        )));
    }
    Ok(())
}
