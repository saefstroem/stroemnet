/// Return the current time in seconds since the UNIX epoch.
pub fn now_unix_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// Return the current time in milliseconds since the UNIX epoch.
pub fn now_millis() -> u64 {
    js_sys::Date::now() as u64
}

/// Sleep for the specified number of milliseconds.
pub async fn sleep_ms(ms: u64) {
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
}

/// Sleep for the specified number of seconds.
pub async fn sleep_secs(s: u64) {
    gloo_timers::future::TimeoutFuture::new((s.saturating_mul(1000)) as u32).await;
}
