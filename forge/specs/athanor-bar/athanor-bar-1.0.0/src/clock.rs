//! The time zone the clock shows (doc_bar.md, BR3). `$TZ` wins, as for every program.
//! Otherwise the zone is the target of /etc/localtime, which timedatectl relinks. GLib's
//! local zone is cached for the life of the process, so the bar names the zone itself.
//! [`Clock`] reads the zone and the wall clock again at every tick: nothing is counted, so a
//! resume or a clock that jumps shows at once.

use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::path::{Component, Path};
use std::rc::Rc;

use gio::prelude::*;

use crate::i18n::tr;

const INTERFACE: &str = "org.gnome.desktop.interface";
const CLOCK_FORMAT: &str = "clock-format";

/// The user's `clock-format` (doc_languages.md, LN8): `Some(true)` for a 24-hour clock.
pub fn clock_format(value: Option<&str>) -> Option<bool> {
    match value {
        Some("24h") => Some(true),
        Some("12h") => Some(false),
        _ => None,
    }
}

/// Whether the clock shows 24 hours. The user's `clock-format` decides when it is set.
/// Otherwise the locale does: `locale_time` is 13:00 formatted with `%X` (the locale's time
/// format) and `locale_pm` is `%p` at 13:00. A locale whose time format shows no PM marker is
/// a 24-hour locale, even when it defines one (en_GB).
pub fn twenty_four_hour(clock_format: Option<bool>, locale_time: &str, locale_pm: &str) -> bool {
    clock_format.unwrap_or_else(|| locale_pm.is_empty() || !locale_time.contains(locale_pm))
}

/// The zone identifier, or `None` for GLib's local zone.
pub fn zone(tz: Option<&str>, localtime: Option<&Path>) -> Option<String> {
    let tz = tz
        .map(|tz| tz.strip_prefix(':').unwrap_or(tz))
        .filter(|tz| !tz.is_empty());
    match tz {
        Some(tz) if Path::new(tz).is_absolute() => below_zoneinfo(Path::new(tz)),
        Some(tz) => Some(tz.to_string()),
        None => below_zoneinfo(localtime?),
    }
}

/// `Europe/Rome` from `…/zoneinfo/Europe/Rome`. Only plain names may follow `zoneinfo`.
fn below_zoneinfo(path: &Path) -> Option<String> {
    let mut components = path.components();
    components
        .by_ref()
        .find(|component| component.as_os_str() == "zoneinfo")?;
    let names = components
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<&str>>>()?;
    (!names.is_empty()).then(|| names.join("/"))
}

/// The zone to show. An identifier GLib does not know falls back to local time; the flag
/// says so, for the caller to log.
pub fn time_zone(id: Option<&str>) -> (glib::TimeZone, bool) {
    match id {
        Some(id) => match glib::TimeZone::from_identifier(Some(id)) {
            Some(zone) => (zone, false),
            None => (glib::TimeZone::local(), true),
        },
        None => (glib::TimeZone::local(), false),
    }
}

/// The settings that hold `clock-format`, `None` without the key.
fn interface_settings() -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(INTERFACE, true);
    if !schema.is_some_and(|schema| schema.has_key(CLOCK_FORMAT)) {
        tracing::warn!("{INTERFACE} has no {CLOCK_FORMAT}; the clock follows the locale");
        return None;
    }
    Some(gio::Settings::new(INTERFACE))
}

/// The user's `clock-format` when they set it, else the locale's own 12/24-hour convention:
/// the key's default says nothing of the region (doc_languages.md, LN8).
fn read_hours24(interface: Option<&gio::Settings>) -> bool {
    let chosen = interface
        .and_then(|interface| interface.user_value(CLOCK_FORMAT))
        .and_then(|value| clock_format(value.str()));
    let now = glib::DateTime::from_utc(2000, 1, 1, 13, 0, 0.0).ok();
    let format = |pattern| {
        now.as_ref()
            .and_then(|now| now.format(pattern).ok())
            .map(|text| text.to_string())
            .unwrap_or_default()
    };
    twenty_four_hour(chosen, &format("%X"), &format("%p"))
}

/// The bar's clock: the zone last read and the zone it gave, and whether it shows 24 hours,
/// the user's `clock-format` followed live, else the locale (BR3).
pub struct Clock {
    zone: RefCell<Option<(Option<String>, glib::TimeZone)>>,
    /// The last tick could not read or format the time: logged once, not every second.
    failed: Cell<bool>,
    hours24: Rc<Cell<bool>>,
    /// Keeps `clock-format` watched.
    _interface: Option<gio::Settings>,
}

impl Clock {
    pub fn new() -> Clock {
        let interface = interface_settings();
        let hours24 = Rc::new(Cell::new(read_hours24(interface.as_ref())));
        if let Some(interface) = &interface {
            let watched = hours24.clone();
            interface.connect_changed(Some(CLOCK_FORMAT), move |interface, _| {
                watched.set(read_hours24(Some(interface)));
            });
        }
        Clock {
            zone: RefCell::new(None),
            failed: Cell::new(false),
            hours24,
            _interface: interface,
        }
    }

    fn current_zone(&self) -> glib::TimeZone {
        let link = fs::read_link("/etc/localtime").ok();
        let id = zone(
            env::var("TZ").ok().as_deref(),
            link.as_deref().map(Path::new),
        );
        let mut last = self.zone.borrow_mut();
        match &*last {
            Some((last_id, tz)) if *last_id == id => tz.clone(),
            _ => {
                let (tz, unknown) = time_zone(id.as_deref());
                if unknown {
                    // Logged when the zone changes, not every second.
                    tracing::warn!(
                        zone = id.as_deref().unwrap_or(""),
                        "unknown time zone; the clock shows local time"
                    );
                }
                *last = Some((id, tz.clone()));
                tz
            }
        }
    }

    /// The time now in the zone the clock shows.
    pub fn now(&self) -> Result<glib::DateTime, glib::BoolError> {
        glib::DateTime::now(&self.current_zone())
    }

    /// The text the bar shows and the one screen readers say, now; `None`, logged once until
    /// it works again, when the time cannot be read or formatted.
    pub fn texts(&self) -> Option<(String, String)> {
        let (short_pattern, long_pattern) = if self.hours24.get() {
            (
                // TRANSLATORS: a g_date_time_format() pattern for the clock on the bar.
                tr("%a %-d %b %H:%M"),
                // TRANSLATORS: a g_date_time_format() pattern, read aloud by screen readers.
                tr("%A %-d %B %Y, %H:%M"),
            )
        } else {
            (
                // TRANSLATORS: a g_date_time_format() pattern for the clock on the bar (12-hour clock).
                tr("%a %-d %b %-I:%M %p"),
                // TRANSLATORS: a g_date_time_format() pattern, read aloud by screen readers (12-hour clock).
                tr("%A %-d %B %Y, %-I:%M %p"),
            )
        };
        let texts = self.now().and_then(|now| {
            let short = now.format(&short_pattern)?;
            let long = now.format(&long_pattern)?;
            Ok((short.to_string(), long.to_string()))
        });
        match texts {
            Ok(texts) => {
                self.failed.set(false);
                Some(texts)
            }
            Err(err) => {
                if !self.failed.replace(true) {
                    tracing::error!(error = %err, "cannot read or format the time; the clock is not updated");
                }
                None
            }
        }
    }
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_formats_are_a_choice() {
        assert_eq!(clock_format(Some("24h")), Some(true));
        assert_eq!(clock_format(Some("12h")), Some(false));
        assert_eq!(clock_format(Some("")), None);
        assert_eq!(clock_format(None), None);
    }

    #[test]
    fn the_users_setting_wins_over_the_locale() {
        assert!(twenty_four_hour(Some(true), "01:00:00 PM", "PM"));
        assert!(!twenty_four_hour(Some(false), "13:00:00", ""));
    }

    #[test]
    fn without_it_the_locale_decides_by_its_pm_marker() {
        assert!(!twenty_four_hour(None, "01:00:00 PM", "PM")); // en_US
        assert!(twenty_four_hour(None, "13:00:00", "pm")); // en_GB: a marker it does not use
        assert!(twenty_four_hour(None, "13:00:00", "")); // de_DE
    }

    #[test]
    fn an_absolute_or_relative_link_gives_the_zone() {
        assert_eq!(
            zone(None, Some(Path::new("/usr/share/zoneinfo/Europe/Rome"))).as_deref(),
            Some("Europe/Rome")
        );
        assert_eq!(
            zone(
                None,
                Some(Path::new(
                    "../usr/share/zoneinfo/America/Argentina/Buenos_Aires"
                ))
            )
            .as_deref(),
            Some("America/Argentina/Buenos_Aires")
        );
    }

    #[test]
    fn tz_wins_without_its_colon_and_an_empty_tz_counts_as_unset() {
        let rome = Some(Path::new("/usr/share/zoneinfo/Europe/Rome"));
        assert_eq!(
            zone(Some(":Asia/Tokyo"), rome).as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            zone(Some("/usr/share/zoneinfo/UTC"), rome).as_deref(),
            Some("UTC")
        );
        assert_eq!(
            zone(Some("CET-1CEST,M3.5.0,M10.5.0/3"), rome).as_deref(),
            Some("CET-1CEST,M3.5.0,M10.5.0/3")
        );
        assert_eq!(zone(Some(""), rome).as_deref(), Some("Europe/Rome"));
    }

    #[test]
    fn a_link_that_is_absent_or_outside_zoneinfo_means_local_time() {
        assert_eq!(zone(None, None), None);
        assert_eq!(zone(None, Some(Path::new("/etc/athanor/localtime"))), None);
        assert_eq!(zone(None, Some(Path::new("/usr/share/zoneinfo"))), None);
        assert_eq!(
            zone(
                None,
                Some(Path::new("/usr/share/zoneinfo/../../../etc/passwd"))
            ),
            None
        );
    }

    #[test]
    fn a_zone_glib_knows_is_used() {
        let (utc, unknown) = time_zone(Some("UTC"));
        assert!(!unknown);
        assert_eq!(utc.identifier(), "UTC");
        // A POSIX rule needs no tzdata, so the test does not depend on the build image.
        let rule = "CET-1CEST,M3.5.0,M10.5.0/3";
        let (posix, unknown) = time_zone(Some(rule));
        assert!(!unknown);
        assert_eq!(posix.identifier(), rule);
    }

    #[test]
    fn an_unknown_zone_falls_back_to_local_time_and_says_so() {
        let (zone, unknown) = time_zone(Some("Mars/Olympus_Mons"));
        assert!(unknown);
        assert_eq!(zone.identifier(), glib::TimeZone::local().identifier());
        let (zone, unknown) = time_zone(None);
        assert!(!unknown);
        assert_eq!(zone.identifier(), glib::TimeZone::local().identifier());
    }
}
