use serde::Serialize;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = defaultObserverChannels)]
pub fn default_observer_channels() -> Result<JsValue, JsError> {
    default_observer_channels_json()
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&format!("serialize defaults: {e}")))
}

pub fn default_observer_channels_json() -> serde_json::Value {
    serde_json::json!({
        "ethereum-sepolia": {
            "rpc_url": "https://eth-sepolia.api.onfinality.io/public",
            "htlc_address": "0xb249849CAc76bcd86A566016AcA5e7FC63dA5122",
            "minimum_block_confirmations": 1u64,
        },
        "igra-galleon": {
            "rpc_url": "https://galleon-testnet.igralabs.com:8545",
            "htlc_address": "0x5C1f98eE073186BF1684b06b3CFE863a8bB569b4",
            "minimum_block_confirmations": 1u64,
        },
        "kaspa-tn10": {
            "wrpc_url": "wss://tn10.stroem.finance",
            "network_id": "testnet-10",
            "minimum_block_confirmations": 30,
        },
    })
}
