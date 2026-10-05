//! The words of the panel (NC11), pure: the relative time, the do-not-disturb state, a row's
//! accessible name, the opening announcement and the "until 08:00" of the menu.

use athanor_services::notifications::Dnd;

use crate::i18n::{tr, tr_with};

/// "now", "5 min", "2 h", "Yesterday", then the date.
pub fn relative_time(now: i64, then: i64) -> String {
    let age = now.saturating_sub(then);
    match age {
        ..60 => tr("now"),
        60..3600 => tr_with("{n} min", "n", &(age / 60).to_string()),
        3600..86_400 => tr_with("{n} h", "n", &(age / 3600).to_string()),
        // ponytail: "Yesterday" is the 24 to 48 hours before, not the calendar day; the
        // calendar day needs the zone, upgrade if the difference is ever noticed.
        86_400..172_800 => tr("Yesterday"),
        _ => glib::DateTime::from_unix_local(then)
            .and_then(|date| date.format("%x"))
            .map_or_else(|_| String::new(), |date| date.to_string()),
    }
}

/// "On until 07:00 · schedule", "Off". `local_until` is `dnd.until` as the clock reads it.
pub fn dnd_words(dnd: &Dnd, local_until: Option<String>) -> String {
    if !dnd.on {
        return tr("Off");
    }
    let mut words = match local_until {
        Some(time) => tr_with("On until {time}", "time", &time),
        None => tr("On"),
    };
    let why = match dnd.reason.as_str() {
        "schedule" => Some(tr("schedule")),
        "fullscreen" => Some(tr("fullscreen")),
        "screen-sharing" => Some(tr("screen sharing")),
        _ => None,
    };
    if let Some(why) = why {
        words.push_str(" \u{b7} ");
        words.push_str(&why);
    }
    words
}

/// The application, summary, body and time a screen reader reads for a row.
pub fn accessible_name(app: &str, summary: &str, body: &str, time: &str) -> String {
    [app, summary, body, time]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// "Notification center, 3 unread".
pub fn announcement(unread: usize) -> String {
    match unread {
        0 => tr("Notification center, no unread"),
        n => tr_with("Notification center, {n} unread", "n", &n.to_string()),
    }
}

/// The next `hour`:00 after `now`, in `now`'s zone: today's while it is ahead, else
/// tomorrow's (CC13's "until 08:00", across midnight and across a change of offset).
pub fn next_at(now: &glib::DateTime, hour: i32) -> Option<glib::DateTime> {
    let today = glib::DateTime::new(
        &now.timezone(),
        now.year(),
        now.month(),
        now.day_of_month(),
        hour,
        0,
        0.0,
    )
    .ok()?;
    if today > *now {
        Some(today)
    } else {
        today.add_days(1).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dnd(on: bool, reason: &str, until: i64) -> Dnd {
        Dnd {
            on,
            reason: reason.into(),
            until,
            unavailable: Vec::new(),
        }
    }

    #[test]
    fn relative_time_changes_band_at_its_edges() {
        let at = |age: i64| relative_time(1_000_000, 1_000_000 - age);
        assert_eq!(at(-5), "now", "a clock a little behind is still now");
        assert_eq!(at(0), "now");
        assert_eq!(at(59), "now");
        assert_eq!(at(60), "1 min");
        assert_eq!(at(3599), "59 min");
        assert_eq!(at(3600), "1 h");
        assert_eq!(at(86_399), "23 h");
        assert_eq!(at(86_400), "Yesterday");
        assert_eq!(at(172_799), "Yesterday");
        let date = at(172_800);
        assert!(!date.is_empty() && date != "Yesterday" && !date.ends_with(" h"));
    }

    #[test]
    fn do_not_disturb_says_why_and_until_when() {
        let until = Some("07:00".to_owned());
        assert_eq!(dnd_words(&dnd(false, "", 0), None), "Off");
        assert_eq!(dnd_words(&dnd(true, "manual", 5), until.clone()), "On until 07:00");
        assert_eq!(dnd_words(&dnd(true, "manual", 0), None), "On");
        assert_eq!(
            dnd_words(&dnd(true, "schedule", 5), until),
            "On until 07:00 \u{b7} schedule"
        );
        assert_eq!(dnd_words(&dnd(true, "fullscreen", 0), None), "On \u{b7} fullscreen");
        assert_eq!(
            dnd_words(&dnd(true, "screen-sharing", 0), None),
            "On \u{b7} screen sharing"
        );
    }

    #[test]
    fn a_row_is_named_by_its_four_parts_and_skips_an_empty_one() {
        assert_eq!(
            accessible_name("Mail", "Hello", "Are you there?", "5 min"),
            "Mail, Hello, Are you there?, 5 min"
        );
        assert_eq!(accessible_name("Mail", "Hello", "", "now"), "Mail, Hello, now");
    }

    #[test]
    fn the_announcement_counts_the_unread() {
        assert_eq!(announcement(0), "Notification center, no unread");
        assert_eq!(announcement(3), "Notification center, 3 unread");
    }

    fn rome(y: i32, mo: i32, d: i32, h: i32, mi: i32) -> glib::DateTime {
        let zone = glib::TimeZone::from_identifier(Some("Europe/Rome")).expect("zone");
        glib::DateTime::new(&zone, y, mo, d, h, mi, 0.0).expect("date")
    }

    #[test]
    fn until_eight_is_today_while_ahead_and_tomorrow_after() {
        let early = next_at(&rome(2026, 10, 5, 7, 0), 8).expect("next");
        assert_eq!((early.day_of_month(), early.hour()), (5, 8));
        let late = next_at(&rome(2026, 10, 5, 23, 30), 8).expect("next");
        assert_eq!((late.day_of_month(), late.hour()), (6, 8));
        let exactly = next_at(&rome(2026, 10, 5, 8, 0), 8).expect("next");
        assert_eq!(exactly.day_of_month(), 6, "08:00 sharp asks for tomorrow's");
    }

    #[test]
    fn until_eight_holds_across_the_end_of_summer_time() {
        // Rome leaves summer time at 03:00 on 2026-10-25: that night has 25 hours.
        let now = rome(2026, 10, 24, 22, 0);
        let next = next_at(&now, 8).expect("next");
        assert_eq!((next.day_of_month(), next.hour()), (25, 8));
        assert_eq!(next.to_unix() - now.to_unix(), 11 * 3600);
    }
}
