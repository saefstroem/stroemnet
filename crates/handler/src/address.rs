use std::str::FromStr;

use alloy::primitives::Address;
use stroemnet_protocol::ChannelId;

/// Compute the equivalence of two addresses dependin on the channel id
pub fn normalised_address_eq(channel: ChannelId, a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a == b {
        return true;
    }
    match channel {
        ChannelId::EthereumSepolia | ChannelId::IgraGalleon => {
            match (Address::from_str(a), Address::from_str(b)) {
                (Ok(la), Ok(lb)) => la == lb,
                _ => false,
            }
        }
        ChannelId::KaspaTn10 => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_addresses_compare_case_insensitively() {
        let lower = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd";
        let upper = "0xABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD";
        assert!(normalised_address_eq(
            ChannelId::EthereumSepolia,
            lower,
            upper
        ));
        assert!(!normalised_address_eq(
            ChannelId::EthereumSepolia,
            lower,
            "0x0000000000000000000000000000000000000001"
        ));
    }

    #[test]
    fn kaspa_addresses_compare_exactly_after_trim() {
        assert!(normalised_address_eq(
            ChannelId::KaspaTn10,
            "kaspa:abc",
            " kaspa:abc "
        ));
        assert!(!normalised_address_eq(
            ChannelId::KaspaTn10,
            "kaspa:abc",
            "kaspa:xyz"
        ));
    }
}
