//! COSMIC's configuration on disk, read without libcosmic. cosmic-config resolves every
//! key on its own, the user's file first, then the system directories; so do we.

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// The largest key read: cosmic-config's records are a few kilobytes at most.
const LIMIT: u64 = 64 * 1024;

/// `<dir>/cosmic` for every directory in `XDG_DATA_DIRS`: the defaults the packages ship.
pub(crate) fn system_dirs() -> Vec<PathBuf> {
    let data = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty());
    let data = data.as_deref().unwrap_or("/usr/local/share:/usr/share");
    data.split(':')
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("cosmic"))
        .collect()
}

/// `$XDG_CONFIG_HOME/cosmic`, the only directory whose keys change while a session runs.
pub(crate) fn user_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("cosmic"))
}

/// The directory of a component's keys under a `cosmic` directory.
pub(crate) fn component(dir: &std::path::Path, component: &str) -> PathBuf {
    versioned(dir, component, 1)
}

/// The directory of a component whose keys are at `version`: COSMIC versions each
/// component on its own (the built themes are `v2`, spike VL-S1).
pub(crate) fn versioned(dir: &std::path::Path, component: &str, version: u32) -> PathBuf {
    dir.join(component).join(format!("v{version}"))
}

/// The first copy of a key, highest directory first; `None` where no directory has one. A
/// copy that cannot be read, is not a regular file, is larger than `LIMIT` or is not UTF-8
/// is an error, not an absence.
pub(crate) fn key(
    dirs: &[PathBuf],
    component_name: &str,
    name: &str,
) -> io::Result<Option<String>> {
    for dir in dirs {
        let path = component(dir, component_name).join(name);
        match read(&path) {
            Ok(text) => return Ok(Some(text)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!("{}: {err}", path.display()),
                ))
            }
        }
    }
    Ok(None)
}

/// Opened without blocking, so that a FIFO or a device never hangs the client.
fn read(path: &Path) -> io::Result<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let mut text = String::new();
    file.take(LIMIT + 1).read_to_string(&mut text)?;
    if text.len() as u64 > LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("larger than {LIMIT} bytes"),
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a missing copy lets a lower directory answer; any other failure is the caller's.
    #[test]
    fn only_a_missing_copy_is_absent() {
        let root = std::env::temp_dir().join(format!("athanor-cosmic-key-{}", std::process::id()));
        let (user, system) = (root.join("user"), root.join("system"));
        let at = |dir: &Path| component(dir, "c").join("k");
        std::fs::create_dir_all(at(&system).parent().expect("parent")).expect("system");
        std::fs::write(at(&system), "system").expect("system key");
        let dirs = [user.clone(), system];
        assert_eq!(
            key(&dirs, "c", "k").expect("system copy").as_deref(),
            Some("system")
        );
        assert_eq!(key(&dirs, "c", "none").expect("no copy"), None);
        std::fs::create_dir_all(at(&user)).expect("a directory where the key is");
        assert!(key(&dirs, "c", "k").is_err(), "a directory");
        std::fs::remove_dir(at(&user)).expect("remove");
        let made = std::process::Command::new("mkfifo")
            .arg(at(&user))
            .status()
            .expect("mkfifo");
        assert!(made.success(), "mkfifo");
        assert!(
            key(&dirs, "c", "k").is_err(),
            "a FIFO, refused without blocking"
        );
        std::fs::remove_file(at(&user)).expect("remove");
        std::fs::write(at(&user), vec![b' '; LIMIT as usize + 1]).expect("large");
        assert!(key(&dirs, "c", "k").is_err(), "too large");
        std::fs::write(at(&user), [0xff, 0xfe]).expect("not UTF-8");
        assert!(key(&dirs, "c", "k").is_err(), "not UTF-8");
        std::fs::write(at(&user), "user").expect("user key");
        assert_eq!(
            key(&dirs, "c", "k").expect("user copy").as_deref(),
            Some("user")
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
