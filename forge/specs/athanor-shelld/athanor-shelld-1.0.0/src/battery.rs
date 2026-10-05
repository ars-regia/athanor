//! Low-battery warnings: one per discharge, from UPower's own `WarningLevel`, which accounts
//! for the machine's capacity and discharge rate better than a fixed percentage would.

use athanor_services::battery::{Battery, Charge};

/// UPower's `WarningLevel` values that call for a warning.
const LOW: u32 = 3;
const CRITICAL: u32 = 4;
const ACTION: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    Critical,
}

/// What has been said since the battery last charged.
#[derive(Debug, Default)]
pub struct LowBattery {
    low: bool,
    critical: bool,
}

impl LowBattery {
    pub fn new() -> Self {
        Self::default()
    }

    /// The warning this reading calls for, if it has not been given in this discharge.
    pub fn observe(&mut self, battery: Option<&Battery>) -> Option<Level> {
        let battery = battery?;
        if battery.charge != Charge::Discharging {
            *self = Self::default();
            return None;
        }
        match battery.warning_level? {
            LOW if !self.low => {
                self.low = true;
                Some(Level::Low)
            }
            CRITICAL | ACTION if !self.critical => {
                self.critical = true;
                Some(Level::Critical)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(charge: Charge, warning_level: Option<u32>) -> Battery {
        Battery {
            percent: 10.0,
            charge,
            seconds: None,
            warning_level,
        }
    }

    fn discharging(level: u32) -> Battery {
        battery(Charge::Discharging, Some(level))
    }

    #[test]
    fn each_level_is_reported_once() {
        let mut low = LowBattery::new();
        let seen: Vec<_> = [2, 3, 3, 4, 4]
            .into_iter()
            .map(|level| low.observe(Some(&discharging(level))))
            .collect();
        assert_eq!(
            seen,
            [None, Some(Level::Low), None, Some(Level::Critical), None]
        );
    }

    #[test]
    fn action_level_is_critical_and_does_not_repeat_it() {
        let mut low = LowBattery::new();
        assert_eq!(low.observe(Some(&discharging(5))), Some(Level::Critical));
        assert_eq!(low.observe(Some(&discharging(4))), None);
    }

    #[test]
    fn charging_rearms_both_levels() {
        let mut low = LowBattery::new();
        assert_eq!(low.observe(Some(&discharging(4))), Some(Level::Critical));
        assert_eq!(low.observe(Some(&battery(Charge::Charging, Some(1)))), None);
        assert_eq!(low.observe(Some(&discharging(3))), Some(Level::Low));
        assert_eq!(low.observe(Some(&discharging(4))), Some(Level::Critical));
    }

    #[test]
    fn no_battery_and_no_level_say_nothing() {
        let mut low = LowBattery::new();
        assert_eq!(low.observe(None), None);
        assert_eq!(low.observe(Some(&battery(Charge::Discharging, None))), None);
    }
}
