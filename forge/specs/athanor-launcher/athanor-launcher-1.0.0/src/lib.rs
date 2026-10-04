//! The launcher's decisions, without a GTK type (doc_bar.md, BR1): tested without a display.

pub mod keys;
pub mod list;
pub mod menu;
pub mod place;
pub mod query;

/// Device nodes the launcher may open for writing, besides the directories of its Landlock
/// ruleset. `/dev/null` is what GSubprocess opens for STDOUT_SILENCE and STDERR_SILENCE
/// (qalc, `flatpak list`, the preview's `systemctl stop`).
pub const DEVICE_WRITES: [&str; 2] = ["/dev/dri", "/dev/null"];

#[cfg(test)]
mod tests {
    use super::DEVICE_WRITES;
    use athanor_unit::sandbox;
    use gtk4::gio;
    use std::path::Path;

    /// Under the launcher's own write grants, a child with silenced output starts.
    #[test]
    fn a_silenced_child_starts_under_the_launchers_grants() {
        let dir = std::env::temp_dir();
        let started = std::thread::spawn(move || {
            let devices: Vec<&Path> = DEVICE_WRITES.iter().map(Path::new).collect();
            sandbox::restrict_writes(&[dir.as_path()], &devices).map_err(|err| err.to_string())?;
            let flags = gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE;
            gio::Subprocess::newv(&[std::ffi::OsStr::new("true")], flags).map(drop).map_err(|err| err.to_string())
        })
        .join();
        assert_eq!(started.map_err(|_| "panic".to_owned()), Ok(Ok(())));
    }
}
