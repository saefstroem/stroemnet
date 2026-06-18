use stroemnet_protocol::ChannelId;
use stroemnet_protocol::v1::CommitmentV1;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use crate::StroemGateway;

#[wasm_bindgen]
impl StroemGateway {
    #[wasm_bindgen(js_name = requestQuote)]
    /// Requests a quote for a potential swap with the given parameters.
    /// The quote details will be returned asynchronously via the `quote` event listener.
    /// Therefore you should first add a listener for the `quote` event, and then call this function to request a quote.
    pub async fn request_quote(
        &self,
        swap_id: Vec<u8>,
        origin: u8,
        destination: u8,
        amount: String,
    ) -> Result<(), JsError> {
        let node = self.require_node()?;
        let swap_id = to_32(&swap_id, "swap_id")?;
        let origin = ChannelId::try_from(origin)
            .map_err(|e| JsError::new(&format!("origin chain id: {e}")))?;
        let destination = ChannelId::try_from(destination)
            .map_err(|e| JsError::new(&format!("destination chain id: {e}")))?;
        node.request_quote(swap_id, origin, destination, amount)
            .await
            .map_err(|e| JsError::new(&format!("request_quote: {e}")))
    }

    #[wasm_bindgen(js_name = registerCommitment)]
    /// Registers a commitment and its matching secret with the local node so the node
    /// can complete the swap once the on-chain deposit is made, and returns the
    /// source-chain deposit target.
    ///
    /// This does NOT submit anything on-chain: the commitment is built from a `quote`
    /// event, registered here together with the secret that matches its secret hash,
    /// and the actual deposit is performed separately by the caller (an EVM HTLC
    /// transaction, or a transfer to the returned Kaspa P2SH address).
    pub async fn register_commitment(
        &self,
        commitment: JsValue,
        secret: Vec<u8>,
        expected_amount_out: String,
    ) -> Result<String, JsError> {
        let node = self.require_node()?;
        let commitment: CommitmentV1 = serde_wasm_bindgen::from_value(commitment)
            .map_err(|e| JsError::new(&format!("commitment: {e}")))?;
        let secret = to_32(&secret, "secret")?;
        node.register_commitment(commitment, secret, expected_amount_out)
            .await
            .map_err(|e| JsError::new(&format!("register_commitment: {e}")))
    }
}

fn to_32(bytes: &[u8], field: &str) -> Result<[u8; 32], JsError> {
    if bytes.len() != 32 {
        return Err(JsError::new(&format!(
            "{field}: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    Ok(out)
}
