//! The directory the notifier writes, its one write grant under Landlock (`athanor_unit::sandbox`,
//! applied at start while the process has one thread). This process needs almost nothing, so
//! reads are handled too: `/usr` (its libraries and the translations) and the state directory
//! of athanor-update. Connecting to the bus sockets is not a filesystem access Landlock
//! mediates.
use std::path::PathBuf;

/// `$XDG_STATE_HOME/athanor-update-notify`, created if missing.
pub fn state_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let dir = base.filter(|dir| dir.is_absolute()).ok_or("neither XDG_STATE_HOME nor HOME is an absolute path")?.join("athanor-update-notify");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use athanor_unit::{dirs, sandbox};
    use std::path::Path;

    #[test]
    fn reads_outside_the_grants_and_writes_outside_the_state_directory_are_denied() {
        let base = std::env::temp_dir().join(format!("athanor-update-notify-landlock-{}", std::process::id()));
        let (readable, writable, hidden, link) = (base.join("readable"), base.join("writable"), base.join("hidden"), base.join("link"));
        for dir in [&readable, &writable, &hidden] {
            std::fs::create_dir_all(dir).expect("mkdir");
            std::fs::write(dir.join("file"), b"x").expect("write");
        }
        // A link put at the state directory's path is refused, not followed to what it names.
        std::os::unix::fs::symlink(&hidden, &link).expect("symlink");
        assert!(dirs::owned_dir(&link).is_err(), "a link at the state directory's path");
        let base_for_thread = base.clone();
        // Landlock confines the calling thread and its children: the test binary stays free.
        std::thread::spawn(move || {
            let grant = dirs::owned_dir(&writable).expect("the state directory");
            sandbox::restrict(&[&readable], &[grant]).expect("Landlock must be enforced, not skipped");
            assert!(std::fs::read(readable.join("file")).is_ok());
            assert_eq!(std::fs::write(readable.join("new"), b"x").expect_err("read-only").kind(), std::io::ErrorKind::PermissionDenied);
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert_eq!(std::fs::read(hidden.join("file")).expect_err("not granted").kind(), std::io::ErrorKind::PermissionDenied);
            assert_eq!(std::fs::read(base_for_thread.join("hidden/file")).expect_err("not granted").kind(), std::io::ErrorKind::PermissionDenied);
            assert_eq!(std::fs::write(link.join("new"), b"x").expect_err("through the link").kind(), std::io::ErrorKind::PermissionDenied);
        })
        .join()
        .expect("sandboxed thread");
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn the_local_zone_is_readable_under_confinement() {
        // Fedora links /etc/localtime into /usr/share/zoneinfo; Landlock judges the resolved file.
        let Ok(target) = std::fs::read_link("/etc/localtime") else {
            eprintln!("skipped: /etc/localtime is a regular file or missing, not a symlink, on this host");
            return;
        };
        let resolved = std::fs::canonicalize(Path::new("/etc").join(&target));
        if !resolved.as_ref().is_ok_and(|real| real.starts_with("/usr")) {
            eprintln!("skipped: /etc/localtime resolves to {resolved:?}, not into /usr, on this host");
            return;
        }
        let writable = std::env::temp_dir().join(format!("athanor-update-notify-zone-{}", std::process::id()));
        std::fs::create_dir_all(&writable).expect("mkdir");
        let dir = writable.clone();
        std::thread::spawn(move || {
            let grant = dirs::owned_dir(&dir).expect("the state directory");
            sandbox::restrict(&[Path::new("/usr")], &[grant]).expect("Landlock must be enforced, not skipped");
            assert!(std::fs::read("/etc/localtime").is_ok_and(|zone| zone.starts_with(b"TZif")));
        })
        .join()
        .expect("sandboxed thread");
        std::fs::remove_dir_all(writable).expect("cleanup");
    }
}
