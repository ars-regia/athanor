//! The user's cosmic-idle times, read from `com.system76.CosmicIdle` v1 for `athanor-idle`'s
//! migration and its follow of cosmic-settings' Power page (doc_session_daemons.md, SD3,
//! SD16). Only the user's copy is read: the system's are defaults, not a choice.
//!
//! Each key holds `None` or `Some(<milliseconds>)` in RON. The reader tells every outcome
//! apart, because the follow fails closed: an absent key is one the page never wrote, while
//! an unreadable or unparsable one may hide a choice and must change nothing.

use crate::cosmic_config;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const COMPONENT: &str = "com.system76.CosmicIdle";
/// The largest key that is read; a real one holds a few bytes.
const MAX_BYTES: u64 = 4096;

/// What a read found for one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// No file for the key, or no store at all.
    Absent,
    /// A file that could not be read: an I/O error, a link, a FIFO, too large, not UTF-8.
    Unreadable,
    /// A file whose text is neither `None` nor `Some(<integer>)`.
    Unparsable,
    /// `None`: never.
    Never,
    /// `Some(n)`, in milliseconds; `Some(0)` included, which the caller judges.
    Millis(u64),
}

/// The three keys of cosmic-idle's configuration, version 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Idle {
    pub screen_off_time: Key,
    pub suspend_on_battery_time: Key,
    pub suspend_on_ac_time: Key,
}

/// The directory of the user's keys, which a watcher follows; `None` with no home.
pub fn user_dir() -> Option<PathBuf> {
    Some(cosmic_config::component(
        &cosmic_config::user_dir()?,
        COMPONENT,
    ))
}

/// The user's keys; every key is `Absent` when there is no home to read them from.
pub fn read_user() -> Idle {
    match user_dir() {
        Some(dir) => read_in(&dir),
        None => Idle {
            screen_off_time: Key::Absent,
            suspend_on_battery_time: Key::Absent,
            suspend_on_ac_time: Key::Absent,
        },
    }
}

/// The keys in `dir`, a `com.system76.CosmicIdle/v1` directory.
pub fn read_in(dir: &Path) -> Idle {
    Idle {
        screen_off_time: read_key(&dir.join("screen_off_time")),
        suspend_on_battery_time: read_key(&dir.join("suspend_on_battery_time")),
        suspend_on_ac_time: read_key(&dir.join("suspend_on_ac_time")),
    }
}

/// One key, opened `O_NOFOLLOW | O_NONBLOCK`; it must be a regular file, so a FIFO or a link
/// another process put there cannot block the daemon.
fn read_key(path: &Path) -> Key {
    let text = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY)
        .open(path)
        .and_then(|file| {
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            let mut text = String::new();
            file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
            Ok(text)
        });
    match text {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Key::Absent,
        Err(_) => Key::Unreadable,
        Ok(text) if text.len() as u64 > MAX_BYTES => Key::Unreadable,
        Ok(text) => parse(&text),
    }
}

/// `None` or `Some(<decimal digits>)`, with RON's surrounding white space.
fn parse(text: &str) -> Key {
    let text = text.trim();
    if text == "None" {
        return Key::Never;
    }
    let Some(inner) = text
        .strip_prefix("Some")
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .map(str::trim)
    else {
        return Key::Unparsable;
    };
    // `parse` alone would also take a leading `+`.
    if inner.is_empty() || !inner.bytes().all(|b| b.is_ascii_digit()) {
        return Key::Unparsable;
    }
    inner.parse().map_or(Key::Unparsable, Key::Millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn values() {
        assert_eq!(parse("None"), Key::Never);
        assert_eq!(parse("None\n"), Key::Never);
        assert_eq!(parse("Some(900000)"), Key::Millis(900_000));
        assert_eq!(parse(" Some( 0 )\n"), Key::Millis(0));
        for text in [
            "",
            "none",
            "Some()",
            "Some(-1)",
            "Some(+5)",
            "Some(1.5)",
            "Some(5",
            "Some(99999999999999999999999)",
            "900000",
            "Some(5) x",
        ] {
            assert_eq!(parse(text), Key::Unparsable, "{text:?}");
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-idle-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn tells_absent_unreadable_and_unparsable_apart() {
        let dir = scratch("keys");
        fs::write(dir.join("screen_off_time"), "None\n").expect("write");
        fs::write(dir.join("suspend_on_battery_time"), "Some(x)").expect("write");
        assert_eq!(
            read_in(&dir),
            Idle {
                screen_off_time: Key::Never,
                suspend_on_battery_time: Key::Unparsable,
                suspend_on_ac_time: Key::Absent,
            }
        );
        // A directory, a link and an oversized file in place of a key cannot be read.
        fs::create_dir(dir.join("suspend_on_ac_time")).expect("dir");
        std::os::unix::fs::symlink(dir.join("suspend_on_battery_time"), dir.join("link"))
            .expect("link");
        fs::rename(dir.join("link"), dir.join("screen_off_time")).expect("rename");
        assert_eq!(read_key(&dir.join("suspend_on_ac_time")), Key::Unreadable);
        assert_eq!(read_key(&dir.join("screen_off_time")), Key::Unreadable);
        fs::write(dir.join("big"), " ".repeat(5000) + "None").expect("write");
        assert_eq!(read_key(&dir.join("big")), Key::Unreadable);
        // No directory at all: every key is absent.
        let gone = dir.join("missing");
        assert_eq!(read_in(&gone).screen_off_time, Key::Absent);
        drop(fs::remove_dir_all(&dir));
    }
}
