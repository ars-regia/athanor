//! The window state of SE5 (doc_settings.md): `$XDG_RUNTIME_DIR/athanor-settings/state` holds
//! the target of the open window, written on every navigation and removed when the window
//! closes, so that a Settings restarted after a crash reopens at that target. It is the only
//! preference Settings keeps of its own (SE1).

use std::fs;
use std::io;
use std::path::Path;

use crate::registry::Registry;
use crate::target::{self, Target};

/// The file's name in Settings' runtime directory.
pub const FILE: &str = "state";
const OPEN: &str = "open ";
/// More than any target the registry holds; a longer file is not Settings'.
const MAX_BYTES: u64 = 256;

/// The text the file holds while the window shows `target`.
pub fn encode(target: Target) -> String {
    format!("{OPEN}{target}\n")
}

/// The target to reopen at, when `text` says a window was open at a target the registry
/// knows. Anything else reopens nothing.
pub fn decode(registry: &Registry, text: &str) -> Option<Target> {
    let line = text.strip_suffix('\n')?;
    target::parse(registry, line.strip_prefix(OPEN)?).ok()
}

/// Writes the state of a window at `target` into `dir`, through a file renamed over the old
/// one, so a crash mid-write leaves the previous state whole.
pub fn save(dir: &Path, target: Target) -> io::Result<()> {
    let temporary = dir.join(format!("{FILE}.new"));
    fs::write(&temporary, encode(target))?;
    fs::rename(&temporary, dir.join(FILE))
}

/// Removes the state when the window closes. A file already gone is no error.
pub fn clear(dir: &Path) -> io::Result<()> {
    match fs::remove_file(dir.join(FILE)) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The target a window was open at when Settings last stopped, if the file says one was.
pub fn load(registry: &Registry, dir: &Path) -> io::Result<Option<Target>> {
    let path = dir.join(FILE);
    let size = match fs::metadata(&path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    if size > MAX_BYTES {
        return Ok(None);
    }
    Ok(decode(registry, &fs::read_to_string(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new directory of this test alone: `create_dir` refuses a path that already exists, a
    /// directory or a link another run left, and the clock's nanoseconds keep runs apart.
    fn dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "athanor-settings-core-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&dir).expect("create the test directory");
        dir
    }

    #[test]
    fn the_state_names_the_target_of_the_open_window() {
        let registry = Registry::new();
        for target in [
            Target::Landing,
            Target::Page("about"),
            Target::Section("power", "idle"),
        ] {
            assert_eq!(decode(&registry, &encode(target)), Some(target));
        }
        assert_eq!(decode(&registry, "open printers\n"), None);
        assert_eq!(
            decode(&registry, "open about"),
            None,
            "a torn line reopens nothing"
        );
        assert_eq!(decode(&registry, "closed\n"), None);
        assert_eq!(decode(&registry, ""), None);
    }

    #[test]
    fn a_saved_state_is_loaded_and_a_cleared_one_reopens_nothing() {
        let registry = Registry::new();
        let dir = dir("state");
        assert_eq!(load(&registry, &dir).expect("no file"), None);
        save(&dir, Target::Page("about")).expect("save");
        save(&dir, Target::Section("users", "fingerprint")).expect("save again");
        assert_eq!(
            load(&registry, &dir).expect("load"),
            Some(Target::Section("users", "fingerprint"))
        );
        clear(&dir).expect("clear");
        clear(&dir).expect("clear twice");
        assert_eq!(load(&registry, &dir).expect("no file"), None);
        fs::write(dir.join(FILE), "x".repeat(4096)).expect("write");
        assert_eq!(load(&registry, &dir).expect("too long"), None);
        fs::remove_dir_all(&dir).expect("cleaned up");
    }
}
