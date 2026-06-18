use std::fmt::Display;

use borsh::{BorshDeserialize, BorshSerialize};

#[repr(u8)]
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Copy,
    BorshDeserialize,
    BorshSerialize,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
/// An enum containing all the different channels that are currently supported in Stroemnet
/// No differentiation is made between mainnet/testnet channels.
pub enum ChannelId {
    KaspaTn10,
    EthereumSepolia,
    IgraGalleon,
}

impl ChannelId {
    /// Returns the number of decimals used by the channel's native token.
    pub fn decimals(&self) -> u8 {
        match self {
            ChannelId::KaspaTn10 => 8,
            ChannelId::EthereumSepolia | ChannelId::IgraGalleon => 18,
        }
    }

    /// Returns the estimated finality time in seconds for the channel.
    pub fn finality_secs(&self) -> u64 {
        match self {
            ChannelId::KaspaTn10 => 180,
            ChannelId::EthereumSepolia => 15 * 60,
            ChannelId::IgraGalleon => 3600,
        }
    }

    pub fn uses_synthetic_clock(&self) -> bool {
        matches!(self, ChannelId::IgraGalleon)
    }

    /// Returns the ticker symbol for the channel's native token.
    pub fn ticker_symbol(&self) -> &'static str {
        match self {
            ChannelId::KaspaTn10 => "KAS",
            ChannelId::EthereumSepolia => "ETH",
            ChannelId::IgraGalleon => "iKAS",
        }
    }

    /// Returns true if the channel is based on the Ethereum Virtual Machine (EVM).
    pub fn is_evm(self) -> bool {
        match self {
            ChannelId::EthereumSepolia | ChannelId::IgraGalleon => true,
            ChannelId::KaspaTn10 => false,
        }
    }

    /// Returns true if the channel is based on the UTXO model.
    pub fn is_utxo(self) -> bool {
        match self {
            ChannelId::KaspaTn10 => true,
            ChannelId::EthereumSepolia | ChannelId::IgraGalleon => false,
        }
    }

    /// Returns a URL to a block explorer for the given transaction hash on this channel.
    pub fn explorer_url(self, tx: &str) -> String {
        match self {
            ChannelId::EthereumSepolia => format!("https://sepolia.etherscan.io/tx/{tx}"),
            ChannelId::KaspaTn10 => format!("https://explorer-tn10.kaspa.org/txs/{tx}"),
            ChannelId::IgraGalleon => {
                format!("https://explorer.galleon-testnet.igralabs.com/tx/{tx}")
            }
        }
    }
}

impl Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelId::KaspaTn10 => write!(f, "Kaspa TN10"),
            ChannelId::EthereumSepolia => write!(f, "Sepolia"),
            ChannelId::IgraGalleon => write!(f, "Igra Galleon"),
        }
    }
}

impl TryFrom<u8> for ChannelId {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ChannelId::KaspaTn10),
            1 => Ok(ChannelId::EthereumSepolia),
            2 => Ok(ChannelId::IgraGalleon),
            _ => Err("Invalid ChannelId".into()),
        }
    }
}

impl TryFrom<&str> for ChannelId {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "kaspa" | "kaspa-tn10" | "kaspa_tn10" | "kaspatn10" => Ok(ChannelId::KaspaTn10),
            "ethereum" | "sepolia" | "ethereum-sepolia" | "ethereum_sepolia"
            | "ethereumsepolia" => Ok(ChannelId::EthereumSepolia),
            "igra" | "igra-galleon" | "igra_galleon" | "igragalleon" => Ok(ChannelId::IgraGalleon),
            _ => Err(format!("Invalid ChannelId string: {}", value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_id_decimals() {
        assert_eq!(ChannelId::KaspaTn10.decimals(), 8);
        assert_eq!(ChannelId::EthereumSepolia.decimals(), 18);
    }

    #[test]
    fn test_channel_id_to_string() {
        assert_eq!(ChannelId::KaspaTn10.to_string(), "Kaspa TN10");
        assert_eq!(ChannelId::EthereumSepolia.to_string(), "Sepolia");
        assert_eq!(ChannelId::IgraGalleon.to_string(), "Igra Galleon");
    }

    #[test]
    fn test_channel_id_try_from_valid() {
        assert_eq!(ChannelId::try_from(0u8).unwrap(), ChannelId::KaspaTn10);
        assert_eq!(
            ChannelId::try_from(1u8).unwrap(),
            ChannelId::EthereumSepolia
        );
    }

    #[test]
    fn test_channel_id_try_from_invalid() {
        assert!(ChannelId::try_from(3u8).is_err());
        assert!(ChannelId::try_from(255u8).is_err());
    }

    #[test]
    fn test_channel_id_ticker_symbol() {
        assert_eq!(ChannelId::KaspaTn10.ticker_symbol(), "KAS");
        assert_eq!(ChannelId::EthereumSepolia.ticker_symbol(), "ETH");
    }

    #[test]
    fn test_channel_id_borsh_roundtrip() {
        for id in [ChannelId::KaspaTn10, ChannelId::EthereumSepolia] {
            let bytes = borsh::to_vec(&id).unwrap();
            let decoded = ChannelId::try_from_slice(&bytes).unwrap();
            assert_eq!(id, decoded);
        }
    }

    #[test]
    fn test_channel_id_finality_secs() {
        assert_eq!(ChannelId::KaspaTn10.finality_secs(), 180);
        assert_eq!(ChannelId::EthereumSepolia.finality_secs(), 900);
    }

    #[test]
    fn test_is_evm_partition() {
        assert!(ChannelId::EthereumSepolia.is_evm());
        assert!(!ChannelId::KaspaTn10.is_evm());
    }

    #[test]
    fn test_is_utxo_partition() {
        assert!(ChannelId::KaspaTn10.is_utxo());
        assert!(!ChannelId::EthereumSepolia.is_utxo());
    }

    #[test]
    fn test_explorer_url_per_chain() {
        let eth = ChannelId::EthereumSepolia.explorer_url("0xabc");
        assert!(eth.contains("etherscan"));
        assert!(eth.contains("0xabc"));

        let kas = ChannelId::KaspaTn10.explorer_url("deadbeef");
        assert!(kas.contains("kaspa"));
        assert!(kas.contains("deadbeef"));
    }
}
