#![cfg(target_arch = "wasm32")]
#![allow(clippy::arc_with_non_send_sync)]

mod address_validation;
mod defaults;
mod gateway;
mod prices;

pub use address_validation::{validate_eth_address, validate_kas_address};
pub use defaults::{
    default_bootstrap_peers, default_gateway_config, default_observer_channels_json,
};
pub use gateway::StroemGateway;
pub use prices::get_prices;
