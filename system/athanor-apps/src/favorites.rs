//! The favourites a program shows and changes (doc_bar.md, BR7): loaded at start and
//! imported once when the file is absent, read again when its directory changes, and
//! changed only under the file's lock (`athanor_layout::favorites::update`), so the bar
//! and the dock writing at once lose neither change.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use athanor_layout::favorites::{self, FavoritesError};

enum State {
    Loaded(Vec<String>),
    /// A rejected file, a crash-loop give-up, or no configuration directory: nothing is
    /// pinned, unpinned or moved, and the file is never replaced.
    Unavailable,
}

pub struct Store {
    file: Option<PathBuf>,
    state: RefCell<State>,
}

impl Store {
    /// `file` is `None` after a crash-loop give-up (SH8) or with no configuration
    /// directory. A failed save of an import is logged by `load_or_import`, which still
    /// returns the imported list.
    pub fn load(file: Option<PathBuf>) -> Store {
        Store::load_with(
            file,
            athanor_compositor_client::favorites::cosmic_favorites,
            Path::new(favorites::VENDOR_FILE),
        )
    }

    fn load_with(
        file: Option<PathBuf>,
        cosmic: impl FnOnce() -> Option<Vec<String>>,
        vendor: &Path,
    ) -> Store {
        let state = match &file {
            None => State::Unavailable,
            Some(path) => match favorites::load_or_import(path, cosmic, vendor) {
                Ok(ids) => State::Loaded(ids),
                Err(err) => {
                    tracing::error!(error = %err, file = %path.display(), "the favourites are unavailable; the file is left as it is");
                    State::Unavailable
                }
            },
        };
        Store {
            file,
            state: RefCell::new(state),
        }
    }

    /// The favourites now; `None` while they are unavailable.
    pub fn ids(&self) -> Option<Vec<String>> {
        match &*self.state.borrow() {
            State::Loaded(ids) => Some(ids.clone()),
            State::Unavailable => None,
        }
    }

    /// Reads the file again. A file removed since start is a deliberate user act, not an
    /// error: nothing is pinned, and the next start imports again.
    pub fn reload(&self) {
        let Some(file) = &self.file else { return };
        let now = match favorites::read(file) {
            Ok(ids) => State::Loaded(ids.unwrap_or_default()),
            Err(err) => {
                tracing::error!(error = %err, file = %file.display(), "the favourites are unavailable; the file is left as it is");
                State::Unavailable
            }
        };
        self.state.replace(now);
    }

    /// Applies `change` to the file as it is on disk, under its lock, and keeps the
    /// result. `false`, with nothing written, while the favourites are unavailable or when
    /// the change fails (logged).
    pub fn change(
        &self,
        change: impl FnOnce(&[String]) -> Result<Vec<String>, FavoritesError>,
    ) -> bool {
        if matches!(*self.state.borrow(), State::Unavailable) {
            return false;
        }
        let Some(file) = &self.file else { return false };
        match favorites::update(file, change) {
            Ok(ids) => {
                self.state.replace(State::Loaded(ids));
                true
            }
            Err(err) => {
                tracing::error!(error = %err, "the favourites were not changed");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const A: &str = "org.example.A.desktop";
    const B: &str = "org.example.B.desktop";
    const C: &str = "org.example.C.desktop";
    const BROKEN: &str = "schema = 1\n[output";

    /// A fresh directory per call: tests run in parallel.
    fn scratch(name: &str) -> PathBuf {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "athanor-apps-{}-{}-{name}",
            std::process::id(),
            CALLS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("scratch");
        dir.join("favorites.toml")
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    /// A store on `file` that imports nothing: every test writes the file first.
    fn store(file: &Path) -> Store {
        Store::load_with(
            Some(file.to_path_buf()),
            || None,
            Path::new("/nonexistent/athanor-apps-vendor.toml"),
        )
    }

    #[test]
    fn the_file_is_loaded() {
        let file = scratch("loaded");
        favorites::save(&file, &ids(&[A, B])).expect("save");
        assert_eq!(store(&file).ids(), Some(ids(&[A, B])));
    }

    #[test]
    fn a_file_rejected_while_running_blocks_pinning_and_is_left_untouched() {
        let file = scratch("rejected");
        favorites::save(&file, &ids(&[A])).expect("save");
        let store = store(&file);
        fs::write(&file, BROKEN).expect("break");
        store.reload();
        assert_eq!(store.ids(), None);
        assert!(!store.change(|list| favorites::pinned(list, B)));
        assert!(!store.change(|list| Ok(favorites::moved(list, A, B))));
        assert_eq!(fs::read_to_string(&file).expect("read"), BROKEN);
        // Fixed by hand: the next change applies.
        favorites::save(&file, &ids(&[A])).expect("fix");
        store.reload();
        assert!(store.change(|list| favorites::pinned(list, B)));
        assert_eq!(favorites::read(&file).expect("read"), Some(ids(&[A, B])));
    }

    #[test]
    fn another_writers_change_is_kept_when_this_store_pins() {
        let file = scratch("two-writers");
        favorites::save(&file, &ids(&[A])).expect("save");
        let store = store(&file);
        // The bar, or another surface, pins B; this store has not reloaded yet.
        favorites::save(&file, &ids(&[A, B])).expect("other writer");
        assert!(store.change(|list| favorites::pinned(list, C)));
        assert_eq!(favorites::read(&file).expect("read"), Some(ids(&[A, B, C])));
        assert_eq!(store.ids(), Some(ids(&[A, B, C])));
    }

    #[test]
    fn without_a_file_the_favourites_are_unavailable() {
        let store = Store::load_with(
            None,
            || Some(vec![A.to_owned()]),
            Path::new("/nonexistent/athanor-apps-vendor.toml"),
        );
        assert_eq!(store.ids(), None);
        assert!(!store.change(|list| favorites::pinned(list, A)));
        store.reload();
        assert_eq!(store.ids(), None);
    }
}
