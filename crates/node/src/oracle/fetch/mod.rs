use reqwest::Client;

mod aggregate;
mod bybit;
#[cfg(not(target_arch = "wasm32"))]
mod gateio;
#[cfg(not(target_arch = "wasm32"))]
mod mexc;
mod retry;
mod robust;

#[derive(Debug, Clone, Copy)]
pub(super) struct PriceSample {
    pub price: f64,
    pub volume_usd: f64,
}

#[derive(Debug, Clone)]
pub struct PriceFeed {
    client: Client,
}

impl PriceFeed {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub fn with_default_client() -> Self {
        Self::new(Client::new())
    }
}
