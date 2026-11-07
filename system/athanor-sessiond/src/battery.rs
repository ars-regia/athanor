//! The battery notices (`doc_session_daemons.md` SD14, F-notif-43): which notice the display
//! device's `WarningLevel` asks for, when one is raised and when it closes, and its text.
//! The thresholds are UPower's, not ours; peripheral batteries are excluded (SD21).

use athanor_i18n::Catalog;
use athanor_services::battery::{self, Battery, Charge};
use athanor_services::props::{self, Props};

/// The page `os.athanor.ControlCenter1.Show` opens from a notice's action
/// (`doc_control_center.md` CC5).
pub const SETTINGS_PAGE: &str = "battery";
/// The action key of "Battery settings" (`doc_notification_center.md` NC13).
pub const SETTINGS_ACTION: &str = "battery-settings";

/// UPower's `WarningLevel` values that raise a notice.
const LOW: u32 = 3;
const CRITICAL: u32 = 4;
const ACTION: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    Critical,
    /// UPower is about to take its `CriticalPowerAction`.
    Action,
}

/// The notice the display device asks for: none for a machine without a battery, while it
/// charges or is full, or above UPower's low level.
pub fn wanted(device: &Props) -> Option<Level> {
    let battery = battery::battery(device)?;
    if matches!(battery.charge, Charge::Charging | Charge::Full) {
        return None;
    }
    match props::get_u32(device, "WarningLevel")? {
        LOW => Some(Level::Low),
        CRITICAL => Some(Level::Critical),
        ACTION => Some(Level::Action),
        _ => None,
    }
}

/// What the bus side does after a change of level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The notice shown until now, to close.
    pub close: Option<Shown>,
    /// The notice to raise.
    pub raise: Option<Level>,
}

/// A notice shown: the unique name of the server that raised it, and its id there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub server: String,
    pub id: u32,
}

/// The notice shown, and the level it was raised for. A level raises its notice once, when
/// it is entered: a notice the person dismissed stays dismissed while the level holds.
#[derive(Debug, Default)]
pub struct Notices {
    level: Option<Level>,
    shown: Option<Shown>,
}

impl Notices {
    /// The step for the level now wanted; `None` when the level did not change.
    pub fn update(&mut self, level: Option<Level>) -> Option<Step> {
        if level == self.level {
            return None;
        }
        self.level = level;
        Some(Step {
            close: self.shown.take(),
            raise: level,
        })
    }

    /// `server`, the unique name asked, answered the raise for the current level with `id`.
    pub fn raised(&mut self, server: String, id: u32) {
        self.shown = Some(Shown { server, id });
    }

    /// The raise was not answered: the level raises its notice again at the next change of
    /// the device, as when it is entered.
    pub fn failed(&mut self) {
        self.level = None;
    }

    /// `sender` closed its notice `id`, or the person dismissed it.
    pub fn closed(&mut self, sender: &str, id: u32) {
        if self.is_shown(sender, id) {
            self.shown = None;
        }
    }

    /// Whether `id` from `sender` is the notice shown now, so its action is the person's.
    pub fn is_shown(&self, sender: &str, id: u32) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.server == sender && shown.id == id)
    }
}

/// One notification, as `org.freedesktop.Notifications.Notify` takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub icon: &'static str,
    pub summary: String,
    pub body: String,
    /// 1 normal, 2 critical.
    pub urgency: u8,
    pub resident: bool,
    /// Key and label pairs.
    pub actions: Vec<(&'static str, String)>,
}

/// The notice for `level`, with the charge and the time left of `battery` when known.
pub fn notice(level: Level, battery: Option<&Battery>, catalog: &Catalog) -> Notice {
    let settings = vec![(SETTINGS_ACTION, catalog.tr("Battery settings").to_owned())];
    match level {
        Level::Low => Notice {
            icon: "battery-caution-symbolic",
            summary: catalog.tr("Battery low").to_owned(),
            body: battery
                .map(|battery| remaining(battery, catalog))
                .unwrap_or_default(),
            urgency: 1,
            resident: false,
            actions: settings,
        },
        Level::Critical => Notice {
            icon: "battery-empty-symbolic",
            summary: catalog.tr("Battery critically low").to_owned(),
            body: catalog.tr("Connect the charger").to_owned(),
            urgency: 2,
            resident: true,
            actions: settings,
        },
        Level::Action => Notice {
            icon: "battery-empty-symbolic",
            summary: catalog
                .tr("The computer will act soon to save its state")
                .to_owned(),
            body: String::new(),
            urgency: 2,
            resident: false,
            actions: Vec::new(),
        },
    }
}

/// "<n> % remaining, about <time> left", or without the time while UPower does not know it.
fn remaining(battery: &Battery, catalog: &Catalog) -> String {
    // At most 100 after `battery::battery`, so the cast cannot truncate.
    let percent = (battery.percent.round() as u32).to_string();
    match battery.seconds {
        Some(seconds) => {
            let (hours, minutes) = battery::hours_minutes(seconds);
            catalog
                .tr("{percent} % remaining, about {hours} h {minutes} min left")
                .replace("{percent}", &percent)
                .replace("{hours}", &hours.to_string())
                .replace("{minutes}", &minutes.to_string())
        }
        None => catalog
            .tr("{percent} % remaining")
            .replace("{percent}", &percent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{OwnedValue, Value};

    fn device(state: u32, warning: u32) -> Props {
        let mut props = Props::new();
        let mut put = |name: &str, value: Value<'_>| {
            props.insert(name.into(), value.try_to_owned().expect("owned"));
        };
        put("Type", Value::U32(2));
        put("IsPresent", Value::Bool(true));
        put("Percentage", Value::F64(9.4));
        put("State", Value::U32(state));
        put("TimeToEmpty", Value::I64(3900));
        put("WarningLevel", Value::U32(warning));
        props
    }

    const DISCHARGING: u32 = 2;
    const CHARGING: u32 = 1;

    #[test]
    fn the_levels_follow_upower_and_charging_clears_them() {
        assert_eq!(wanted(&device(DISCHARGING, 1)), None);
        assert_eq!(wanted(&device(DISCHARGING, 3)), Some(Level::Low));
        assert_eq!(wanted(&device(DISCHARGING, 4)), Some(Level::Critical));
        assert_eq!(wanted(&device(DISCHARGING, 5)), Some(Level::Action));
        assert_eq!(wanted(&device(CHARGING, 4)), None);
        assert_eq!(wanted(&device(4, 3)), None, "full");
        let mut ups = device(DISCHARGING, 4);
        ups.insert("Type".into(), OwnedValue::from(3u32));
        assert_eq!(wanted(&ups), None, "not the machine's battery");
    }

    #[test]
    fn a_level_raises_its_notice_once_and_leaving_it_closes_it() {
        let mut notices = Notices::default();
        assert_eq!(notices.update(None), None);
        let step = notices.update(Some(Level::Low)).expect("entered low");
        assert_eq!(
            step,
            Step {
                close: None,
                raise: Some(Level::Low)
            }
        );
        notices.raised(":1.5".into(), 7);
        assert_eq!(notices.update(Some(Level::Low)), None, "shown once");
        // The person dismisses it: it stays dismissed while the level holds.
        notices.closed(":1.5", 7);
        assert_eq!(notices.update(Some(Level::Low)), None);
        let step = notices
            .update(Some(Level::Critical))
            .expect("entered critical");
        assert_eq!(
            step,
            Step {
                close: None,
                raise: Some(Level::Critical)
            }
        );
        notices.raised(":1.5".into(), 8);
        assert!(notices.is_shown(":1.5", 8) && !notices.is_shown(":1.5", 7));
        // Charging: the critical notice closes and nothing replaces it.
        let step = notices.update(None).expect("charging");
        let shown = Shown {
            server: ":1.5".into(),
            id: 8,
        };
        assert_eq!(
            step,
            Step {
                close: Some(shown),
                raise: None
            }
        );
        assert!(!notices.is_shown(":1.5", 8));
        // Discharging again below the level raises it again.
        let step = notices.update(Some(Level::Low)).expect("low again");
        assert_eq!(
            step,
            Step {
                close: None,
                raise: Some(Level::Low)
            }
        );
    }

    #[test]
    fn only_the_server_that_raised_a_notice_speaks_for_it() {
        let mut notices = Notices::default();
        notices.update(Some(Level::Critical));
        notices.raised(":1.5".into(), 3);
        assert!(!notices.is_shown(":1.9", 3), "another sender's id 3");
        notices.closed(":1.9", 3);
        notices.closed(":1.5", 4);
        assert!(notices.is_shown(":1.5", 3));
    }

    #[test]
    fn a_raise_that_failed_is_tried_again_at_the_next_change() {
        let mut notices = Notices::default();
        assert!(notices.update(Some(Level::Low)).is_some());
        notices.failed();
        let step = notices.update(Some(Level::Low)).expect("tried again");
        assert_eq!(
            step,
            Step {
                close: None,
                raise: Some(Level::Low)
            }
        );
    }

    #[test]
    fn the_texts_and_hints_are_those_of_sd14() {
        let catalog = Catalog::empty();
        let battery = battery::battery(&device(DISCHARGING, 3));
        let low = notice(Level::Low, battery.as_ref(), &catalog);
        assert_eq!(low.summary, "Battery low");
        assert_eq!(low.body, "9 % remaining, about 1 h 5 min left");
        assert_eq!((low.urgency, low.resident), (1, false));
        assert_eq!(
            low.actions,
            vec![(SETTINGS_ACTION, "Battery settings".to_owned())]
        );

        let unknown = Battery {
            seconds: None,
            ..battery.expect("a battery")
        };
        assert_eq!(
            notice(Level::Low, Some(&unknown), &catalog).body,
            "9 % remaining"
        );

        let critical = notice(Level::Critical, None, &catalog);
        assert_eq!(critical.summary, "Battery critically low");
        assert_eq!(critical.body, "Connect the charger");
        assert_eq!((critical.urgency, critical.resident), (2, true));
        assert_eq!(critical.actions.len(), 1);

        let action = notice(Level::Action, None, &catalog);
        assert_eq!(
            action.summary,
            "The computer will act soon to save its state"
        );
        assert_eq!(action.urgency, 2);
        assert!(action.actions.is_empty());
    }
}
