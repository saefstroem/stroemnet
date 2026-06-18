use serde::Serialize;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

use super::{default_bootstrap_peers, default_observer_channels_json};

#[wasm_bindgen(js_name = getDefaultConfig)]
pub fn default_gateway_config() -> Result<JsValue, JsError> {
    let value = serde_json::json!({
        "observerChannels": default_observer_channels_json(),
        "bootstrapPeers": default_bootstrap_peers(),
        "handler": {
            "minTradeUsd": 1.0,
            "maxTradeUsd": 100000.0,
            "spreadPercent": 0.5,
            "commitBufferSecs": 960,
        },
    });
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&format!("serialize default config: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StroemGateway;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn default_config_constructs_a_gateway() {
        let cfg =
            default_gateway_config().unwrap_or_else(|_| panic!("default config must serialize"));
        assert!(
            StroemGateway::new(cfg).is_ok(),
            "getDefaultConfig() must produce a config the StroemGateway constructor accepts",
        );
    }
}
