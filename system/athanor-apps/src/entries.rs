//! The row's content and acts without its widgets (doc_bar.md, BR2, BR3, BR7): the
//! installed applications, the entries with their desktop entries, their labels, and
//! what a press, a menu row or an opener does. The GTK row and a surface drawn without
//! GTK both show these, so the launch path behind the security context stays one.

use std::collections::HashMap;

use athanor_compositor_client::{Client, Opener, WindowId};
use athanor_unit::text;
use gio::glib;
use gio::prelude::*;
use gio_unix::DesktopAppInfo;

use crate::i18n::{tr, tr_with};
use crate::model::{self, AppIndex, Entry, Open};

const NOTIFY_TIMEOUT_MS: i32 = 5000;

/// The icon of an entry with no desktop entry or no icon of its own.
pub const FALLBACK_ICON: &str = "application-x-executable-symbolic";

/// An entry and the desktop entry it launches, when one is installed.
pub type State = (Entry<WindowId>, Option<DesktopAppInfo>);

/// The installed applications: the index windows are matched through, and the desktop
/// entries by id.
pub struct Apps {
    index: AppIndex,
    infos: HashMap<String, DesktopAppInfo>,
}

impl Apps {
    pub fn installed() -> Apps {
        let infos: HashMap<String, DesktopAppInfo> = gio::AppInfo::all()
            .into_iter()
            .filter_map(|info| info.downcast::<DesktopAppInfo>().ok())
            .filter_map(|info| Some((info.id()?.to_string(), info)))
            .collect();
        let index = AppIndex::new(infos.iter().map(|(id, info)| {
            (
                id.clone(),
                info.startup_wm_class().map(|class| class.to_string()),
                info.should_show(),
            )
        }));
        Apps { index, infos }
    }

    /// The entries of `favorites` and `windows`, each with its desktop entry.
    pub fn states(&self, favorites: &[String], windows: &[Open<WindowId>]) -> Vec<State> {
        model::entries(favorites, windows, &self.index)
            .into_iter()
            .map(|entry| {
                let info = entry
                    .desktop_id
                    .as_deref()
                    .and_then(|id| self.infos.get(id).cloned());
                (entry, info)
            })
            .collect()
    }
}

/// The client's windows, their titles reduced to one safe line (SH12).
pub fn windows(client: &Client) -> Vec<Open<WindowId>> {
    client
        .windows()
        .into_iter()
        .map(|window| Open {
            id: window.id,
            app_id: window.app_id,
            title: text::line(&window.title, text::TITLE_CHARS),
            activated: window.state.activated,
            minimized: window.state.minimized,
        })
        .collect()
}

/// The window's app id is set by the client (`xdg_toplevel.set_app_id`): arbitrary UTF-8,
/// control and bidi characters included (SH12). With no desktop entry to name it, it is
/// shown only sanitised, never raw; an id that sanitises to nothing falls back like an
/// empty one.
pub fn app_name(entry: &Entry<WindowId>, info: Option<&DesktopAppInfo>) -> String {
    match info {
        Some(info) => info.name().to_string(),
        None => {
            let sanitized = text::line(&entry.app_id, text::NAME_CHARS);
            if sanitized.is_empty() {
                tr("Unknown application")
            } else {
                sanitized
            }
        }
    }
}

/// The tooltip and accessible label of an entry's button: the app name alone with no
/// window or an empty title, `app: title` with exactly one titled window (the app is
/// substituted first, so a title containing the literal text "{app}" is not mistaken for
/// the placeholder), and a count with two or more.
pub fn entry_label<K>(name: &str, windows: &[Open<K>]) -> String {
    match windows {
        [] => name.to_owned(),
        [window] if !window.title.is_empty() => tr_with(
            &tr_with("{app}: {title}", "app", name),
            "title",
            &window.title,
        ),
        [_] => name.to_owned(),
        _ => tr_with(
            &tr_with("{app} ({count} windows)", "app", name),
            "count",
            &windows.len().to_string(),
        ),
    }
}

/// The name of a window in an entry's menu.
pub fn window_title<K>(window: &Open<K>) -> String {
    if window.title.is_empty() {
        tr("Untitled window")
    } else {
        window.title.clone()
    }
}

/// Whether the entry has a window that holds the focus.
pub fn active<K>(entry: &Entry<K>) -> bool {
    entry
        .windows
        .iter()
        .any(|window| window.activated && !window.minimized)
}

/// The icon and the name of an opener's button.
pub fn opener(opener: Opener) -> (&'static str, String) {
    match opener {
        Opener::Launcher => ("system-search-symbolic", tr("Launcher")),
        Opener::AppLibrary => ("view-app-grid-symbolic", tr("Applications")),
        Opener::Workspaces => ("view-paged-symbolic", tr("Workspaces")),
    }
}

/// Shows the opener's component through the client, which starts it when it does not run.
pub async fn open(client: &Client, opener: Opener) {
    if let Err(err) = client.open(opener).await {
        tracing::error!(error = %err, opener = ?opener, "cannot open the component");
    }
}

pub fn activate(client: &Client, window: &Open<WindowId>) {
    let restored = if window.minimized {
        client.window_actions().unminimize(window.id)
    } else {
        Ok(())
    };
    if let Err(err) = restored.and_then(|()| client.window_actions().activate(window.id)) {
        tracing::error!(error = %err, "cannot activate the window");
    }
}

pub fn minimize(client: &Client, window: WindowId) {
    if let Err(err) = client.window_actions().minimize(window) {
        tracing::error!(error = %err, "cannot minimise the window");
    }
}

pub fn close(client: &Client, window: WindowId) {
    if let Err(err) = client.window_actions().close(window) {
        tracing::error!(error = %err, "cannot close the window");
    }
}

/// BR2: every start goes through the compositor client, behind a security context. A
/// start that fails closed is logged at err and told to the user (BR2.6) under `app`, the
/// program's name.
pub async fn launch(client: &Client, info: &DesktopAppInfo, app: &str) {
    if let Err(err) = client.launch(info).await {
        let name = info.name().to_string();
        tracing::error!(error = %err, app = %name, "the application was not started");
        notify_failure(app, &name).await;
    }
}

async fn notify_failure(app: &str, name: &str) {
    let args = (
        app,
        0u32,
        "dialog-error-symbolic",
        tr_with("{name} could not start", "name", name),
        tr("The reason is in the system journal."),
        Vec::<String>::new(),
        HashMap::<String, glib::Variant>::new(),
        -1i32,
    )
        .to_variant();
    let sent = async {
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        bus.call_future(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            NOTIFY_TIMEOUT_MS,
        )
        .await
    };
    if let Err(err) = sent.await {
        tracing::error!(error = %err, "cannot send the notification of the failed start");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(title: &str) -> Open<u32> {
        Open {
            id: 1,
            app_id: "a".to_owned(),
            title: title.to_owned(),
            activated: false,
            minimized: false,
        }
    }

    fn entry(app_id: &str) -> Entry<WindowId> {
        Entry {
            desktop_id: None,
            app_id: app_id.to_owned(),
            pinned: false,
            windows: Vec::new(),
        }
    }

    #[test]
    fn an_app_id_of_hidden_characters_only_falls_back_to_unknown_application() {
        assert_eq!(
            app_name(&entry("\u{202e}\u{7}"), None),
            tr("Unknown application")
        );
        assert_eq!(app_name(&entry(""), None), tr("Unknown application"));
    }

    #[test]
    fn an_app_id_with_no_desktop_entry_is_shown_sanitised_not_raw() {
        assert_eq!(
            app_name(&entry("org.athanor.CcWindow3\u{202e}evil"), None),
            "org.athanor.CcWindow3evil"
        );
    }

    #[test]
    fn no_window_shows_the_app_name_alone() {
        assert_eq!(entry_label("Firefox", &[] as &[Open<u32>]), "Firefox");
    }

    #[test]
    fn one_window_with_an_empty_title_shows_the_app_name_alone() {
        assert_eq!(entry_label("Firefox", &[open("")]), "Firefox");
    }

    #[test]
    fn one_titled_window_pairs_the_app_and_the_title() {
        assert_eq!(entry_label("Firefox", &[open("Inbox")]), "Firefox: Inbox");
    }

    #[test]
    fn a_title_holding_the_placeholder_text_stays_literal() {
        assert_eq!(
            entry_label("Firefox", &[open("about {app}")]),
            "Firefox: about {app}"
        );
    }

    #[test]
    fn two_or_more_windows_are_counted() {
        assert_eq!(
            entry_label("Firefox", &[open("a"), open("b")]),
            "Firefox (2 windows)"
        );
    }

    #[test]
    fn a_title_of_hidden_characters_only_falls_back_to_the_app_name() {
        let window = Open {
            id: 1u32,
            app_id: "a".into(),
            title: text::line("\u{202e}\u{7}", text::TITLE_CHARS),
            activated: false,
            minimized: false,
        };
        assert_eq!(entry_label("App", &[window]), "App");
    }

    #[test]
    fn an_untitled_window_is_named_as_such_in_the_menu() {
        assert_eq!(window_title(&open("")), tr("Untitled window"));
        assert_eq!(window_title(&open("Inbox")), "Inbox");
    }

    #[test]
    fn only_a_focused_window_that_is_not_minimised_makes_the_entry_active() {
        let mut item = Entry {
            desktop_id: None,
            app_id: "a".to_owned(),
            pinned: false,
            windows: vec![open("x")],
        };
        assert!(!active(&item));
        item.windows[0].activated = true;
        assert!(active(&item));
        item.windows[0].minimized = true;
        assert!(!active(&item));
    }
}
