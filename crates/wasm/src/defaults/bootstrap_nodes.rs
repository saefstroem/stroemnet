use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = defaultBootstrapPeers)]
pub fn default_bootstrap_peers() -> Vec<String> {
    stroemnet_p2p::SEED_NODES
        .iter()
        .map(|u| (*u).to_string())
        .collect()
}
