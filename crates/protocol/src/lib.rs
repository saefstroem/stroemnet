pub mod channels;
pub mod clock;
pub mod swap_tracker;
pub mod v1;

pub use channels::ChannelId;
pub use clock::{ChainClock, now_millis, now_unix_secs, sleep_ms, sleep_secs};
pub use swap_tracker::{SwapRecord, SwapStage, SwapTracker, SwapTrackerError};

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(fut);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(fut);
}
