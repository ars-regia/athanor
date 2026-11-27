//! The writer of `windows.toml` (`doc_own_stack.md` decision 38, SE14): the one module that
//! writes the file, through the store's lock and atomic write (`store`). Of SE14's keys the file
//! holds those Sulphur applies, the README's list; a key or a table the list does not name,
//! already in the file, is kept (ADR-0150 point 3).

use crate::input::{check, Accepts};
use crate::store::{Error, Store};
use toml::Value;

/// The file, in the store's directory.
pub const FILE: &str = "windows.toml";

/// Each top-level key and what it accepts: the table of
/// `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/README.md`, which a test compares.
pub(crate) const KEYS: &[(&str, Accepts)] = &[("active_hint", Accepts::Boolean)];

/// Sets `key` to `value`, or removes it with `None`, so that the default applies. A key the
/// file does not take, or a value the key does not accept, is refused and nothing is written.
pub fn set(store: &Store, key: &str, value: Option<Value>) -> Result<(), Error> {
    store.update(FILE, |path, table| {
        let refuse = |reason: String| Error::Refused {
            path: path.to_owned(),
            reason,
        };
        let Some(&(_, accepts)) = KEYS.iter().find(|(name, _)| *name == key) else {
            return Err(refuse(format!("{key} is not a key of {FILE}")));
        };
        match value {
            Some(value) => {
                check(accepts, &value).map_err(|why| refuse(format!("{key}: {why}")))?;
                table.insert(key.to_owned(), value);
            }
            None => {
                table.remove(key);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::tests::{accepts, readme};
    use crate::store::tests::scratch;
    use std::collections::BTreeSet;
    use std::fs;
    use toml::Table;

    #[test]
    fn the_hint_is_written_and_unknown_keys_kept() {
        let dir = scratch("windows-set");
        fs::write(
            dir.join(FILE),
            "schema = 1\nworkspace_wraparound = true\n[overview]\nlayout = \"rows\"\n",
        )
        .expect("seed");
        let store = Store::at(dir.clone());
        set(&store, "active_hint", Some(false.into())).expect("hint");
        let table: Table =
            toml::from_str(&fs::read_to_string(dir.join(FILE)).expect("read")).expect("toml");
        assert_eq!(table["active_hint"].as_bool(), Some(false));
        assert_eq!(table["workspace_wraparound"].as_bool(), Some(true));
        assert_eq!(table["overview"]["layout"].as_str(), Some("rows"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_key_or_a_value_out_of_the_list_is_refused() {
        let dir = scratch("windows-refused");
        let store = Store::at(dir.clone());
        for (key, value) in [
            ("active_hint", Value::Integer(0)),
            ("edge_snap_threshold", Value::Integer(8)),
        ] {
            let result = set(&store, key, Some(value));
            assert!(
                matches!(result, Err(Error::Refused { .. })),
                "{key}: {result:?}"
            );
        }
        assert!(!dir.join(FILE).exists(), "nothing written");
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// `KEYS` is the README's list of `windows.toml`'s keys, both ways.
    #[test]
    fn the_keys_are_the_readmes() {
        let listed: BTreeSet<String> = readme()
            .into_iter()
            .filter(|(_, places, _)| places.contains("`windows.toml`"))
            .map(|(key, _, values)| format!("{key} {:?}", accepts(&values)))
            .collect();
        let ours: BTreeSet<String> = KEYS
            .iter()
            .map(|(key, accepts)| format!("{key} {accepts:?}"))
            .collect();
        assert_eq!(ours, listed);
    }
}
