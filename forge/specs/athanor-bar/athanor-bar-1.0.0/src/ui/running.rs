//! The running applications of the `bar` preset (doc_bar.md, BR3, BR7): the favourites and
//! the open windows, grouped by app id, minimised windows included. A press launches,
//! activates or minimises; with several windows, the secondary button, Shift+F10 or the
//! Menu key, a menu lists the windows with minimise and close, a new window, and pinning.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use athanor_bar::running::{self, Entry, Open, Primary};
use athanor_compositor_client::WindowId;
use athanor_layout::favorites;
use gio_unix::DesktopAppInfo;
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use super::popup;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const NOTIFY_TIMEOUT_MS: i32 = 5000;

struct RunningUi {
    row: gtk4::Box,
    shown: RefCell<Vec<Entry<WindowId>>>,
    /// A menu is open: the row is rebuilt when it closes, not under the pointer.
    menu_open: Rc<Cell<bool>>,
}

impl ModuleUi for RunningUi {
    fn widget(&self) -> gtk4::Widget {
        self.row.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if !matches!(changed, Changed::Windows | Changed::Favorites) || self.menu_open.get() {
            return;
        }
        let Some(client) = bar.client() else { return };
        let windows: Vec<Open<WindowId>> = client
            .windows()
            .into_iter()
            .map(|window| Open {
                id: window.id,
                app_id: window.app_id,
                title: window.title,
                activated: window.state.activated,
                minimized: window.state.minimized,
            })
            .collect();
        let favorites = bar.favorites().unwrap_or_default();
        let entries =
            running::entries(&favorites, &windows, |id| DesktopAppInfo::new(id).is_some());
        if *self.shown.borrow() == entries {
            return;
        }
        while let Some(child) = self.row.first_child() {
            self.row.remove(&child);
        }
        for entry in &entries {
            self.row.append(&entry_button(bar, entry, &self.menu_open));
        }
        self.row.set_visible(!entries.is_empty());
        self.shown.replace(entries);
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    bar.client()?;
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    row.update_property(&[Property::Label(&tr("Running applications"))]);
    row.set_visible(false);
    Some(Box::new(RunningUi {
        row,
        shown: RefCell::new(Vec::new()),
        menu_open: Rc::new(Cell::new(false)),
    }))
}

fn app_name(entry: &Entry<WindowId>, info: Option<&DesktopAppInfo>) -> String {
    match info {
        Some(info) => info.name().to_string(),
        None if entry.app_id.is_empty() => tr("Unknown application"),
        None => entry.app_id.clone(),
    }
}

/// The tooltip and accessible label of an entry's button: the app name alone with no
/// window or an empty title, `app: title` with exactly one titled window (the app is
/// substituted first, so a title containing the literal text "{app}" is not mistaken for
/// the placeholder), and a count with two or more.
fn entry_label<K>(name: &str, windows: &[Open<K>]) -> String {
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

fn entry_button(
    bar: &Rc<Bar>,
    entry: &Entry<WindowId>,
    menu_open: &Rc<Cell<bool>>,
) -> gtk4::Button {
    let info = entry.desktop_id.as_deref().and_then(DesktopAppInfo::new);
    let name = app_name(entry, info.as_ref());
    let label = entry_label(&name, &entry.windows);
    let image = match info.as_ref().and_then(|info| info.icon()) {
        Some(icon) => gtk4::Image::from_gicon(&icon),
        None => gtk4::Image::from_icon_name("application-x-executable-symbolic"),
    };
    image.set_pixel_size(24);
    let button = gtk4::Button::new();
    button.set_child(Some(&image));
    button.add_css_class("bar-button");
    button.set_tooltip_text(Some(&label));
    button.update_property(&[Property::Label(&label)]);
    if !entry.windows.is_empty() {
        button.add_css_class("running");
        button.update_property(&[Property::Description(&tr("Running"))]);
    }
    if entry
        .windows
        .iter()
        .any(|window| window.activated && !window.minimized)
    {
        button.add_css_class("active");
    }

    let menu = popup::attach(bar, &button);
    let (weak_bar, closed_flag) = (Rc::downgrade(bar), menu_open.clone());
    menu.connect_closed(move |_| {
        closed_flag.set(false);
        // Deferred: a refresh now would remove the button that owns this popover.
        let weak_bar = weak_bar.clone();
        glib::idle_add_local_once(move || {
            if let Some(bar) = weak_bar.upgrade() {
                bar.refresh(Changed::Windows);
            }
        });
    });
    let show_menu: Rc<dyn Fn()> = {
        let (weak_bar, weak_menu, entry, info, menu_open) = (
            Rc::downgrade(bar),
            menu.downgrade(),
            entry.clone(),
            info.clone(),
            menu_open.clone(),
        );
        Rc::new(move || {
            let (Some(bar), Some(menu)) = (weak_bar.upgrade(), weak_menu.upgrade()) else {
                return;
            };
            menu.set_child(Some(&menu_content(&bar, &menu, &entry, info.as_ref())));
            menu_open.set(true);
            bar.popover_opened(&menu);
            menu.popup();
        })
    };

    let (weak_bar, entry_for_click, info_for_click, choose) = (
        Rc::downgrade(bar),
        entry.clone(),
        info.clone(),
        show_menu.clone(),
    );
    button.connect_clicked(move |_| {
        let Some(bar) = weak_bar.upgrade() else {
            return;
        };
        match running::primary(&entry_for_click) {
            Primary::Launch => {
                if let Some(info) = &info_for_click {
                    launch(&bar, info);
                }
            }
            Primary::Activate(id) => {
                if let Some(window) = entry_for_click
                    .windows
                    .iter()
                    .find(|window| window.id == id)
                {
                    activate(&bar, window);
                }
            }
            Primary::Minimize(id) => {
                if let Some(Err(err)) = bar.client().map(|client| client.minimize(id)) {
                    tracing::error!(error = %err, "cannot minimise the window");
                }
            }
            Primary::Choose => choose(),
        }
    });

    let secondary = gtk4::GestureClick::new();
    secondary.set_button(gdk::BUTTON_SECONDARY);
    let open = show_menu.clone();
    secondary.connect_pressed(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        open();
    });
    button.add_controller(secondary);
    let keys = gtk4::ShortcutController::new();
    if let Some(trigger) = gtk4::ShortcutTrigger::parse_string("<Shift>F10|Menu") {
        let open = show_menu;
        let action = gtk4::CallbackAction::new(move |_, _| {
            open();
            glib::Propagation::Stop
        });
        keys.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
    }
    button.add_controller(keys);
    button
}

fn menu_row(label: &str) -> gtk4::Button {
    let text = gtk4::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.set_max_width_chars(40);
    let button = gtk4::Button::new();
    button.set_child(Some(&text));
    button.add_css_class("bar-row");
    button.set_hexpand(true);
    button
}

fn icon_button(icon: &str, name: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-row");
    button.set_tooltip_text(Some(name));
    button.update_property(&[Property::Label(name)]);
    button
}

/// Runs `act` with the bar after closing the menu. The menu closes first, and the act runs
/// on idle, because pinning rebuilds the row that owns the menu.
fn on_press(
    button: &gtk4::Button,
    bar: &Rc<Bar>,
    menu: &gtk4::Popover,
    act: impl Fn(&Rc<Bar>) + Clone + 'static,
) {
    let (weak_bar, weak_menu) = (Rc::downgrade(bar), menu.downgrade());
    button.connect_clicked(move |_| {
        if let Some(menu) = weak_menu.upgrade() {
            menu.popdown();
        }
        let (weak_bar, act) = (weak_bar.clone(), act.clone());
        glib::idle_add_local_once(move || {
            if let Some(bar) = weak_bar.upgrade() {
                act(&bar);
            }
        });
    });
}

fn menu_content(
    bar: &Rc<Bar>,
    menu: &gtk4::Popover,
    entry: &Entry<WindowId>,
    info: Option<&DesktopAppInfo>,
) -> gtk4::Box {
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    for window in &entry.windows {
        let title = if window.title.is_empty() {
            tr("Untitled window")
        } else {
            window.title.clone()
        };
        let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        let row = menu_row(&title);
        let minimise = icon_button(
            "window-minimize-symbolic",
            &tr_with("Minimise {title}", "title", &title),
        );
        let close = icon_button(
            "window-close-symbolic",
            &tr_with("Close {title}", "title", &title),
        );
        let target = window.clone();
        on_press(&row, bar, menu, move |bar| activate(bar, &target));
        let id = window.id;
        on_press(&minimise, bar, menu, move |bar| {
            if let Some(Err(err)) = bar.client().map(|client| client.minimize(id)) {
                tracing::error!(error = %err, "cannot minimise the window");
            }
        });
        on_press(&close, bar, menu, move |bar| {
            if let Some(Err(err)) = bar.client().map(|client| client.close(id)) {
                tracing::error!(error = %err, "cannot close the window");
            }
        });
        line.append(&row);
        line.append(&minimise);
        line.append(&close);
        list.append(&line);
    }
    let Some(info) = info else { return list };
    if !entry.windows.is_empty() {
        list.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    }
    let open = menu_row(&if entry.windows.is_empty() {
        tr("Open")
    } else {
        tr("New Window")
    });
    let app = info.clone();
    on_press(&open, bar, menu, move |bar| launch(bar, &app));
    list.append(&open);
    // Pinning needs the favourites and a desktop id; when either is missing there is no row.
    let (Some(ids), Some(id)) = (bar.favorites(), entry.desktop_id.clone()) else {
        return list;
    };
    let pin = if entry.pinned {
        let row = menu_row(&tr("Unpin from Bar"));
        on_press(&row, bar, menu, move |bar| {
            bar.set_favorites(favorites::unpinned(&ids, &id))
        });
        row
    } else {
        let row = menu_row(&tr("Pin to Bar"));
        on_press(&row, bar, menu, move |bar| {
            match favorites::pinned(&ids, &id) {
                Ok(ids) => bar.set_favorites(ids),
                Err(err) => tracing::error!(error = %err, "the application was not pinned"),
            }
        });
        row
    };
    list.append(&pin);
    list
}

fn activate(bar: &Bar, window: &Open<WindowId>) {
    let Some(client) = bar.client() else { return };
    let restored = if window.minimized {
        client.unminimize(window.id)
    } else {
        Ok(())
    };
    if let Err(err) = restored.and_then(|()| client.activate(window.id)) {
        tracing::error!(error = %err, "cannot activate the window");
    }
}

/// BR2: every start goes through the compositor client, behind a security context. A
/// start that fails closed is logged at err and told to the user (BR2.6).
fn launch(bar: &Rc<Bar>, info: &DesktopAppInfo) {
    let (weak_bar, info) = (Rc::downgrade(bar), info.clone());
    glib::spawn_future_local(async move {
        let Some(bar) = weak_bar.upgrade() else {
            return;
        };
        let Some(client) = bar.client() else { return };
        if let Err(err) = client.launch(&info).await {
            let name = info.name().to_string();
            tracing::error!(error = %err, app = %name, "the application was not started");
            notify_failure(&name).await;
        }
    });
}

async fn notify_failure(name: &str) {
    let args = (
        "athanor-bar",
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
}
