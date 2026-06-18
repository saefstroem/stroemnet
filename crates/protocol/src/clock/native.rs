use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Returns the current time in seconds since the UNIX epoch.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Returns the current time in milliseconds since the UNIX epoch.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Sleeps for the specified number of milliseconds.
pub async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// Sleeps for the specified number of seconds.
pub async fn sleep_secs(s: u64) {
    tokio::time::sleep(Duration::from_secs(s)).await;
}
