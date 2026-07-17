use serde::Serialize;
use stroemnet_node::oracle::PriceFeed;
use stroemnet_protocol::ChannelId;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = getPrices)]
pub async fn get_prices() -> Result<JsValue, JsError> {
    let feed = PriceFeed::with_default_client();
    let channels = vec![
        ChannelId::KaspaTn10,
        ChannelId::EthereumSepolia,
        ChannelId::IgraGalleon,
    ];
    let prices = feed
        .aggregate(&channels)
        .await
        .map_err(|e| JsError::new(&format!("price feed: {e}")))?;

    let mut map = serde_json::Map::new();
    for (channel, price) in prices {
        map.insert((channel as u8).to_string(), serde_json::json!(price));
    }
    serde_json::Value::Object(map)
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&format!("serialize prices: {e}")))
}
