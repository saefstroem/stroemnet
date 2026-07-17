use k256::schnorr::SigningKey;
use kaspa_addresses::{Address, Prefix, Version};

use super::super::error::{KaspaError, Result};

/// Parse a string network id to some prefix
pub(super) fn parse_network_prefix(network_id: &str) -> Result<Prefix> {
    match network_id {
        s if s.starts_with("mainnet") => Ok(Prefix::Mainnet),
        s if s.starts_with("testnet") => Ok(Prefix::Testnet),
        s if s.starts_with("simnet") => Ok(Prefix::Simnet),
        s if s.starts_with("devnet") => Ok(Prefix::Devnet),
        other => Err(KaspaError::Other(format!(
            "unknown kaspa network id: {other}"
        ))),
    }
}

/// Compute a k256 signing key from private key
pub(crate) fn signing_key(private_key: &str) -> Result<SigningKey> {
    let secret_bytes = hex::decode(private_key.trim_start_matches("0x"))
        .map_err(|e| KaspaError::Other(format!("private key hex: {e}")))?;
    SigningKey::from_bytes(&secret_bytes)
        .map_err(|e| KaspaError::Other(format!("schnorr signing key: {e}")))
}

/// Retrieve pubkey bytes from a signing key
pub(crate) fn pubkey_bytes(key: &SigningKey) -> Result<[u8; 32]> {
    key.verifying_key()
        .to_bytes()
        .as_slice()
        .try_into()
        .map_err(|_| KaspaError::Other("verifying key not 32 bytes".into()))
}

/// Compute lp address from a private key converting the lp address to string
pub(crate) fn lp_address_from_private_key(network_id: &str, private_key: &str) -> Result<String> {
    let key = signing_key(private_key)?;
    let prefix = parse_network_prefix(network_id)?;
    Ok(Address::new(prefix, Version::PubKey, &pubkey_bytes(&key)?).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_prefix_known_and_unknown() {
        assert!(matches!(
            parse_network_prefix("testnet-10"),
            Ok(Prefix::Testnet)
        ));
        assert!(matches!(
            parse_network_prefix("mainnet"),
            Ok(Prefix::Mainnet)
        ));
        assert!(parse_network_prefix("bogus").is_err());
    }

    #[test]
    fn lp_address_is_derivable() {
        let pk = "0101010101010101010101010101010101010101010101010101010101010101";
        assert!(lp_address_from_private_key("testnet-10", pk).is_ok());
    }
}
