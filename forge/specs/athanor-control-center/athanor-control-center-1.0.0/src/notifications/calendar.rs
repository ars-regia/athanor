//! The month calendar below the list (NC11): no events, today marked, week numbers from a
//! setting that is off until the user turns it on.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use gtk4::glib;

/// The file whose content `true` turns the week numbers on.
fn setting_path(config: &Path) -> PathBuf {
    config.join("athanor/control-center/calendar-week-numbers")
}

fn parse(content: &str) -> bool {
    content.trim() == "true"
}

/// The setting, read as a user's file that anyone may have swapped: only a regular file,
/// and at most 16 bytes of it.
fn read_setting(path: &Path) -> bool {
    // The type is checked before the open, which would block on a FIFO, and again on the
    // open file.
    if !std::fs::metadata(path).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let Ok(file) = File::open(path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut content = String::new();
    file.take(16).read_to_string(&mut content).is_ok() && parse(&content)
}

/// The first day of the week and the month names come from the locale, which GTK reads.
pub fn new() -> gtk4::Calendar {
    let calendar = gtk4::Calendar::new();
    refresh(&calendar);
    calendar
}

/// Reads the setting again, each time the panel opens.
pub fn refresh(calendar: &gtk4::Calendar) {
    calendar.set_show_week_numbers(read_setting(&setting_path(&glib::user_config_dir())));
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

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("athanor-cc-calendar-{}-{name}", std::process::id()))
    }

    #[test]
    fn only_a_small_regular_file_is_read() {
        let file = scratch("file");
        std::fs::write(&file, format!("true{}", " ".repeat(500))).unwrap();
        assert!(read_setting(&file), "the first 16 bytes are enough");
        std::fs::write(&file, format!("{}true", " ".repeat(500))).unwrap();
        assert!(!read_setting(&file), "nothing past 16 bytes is read");
        std::fs::remove_file(&file).unwrap();
        assert!(!read_setting(&file), "a missing file is off");
        assert!(!read_setting(&std::env::temp_dir()), "a directory is off");
        assert!(!read_setting(Path::new("/dev/zero")), "a device is off");
    }

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
