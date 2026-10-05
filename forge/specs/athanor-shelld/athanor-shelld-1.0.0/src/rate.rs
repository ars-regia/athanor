//! The rate limit of NC12: an application that sends more than `LIMIT` notifications in
//! `WINDOW_MS` loses popups and sound until it slows down. Its notifications are still listed.

use std::collections::{HashMap, VecDeque};

pub const LIMIT: usize = 20;
pub const WINDOW_MS: u64 = 10_000;

#[derive(Default)]
struct Window {
    /// When the admitted notifications arrived, oldest first.
    admitted: VecDeque<u64>,
    /// The journal has already said this burst was cut.
    warned: bool,
}

#[derive(Default)]
pub struct RateLimit {
    keys: HashMap<String, Window>,
}

impl RateLimit {
    #[must_use]
    pub fn new() -> RateLimit {
        RateLimit::default()
    }

    /// Whether the notification of `key` arriving at `now_ms` (a monotonic clock) is within
    /// the limit. A refused one is not counted, so the window slides at the pace of the
    /// admitted ones. One warning line per burst.
    pub fn admit(&mut self, key: &str, now_ms: u64) -> bool {
        // Keys whose last admitted notification left the window are dropped: the map holds
        // only the senders of the last ten seconds.
        self.keys.retain(|_, w| {
            w.admitted
                .back()
                .is_some_and(|last| now_ms.saturating_sub(*last) < WINDOW_MS)
        });
        let window = self.keys.entry(key.to_owned()).or_default();
        while window
            .admitted
            .front()
            .is_some_and(|first| now_ms.saturating_sub(*first) >= WINDOW_MS)
        {
            window.admitted.pop_front();
        }
        if window.admitted.len() >= LIMIT {
            if !window.warned {
                window.warned = true;
                tracing::warn!(
                    sender = key,
                    "more than {LIMIT} notifications in 10 s: popups and sounds are withheld until it slows down"
                );
            }
            return false;
        }
        window.warned = false;
        window.admitted.push_back(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_twenty_and_refuses_the_next() {
        let mut rate = RateLimit::new();
        assert!((0..20).all(|i| rate.admit("a", i * 100)));
        assert!(!rate.admit("a", 2_000));
        assert!(!rate.admit("a", 9_999));
    }

    #[test]
    fn admits_again_once_the_window_slides() {
        let mut rate = RateLimit::new();
        for i in 0..20 {
            assert!(rate.admit("a", i * 100));
        }
        assert!(!rate.admit("a", 9_000));
        // The first one (at 0) has left the window; the second (at 100) has not.
        assert!(rate.admit("a", 10_000));
        assert!(!rate.admit("a", 10_050));
        assert!(rate.admit("a", 10_100));
    }

    #[test]
    fn keys_are_independent() {
        let mut rate = RateLimit::new();
        for _ in 0..20 {
            assert!(rate.admit("a", 0));
        }
        assert!(!rate.admit("a", 1));
        assert!(rate.admit("b", 1));
    }

    #[test]
    fn forgets_keys_whose_window_has_passed() {
        let mut rate = RateLimit::new();
        for i in 0..1000 {
            assert!(rate.admit(&format!(":1.{i}"), 0));
        }
        assert!(rate.admit("late", WINDOW_MS));
        assert_eq!(rate.keys.len(), 1);
    }
}
