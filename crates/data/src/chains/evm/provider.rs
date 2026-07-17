use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;

use crate::chains::net::{NETWORK_TIMEOUT, timed};
use crate::{DataError, Result};

/// Build the provider for the EVM network, this consists of a read provider
/// and an optional signed provider if the user provided the private key.
/// For wasm for example, we dont provide the private key as wasm instances cannot serve as LPs
pub(super) async fn build_providers(
    rpc_url: &str,
    private_key: Option<&str>,
) -> Result<(DynProvider, Option<DynProvider>)> {
    // Create the read provider
    let read_provider = ProviderBuilder::new()
        .connect(rpc_url)
        .await
        .map_err(|e| DataError::Connect(format!("evm provider: {e}")))?
        .erased();

    // Create a signed provider if we have a stored private key
    let signed_provider = match private_key {
        Some(pk) => {
            let signer: PrivateKeySigner = pk
                .parse()
                .map_err(|e| DataError::Config(format!("private_key: {e}")))?;
            Some(
                ProviderBuilder::new()
                    .wallet(signer)
                    .connect(rpc_url)
                    .await
                    .map_err(|e| DataError::Connect(format!("evm signed provider: {e}")))?
                    .erased(),
            )
        }
        None => None,
    };

    Ok((read_provider, signed_provider))
}

/// Get a timed out latest block timestamp to use, straight from some provider whilst handling errors (timeouts)
pub(crate) async fn current_block_timestamp<P: Provider>(provider: &P) -> Option<u64> {
    let fetch = provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest);
    match timed(NETWORK_TIMEOUT, fetch).await {
        Some(Ok(Some(block))) => Some(block.header.timestamp),
        _ => None,
    }
}
