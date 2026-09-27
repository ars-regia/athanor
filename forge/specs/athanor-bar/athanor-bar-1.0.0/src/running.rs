//! Favourites and open windows grouped into the entries of the running-applications module
//! (doc_bar.md, BR3, BR7), minimised windows included. An app id comes from the window, so
//! from the application: it becomes a desktop id only when it is one, and never a path.

use athanor_layout::favorites::is_desktop_id;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open<K> {
    pub id: K,
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    pub minimized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<K> {
    /// What the entry launches and pins; `None` when the app id is not a desktop id.
    pub desktop_id: Option<String>,
    pub app_id: String,
    pub pinned: bool,
    pub windows: Vec<Open<K>>,
}

/// What a press on the entry does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary<K> {
    Launch,
    Activate(K),
    Minimize(K),
    /// Several windows: the menu lists them.
    Choose,
}

pub fn desktop_id(app_id: &str) -> Option<String> {
    let id = format!("{app_id}.desktop");
    is_desktop_id(&id).then_some(id)
}

/// The favourites first, in their order, each with its windows; then one entry per app id
/// no favourite claimed, in window order, and one per window without an app id. A
/// favourite with no window shows only when `installed` finds its desktop id.
pub fn entries<K: Clone>(
    favorites: &[String],
    windows: &[Open<K>],
    installed: impl Fn(&str) -> bool,
) -> Vec<Entry<K>> {
    let mut entries: Vec<Entry<K>> = favorites
        .iter()
        .map(|id| Entry {
            desktop_id: Some(id.clone()),
            app_id: id.strip_suffix(".desktop").unwrap_or(id).to_owned(),
            pinned: true,
            windows: Vec::new(),
        })
        .collect();
    for window in windows {
        let joined = if window.app_id.is_empty() {
            None
        } else {
            entries
                .iter()
                .position(|entry| entry.app_id.eq_ignore_ascii_case(&window.app_id))
        };
        match joined.and_then(|index| entries.get_mut(index)) {
            Some(entry) => entry.windows.push(window.clone()),
            None => entries.push(Entry {
                desktop_id: desktop_id(&window.app_id),
                app_id: window.app_id.clone(),
                pinned: false,
                windows: vec![window.clone()],
            }),
        }
    }
    entries.retain(|entry| {
        !entry.windows.is_empty() || entry.desktop_id.as_deref().is_some_and(&installed)
    });
    entries
}

pub fn primary<K: Copy>(entry: &Entry<K>) -> Primary<K> {
    match entry.windows.as_slice() {
        [] => Primary::Launch,
        [window] if window.activated && !window.minimized => Primary::Minimize(window.id),
        [window] => Primary::Activate(window.id),
        _ => Primary::Choose,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(id: u32, app_id: &str) -> Open<u32> {
        Open {
            id,
            app_id: app_id.to_owned(),
            title: format!("window {id}"),
            activated: false,
            minimized: false,
        }
    }

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn shape(entries: &[Entry<u32>]) -> Vec<(Option<&str>, Vec<u32>)> {
        entries
            .iter()
            .map(|entry| {
                (
                    entry.desktop_id.as_deref(),
                    entry.windows.iter().map(|window| window.id).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn favourites_come_first_then_the_other_applications_in_window_order() {
        let windows = [
            open(1, "org.gnome.Ptyxis"),
            open(2, "firefox"),
            open(3, "org.gnome.Ptyxis"),
        ];
        let found = entries(
            &list(&["firefox.desktop", "org.gnome.Nautilus.desktop"]),
            &windows,
            |_| true,
        );
        assert_eq!(
            shape(&found),
            vec![
                (Some("firefox.desktop"), vec![2]),
                (Some("org.gnome.Nautilus.desktop"), vec![]),
                (Some("org.gnome.Ptyxis.desktop"), vec![1, 3]),
            ]
        );
        assert_eq!(
            found.iter().map(|entry| entry.pinned).collect::<Vec<_>>(),
            [true, true, false]
        );
    }

    #[test]
    fn a_window_joins_its_favourite_whatever_the_ascii_case() {
        let found = entries(
            &list(&["org.mozilla.firefox.desktop"]),
            &[open(1, "Org.Mozilla.Firefox")],
            |_| true,
        );
        assert_eq!(
            shape(&found),
            vec![(Some("org.mozilla.firefox.desktop"), vec![1])]
        );
    }

    #[test]
    fn a_window_without_an_app_id_is_an_entry_of_its_own() {
        let found = entries(&[], &[open(1, ""), open(2, "")], |_| true);
        assert_eq!(shape(&found), vec![(None, vec![1]), (None, vec![2])]);
    }

    #[test]
    fn an_app_id_becomes_a_desktop_id_only_when_it_is_one() {
        assert_eq!(desktop_id("../x"), None);
        assert_eq!(desktop_id("a/b"), None);
        assert_eq!(desktop_id(""), None);
        assert_eq!(
            desktop_id("steam_app_570").as_deref(),
            Some("steam_app_570.desktop")
        );
        assert_eq!(
            desktop_id("Org.Mozilla.Firefox").as_deref(),
            Some("Org.Mozilla.Firefox.desktop")
        );
    }

    #[test]
    fn installed_is_asked_only_about_desktop_ids_and_hides_only_idle_favourites() {
        let installed = |id: &str| {
            assert!(!id.contains('/'), "installed was asked about {id}");
            id != "missing.desktop"
        };
        let found = entries(
            &list(&["missing.desktop", "kept.desktop"]),
            &[open(1, "../x")],
            installed,
        );
        assert_eq!(
            shape(&found),
            vec![(Some("kept.desktop"), vec![]), (None, vec![1])]
        );
        let found = entries(
            &list(&["missing.desktop"]),
            &[open(2, "missing")],
            installed,
        );
        assert_eq!(shape(&found), vec![(Some("missing.desktop"), vec![2])]);
    }

    #[test]
    fn the_primary_action_launches_activates_minimises_or_lets_the_user_choose() {
        let entry = |windows: Vec<Open<u32>>| Entry {
            desktop_id: Some("a.desktop".to_owned()),
            app_id: "a".to_owned(),
            pinned: true,
            windows,
        };
        let mut focused = open(1, "a");
        focused.activated = true;
        let mut minimised = focused.clone();
        minimised.minimized = true;
        assert_eq!(primary(&entry(vec![])), Primary::Launch);
        assert_eq!(primary(&entry(vec![focused])), Primary::Minimize(1));
        assert_eq!(primary(&entry(vec![minimised])), Primary::Activate(1));
        assert_eq!(primary(&entry(vec![open(1, "a")])), Primary::Activate(1));
        assert_eq!(
            primary(&entry(vec![open(1, "a"), open(2, "a")])),
            Primary::Choose
        );
    }
}
