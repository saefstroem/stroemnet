#[cfg(not(target_arch = "wasm32"))]
mod accept;
mod dial;
mod read;
mod resolve;

pub(crate) use dial::spawn_bootstrap_with_counter;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use accept::spawn_accept;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use dial::spawn_addr_dial_driver;
