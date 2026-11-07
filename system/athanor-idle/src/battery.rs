//! What the daemon knows of UPower's `OnBattery` (SD4). It is read at start, at every change
//! of UPower's properties and whenever `org.freedesktop.UPower` gains an owner, since a
//! restarted UPower announces its state with no `PropertiesChanged`. A failed read keeps what
//! was known, mains before the first success, and is retried on a timer until a read succeeds:
//! UPower is activated on the bus, and the first read can run out of time while it starts.

use std::time::Duration;

/// The first retry after a failed read.
pub const RETRY: Duration = Duration::from_secs(30);
/// The longest wait between two retries.
pub const RETRY_MAX: Duration = Duration::from_secs(300);

/// The last `OnBattery` read, and the wait before the next retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    on_battery: bool,
    next_retry: Duration,
}

impl Default for Battery {
    fn default() -> Self {
        Battery {
            on_battery: false,
            next_retry: RETRY,
        }
    }
}

impl Battery {
    /// On mains until a read says otherwise.
    pub fn new() -> Self {
        Battery::default()
    }

    pub fn on_battery(&self) -> bool {
        self.on_battery
    }

    /// Takes the result of a read, `None` for a failure. Returns how long to wait before
    /// reading again: `None` after a success; after a failure 30 s, doubled at each further
    /// failure up to 5 min.
    pub fn answer(&mut self, read: Option<bool>) -> Option<Duration> {
        match read {
            Some(on_battery) => {
                self.on_battery = on_battery;
                self.next_retry = RETRY;
                None
            }
            None => {
                let wait = self.next_retry;
                self.next_retry = (wait * 2).min(RETRY_MAX);
                Some(wait)
            }
        }
    }
}

/// Whether a `NameOwnerChanged` of `org.freedesktop.UPower` asks for a read: when the name
/// gains an owner, not when it loses one.
pub fn gained_owner(new_owner: Option<&str>) -> bool {
    new_owner.is_some_and(|owner| !owner.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_start_read_keeps_mains_and_retries_with_a_bounded_backoff() {
        let mut battery = Battery::new();
        assert_eq!(battery.answer(None), Some(RETRY));
        assert!(!battery.on_battery(), "mains until a read succeeds");
        assert_eq!(battery.answer(None), Some(Duration::from_secs(60)));
        assert_eq!(battery.answer(None), Some(Duration::from_secs(120)));
        assert_eq!(battery.answer(None), Some(Duration::from_secs(240)));
        assert_eq!(battery.answer(None), Some(RETRY_MAX));
        assert_eq!(battery.answer(None), Some(RETRY_MAX));
        assert_eq!(
            battery.answer(Some(true)),
            None,
            "a success stops the retries"
        );
        assert!(battery.on_battery());
    }

    #[test]
    fn a_later_failure_keeps_the_value_and_starts_the_backoff_again() {
        let mut battery = Battery::new();
        assert_eq!(battery.answer(None), Some(RETRY));
        assert_eq!(battery.answer(None), Some(Duration::from_secs(60)));
        assert_eq!(battery.answer(Some(true)), None);
        assert_eq!(
            battery.answer(None),
            Some(RETRY),
            "the backoff starts again"
        );
        assert!(battery.on_battery(), "the previous value is kept");
    }

    #[test]
    fn a_new_owner_asks_for_a_read_and_a_lost_one_does_not() {
        assert!(gained_owner(Some(":1.42")));
        assert!(!gained_owner(None));
        assert!(!gained_owner(Some("")));
    }
}
