//! The rate limit of NC12: an application that sends more than `LIMIT` notifications in
//! `WINDOW_MS` loses popups and sound until it slows down. Its notifications are still listed.

use std::collections::{HashMap, VecDeque};

pub const LIMIT: usize = 20;
pub const WINDOW_MS: u64 = 10_000;

#[derive(Default)]
struct Window {
    /// When the last `LIMIT` notifications arrived, admitted or not, oldest first.
    arrivals: VecDeque<u64>,
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
    /// the limit: fewer than `LIMIT` arrivals in the last `WINDOW_MS`. Every arrival counts,
    /// refused ones too, so a sender that does not slow down stays refused. One warning line
    /// per burst; the burst ends when the sender is admitted again.
    pub fn admit(&mut self, key: &str, now_ms: u64) -> bool {
        // Keys whose last arrival left the window are dropped: the map holds only the
        // senders of the last ten seconds.
        // ponytail: O(n) per call, n = the distinct senders of one window; an expiry queue
        // if a machine ever sees thousands of senders a second.
        self.keys.retain(|_, w| {
            w.arrivals
                .back()
                .is_some_and(|last| now_ms.saturating_sub(*last) < WINDOW_MS)
        });
        let window = self.keys.entry(key.to_owned()).or_default();
        let admitted = window.arrivals.len() < LIMIT
            || window
                .arrivals
                .front()
                .is_some_and(|first| now_ms.saturating_sub(*first) >= WINDOW_MS);
        window.arrivals.push_back(now_ms);
        if window.arrivals.len() > LIMIT {
            window.arrivals.pop_front();
        }
        if admitted {
            window.warned = false;
        } else if !window.warned {
            window.warned = true;
            tracing::warn!(
                sender = key,
                "more than {LIMIT} notifications in {} s: popups and sounds are withheld until it slows down",
                WINDOW_MS / 1000
            );
        }
        admitted
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
        // The refusal counted: the oldest of the last twenty arrivals is now the one at 100.
        assert!(!rate.admit("a", 10_000));
        assert!(rate.admit("a", 10_100 + 900));
    }

    #[test]
    fn a_flood_stays_refused_after_the_first_twenty() {
        let mut rate = RateLimit::new();
        // Three a second for thirty seconds.
        let admitted = (0..90).filter(|i| rate.admit("a", i * 333)).count();
        assert_eq!(admitted, 20, "the flood never slows down, so it never gets through again");
        assert!(rate.keys["a"].warned);
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
