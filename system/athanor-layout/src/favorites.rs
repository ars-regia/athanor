//! The favourites of the bar and the dock (doc_bar.md, BR7): `~/.config/athanor/favorites.toml`,
//! a schema and a list of desktop ids. They are not part of the layout document. At the
//! first start, when the file is absent, they are imported once from COSMIC's list, else
//! from the vendor list; afterwards the file is the only source.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::atomic::write_atomically;
use crate::user::write_target;

pub const SCHEMA: i64 = 1;
pub const VENDOR_FILE: &str = "/usr/share/athanor/favorites.toml";
/// More than a bar or a dock can show; a longer list is a broken or hostile file.
pub const MAX_FAVORITES: usize = 64;
/// NAME_MAX: a longer id cannot be a file name.
pub const MAX_ID_BYTES: usize = 255;
/// 64 ids of 255 bytes with their quoting fit well within this.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum FavoritesError {
    Unreadable(String),
    Unwritable(String),
    Malformed(String),
    NewerSchema(i64),
    UnknownKey(String),
    BadId(String),
    TooMany(usize),
}

impl fmt::Display for FavoritesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FavoritesError::Unreadable(reason) => write!(f, "cannot read the favourites: {reason}"),
            FavoritesError::Unwritable(reason) => {
                write!(f, "cannot write the favourites: {reason}")
            }
            FavoritesError::Malformed(reason) => {
                write!(f, "the favourites file is malformed: {reason}")
            }
            FavoritesError::NewerSchema(schema) => {
                write!(
                    f,
                    "the favourites file has schema {schema}, newer than {SCHEMA}"
                )
            }
            FavoritesError::UnknownKey(key) => {
                write!(f, "the favourites file has an unknown key `{key}`")
            }
            FavoritesError::BadId(id) => write!(f, "`{id}` is not a desktop id"),
            FavoritesError::TooMany(count) => {
                write!(
                    f,
                    "{count} favourites, more than the {MAX_FAVORITES} allowed"
                )
            }
        }
    }
}

impl Error for FavoritesError {}

pub fn user_file(config_home: &Path) -> PathBuf {
    config_home.join("athanor/favorites.toml")
}

/// A desktop file name (Desktop Entry Specification, "Desktop File ID") with no path
/// separator: letters, digits, `.`, `_` and `-`, ending in `.desktop`, at most NAME_MAX.
pub fn is_desktop_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES
        && id.strip_suffix(".desktop").is_some_and(|stem| {
            !stem.is_empty()
                && stem
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
}

/// The schema is read first, so a newer file is reported as newer, whatever else it holds.
pub fn parse(text: &str) -> Result<Vec<String>, FavoritesError> {
    let table: Table = text
        .parse()
        .map_err(|err: toml::de::Error| FavoritesError::Malformed(err.message().to_owned()))?;
    match table.get("schema") {
        Some(Value::Integer(SCHEMA)) => {}
        Some(Value::Integer(schema)) if *schema > SCHEMA => {
            return Err(FavoritesError::NewerSchema(*schema))
        }
        _ => {
            return Err(FavoritesError::Malformed(format!(
                "`schema` must be {SCHEMA}"
            )))
        }
    }
    if let Some(key) = table
        .keys()
        .find(|key| !matches!(key.as_str(), "schema" | "favorites"))
    {
        return Err(FavoritesError::UnknownKey(key.clone()));
    }
    let Some(Value::Array(items)) = table.get("favorites") else {
        return Err(FavoritesError::Malformed(
            "`favorites` must be a list".to_owned(),
        ));
    };
    if items.len() > MAX_FAVORITES {
        return Err(FavoritesError::TooMany(items.len()));
    }
    let mut ids: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(id) = item else {
            return Err(FavoritesError::Malformed(
                "favourites are strings".to_owned(),
            ));
        };
        if !is_desktop_id(id) {
            return Err(FavoritesError::BadId(id.escape_debug().to_string()));
        }
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    Ok(ids)
}

pub fn to_toml(ids: &[String]) -> String {
    let list = Value::Array(ids.iter().cloned().map(Value::String).collect());
    format!("schema = {SCHEMA}\nfavorites = {list}\n")
}

/// `Ok(None)` when the file does not exist. The read stops one byte past the bound.
pub fn read(path: &Path) -> Result<Option<Vec<String>>, FavoritesError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(FavoritesError::Unreadable(format!(
                "{}: {err}",
                path.display()
            )))
        }
    };
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| match err.kind() {
            io::ErrorKind::InvalidData => FavoritesError::Malformed("not UTF-8".to_owned()),
            _ => FavoritesError::Unreadable(format!("{}: {err}", path.display())),
        })?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(FavoritesError::Malformed(format!(
            "larger than {MAX_FILE_BYTES} bytes"
        )));
    }
    parse(&text).map(Some)
}

/// Validates, then writes through a link the way the layout document is written, so a
/// file kept in a dotfiles repository stays a link.
pub fn save(path: &Path, ids: &[String]) -> Result<(), FavoritesError> {
    if ids.len() > MAX_FAVORITES {
        return Err(FavoritesError::TooMany(ids.len()));
    }
    if let Some(bad) = ids.iter().find(|id| !is_desktop_id(id)) {
        return Err(FavoritesError::BadId(bad.escape_debug().to_string()));
    }
    let unwritable =
        |err: io::Error| FavoritesError::Unwritable(format!("{}: {err}", path.display()));
    let target = write_target(path).map_err(unwritable)?;
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(unwritable)?;
    }
    write_atomically(&target, &to_toml(ids)).map_err(unwritable)
}

/// The favourites at start. An existing file wins. When it is absent: COSMIC's list, else
/// the vendor list, else nothing, saved once so the import never runs again. A save that
/// fails is logged and the list is still returned; the import then runs again at the next
/// start. A file that exists but is rejected is returned as the error and never replaced.
pub fn load_or_import(
    path: &Path,
    cosmic: impl FnOnce() -> Option<Vec<String>>,
    vendor: &Path,
) -> Result<Vec<String>, FavoritesError> {
    if let Some(ids) = read(path)? {
        return Ok(ids);
    }
    let ids = match cosmic() {
        Some(ids) => sanitized(ids),
        None => match read(vendor) {
            Ok(Some(ids)) => ids,
            Ok(None) => Vec::new(),
            Err(err) => {
                tracing::error!(error = %err, file = %vendor.display(), "the vendor favourites are unusable");
                Vec::new()
            }
        },
    };
    // Under the same lock `update` takes: a concurrent first start (the dock of 2c) may have
    // imported and saved while this one was reading COSMIC's or the vendor's list, so the
    // file is re-checked once the lock is held, and that import wins over this one's.
    let saved = lock_writer(path).and_then(|_lock| match read(path)? {
        Some(existing) => Ok(Some(existing)),
        None => {
            save(path, &ids)?;
            Ok(None)
        }
    });
    match saved {
        Ok(Some(existing)) => Ok(existing),
        Ok(None) => Ok(ids),
        Err(err) => {
            tracing::warn!(error = %err, "the imported favourites were not saved; the import runs again at the next start");
            Ok(ids)
        }
    }
}

/// COSMIC's list as this file accepts it: desktop ids only, once each, at most the bound.
fn sanitized(ids: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for id in ids {
        if kept.len() == MAX_FAVORITES {
            break;
        }
        if is_desktop_id(&id) && !kept.contains(&id) {
            kept.push(id);
        }
    }
    kept
}

/// `ids` with `id` at the end; unchanged when it is already there.
pub fn pinned(ids: &[String], id: &str) -> Result<Vec<String>, FavoritesError> {
    if !is_desktop_id(id) {
        return Err(FavoritesError::BadId(id.escape_debug().to_string()));
    }
    if ids.iter().any(|known| known == id) {
        return Ok(ids.to_vec());
    }
    if ids.len() >= MAX_FAVORITES {
        return Err(FavoritesError::TooMany(ids.len() + 1));
    }
    let mut out = ids.to_vec();
    out.push(id.to_owned());
    Ok(out)
}

pub fn unpinned(ids: &[String], id: &str) -> Vec<String> {
    ids.iter().filter(|known| *known != id).cloned().collect()
}

/// `ids` with `dragged` moved to the place `target` holds (doc_bar.md, BR7: dragging
/// reorders the favourites). Unchanged when either id is absent: a drop can carry any
/// string another client offers.
pub fn moved(ids: &[String], dragged: &str, target: &str) -> Vec<String> {
    let (Some(from), Some(to)) = (
        ids.iter().position(|id| id == dragged),
        ids.iter().position(|id| id == target),
    ) else {
        return ids.to_vec();
    };
    let mut out = ids.to_vec();
    let id = out.remove(from);
    out.insert(to, id);
    out
}

/// The exclusive lock every writer of the file takes (see `update`): opens, creating if
/// needed, `.favorites.toml.lock` beside `path`'s target, creating the target's directory
/// first, and blocks until it is held. The lock is a separate file: the file itself is
/// replaced by a rename on every save, so a lock on it would lock the old inode. Dropping
/// the returned file releases the lock.
fn lock_writer(path: &Path) -> Result<fs::File, FavoritesError> {
    let unwritable =
        |err: io::Error| FavoritesError::Unwritable(format!("{}: {err}", path.display()));
    let target = write_target(path).map_err(unwritable)?;
    let dir = target.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(unwritable)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".favorites.toml.lock"))
        .map_err(unwritable)?;
    // ponytail: a blocking lock on the GTK thread; writers hold it for one small file's
    // read and rename. A try_lock with a retry on idle if a writer is ever seen holding it long.
    lock.lock().map_err(unwritable)?;
    Ok(lock)
}

/// Reads the file, applies `change` and saves the result, under an exclusive lock that every
/// writer of the file takes (the bar, the dock of 2c): two writers never lose each other's
/// change. An absent file reads as an empty list; a rejected one is returned as the error and
/// left untouched.
pub fn update(
    path: &Path,
    change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>,
) -> Result<Vec<String>, FavoritesError> {
    let _lock = lock_writer(path)?;
    let current = read(path)?.unwrap_or_default();
    let ids = change(&current)?;
    save(path, &ids)?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scratch;
    use std::cell::Cell;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn the_documented_file_parses_in_order_without_duplicates() {
        let text = "schema = 1\nfavorites = [\"org.mozilla.firefox.desktop\", \"org.gnome.Ptyxis.desktop\", \"org.mozilla.firefox.desktop\"]\n";
        assert_eq!(
            parse(text),
            Ok(ids(&[
                "org.mozilla.firefox.desktop",
                "org.gnome.Ptyxis.desktop"
            ]))
        );
    }

    #[test]
    fn the_vendor_file_of_the_bar_parses() {
        let text =
            include_str!("../../../forge/specs/athanor-bar/athanor-bar-1.0.0/data/favorites.toml");
        assert_eq!(parse(text).expect("the vendor favourites").len(), 4);
    }

    #[test]
    fn the_schema_is_checked_first() {
        assert_eq!(
            parse("schema = 2\nfavorites = 7\nextra = 1\n"),
            Err(FavoritesError::NewerSchema(2))
        );
        for text in [
            "favorites = []\n",
            "schema = \"1\"\nfavorites = []\n",
            "schema = 0\nfavorites = []\n",
        ] {
            assert!(
                matches!(parse(text), Err(FavoritesError::Malformed(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn unknown_keys_and_wrong_types_are_refused() {
        assert_eq!(
            parse("schema = 1\nfavorites = []\norder = 3\n"),
            Err(FavoritesError::UnknownKey("order".to_owned()))
        );
        for text in [
            "schema = 1\n",
            "schema = 1\nfavorites = \"a.desktop\"\n",
            "schema = 1\nfavorites = [1]\n",
            "not toml at all [",
        ] {
            assert!(
                matches!(parse(text), Err(FavoritesError::Malformed(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn ids_that_are_not_desktop_ids_are_refused() {
        let long = format!("{}.desktop", "x".repeat(MAX_ID_BYTES));
        for id in [
            "../../etc/passwd.desktop",
            "a b.desktop",
            "firefox",
            ".desktop",
            "a/b.desktop",
            long.as_str(),
        ] {
            assert!(!is_desktop_id(id), "{id:?}");
            let text = format!(
                "schema = 1\nfavorites = [{}]\n",
                toml::Value::String(id.to_owned())
            );
            assert!(
                matches!(parse(&text), Err(FavoritesError::BadId(_))),
                "{id:?}"
            );
        }
        assert!(is_desktop_id("org.mozilla.firefox.desktop"));
        assert!(is_desktop_id("steam_app-570.desktop"));
    }

    #[test]
    fn more_than_the_bound_is_refused() {
        let many: Vec<String> = (0..=MAX_FAVORITES)
            .map(|n| format!("app{n}.desktop"))
            .collect();
        assert_eq!(
            parse(&to_toml(&many)),
            Err(FavoritesError::TooMany(MAX_FAVORITES + 1))
        );
        assert_eq!(
            save(&scratch("too-many").join("favorites.toml"), &many),
            Err(FavoritesError::TooMany(MAX_FAVORITES + 1))
        );
    }

    #[test]
    fn written_lists_read_back_unchanged() {
        let list = ids(&["a-b_c.desktop", "org.example.App.desktop"]);
        assert_eq!(parse(&to_toml(&list)), Ok(list.clone()));
        let path = scratch("round-trip").join("athanor/favorites.toml");
        save(&path, &list).expect("save creates the directory");
        assert_eq!(read(&path), Ok(Some(list)));
    }

    #[test]
    fn an_absent_file_reads_as_none() {
        assert_eq!(read(&scratch("absent").join("favorites.toml")), Ok(None));
    }

    #[test]
    fn a_file_over_the_size_bound_is_malformed() {
        let path = scratch("large").join("favorites.toml");
        let padding = "#".repeat(usize::try_from(MAX_FILE_BYTES).expect("fits"));
        std::fs::write(&path, format!("schema = 1\nfavorites = []\n{padding}\n")).expect("write");
        assert!(matches!(read(&path), Err(FavoritesError::Malformed(_))));
    }

    #[test]
    fn an_existing_file_wins_and_cosmic_is_not_asked() {
        let path = scratch("existing").join("favorites.toml");
        save(&path, &ids(&["mine.desktop"])).expect("save");
        let asked = Cell::new(false);
        let found = load_or_import(
            &path,
            || {
                asked.set(true);
                Some(ids(&["cosmic.desktop"]))
            },
            Path::new("/nonexistent"),
        );
        assert_eq!(found, Ok(ids(&["mine.desktop"])));
        assert!(!asked.get());
    }

    #[test]
    fn the_first_start_imports_cosmic_filtered_and_saves_it() {
        let path = scratch("import").join("athanor/favorites.toml");
        let found = load_or_import(
            &path,
            || {
                Some(ids(&[
                    "a.desktop",
                    "a.desktop",
                    "../x.desktop",
                    "b.desktop",
                ]))
            },
            Path::new("/nonexistent"),
        );
        assert_eq!(found, Ok(ids(&["a.desktop", "b.desktop"])));
        assert_eq!(read(&path), Ok(Some(ids(&["a.desktop", "b.desktop"]))));
    }

    #[test]
    fn a_concurrent_first_start_does_not_overwrite_the_other_writers_import() {
        let path = scratch("concurrent-import").join("favorites.toml");
        let found = load_or_import(
            &path,
            || {
                // Another writer's own first-start import lands on disk while this one is
                // still asking COSMIC for its list; the lock, taken after, must see it.
                save(&path, &ids(&["other.desktop"])).expect("the other writer saves first");
                Some(ids(&["mine.desktop"]))
            },
            Path::new("/nonexistent"),
        );
        assert_eq!(found, Ok(ids(&["other.desktop"])));
        assert_eq!(read(&path), Ok(Some(ids(&["other.desktop"]))));
    }

    #[test]
    fn without_cosmic_the_vendor_list_then_nothing() {
        let dir = scratch("vendor");
        let vendor = dir.join("vendor.toml");
        save(&vendor, &ids(&["vendor.desktop"])).expect("save");
        assert_eq!(
            load_or_import(&dir.join("one/favorites.toml"), || None, &vendor),
            Ok(ids(&["vendor.desktop"]))
        );
        std::fs::write(&vendor, "schema = 9\n").expect("write");
        assert_eq!(
            load_or_import(&dir.join("two/favorites.toml"), || None, &vendor),
            Ok(Vec::new())
        );
        assert_eq!(
            load_or_import(
                &dir.join("three/favorites.toml"),
                || None,
                &dir.join("absent.toml")
            ),
            Ok(Vec::new())
        );
    }

    #[test]
    fn a_rejected_file_is_an_error_and_is_left_untouched() {
        let path = scratch("rejected").join("favorites.toml");
        let text = "schema = 2\nfavorites = [\"future.desktop\"]\n";
        std::fs::write(&path, text).expect("write");
        assert_eq!(
            load_or_import(
                &path,
                || Some(ids(&["cosmic.desktop"])),
                Path::new("/nonexistent")
            ),
            Err(FavoritesError::NewerSchema(2))
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), text);
    }

    #[test]
    fn a_dragged_favourite_takes_the_place_of_its_target() {
        let list = ids(&["a", "b", "c"]);
        assert_eq!(moved(&list, "c", "a"), ids(&["c", "a", "b"]));
        assert_eq!(moved(&list, "a", "c"), ids(&["b", "c", "a"]));
        assert_eq!(moved(&list, "b", "c"), ids(&["a", "c", "b"]));
    }

    #[test]
    fn a_move_with_an_unknown_id_changes_nothing() {
        let list = ids(&["a", "b", "c"]);
        assert_eq!(moved(&list, "x", "a"), list);
        assert_eq!(moved(&list, "a", "x"), list);
        assert_eq!(moved(&list, "a", "a"), list);
        assert_eq!(moved(&[], "a", "b"), Vec::<String>::new());
    }

    #[test]
    fn pin_appends_once_and_unpin_removes() {
        let list = ids(&["a.desktop"]);
        assert_eq!(
            pinned(&list, "b.desktop"),
            Ok(ids(&["a.desktop", "b.desktop"]))
        );
        assert_eq!(pinned(&list, "a.desktop"), Ok(list.clone()));
        assert!(matches!(
            pinned(&list, "../b.desktop"),
            Err(FavoritesError::BadId(_))
        ));
        let full: Vec<String> = (0..MAX_FAVORITES)
            .map(|n| format!("app{n}.desktop"))
            .collect();
        assert_eq!(
            pinned(&full, "one-more.desktop"),
            Err(FavoritesError::TooMany(MAX_FAVORITES + 1))
        );
        assert_eq!(
            unpinned(&ids(&["a.desktop", "b.desktop"]), "a.desktop"),
            ids(&["b.desktop"])
        );
    }

    #[test]
    fn two_writers_pinning_at_once_lose_neither_change() {
        let dir = scratch("two-writers");
        let path = dir.join("favorites.toml");
        save(&path, &[]).expect("seed");
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    update(&path, |ids| pinned(ids, &format!("app{n}.desktop")))
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("joins").expect("updates");
        }
        let mut ids = read(&path).expect("reads").expect("exists");
        ids.sort();
        assert_eq!(
            ids,
            (0..8)
                .map(|n| format!("app{n}.desktop"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_rejected_file_is_not_updated() {
        let dir = scratch("update-rejected");
        let path = dir.join("favorites.toml");
        std::fs::write(&path, "schema = 2\n").expect("write");
        assert!(update(&path, |ids| pinned(ids, "a.desktop")).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "schema = 2\n"
        );
    }
}
