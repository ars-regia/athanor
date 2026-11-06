//! `inhibitors.json` (`doc_session.md` SN7): the inhibitor table, kept across a restart.

use crate::inhibitors::{Inhibitor, Inhibitors, Kind};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
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

/// Writes the table through a temporary file and a rename, mode 0600.
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
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    out.write_all(&json)?;
    out.sync_all()?;
    fs::rename(&tmp, path)
}

/// Reads the saved entries. No file is an empty list; a file that does not parse (or has
/// another version) is removed and reported as an error, and the caller starts empty (SN7).
pub fn load(path: &Path) -> io::Result<Vec<(u32, Inhibitor)>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<File>(&bytes) {
        Ok(f) if f.version == 1 => Ok(f
            .inhibitors
            .into_iter()
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
                        app_id: e.app_id,
                        reason: e.reason,
                    },
                )
            })
            .collect()),
        parsed => {
            fs::remove_file(path)?;
            let why = match parsed {
                Ok(f) => format!("unknown version {}", f.version),
                Err(e) => e.to_string(),
            };
            Err(io::Error::new(io::ErrorKind::InvalidData, why))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("athanor-idle-{name}-{}", std::process::id()));
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
}
