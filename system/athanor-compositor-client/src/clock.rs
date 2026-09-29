//! COSMIC's clock settings (`com.system76.CosmicAppletTime`), which the bar's clock follows
//! so that both clocks of a session agree.

use std::path::PathBuf;

use gtk4::gio;
use gtk4::prelude::*;

use crate::cosmic_config;

const APPLET_TIME: &str = "com.system76.CosmicAppletTime";

/// COSMIC's `military_time`: `Some(true)` for a 24-hour clock. `None` when no directory
/// sets it, or when the value is not a boolean.
pub fn military_time() -> Option<bool> {
    military_time_in(&cosmic_config::dirs())
}

fn military_time_in(dirs: &[PathBuf]) -> Option<bool> {
    cosmic_config::key(dirs, APPLET_TIME, "military_time")?
        .trim()
        .parse()
        .ok()
}

/// Calls `on_change` after each change in the user's clock settings, for as long as the
/// returned monitor lives. A directory that does not exist yet is watched too: GIO polls
/// for it.
#[must_use = "the watch stops when the monitor is dropped"]
pub fn watch(on_change: impl Fn() + 'static) -> Option<gio::FileMonitor> {
    let dir = cosmic_config::component(&cosmic_config::user_dir()?, APPLET_TIME);
    match gio::File::for_path(&dir)
        .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
    {
        Ok(monitor) => {
            monitor.connect_changed(move |_, _, _, _| on_change());
            Some(monitor)
        }
        Err(err) => {
            tracing::warn!(error = %err, "clock setting changes are not followed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn military_time_reads_the_first_directory_that_sets_it() {
        let base = std::env::temp_dir().join(format!("athanor-clock-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&base);
        let (user, system) = (base.join("user"), base.join("system"));
        assert_eq!(military_time_in(&[user.clone(), system.clone()]), None);
        let dir = cosmic_config::component(&system, APPLET_TIME);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("military_time"), "true\n").expect("write");
        assert_eq!(
            military_time_in(&[user.clone(), system.clone()]),
            Some(true)
        );
        let dir = cosmic_config::component(&user, APPLET_TIME);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("military_time"), "yes").expect("write");
        assert_eq!(
            military_time_in(&[user, system]),
            None,
            "an unreadable value is no setting"
        );
    }
}
