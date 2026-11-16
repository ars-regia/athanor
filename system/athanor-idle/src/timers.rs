//! The timer choice (SD4, "Timers"): which of the two `ext_idle_notification_v1` objects exist.

use std::time::Duration;

/// The three keys of `org.athanor.desktop.idle` (SD3), in seconds; 0 means never.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub blank_after: u32,
    pub suspend_after_battery: u32,
    pub suspend_after_ac: u32,
}

impl Default for Config {
    /// The defaults of SD3's table (D6, `doc_lock_and_prompts.md` LP5).
    fn default() -> Self {
        Config {
            blank_after: 300,
            suspend_after_battery: 900,
            suspend_after_ac: 0,
        }
    }
}

/// What the inhibitor table asks of the timers (SD5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// No inhibitor holds.
    None,
    /// Only the suspend timer is removed; the blank keeps running.
    SuspendOnly,
    /// Neither timer exists.
    All,
}

/// The notifications to hold: `None` creates no notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    pub blank: Option<Duration>,
    pub suspend: Option<Duration>,
}

fn seconds(s: u32) -> Option<Duration> {
    (s > 0).then(|| Duration::from_secs(u64::from(s)))
}

/// The suspend time follows UPower's `OnBattery`; a value of 0 creates no timer.
pub fn choose(config: &Config, on_battery: bool, hold: Hold) -> Timeouts {
    let suspend = if on_battery {
        config.suspend_after_battery
    } else {
        config.suspend_after_ac
    };
    match hold {
        Hold::All => Timeouts {
            blank: None,
            suspend: None,
        },
        Hold::SuspendOnly => Timeouts {
            blank: seconds(config.blank_after),
            suspend: None,
        },
        Hold::None => Timeouts {
            blank: seconds(config.blank_after),
            suspend: seconds(suspend),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Option<Duration> = |n| Some(Duration::from_secs(n));

    #[test]
    fn defaults_on_mains_and_on_battery() {
        let c = Config::default();
        assert_eq!(
            choose(&c, false, Hold::None),
            Timeouts {
                blank: S(300),
                suspend: None
            }
        );
        assert_eq!(
            choose(&c, true, Hold::None),
            Timeouts {
                blank: S(300),
                suspend: S(900)
            }
        );
    }

    #[test]
    fn zero_creates_no_timer() {
        let c = Config {
            blank_after: 0,
            suspend_after_battery: 0,
            suspend_after_ac: 60,
        };
        assert_eq!(
            choose(&c, true, Hold::None),
            Timeouts {
                blank: None,
                suspend: None
            }
        );
        assert_eq!(
            choose(&c, false, Hold::None),
            Timeouts {
                blank: None,
                suspend: S(60)
            }
        );
    }

    #[test]
    fn holds() {
        let c = Config {
            blank_after: 60,
            suspend_after_battery: 120,
            suspend_after_ac: 120,
        };
        assert_eq!(
            choose(&c, true, Hold::All),
            Timeouts {
                blank: None,
                suspend: None
            }
        );
        assert_eq!(
            choose(&c, true, Hold::SuspendOnly),
            Timeouts {
                blank: S(60),
                suspend: None
            }
        );
    }
}
