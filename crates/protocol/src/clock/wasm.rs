pub fn now_unix_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

pub fn now_millis() -> u64 {
    js_sys::Date::now() as u64
}

pub async fn sleep_ms(ms: u64) {
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
}

pub async fn sleep_secs(s: u64) {
    gloo_timers::future::TimeoutFuture::new((s.saturating_mul(1000)) as u32).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_secs_is_positive() {
        assert!(now_unix_secs() > 0);
    }
}
