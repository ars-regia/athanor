//! Favourites and open windows grouped into the entries of the running-applications module
//! (doc_bar.md, BR3, BR7), minimised windows included. An app id comes from the window, so
//! from the application: it becomes a desktop id only when it is one, and never a path.

use std::collections::{BTreeSet, HashMap};

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

/// The installed applications, by what a window can be matched with: the stem of a desktop
/// id (`firefox` for `firefox.desktop`), and the `StartupWMClass` a desktop entry declares
/// for an app whose windows do not carry its desktop id. Both compare ignoring ASCII case.
#[derive(Clone, Debug, Default)]
pub struct AppIndex {
    ids: BTreeSet<String>,
    stems: HashMap<String, String>,
    wm_classes: HashMap<String, String>,
}

impl AppIndex {
    /// `apps` are `(desktop id, StartupWMClass, shown)` triples, `shown` being
    /// `DesktopAppInfo::should_show()` (not `NoDisplay`/`Hidden`). When two entries claim
    /// the same key, a shown entry wins over a hidden one, so a helper like a NoDisplay
    /// URL-handler entry never steals a class or a stem from the application it belongs
    /// to; among ties of the same visibility, the first desktop id in byte order wins, so
    /// the result does not depend on the order GIO lists them in.
    pub fn new(apps: impl IntoIterator<Item = (String, Option<String>, bool)>) -> AppIndex {
        let mut apps: Vec<_> = apps
            .into_iter()
            .filter(|(id, _, _)| is_desktop_id(id))
            .collect();
        apps.sort_by(|(id_a, _, shown_a), (id_b, _, shown_b)| {
            (!shown_a, id_a).cmp(&(!shown_b, id_b))
        });
        let mut index = AppIndex::default();
        for (id, wm_class, _shown) in apps {
            if let Some(stem) = id.strip_suffix(".desktop") {
                index
                    .stems
                    .entry(stem.to_ascii_lowercase())
                    .or_insert_with(|| id.clone());
            }
            if let Some(class) = wm_class.filter(|class| !class.is_empty()) {
                index
                    .wm_classes
                    .entry(class.to_ascii_lowercase())
                    .or_insert_with(|| id.clone());
            }
            index.ids.insert(id);
        }
        index
    }

    pub fn contains(&self, desktop_id: &str) -> bool {
        self.ids.contains(desktop_id)
    }

    /// The desktop id for a window's app id: an installed one by stem, then by
    /// `StartupWMClass`; otherwise the app id as a desktop id, when it can be one. `None`
    /// for an empty app id.
    pub fn resolve(&self, app_id: &str) -> Option<String> {
        if app_id.is_empty() {
            return None;
        }
        let key = app_id.to_ascii_lowercase();
        self.stems
            .get(&key)
            .or_else(|| self.wm_classes.get(&key))
            .cloned()
            .or_else(|| desktop_id(app_id))
    }
}

/// The favourites first, in their order, each with its windows; then one entry per app no
/// favourite claimed, in window order, and one per window without an app id. Windows are
/// matched through `index`: a window joins the entry of the desktop id its app id resolves
/// to, or, when either side has none, the entry of the same app id. A favourite with no
/// window shows only when `index` holds its desktop id.
pub fn entries<K: Clone>(
    favorites: &[String],
    windows: &[Open<K>],
    index: &AppIndex,
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
        let resolved = index.resolve(&window.app_id);
        let joined = if window.app_id.is_empty() {
            None
        } else {
            entries
                .iter()
                .position(|entry| match (&entry.desktop_id, &resolved) {
                    (Some(entry_id), Some(window_id)) => entry_id.eq_ignore_ascii_case(window_id),
                    _ => entry.app_id.eq_ignore_ascii_case(&window.app_id),
                })
        };
        match joined.and_then(|position| entries.get_mut(position)) {
            Some(entry) => entry.windows.push(window.clone()),
            None => entries.push(Entry {
                desktop_id: resolved,
                app_id: window.app_id.clone(),
                pinned: false,
                windows: vec![window.clone()],
            }),
        }
    }
    entries.retain(|entry| {
        !entry.windows.is_empty()
            || entry
                .desktop_id
                .as_deref()
                .is_some_and(|id| index.contains(id))
    });
    entries
}

/// Whether `a` and `b` show the same buttons acting on the same windows. Then only the
/// presentation changed (a title, the focus, a minimised window) and the row is updated in
/// place, not rebuilt.
pub fn same_shape<K: PartialEq>(a: &[Entry<K>], b: &[Entry<K>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.desktop_id == b.desktop_id
                && a.app_id == b.app_id
                && a.pinned == b.pinned
                && a.windows.len() == b.windows.len()
                && a.windows.iter().zip(&b.windows).all(|(x, y)| x.id == y.id)
        })
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

    fn titled(id: u32, app_id: &str, title: &str) -> Open<u32> {
        Open {
            title: title.to_owned(),
            ..open(id, app_id)
        }
    }

    fn index(apps: &[(&str, Option<&str>)]) -> AppIndex {
        AppIndex::new(
            apps.iter()
                .map(|(id, class)| ((*id).to_owned(), class.map(str::to_owned), true)),
        )
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
            &index(&[
                ("firefox.desktop", None),
                ("org.gnome.Nautilus.desktop", None),
            ]),
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
            &index(&[("org.mozilla.firefox.desktop", None)]),
        );
        assert_eq!(
            shape(&found),
            vec![(Some("org.mozilla.firefox.desktop"), vec![1])]
        );
    }

    #[test]
    fn a_window_without_an_app_id_is_an_entry_of_its_own() {
        let found = entries(&[], &[open(1, ""), open(2, "")], &AppIndex::default());
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
    fn the_index_hides_only_idle_favourites_it_does_not_hold() {
        let installed = index(&[("kept.desktop", None), ("../x.desktop", None)]);
        assert!(!installed.contains("../x.desktop"));
        let found = entries(
            &list(&["missing.desktop", "kept.desktop"]),
            &[open(1, "../x")],
            &installed,
        );
        assert_eq!(
            shape(&found),
            vec![(Some("kept.desktop"), vec![]), (None, vec![1])]
        );
        let found = entries(
            &list(&["missing.desktop"]),
            &[open(2, "missing")],
            &installed,
        );
        assert_eq!(shape(&found), vec![(Some("missing.desktop"), vec![2])]);
    }

    #[test]
    fn a_window_resolves_by_stem_ignoring_ascii_case() {
        let apps = index(&[("google-chrome.desktop", None)]);
        assert_eq!(
            apps.resolve("Google-chrome").as_deref(),
            Some("google-chrome.desktop")
        );
    }

    #[test]
    fn a_window_resolves_by_startup_wm_class_when_no_stem_matches() {
        let apps = index(&[("com.visualstudio.code.desktop", Some("Code"))]);
        assert_eq!(
            apps.resolve("code").as_deref(),
            Some("com.visualstudio.code.desktop")
        );
    }

    #[test]
    fn a_stem_wins_over_a_startup_wm_class() {
        let apps = index(&[
            ("com.visualstudio.code.desktop", Some("code")),
            ("code.desktop", None),
        ]);
        assert_eq!(apps.resolve("code").as_deref(), Some("code.desktop"));
    }

    #[test]
    fn ties_go_to_the_first_desktop_id_whatever_the_listing_order() {
        let forward = index(&[("a.desktop", Some("X")), ("b.desktop", Some("x"))]);
        let backward = index(&[("b.desktop", Some("x")), ("a.desktop", Some("X"))]);
        assert_eq!(forward.resolve("x").as_deref(), Some("a.desktop"));
        assert_eq!(backward.resolve("x").as_deref(), Some("a.desktop"));
    }

    #[test]
    fn a_shown_entry_wins_a_startup_wm_class_tie_over_a_hidden_one() {
        let hidden_first = AppIndex::new([
            (
                "foo-url-handler.desktop".to_owned(),
                Some("Foo".to_owned()),
                false,
            ),
            ("foo.desktop".to_owned(), Some("Foo".to_owned()), true),
        ]);
        assert_eq!(hidden_first.resolve("Foo").as_deref(), Some("foo.desktop"));
        let shown_first = AppIndex::new([
            ("foo.desktop".to_owned(), Some("Foo".to_owned()), true),
            (
                "foo-url-handler.desktop".to_owned(),
                Some("Foo".to_owned()),
                false,
            ),
        ]);
        assert_eq!(shown_first.resolve("Foo").as_deref(), Some("foo.desktop"));
    }

    #[test]
    fn an_unknown_app_id_is_its_own_desktop_id_or_none() {
        let apps = AppIndex::default();
        assert_eq!(
            apps.resolve("org.example.App").as_deref(),
            Some("org.example.App.desktop")
        );
        assert_eq!(apps.resolve("a b"), None);
        assert_eq!(apps.resolve(""), None);
    }

    #[test]
    fn a_window_matched_by_wm_class_joins_its_favourite() {
        let apps = index(&[("com.visualstudio.code.desktop", Some("Code"))]);
        let favorites = vec!["com.visualstudio.code.desktop".to_owned()];
        let got = entries(&favorites, &[titled(1, "code", "main.rs")], &apps);
        assert_eq!(got.len(), 1);
        assert!(got[0].pinned);
        assert_eq!(got[0].windows.len(), 1);
    }

    #[test]
    fn a_title_or_focus_change_keeps_the_shape_and_a_new_window_does_not() {
        let apps = index(&[("a.desktop", None)]);
        let before = entries(&[], &[titled(1, "a", "one")], &apps);
        let mut renamed = titled(1, "a", "two");
        renamed.activated = true;
        assert!(same_shape(&before, &entries(&[], &[renamed], &apps)));
        let more = entries(&[], &[titled(1, "a", "one"), titled(2, "a", "one")], &apps);
        assert!(!same_shape(&before, &more));
        let pinned = entries(&["a.desktop".to_owned()], &[titled(1, "a", "one")], &apps);
        assert!(!same_shape(&before, &pinned));
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
