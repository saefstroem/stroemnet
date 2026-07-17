#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::WsTransport;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::ws_config;
#[cfg(target_arch = "wasm32")]
pub use web::WsTransport;

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-helpers")))]
pub use native::loopback_pair;
