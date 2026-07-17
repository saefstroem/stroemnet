pub fn normalize_listen_addr(s: &str) -> String {
    s.trim_end_matches('/').to_ascii_lowercase()
}

pub fn listen_addrs_equal(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize_listen_addr("WS://X:3000/"), "ws://x:3000");
        assert_eq!(normalize_listen_addr("ws://x:3000"), "ws://x:3000");
    }
}
