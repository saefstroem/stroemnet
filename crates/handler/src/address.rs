use stroemnet_protocol::ChannelId;

/// Compare two addresses for equality, normalizing them according to the chain's rules.
pub fn normalised_address_eq(chain: ChannelId, a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a == b {
        return true;
    }
    match chain {
        ChannelId::EthereumSepolia | ChannelId::IgraGalleon => {
            use std::str::FromStr;
            match (
                alloy::primitives::Address::from_str(a),
                alloy::primitives::Address::from_str(b),
            ) {
                (Ok(la), Ok(lb)) => la == lb,
                _ => false,
            }
        }
        ChannelId::KaspaTn10 => a == b,
    }
}
