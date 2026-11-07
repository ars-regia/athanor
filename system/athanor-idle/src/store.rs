//! `inhibitors.json` (`doc_session.md` SN7): the inhibitor table, kept across a restart.

use crate::inhibitors::{truncate, Inhibitor, Inhibitors, Kind, MAX_ENTRIES, MAX_TEXT};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FileKind {
    Idle,
    Suspend,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    cookie: u32,
    owner: String,
    app_id: String,
    kind: FileKind,
    reason: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    inhibitors: Vec<Entry>,
}

/// `$XDG_RUNTIME_DIR/athanor-idle`, the directory of `inhibitors.json` and the one the daemon
/// may write (SD17), created with mode 0700. One that exists already must be a directory, not
/// a link, owned by the owner of `runtime` and of mode 0700, or it is refused: another process
/// of the user could otherwise point the daemon's writes elsewhere.
pub fn runtime_dir(runtime: &Path) -> io::Result<PathBuf> {
    let dir = runtime.join("athanor-idle");
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => return Ok(dir),
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    let owner = fs::metadata(runtime)?.uid();
    let found = fs::symlink_metadata(&dir)?;
    let refuse = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {why}", dir.display()),
        ))
    };
    if !found.file_type().is_dir() {
        return refuse("is not a directory");
    }
    if found.uid() != owner {
        return refuse("is not owned by the user");
    }
    if found.permissions().mode() & 0o7777 != 0o700 {
        return refuse("does not have mode 0700");
    }
    Ok(dir)
}

/// The largest file read back (proposed, revision 3).
const MAX_FILE: u64 = 64 * 1024;

/// Writes the table through a fresh temporary file and a rename, mode 0600. A stale
/// temporary file (or a symlink in its place) is removed first and never followed.
pub fn save(path: &Path, table: &Inhibitors) -> io::Result<()> {
    let file = File {
        version: 1,
        inhibitors: table
            .iter()
            .map(|(cookie, i)| Entry {
                cookie,
                owner: i.owner.clone(),
                app_id: i.app_id.clone(),
                kind: match i.kind {
                    Kind::Screen => FileKind::Idle,
                    Kind::SuspendOnly => FileKind::Suspend,
                },
                reason: i.reason.clone(),
            })
            .collect(),
    };
    let json = serde_json::to_vec(&file).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let written = (|| {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)?;
        out.set_permissions(fs::Permissions::from_mode(0o600))?;
        out.write_all(&json)?;
        out.sync_all()
    })();
    match written.and_then(|()| fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Best effort: the error being returned is the one that matters.
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Reads the saved entries, at most [`MAX_ENTRIES`], with the texts cut to [`MAX_TEXT`].
/// No file is an empty list. A file that is not a small regular file, does not parse or has
/// another version is removed and reported as an error; the caller starts empty (SN7).
pub fn load(path: &Path) -> io::Result<Vec<(u32, Inhibitor)>> {
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path);
    let file = match opened {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        // A symlink in its place: O_NOFOLLOW refuses it.
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return discard(path, "a symlink"),
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return discard(path, "not a regular file");
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return discard(path, "too large");
    }
    match serde_json::from_slice::<File>(&bytes) {
        Ok(f) if f.version == 1 => Ok(f
            .inhibitors
            .into_iter()
            .take(MAX_ENTRIES)
            .map(|e| {
                let kind = match e.kind {
                    FileKind::Idle => Kind::Screen,
                    FileKind::Suspend => Kind::SuspendOnly,
                };
                (
                    e.cookie,
                    Inhibitor {
                        kind,
                        owner: e.owner,
                        app_id: truncate(&e.app_id, MAX_TEXT),
                        reason: truncate(&e.reason, MAX_TEXT),
                    },
                )
            })
            .collect()),
        Ok(f) => discard(path, &format!("unknown version {}", f.version)),
        Err(e) => discard(path, &e.to_string()),
    }
}

fn discard(path: &Path, why: &str) -> io::Result<Vec<(u32, Inhibitor)>> {
    fs::remove_file(path)?;
    Err(io::Error::new(io::ErrorKind::InvalidData, why.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_dir_is_created_or_checked() {
        let runtime = dir("runtime");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let made = runtime_dir(&runtime).unwrap();
        assert_eq!(made, runtime.join("athanor-idle"));
        assert_eq!(fs::metadata(&made).unwrap().permissions().mode() & 0o7777, 0o700);
        assert_eq!(runtime_dir(&runtime).unwrap(), made, "an existing one that passes");

        fs::set_permissions(&made, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(runtime_dir(&runtime).is_err(), "mode 0755");
        fs::remove_dir(&made).unwrap();

        let elsewhere = runtime.join("elsewhere");
        fs::DirBuilder::new().mode(0o700).create(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &made).unwrap();
        assert!(runtime_dir(&runtime).is_err(), "a link to a directory");
        fs::remove_file(&made).unwrap();

        fs::write(&made, b"").unwrap();
        assert!(runtime_dir(&runtime).is_err(), "a file");
        fs::remove_dir_all(&runtime).unwrap();
    }

    fn dir(name: &str) -> std::path::PathBuf {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d =
            std::env::temp_dir().join(format!("athanor-idle-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_table_round_trips_in_the_versioned_shape_with_mode_0600() {
        let path = dir("store").join("inhibitors.json");
        let mut t = Inhibitors::new();
        for (kind, owner) in [(Kind::Screen, ":1.5"), (Kind::SuspendOnly, ":1.6")] {
            t.add(Inhibitor {
                kind,
                owner: owner.into(),
                app_id: "org.example.App".into(),
                reason: "playing".into(),
            });
        }
        save(&path, &t).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["inhibitors"][1]["kind"], "suspend");
        assert_eq!(raw["inhibitors"][0]["cookie"], 1);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let back = load(&path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].0, 2);
        assert_eq!(back[1].1.kind, Kind::SuspendOnly);
    }

    #[test]
    fn a_missing_file_is_empty_and_a_corrupt_one_is_removed() {
        let path = dir("corrupt").join("inhibitors.json");
        assert!(load(&path).unwrap().is_empty());
        fs::write(&path, b"{not json").unwrap();
        assert!(load(&path).is_err());
        assert!(!path.exists());
        fs::write(&path, br#"{"version":2,"inhibitors":[]}"#).unwrap();
        assert!(load(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn a_symlinked_or_stale_temporary_file_is_never_followed() {
        let d = dir("tmp");
        let path = d.join("inhibitors.json");
        let victim = d.join("victim");
        fs::write(&victim, b"keep").unwrap();
        let tmp = path.with_extension("json.tmp");
        let _ = fs::remove_file(&tmp);
        std::os::unix::fs::symlink(&victim, &tmp).unwrap();
        save(&path, &Inhibitors::new()).unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert!(!tmp.exists());

        // A stale temporary file with a loose mode does not leak its mode into the result.
        fs::write(&tmp, b"stale").unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();
        save(&path, &Inhibitors::new()).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn load_refuses_a_symlink_a_big_file_and_caps_what_it_keeps() {
        let d = dir("load");
        let path = d.join("inhibitors.json");
        let other = d.join("other.json");
        fs::write(&other, br#"{"version":1,"inhibitors":[]}"#).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        assert!(load(&path).is_err());
        assert!(!path.exists() && other.exists());

        fs::write(&path, vec![b' '; 70_000]).unwrap();
        assert!(load(&path).is_err());
        assert!(!path.exists());

        // 300 small entries are under the file cap and over the entry cap.
        let list: Vec<String> = (1..=300)
            .map(|c| {
                format!(
                    r#"{{"cookie":{c},"owner":":1.1","app_id":"é","kind":"idle","reason":"r"}}"#
                )
            })
            .collect();
        fs::write(
            &path,
            format!(r#"{{"version":1,"inhibitors":[{}]}}"#, list.join(",")),
        )
        .unwrap();
        let got = load(&path).unwrap();
        assert_eq!(got.len(), MAX_ENTRIES);

        let one = format!(
            r#"{{"version":1,"inhibitors":[{{"cookie":1,"owner":":1.1","app_id":"{}","kind":"idle","reason":"r"}}]}}"#,
            "é".repeat(300)
        );
        fs::write(&path, one).unwrap();
        let got = load(&path).unwrap();
        assert_eq!(got[0].1.app_id.len(), MAX_TEXT);
    }
}
