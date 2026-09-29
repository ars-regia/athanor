//! Landlock at start, as the greeter does (`athanor-shell-rs`, `src/sys/sandbox.rs`):
//! first prove the process is single-threaded, then restrict. Unlike the greeter, this
//! process needs almost nothing, so reads are handled too: `/usr` (its libraries and the
//! translations), the state directory of athanor-update, and one directory of its own to
//! write. Connecting to the bus sockets is not a filesystem access Landlock mediates.
use landlock::{Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI};
use std::path::{Path, PathBuf};

/// Fails unless the calling process has exactly one thread: Landlock confines the calling
/// thread and those it creates afterwards, not threads that already exist.
pub fn ensure_single_threaded() -> Result<(), Box<dyn std::error::Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!("{threads} threads exist; Landlock would leave all but one unconfined").into());
    }
    Ok(())
}

/// Read-only beneath `read`, read-write beneath `write`, nothing else. A kernel that cannot
/// enforce the ruleset is an error, not a best effort.
pub fn restrict(read: &[&Path], write: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let all = AccessFs::from_all(ABI::V1);
    let mut ruleset = Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(all)?.create()?;
    for path in read.iter().filter(|path| path.exists()) {
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, AccessFs::from_read(ABI::V1)))?;
    }
    ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(write)?, all))?;
    ruleset.restrict_self()?;
    Ok(())
}

/// `$XDG_STATE_HOME/athanor-update-notify`, created if missing.
pub fn state_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let dir = base.filter(|dir| dir.is_absolute()).ok_or("neither XDG_STATE_HOME nor HOME is an absolute path")?.join("athanor-update-notify");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outside_the_grants_and_writes_outside_the_state_directory_are_denied() {
        let base = std::env::temp_dir().join(format!("athanor-update-notify-landlock-{}", std::process::id()));
        let (readable, writable, hidden) = (base.join("readable"), base.join("writable"), base.join("hidden"));
        for dir in [&readable, &writable, &hidden] {
            std::fs::create_dir_all(dir).expect("mkdir");
            std::fs::write(dir.join("file"), b"x").expect("write");
        }
        let base_for_thread = base.clone();
        // Landlock confines the calling thread and its children: the test binary stays free.
        std::thread::spawn(move || {
            restrict(&[&readable], &writable).expect("Landlock must be enforced, not skipped");
            assert!(std::fs::read(readable.join("file")).is_ok());
            assert_eq!(std::fs::write(readable.join("new"), b"x").expect_err("read-only").kind(), std::io::ErrorKind::PermissionDenied);
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert_eq!(std::fs::read(hidden.join("file")).expect_err("not granted").kind(), std::io::ErrorKind::PermissionDenied);
            assert_eq!(std::fs::read(base_for_thread.join("hidden/file")).expect_err("not granted").kind(), std::io::ErrorKind::PermissionDenied);
        })
        .join()
        .expect("sandboxed thread");
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_second_thread_fails_the_single_thread_check() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let other = std::thread::spawn(move || wait.recv());
        assert!(ensure_single_threaded().is_err());
        release.send(()).expect("release");
        other.join().expect("join").expect("released");
    }
}
