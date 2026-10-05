//! The month calendar below the list (NC11): no events, today marked, week numbers from a
//! setting that is off until the user turns it on.

use std::path::{Path, PathBuf};

use gtk4::glib;

/// The file whose content `true` turns the week numbers on.
fn setting_path(config: &Path) -> PathBuf {
    config.join("athanor/control-center/calendar-week-numbers")
}

fn parse(content: &str) -> bool {
    content.trim() == "true"
}

/// The first day of the week and the month names come from the locale, which GTK reads.
pub fn new() -> gtk4::Calendar {
    let calendar = gtk4::Calendar::new();
    let shown = std::fs::read_to_string(setting_path(&glib::user_config_dir())).is_ok_and(|c| parse(&c));
    calendar.set_show_week_numbers(shown);
    calendar
}

/// Opened on today each time the panel opens, even when it was left on another month.
pub fn show_today(calendar: &gtk4::Calendar) {
    if let Ok(now) = glib::DateTime::now_local() {
        calendar.select_day(&now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_numbers_are_off_unless_the_file_says_true() {
        assert!(parse("true\n"));
        for off in ["", "false", "1", "yes"] {
            assert!(!parse(off), "{off:?}");
        }
        assert_eq!(
            setting_path(Path::new("/home/u/.config")),
            Path::new("/home/u/.config/athanor/control-center/calendar-week-numbers")
        );
    }
}
