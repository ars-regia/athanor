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
use crate::theme;
use athanor_style::appearance::Appearance;
use std::fs;
use std::io;
use std::path::Path;
use toml::{Table, Value};

/// The record, in the store's directory.
pub const RECORD: &str = "import.toml";

/// Where the imported values come from.
const FROM: &str = "cosmic";

/// What one family's import does under the store's lock, reading the user's COSMIC directory and
/// the built themes' file, returning its outcome.
type Family = (
    &'static str,
    fn(&store::Locked<'_>, Option<&Path>, &Path) -> Result<&'static str, Error>,
);

/// A family's outcome in the record (`doc_own_stack.md` decision 38).
const OUTCOMES: [&str; 4] = ["imported", "absent", "kept", "refused"];

/// The appearance family: `appearance.toml`, in its own subdirectory, started from the mode,
/// contrast and accent of COSMIC's built theme, which the outline was drawn with before the
/// file existed. `kept` when the file exists (a file is never overwritten), `refused` when it
/// exists and cannot be read, or when the user's COSMIC keys are none that Athanor ships (keys
/// that match no variant, or an accent with no mode; the outline keeps its factory look until
/// the user changes the appearance), `absent` when the user has no theme keys of their own, or
/// the themes are not installed. The user's `active_hint` is not imported (ADR-0150
/// point 6, question 6): the width is Calmo's.
fn appearance(
    locked: &store::Locked<'_>,
    cosmic: Option<&Path>,
    variants: &Path,
) -> Result<&'static str, Error> {
    let store = locked.store().sub(theme::DIR);
    // Nothing is created when there is nothing to import: the directory is made by the lock.
    let present = fs::symlink_metadata(store.dir().join(theme::FILE)).is_ok();
    let pending = if present {
        None
    } else {
        match source(cosmic, variants)? {
            Ok(appearance) => Some(appearance),
            Err(outcome) => return Ok(outcome),
        }
    };
    let held = store.lock()?;
    match (held.read(theme::FILE), pending) {
        (Ok(Some(_)), _) => Ok("kept"),
        (Ok(None), Some(appearance)) => {
            theme::write_imported(&held, &appearance)?;
            Ok("imported")
        }
        (Ok(None), None) => Ok("absent"),
        (Err(err @ Error::Locked { .. }), _) => Err(err),
        (Err(err), _) => {
            tracing::warn!(%err, "appearance.toml cannot be read: the family is refused");
            Ok("refused")
        }
    }
}

/// The appearance of COSMIC's state, or the outcome that says why there is none.
fn source(
    cosmic: Option<&Path>,
    variants: &Path,
) -> Result<Result<Appearance, &'static str>, Error> {
    let variants = match fs::read_to_string(variants) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Err("absent")),
        Err(err) => {
            tracing::warn!(path = ?variants, %err, "the themes' file cannot be read: the family is refused");
            return Ok(Err("refused"));
        }
    };
    match theme::read_cosmic_in(
        &cosmic
            .map(Path::to_path_buf)
            .into_iter()
            .collect::<Vec<_>>(),
        &variants,
    ) {
        Ok(Some(appearance)) => Ok(Ok(appearance)),
        Ok(None) => Ok(Err("absent")),
        // Any other error, a key that cannot be read or is not a file, is a source the import
        // cannot use: the family is refused, and the others go on.
        Err(err) => {
            tracing::warn!(%err, "COSMIC's appearance cannot be read: the family is refused");
            Ok(Err("refused"))
        }
    }
}

/// The families the import knows: the keyboard and the magnifier come with their writer modules
/// (ADR-0150 point 6; the plan's P2 and P3).
const FAMILIES: &[Family] = &[
    ("bindings", |locked, cosmic, _| {
        crate::shortcuts::import(locked, cosmic)
    }),
    ("appearance", appearance),
];

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

/// `run`, reading the COSMIC directory `cosmic` and the built themes of the image.
fn run_from(store: &Store, cosmic: Option<&Path>) -> Result<Ran, Error> {
    run_with(store, cosmic, Path::new(theme::VARIANTS))
}

/// `run`, reading the COSMIC directory `cosmic` and the themes' file `variants`.
fn run_with(store: &Store, cosmic: Option<&Path>, variants: &Path) -> Result<Ran, Error> {
    let locked = store.lock()?;
    let record = locked.read(RECORD)?;
    let listed = match &record {
        Some(record) => families(store, record)?,
        None => Table::new(),
    };
    let mut outcomes = Vec::new();
    for (name, import) in FAMILIES {
        if !listed.contains_key(*name) {
            outcomes.push((*name, import(&locked, cosmic, variants)?));
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
    use std::path::{Path, PathBuf};
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

    /// Where the appearance import reads COSMIC's state from, in the tests.
    struct Sources {
        cosmic: Vec<PathBuf>,
        variants: PathBuf,
    }

    fn run_s(store: &Store, sources: &Sources) -> Result<Ran, Error> {
        run_with(
            store,
            sources.cosmic.first().map(PathBuf::as_path),
            &sources.variants,
        )
    }

    /// Two accents in both modes and contrasts, each theme naming its variant.
    fn variants() -> String {
        let mut modes = serde_json::Map::new();
        for mode in ["dark", "light"] {
            let mut contrasts = serde_json::Map::new();
            for contrast in ["normal", "high"] {
                let mut accents = serde_json::Map::new();
                for accent in ["purple", "teal"] {
                    let theme = serde_json::json!({
                        // As the shipped file: the accent's text is the same for both contrasts.
                        "accent": format!("{mode} {accent}"),
                        "is_high_contrast": (contrast == "high").to_string(),
                    });
                    accents.insert(accent.into(), theme);
                }
                contrasts.insert(contrast.into(), accents.into());
            }
            modes.insert(mode.into(), contrasts.into());
        }
        serde_json::Value::from(modes).to_string()
    }

    /// COSMIC's state in `dir`: a light, high-contrast, teal theme, with the themes' file.
    fn cosmic_state(dir: &Path) -> (Sources, Appearance) {
        let choice = Appearance {
            dark: false,
            high_contrast: true,
            accent: athanor_style::appearance::Accent::Teal,
            ..Appearance::default()
        };
        let cosmic = dir.join("cosmic");
        theme::write_cosmic_in(&cosmic, &variants(), &choice).expect("COSMIC's keys");
        let file = dir.join("cosmic-variants.json");
        fs::write(&file, variants()).expect("variants");
        (
            Sources {
                cosmic: vec![cosmic],
                variants: file,
            },
            choice,
        )
    }

    /// The upgrade: an absent `appearance.toml` is started from COSMIC's state, once, and the
    /// record says so; a file that exists is kept as it is; a second start changes nothing.
    #[test]
    fn the_appearance_is_imported_once_and_an_existing_file_is_kept() {
        let dir = scratch("import-appearance");
        let (sources, choice) = cosmic_state(&dir);
        let store = Store::at(dir.join("athanor/sulphur"));
        let file = store.dir().join(theme::DIR).join(theme::FILE);
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        let expected = Store::at(dir.join("expected"));
        theme::write_store(&expected, &choice).expect("expected");
        let text = fs::read_to_string(&file).expect("imported");
        assert_eq!(
            text,
            fs::read_to_string(expected.dir().join(theme::FILE)).expect("expected"),
            "light, high contrast, teal"
        );
        assert!(text.contains("is_dark = false") && text.contains("divider = \"#858797ff\""));
        assert!(
            !store.dir().join(theme::FILE).exists(),
            "not in the store's directory"
        );
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("imported"));

        // A second start changes nothing, the file included.
        let before = fs::metadata(&file)
            .expect("meta")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Done)));
        assert_eq!(fs::read_to_string(&file).expect("kept"), text);
        assert_eq!(
            fs::metadata(&file)
                .expect("meta")
                .modified()
                .expect("mtime"),
            before
        );

        // A file the user's own, with a record lost: kept, never overwritten.
        let user = "schema = 1\nis_dark = true\nactive_hint = 9\n";
        fs::write(&file, user).expect("user's file");
        fs::remove_file(store.dir().join(RECORD)).expect("record");
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        assert_eq!(fs::read_to_string(&file).expect("kept"), user);
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("kept"));

        // One that cannot be read is refused, and left as it is.
        fs::write(&file, "not toml [").expect("broken file");
        fs::remove_file(store.dir().join(RECORD)).expect("record");
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        assert_eq!(fs::read_to_string(&file).expect("kept"), "not toml [");
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("refused"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A theme that none of Athanor's built ones matches, set elsewhere, is refused: nothing is
    /// written and the outline keeps its factory look until the appearance is changed.
    #[test]
    fn a_custom_cosmic_theme_is_refused_and_writes_no_file() {
        let dir = scratch("import-custom");
        let (sources, _) = cosmic_state(&dir);
        let theme_key = crate::cosmic_config::versioned(&sources.cosmic[0], theme::LIGHT, 2);
        fs::write(theme_key.join("accent"), "(base: (red: 1.0))").expect("custom accent");
        let store = Store::at(dir.join("sulphur"));
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("refused"));
        assert!(!store.dir().join(theme::DIR).exists());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A user with no theme keys of their own has nothing to import: the family is `absent`
    /// and no file is written; keys that match no variant stay `refused`.
    #[test]
    fn a_user_without_theme_keys_is_absent_and_one_with_foreign_keys_is_refused() {
        let dir = scratch("import-fresh");
        let (mut sources, _) = cosmic_state(&dir);
        sources.cosmic = vec![dir.join("no-user-keys")];
        let store = Store::at(dir.join("sulphur"));
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("absent"));
        assert!(!store.dir().join(theme::DIR).join(theme::FILE).exists());
        // An accent and no mode is a theme of the user's that is none of ours.
        let (sources, _) = cosmic_state(&scratch("import-accent-only"));
        let mode = crate::cosmic_config::versioned(&sources.cosmic[0], theme::MODE, 1);
        fs::remove_file(mode.join("is_dark")).expect("remove");
        let store = Store::at(dir.join("sulphur-2"));
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("refused"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A COSMIC key that is a FIFO is an unusable source: the family is refused, the import
    /// does not fail, and the record is written.
    #[test]
    fn an_unreadable_cosmic_key_refuses_the_family_and_the_import_goes_on() {
        let dir = scratch("import-fifo");
        let (sources, _) = cosmic_state(&dir);
        let accent =
            crate::cosmic_config::versioned(&sources.cosmic[0], theme::LIGHT, 2).join("accent");
        fs::remove_file(&accent).expect("remove");
        assert!(Command::new("mkfifo")
            .arg(&accent)
            .status()
            .expect("mkfifo")
            .success());
        let store = Store::at(dir.join("sulphur"));
        assert!(matches!(run_s(&store, &sources), Ok(Ran::Recorded)));
        let table: Table = toml::from_str(&record(store.dir())).expect("toml");
        assert_eq!(table["families"]["appearance"].as_str(), Some("refused"));
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
