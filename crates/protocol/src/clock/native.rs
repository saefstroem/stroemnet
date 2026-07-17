use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

pub async fn sleep_secs(s: u64) {
    tokio::time::sleep(Duration::from_secs(s)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_returns_recent_epoch_times() {
        assert!(now_unix_secs() > 1_600_000_000);
        assert!(now_millis() > 1_600_000_000_000);
    }
}
