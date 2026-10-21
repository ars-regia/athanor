//! The applications row (doc_bar.md, BR3, BR7): the favourites and the open windows,
//! grouped by app id, minimised windows included. A press launches, activates or
//! minimises; with several windows, the secondary button, Shift+F10 or the Menu key, a
//! menu lists the windows with minimise and close, a new window, and pinning. In the dock
//! (`Host::REORDER`), dragging a pinned entry onto another moves it there. A change that
//! keeps the buttons and their windows (a title, the focus) updates the buttons in place;
//! the installed applications are cached until GIO reports a change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_compositor_client::WindowId;
use athanor_layout::favorites;
use gio_unix::DesktopAppInfo;
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::entries::{self, app_name, entry_label, Apps, State};
use crate::i18n::{tr, tr_with};
use crate::menu;
use crate::model::{self, Entry, Open, Primary};
use crate::Host;

/// One button of the row. Its handlers read `state` when they fire, so an update in place
/// reaches them.
struct Shown {
    button: gtk4::Button,
    image: gtk4::Image,
    state: Rc<RefCell<State>>,
}

/// The row of one surface. Its owner calls `refresh` when the windows or the favourites
/// change.
pub struct Row {
    row: gtk4::Box,
    shown: RefCell<Vec<Shown>>,
    /// A menu is open: the row is rebuilt when it closes, not under the pointer.
    menu_open: Rc<Cell<bool>>,
    /// `None` when stale: the next refresh lists the installed applications again.
    apps: Rc<RefCell<Option<Rc<Apps>>>>,
    monitor: gio::AppInfoMonitor,
    apps_changed: Option<glib::SignalHandlerId>,
    /// Where the entries' menus open: towards the inside of the screen.
    menu_position: gtk4::PositionType,
}

impl Row {
    /// `None` without the compositor client: no windows to list and no secure launch
    /// (SH1, BR2).
    pub fn new<H: Host>(
        host: &Rc<H>,
        orientation: gtk4::Orientation,
        menu_position: gtk4::PositionType,
    ) -> Option<Row> {
        host.client()?;
        let row = gtk4::Box::new(orientation, 2);
        row.update_property(&[Property::Label(&tr("Running applications"))]);
        row.set_visible(false);
        let apps = Rc::new(RefCell::new(None));
        let monitor = gio::AppInfoMonitor::get();
        let (weak_host, stale) = (Rc::downgrade(host), apps.clone());
        let apps_changed = monitor.connect_changed(move |_| {
            stale.replace(None);
            if let Some(host) = weak_host.upgrade() {
                host.refresh_rows();
            }
        });
        Some(Row {
            row,
            shown: RefCell::new(Vec::new()),
            menu_open: Rc::new(Cell::new(false)),
            apps,
            monitor,
            apps_changed: Some(apps_changed),
            menu_position,
        })
    }

    pub fn widget(&self) -> gtk4::Widget {
        self.row.clone().upcast()
    }

    fn apps(&self) -> Rc<Apps> {
        self.apps
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(Apps::installed()))
            .clone()
    }

    pub fn refresh<H: Host>(&self, host: &Rc<H>) {
        if self.menu_open.get() {
            return;
        }
        let Some(client) = host.client() else { return };
        let windows = entries::windows(client);
        let favorites = host.favorites().ids().unwrap_or_default();
        let states = self.apps().states(&favorites, &windows);
        let shown_entries: Vec<Entry<WindowId>> = self
            .shown
            .borrow()
            .iter()
            .map(|shown| shown.state.borrow().0.clone())
            .collect();
        let entries: Vec<Entry<WindowId>> = states.iter().map(|(entry, _)| entry.clone()).collect();
        if model::same_shape(&shown_entries, &entries) {
            // Only the presentation changed, if anything: the buttons stay, so do their
            // accessibles and any focus on them.
            for (shown, state) in self.shown.borrow().iter().zip(states) {
                if *shown.state.borrow() != state {
                    shown.state.replace(state);
                    present(shown);
                }
            }
            return;
        }
        while let Some(child) = self.row.first_child() {
            self.row.remove(&child);
        }
        let shown: Vec<Shown> = states
            .into_iter()
            .map(|state| entry_button(host, state, &self.menu_open, self.menu_position))
            .collect();
        for button in &shown {
            self.row.append(&button.button);
        }
        self.row.set_visible(!shown.is_empty());
        self.shown.replace(shown);
    }
}

impl Drop for Row {
    /// A rebuilt surface drops its row: a handler left connected would keep a dead row's
    /// cache and refresh the host for it.
    fn drop(&mut self) {
        if let Some(handler) = self.apps_changed.take() {
            self.monitor.disconnect(handler);
        }
    }
}

/// Shows `shown`'s state on its button: the label, the icon, and whether the app runs and
/// has the focus. A new button and an update in place both come here.
fn present(shown: &Shown) {
    let state = shown.state.borrow();
    let (entry, info) = &*state;
    let label = entry_label(&app_name(entry, info.as_ref()), &entry.windows);
    let button = &shown.button;
    button.set_tooltip_text(Some(&label));
    button.update_property(&[Property::Label(&label)]);
    match info.as_ref().and_then(|info| info.icon()) {
        Some(icon) => shown.image.set_from_gicon(&icon),
        None => shown.image.set_icon_name(Some(entries::FALLBACK_ICON)),
    }
    if entry.windows.is_empty() {
        button.remove_css_class("running");
        button.reset_property(gtk4::AccessibleProperty::Description);
    } else {
        button.add_css_class("running");
        button.update_property(&[Property::Description(&tr("Running"))]);
    }
    if entries::active(entry) {
        button.add_css_class("active");
    } else {
        button.remove_css_class("active");
    }
}

fn entry_button<H: Host>(
    host: &Rc<H>,
    state: State,
    menu_open: &Rc<Cell<bool>>,
    position: gtk4::PositionType,
) -> Shown {
    let image = gtk4::Image::new();
    image.set_pixel_size(24);
    let button = gtk4::Button::new();
    button.set_child(Some(&image));
    button.add_css_class("bar-button");
    // A pin change changes the shape (`model::same_shape`) and rebuilds the button, so
    // whether it is draggable is decided once, here.
    let reorder_id = state.0.desktop_id.clone().filter(|_| state.0.pinned);
    let shown = Shown {
        button: button.clone(),
        image,
        state: Rc::new(RefCell::new(state)),
    };
    present(&shown);

    let menu = menu::attach(&button, position);
    let (weak_host, closed_flag) = (Rc::downgrade(host), menu_open.clone());
    menu.connect_closed(move |_| {
        closed_flag.set(false);
        // Deferred: a refresh now would remove the button that owns this popover.
        let weak_host = weak_host.clone();
        glib::idle_add_local_once(move || {
            if let Some(host) = weak_host.upgrade() {
                host.hold(false);
                host.refresh_rows();
            }
        });
    });
    let show_menu: Rc<dyn Fn()> = {
        let (weak_host, weak_menu, state, menu_open) = (
            Rc::downgrade(host),
            menu.downgrade(),
            shown.state.clone(),
            menu_open.clone(),
        );
        Rc::new(move || {
            let (Some(host), Some(menu)) = (weak_host.upgrade(), weak_menu.upgrade()) else {
                return;
            };
            let (entry, info) = state.borrow().clone();
            menu.set_child(Some(&menu_content(&host, &menu, &entry, info.as_ref())));
            menu_open.set(true);
            host.hold(true);
            host.menu_opened(&menu);
            menu.popup();
        })
    };

    let (weak_host, state, choose) = (Rc::downgrade(host), shown.state.clone(), show_menu.clone());
    button.connect_clicked(move |_| {
        let Some(host) = weak_host.upgrade() else {
            return;
        };
        let (entry, info) = state.borrow().clone();
        match model::primary(&entry) {
            Primary::Launch => {
                if let Some(info) = &info {
                    launch(&host, info);
                }
            }
            Primary::Activate(id) => {
                if let Some(window) = entry.windows.iter().find(|window| window.id == id) {
                    activate(&*host, window);
                }
            }
            Primary::Minimize(id) => {
                if let Some(client) = host.client() {
                    entries::minimize(client, id);
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
    if H::REORDER {
        if let Some(id) = reorder_id {
            reorderable(host, &button, id);
        }
    }
    shown
}

/// Dragging a pinned entry onto another pinned entry moves it to that place (BR7). The
/// drag carries the desktop id as a string; a drop carrying anything else, the same id,
/// or an id that is no longer pinned changes nothing. The host holds its surface shown
/// for the length of the drag.
fn reorderable<H: Host>(host: &Rc<H>, button: &gtk4::Button, id: String) {
    let source = gtk4::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let offered = id.clone();
    source
        .connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&offered.to_value())));
    let (weak_host, weak_button) = (Rc::downgrade(host), button.downgrade());
    source.connect_drag_begin(move |source, _| {
        if let Some(button) = weak_button.upgrade() {
            source.set_icon(Some(&gtk4::WidgetPaintable::new(Some(&button))), 0, 0);
        }
        if let Some(host) = weak_host.upgrade() {
            host.hold(true);
        }
    });
    let weak_host = Rc::downgrade(host);
    source.connect_drag_end(move |_, _, _| {
        if let Some(host) = weak_host.upgrade() {
            host.hold(false);
        }
    });
    button.add_controller(source);

    let target = gtk4::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let weak_host = Rc::downgrade(host);
    target.connect_drop(move |_, value, _, _| {
        let Ok(dragged) = value.get::<String>() else {
            return false;
        };
        let Some(host) = weak_host.upgrade() else {
            return false;
        };
        let pinned = host.favorites().ids().unwrap_or_default();
        if dragged == id || !pinned.contains(&dragged) {
            return false;
        }
        // Deferred: the change rebuilds the row that owns this target.
        let (weak_host, onto) = (Rc::downgrade(&host), id.clone());
        glib::idle_add_local_once(move || {
            let Some(host) = weak_host.upgrade() else {
                return;
            };
            if host
                .favorites()
                .change(|ids| Ok(favorites::moved(ids, &dragged, &onto)))
            {
                host.refresh_rows();
            }
        });
        true
    });
    button.add_controller(target);
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

/// Runs `act` with the host after closing the menu. The menu closes first, and the act
/// runs on idle, because pinning rebuilds the row that owns the menu.
fn on_press<H: Host>(
    button: &gtk4::Button,
    host: &Rc<H>,
    menu: &gtk4::Popover,
    act: impl Fn(&Rc<H>) + Clone + 'static,
) {
    let (weak_host, weak_menu) = (Rc::downgrade(host), menu.downgrade());
    button.connect_clicked(move |_| {
        if let Some(menu) = weak_menu.upgrade() {
            menu.popdown();
        }
        let (weak_host, act) = (weak_host.clone(), act.clone());
        glib::idle_add_local_once(move || {
            if let Some(host) = weak_host.upgrade() {
                act(&host);
            }
        });
    });
}

fn menu_content<H: Host>(
    host: &Rc<H>,
    menu: &gtk4::Popover,
    entry: &Entry<WindowId>,
    info: Option<&DesktopAppInfo>,
) -> gtk4::Box {
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    for window in &entry.windows {
        let title = entries::window_title(window);
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
        on_press(&row, host, menu, move |host: &Rc<H>| {
            activate(&**host, &target)
        });
        let id = window.id;
        on_press(&minimise, host, menu, move |host: &Rc<H>| {
            if let Some(client) = host.client() {
                entries::minimize(client, id);
            }
        });
        on_press(&close, host, menu, move |host: &Rc<H>| {
            if let Some(client) = host.client() {
                entries::close(client, id);
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
    on_press(&open, host, menu, move |host: &Rc<H>| launch(host, &app));
    list.append(&open);
    // Pinning needs the favourites and a desktop id; when either is missing there is no row.
    if host.favorites().ids().is_none() {
        return list;
    }
    let Some(id) = entry.desktop_id.clone() else {
        return list;
    };
    let pin = menu_row(&host.pin_label(entry.pinned));
    if entry.pinned {
        on_press(&pin, host, menu, move |host: &Rc<H>| {
            if host
                .favorites()
                .change(|ids| Ok(favorites::unpinned(ids, &id)))
            {
                host.refresh_rows();
            }
        });
    } else {
        on_press(&pin, host, menu, move |host: &Rc<H>| {
            if host.favorites().change(|ids| favorites::pinned(ids, &id)) {
                host.refresh_rows();
            }
        });
    }
    list.append(&pin);
    list
}

fn activate<H: Host>(host: &H, window: &Open<WindowId>) {
    if let Some(client) = host.client() {
        entries::activate(client, window);
    }
}

/// BR2: every start goes through the compositor client, behind a security context
/// (`entries::launch`).
fn launch<H: Host>(host: &Rc<H>, info: &DesktopAppInfo) {
    let (weak_host, info) = (Rc::downgrade(host), info.clone());
    glib::spawn_future_local(async move {
        let Some(host) = weak_host.upgrade() else {
            return;
        };
        let Some(client) = host.client() else { return };
        entries::launch(client, &info, H::APP).await;
    });
}
