//! The import of the user's COSMIC preferences into the store, once per user (ADR-0150 points
//! 6 to 8), run by `athanor-preferences-import.service` before Sulphur. It never writes the
//! COSMIC files. Its record, `import.toml` in the store's directory, is written last, with
//! `schema = 1`, `from = "cosmic"` and, in `[families]`, each family's outcome: `imported`,
//! `absent`, `kept` or `refused`. A record that lists every family ends the import at once; a
//! family it does not list is imported alone and the record rewritten with it.
//!
//! A record that cannot be read (not TOML, without `schema` or of a newer one, not a regular
//! file, a link, larger than the bound) makes the import fail and write nothing: removing it
//! imports again (point 8).
//!
//! Each family's import reads its COSMIC sources with `cosmic_config`'s reader, which opens
//! them without blocking, refuses what is not a regular file, stops at its bound, and follows
//! a symbolic link, as a user's dotfiles managed by links need.

use crate::store::{self, Error, Store};
use std::path::Path;
use toml::{Table, Value};

/// The record, in the store's directory.
pub const RECORD: &str = "import.toml";

/// Where the imported values come from.
const FROM: &str = "cosmic";

/// What one family's import does under the store's lock, reading the user's COSMIC directory,
/// returning its outcome.
type Family = (
    &'static str,
    fn(&store::Locked<'_>, Option<&Path>) -> Result<&'static str, Error>,
);

/// A family's outcome in the record (`doc_own_stack.md` decision 38).
const OUTCOMES: [&str; 4] = ["imported", "absent", "kept", "refused"];

/// The families the import knows: the keyboard and the magnifier come with their writer modules
/// (ADR-0150 point 6; the plan's P2 and P3).
const FAMILIES: &[Family] = &[("bindings", crate::shortcuts::import)];

/// What a run did.
#[derive(Debug, PartialEq, Eq)]
pub enum Ran {
    /// The record listed every family: nothing was read or written.
    Done,
    /// The families the record did not list were imported, and the record written.
    Recorded,
}

/// Runs the import into `store`, under its lock, from the user's COSMIC directory.
pub fn run(store: &Store) -> Result<Ran, Error> {
    run_from(store, crate::cosmic_config::user_dir().as_deref())
}

/// `run`, reading the COSMIC directory `cosmic`.
fn run_from(store: &Store, cosmic: Option<&Path>) -> Result<Ran, Error> {
    let locked = store.lock()?;
    let record = locked.read(RECORD)?;
    let listed = match &record {
        Some(record) => families(store, record)?,
        None => Table::new(),
    };
    let mut outcomes = Vec::new();
    for (name, import) in FAMILIES {
        if !listed.contains_key(*name) {
            outcomes.push((*name, import(&locked, cosmic)?));
        }
    }
    if record.is_some() && outcomes.is_empty() {
        return Ok(Ran::Done);
    }
    locked.update(RECORD, |path, table| {
        table.insert("from".to_owned(), Value::String(FROM.to_owned()));
        let families = store::table_mut(path, table, "families")?;
        for (name, outcome) in outcomes {
            families.insert(name.to_owned(), Value::String(outcome.to_owned()));
        }
        Ok(())
    })?;
    Ok(Ran::Recorded)
}

/// The record's `[families]`, its `from` checked.
fn families(store: &Store, record: &Table) -> Result<Table, Error> {
    let refuse = |reason: &str| Error::Refused {
        path: store.dir().join(RECORD),
        reason: reason.to_owned(),
    };
    if record.get("from").and_then(Value::as_str) != Some(FROM) {
        return Err(refuse("from is not \"cosmic\""));
    }
    let Some(Value::Table(families)) = record.get("families") else {
        return Err(refuse("no [families] table"));
    };
    let known = |outcome: &Value| outcome.as_str().is_some_and(|o| OUTCOMES.contains(&o));
    if let Some((name, _)) = families.iter().find(|(_, outcome)| !known(outcome)) {
        return Err(refuse(&format!(
            "the outcome of {name} is not one of {}",
            OUTCOMES.join(", ")
        )));
    }
    Ok(families.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{held, scratch, spawn};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::Instant;

    fn record(dir: &Path) -> String {
        fs::read_to_string(dir.join(RECORD)).expect("record")
    }

    #[test]
    fn a_first_start_writes_the_record_and_a_second_changes_nothing() {
        let dir = scratch("import-twice");
        let store = Store::at(dir.join("athanor/sulphur"));
        let cosmic = dir.join("cosmic");
        assert!(matches!(run_from(&store, Some(&cosmic)), Ok(Ran::Recorded)));
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["schema"].as_integer(), Some(1));
        assert_eq!(table["from"].as_str(), Some("cosmic"));
        assert_eq!(table["families"]["bindings"].as_str(), Some("absent"));
        let snapshot = |dir: &Path| {
            let mut entries: Vec<_> = fs::read_dir(dir)
                .expect("list")
                .map(|entry| {
                    let entry = entry.expect("entry");
                    let meta = entry.metadata().expect("metadata");
                    let text = fs::read(entry.path()).expect("read");
                    (entry.file_name(), meta.modified().expect("mtime"), text)
                })
                .collect();
            entries.sort();
            let dir_time = fs::metadata(dir).expect("dir").modified().expect("mtime");
            (entries, dir_time)
        };
        let before = snapshot(store.dir());
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(matches!(run_from(&store, Some(&cosmic)), Ok(Ran::Done)));
        assert_eq!(
            snapshot(store.dir()),
            before,
            "byte for byte and by modification time"
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A record the import cannot read is left as it is, and nothing is written.
    #[test]
    fn a_corrupt_record_fails_the_import_and_is_kept() {
        let dir = scratch("import-corrupt");
        let store = Store::at(dir.clone());
        for text in [
            "schema = 1\nfrom = \"cosmic\"\n[families\n",
            "from = \"cosmic\"\n[families]\n",
            "schema = 2\nfrom = \"cosmic\"\n[families]\n",
            "schema = 1\nfrom = \"gnome\"\n[families]\n",
            "schema = 1\nfrom = \"cosmic\"\nfamilies = 3\n",
            "schema = 1\nfrom = \"cosmic\"\n[families]\nkeyboard = \"maybe\"\n",
        ] {
            fs::write(dir.join(RECORD), text).expect("seed");
            assert!(run_from(&store, None).is_err(), "{text}");
            assert_eq!(record(&dir), text);
        }
        fs::remove_file(dir.join(RECORD)).expect("remove");
        let made = Command::new("mkfifo")
            .arg(dir.join(RECORD))
            .status()
            .expect("mkfifo");
        assert!(made.success());
        assert!(
            matches!(run_from(&store, None), Err(Error::NotRegular { .. })),
            "a FIFO, refused without blocking"
        );
        fs::remove_file(dir.join(RECORD)).expect("remove");
        let elsewhere = dir.join("elsewhere.toml");
        fs::write(&elsewhere, "schema = 1\nfrom = \"cosmic\"\n[families]\n").expect("target");
        std::os::unix::fs::symlink(&elsewhere, dir.join(RECORD)).expect("link");
        assert!(matches!(run_from(&store, None), Err(Error::Link { .. })));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// An import that cannot finish, here because a stopped process holds the lock, as the
    /// unit's timeout would stop it, writes no record, and the next start imports.
    #[test]
    fn an_import_that_cannot_lock_writes_no_record() {
        let dir = scratch("import-locked");
        let mut holder = spawn(&format!("hold {}", dir.display()));
        held(&mut holder);
        let stopped = Command::new("kill")
            .args(["-STOP", &holder.id().to_string()])
            .status()
            .expect("kill");
        assert!(stopped.success());
        let store = Store::at(dir.clone());
        let start = Instant::now();
        assert!(matches!(run_from(&store, None), Err(Error::Locked { .. })));
        assert!(start.elapsed() < store::LOCK_BOUND * 3);
        assert!(!dir.join(RECORD).exists());
        holder.kill().expect("kill the holder");
        holder.wait().expect("wait");
        assert!(matches!(run_from(&store, None), Ok(Ran::Recorded)));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The reader each family's import reads its COSMIC sources with: a symbolic link to a
    /// regular file is followed; a FIFO is refused without blocking; a file larger than the
    /// bound is read only to the bound and refused.
    #[test]
    fn a_cosmic_source_is_read_bounded_and_through_a_link() {
        let dir = scratch("import-source");
        let real = dir.join("dotfiles-xkb_config");
        fs::write(&real, "(rules: \"\")").expect("real");
        let user = dir.join("cosmic");
        let component = "com.system76.CosmicComp";
        let key = crate::cosmic_config::component(&user, component).join("xkb_config");
        fs::create_dir_all(key.parent().expect("parent")).expect("dirs");
        std::os::unix::fs::symlink(&real, &key).expect("link");
        let read =
            || crate::cosmic_config::key(std::slice::from_ref(&user), component, "xkb_config");
        assert_eq!(read().expect("followed").as_deref(), Some("(rules: \"\")"));
        fs::remove_file(&key).expect("remove");
        let made = Command::new("mkfifo").arg(&key).status().expect("mkfifo");
        assert!(made.success());
        assert!(read().is_err(), "a FIFO, refused without blocking");
        fs::remove_file(&key).expect("remove");
        fs::write(&key, vec![b' '; 1024 * 1024]).expect("large");
        assert!(read().is_err(), "larger than the bound");
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
