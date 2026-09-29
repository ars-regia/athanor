//! The compositor's state in our own types, and the table that turns double-buffered
//! protocol state into changes. No Wayland or GTK type appears here, so every rule is
//! tested without a display.

use std::collections::BTreeMap;

/// A window for as long as it exists. Never reused within one [`crate::Client`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub(crate) u64);

/// A workspace for as long as it exists. Never reused within one [`crate::Client`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub activated: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl WindowState {
    /// Decodes the `state` array of `zcosmic_toplevel_handle_v1`: 32-bit values in the
    /// host's byte order. Values this crate does not know, and a trailing partial value,
    /// are ignored.
    pub(crate) fn from_cosmic(array: &[u8]) -> Self {
        let mut state = Self::default();
        for chunk in array.as_chunks::<4>().0 {
            match u32::from_ne_bytes(*chunk) {
                0 => state.maximized = true,
                1 => state.minimized = true,
                2 => state.activated = true,
                3 => state.fullscreen = true,
                _ => {}
            }
        }
        state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub app_id: String,
    pub title: String,
    pub state: WindowState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tiling {
    Floating,
    Tiled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub active: bool,
    /// `None` until the compositor reports it, and on compositors without COSMIC's
    /// workspace extension.
    pub tiling: Option<Tiling>,
    /// The connector of the first output of the workspace's group, as GDK names it.
    pub output: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenFilter {
    #[default]
    None,
    Greyscale,
    Protanopia,
    Deuteranopia,
    Tritanopia,
    /// A filter the compositor applies that this crate cannot name. It is read, never set.
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    pub magnifier: bool,
    pub inverted: bool,
    pub filter: ScreenFilter,
}

/// A change, delivered after the compositor finished describing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    WindowAdded(Window),
    WindowChanged(Window),
    WindowRemoved(WindowId),
    WorkspaceAdded(Workspace),
    WorkspaceChanged(Workspace),
    WorkspaceRemoved(WorkspaceId),
    /// The names of the configured layouts, in group order.
    KeyboardLayouts(Vec<String>),
    /// The index of the active layout in [`Event::KeyboardLayouts`].
    KeyboardGroup(u32),
    Accessibility(Accessibility),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Change<T> {
    Added(T),
    Changed(T),
}

/// Protocol state is double-buffered: events fill the pending copy, and a `done` makes it
/// current. The pending copy is kept after a commit, because the next events carry only
/// what changed.
#[derive(Debug)]
pub(crate) struct Table<K, T> {
    pending: BTreeMap<K, T>,
    committed: BTreeMap<K, T>,
}

impl<K, T> Default for Table<K, T> {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            committed: BTreeMap::new(),
        }
    }
}

impl<K: Ord + Copy, T: Clone + PartialEq> Table<K, T> {
    pub(crate) fn insert(&mut self, key: K, value: T) {
        self.pending.insert(key, value);
    }

    pub(crate) fn pending(&mut self, key: &K) -> Option<&mut T> {
        self.pending.get_mut(key)
    }

    pub(crate) fn pending_values(&mut self) -> impl Iterator<Item = &mut T> {
        self.pending.values_mut()
    }

    /// Makes the pending copy current. `None` when nothing a reader can see changed.
    pub(crate) fn commit(&mut self, key: &K) -> Option<Change<T>> {
        let next = self.pending.get(key)?;
        match self.committed.insert(*key, next.clone()) {
            None => Some(Change::Added(next.clone())),
            Some(previous) if previous != *next => Some(Change::Changed(next.clone())),
            Some(_) => None,
        }
    }

    pub(crate) fn commit_all(&mut self) -> Vec<Change<T>> {
        let keys: Vec<K> = self.pending.keys().copied().collect();
        keys.iter().filter_map(|key| self.commit(key)).collect()
    }

    /// Forgets the key. `true` when a reader had seen it, so its removal is news.
    pub(crate) fn remove(&mut self, key: &K) -> bool {
        self.pending.remove(key);
        self.committed.remove(key).is_some()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.committed.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn cosmic_state_array_decodes_known_values() {
        let state = WindowState::from_cosmic(&states(&[2, 1]));
        assert_eq!(
            state,
            WindowState {
                activated: true,
                minimized: true,
                ..WindowState::default()
            }
        );
        let all = WindowState::from_cosmic(&states(&[0, 3]));
        assert!(all.maximized && all.fullscreen && !all.activated);
    }

    #[test]
    fn cosmic_state_array_ignores_unknown_values_and_a_partial_tail() {
        let mut bytes = states(&[4, 99, 2]);
        bytes.extend_from_slice(&[1, 0]);
        assert_eq!(
            WindowState::from_cosmic(&bytes),
            WindowState {
                activated: true,
                ..WindowState::default()
            }
        );
        assert_eq!(WindowState::from_cosmic(&[]), WindowState::default());
    }

    #[test]
    fn a_commit_reports_added_then_changed_then_nothing() {
        let mut table: Table<u64, String> = Table::default();
        table.insert(1, "a".into());
        assert_eq!(table.values().count(), 0, "nothing is visible before done");
        assert_eq!(table.commit(&1), Some(Change::Added("a".into())));
        assert_eq!(table.commit(&1), None);
        if let Some(value) = table.pending(&1) {
            value.push('b');
        }
        assert_eq!(table.values().next().map(String::as_str), Some("a"));
        assert_eq!(table.commit(&1), Some(Change::Changed("ab".into())));
        assert_eq!(table.values().count(), 1);
    }

    #[test]
    fn commit_all_reports_only_what_changed() {
        let mut table: Table<u64, u32> = Table::default();
        table.insert(1, 10);
        table.insert(2, 20);
        assert_eq!(table.commit_all().len(), 2);
        for value in table.pending_values() {
            *value = 10;
        }
        assert_eq!(table.commit_all(), vec![Change::Changed(10)]);
    }

    #[test]
    fn removing_a_key_never_committed_is_not_news() {
        let mut table: Table<u64, u32> = Table::default();
        table.insert(1, 10);
        assert!(!table.remove(&1));
        table.insert(2, 20);
        table.commit(&2);
        assert!(table.remove(&2));
        assert!(table.pending(&2).is_none());
        assert_eq!(table.commit(&2), None);
    }
}
