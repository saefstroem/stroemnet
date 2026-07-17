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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn spawn_runs_the_future() {
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        super::spawn(async move {
            f.store(true, Ordering::SeqCst);
        });
        let mut ran = false;
        for _ in 0..200 {
            if flag.load(Ordering::SeqCst) {
                ran = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(ran, "spawned future did not run");
    }
}
