use reqwest::Client;

mod aggregate;
mod bybit;
mod gateio;
mod mexc;

#[derive(Debug, Clone, Copy)]
pub(super) struct PriceSample {
    pub price: f64,
    pub volume_usd: f64,
}

#[derive(Debug, Clone)]
/// The main price feed struct,
/// which holds a shared HTTP client and provides methods
/// to fetch price data from multiple sources and aggregate it.
pub struct PriceFeed {
    client: Client,
}

impl PriceFeed {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Builds a PriceFeed with a default HTTP client. On wasm this uses the
    /// browser fetch backend; on native a plain client (the Oracle loop builds
    /// its own client with a timeout).
    pub fn with_default_client() -> Self {
        Self::new(Client::new())
    }
}
