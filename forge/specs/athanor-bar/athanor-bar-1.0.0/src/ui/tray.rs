//! The tray (doc_bar.md BR5): the host of `org.kde.StatusNotifierWatcher` and one button per
//! item that is not Passive. Left click activates (or opens the menu of an `ItemIsMenu`
//! item, or of one that does not implement `Activate`), middle click is
//! `SecondaryActivate`, right click, Shift+F10 and the Menu key open the menu, the wheel is
//! `Scroll`. One host per bar (`Bar::tray`), shared by every surface's row.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_bar::order::Module;
use athanor_bar::tray::{self as sni, Item, Status, MAX_ITEMS};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use super::menu::Menu;
use super::notifications::{has_icon, texture};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const TIMEOUT_MS: i32 = 5000;
const ICON_PX: i32 = 16;
/// The device pixels a pixmap is chosen for: 16 logical pixels up to scale 2.
const WANTED_PX: u32 = 32;
const FALLBACK: &str = "application-x-executable-symbolic";

struct Known {
    id: String,
    service: String,
    path: String,
    item: Option<Item>,
    fetching: bool,
    again: bool,
    _subscription: gio::SignalSubscription,
}

pub struct Host {
    bar: Weak<Bar>,
    me: Weak<Host>,
    connection: RefCell<Option<gio::DBusConnection>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    known: RefCell<Vec<Known>>,
    pending_open: Cell<bool>,
    /// Bumped on every watcher change: replies for an earlier watcher are dropped.
    generation: Cell<u64>,
}

impl Host {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Host> {
        Rc::new_cyclic(|me: &Weak<Host>| {
            let (appeared, vanished) = (me.clone(), me.clone());
            // The watch lives as long as the process, as in `notifications::Service::start`:
            // gio 0.22 cannot name the id's type, and dropping it does not unwatch.
            let _watch = gio::bus_watch_name(
                gio::BusType::Session,
                WATCHER,
                gio::BusNameWatcherFlags::NONE,
                move |connection, _, owner| {
                    if let Some(host) = appeared.upgrade() {
                        host.appeared(connection, owner);
                    }
                },
                move |_, _| {
                    if let Some(host) = vanished.upgrade() {
                        host.reset();
                        host.changed();
                    }
                },
            );
            Host {
                bar: bar.clone(),
                me: me.clone(),
                connection: RefCell::new(None),
                subscriptions: RefCell::new(Vec::new()),
                known: RefCell::new(Vec::new()),
                pending_open: Cell::new(false),
                generation: Cell::new(0),
            }
        })
    }

    fn appeared(&self, connection: gio::DBusConnection, owner: &str) {
        self.reset();
        let generation = self.generation.get();
        let subscriptions = [
            ("StatusNotifierItemRegistered", true),
            ("StatusNotifierItemUnregistered", false),
        ]
        .into_iter()
        .map(|(member, registered)| {
            let me = self.me.clone();
            connection.subscribe_to_signal(
                Some(owner),
                Some(WATCHER),
                Some(member),
                Some(WATCHER_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let Some(host) = me.upgrade() else {
                        return;
                    };
                    let Some((id,)) = signal.parameters.get::<(String,)>() else {
                        tracing::warn!(
                            member,
                            "the tray watcher sent a signal with an unexpected type"
                        );
                        return;
                    };
                    if registered {
                        host.add(&id);
                    } else {
                        host.known.borrow_mut().retain(|known| known.id != id);
                    }
                    host.changed();
                },
            )
        })
        .collect();
        self.subscriptions.replace(subscriptions);
        self.connection.replace(Some(connection.clone()));
        let (me, owner) = (self.me.clone(), owner.to_owned());
        glib::spawn_future_local(async move {
            let unique = connection
                .unique_name()
                .map(|name| name.to_string())
                .unwrap_or_default();
            if let Err(err) = connection
                .call_future(
                    Some(&owner),
                    WATCHER_PATH,
                    WATCHER,
                    "RegisterStatusNotifierHost",
                    Some(&(unique,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await
            {
                tracing::warn!(error = %err, "the tray watcher did not register the bar as its host");
            }
            let reply = connection
                .call_future(
                    Some(&owner),
                    WATCHER_PATH,
                    PROPERTIES,
                    "Get",
                    Some(&(WATCHER, "RegisteredStatusNotifierItems").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            let Some(host) = me.upgrade() else {
                return;
            };
            if host.generation.get() != generation {
                return;
            }
            match reply {
                Ok(reply) => {
                    let ids = reply
                        .try_child_value(0)
                        .and_then(|boxed| boxed.as_variant())
                        .and_then(|ids| ids.get::<Vec<String>>())
                        .unwrap_or_default();
                    for id in ids.iter().take(MAX_ITEMS) {
                        host.add(id);
                    }
                    host.changed();
                }
                Err(err) => tracing::warn!(error = %err, "the tray watcher did not list its items"),
            }
        });
    }

    fn add(&self, id: &str) {
        {
            let known = self.known.borrow();
            if known.iter().any(|known| known.id == id) {
                return;
            }
            if known.len() >= MAX_ITEMS {
                tracing::warn!("{MAX_ITEMS} tray items are shown; another one is ignored");
                return;
            }
        }
        let Some((service, path)) = sni::split_id(id) else {
            tracing::warn!(
                "the tray watcher announced an item id that is not a bus name and a path"
            );
            return;
        };
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        let (me, key) = (self.me.clone(), id.to_owned());
        let subscription = connection.subscribe_to_signal(
            Some(&service),
            Some(ITEM),
            None,
            Some(&path),
            None,
            gio::DBusSignalFlags::NONE,
            move |_| {
                if let Some(host) = me.upgrade() {
                    host.fetch(&key);
                }
            },
        );
        self.known.borrow_mut().push(Known {
            id: id.to_owned(),
            service,
            path,
            item: None,
            fetching: false,
            again: false,
            _subscription: subscription,
        });
        self.fetch(id);
    }

    fn fetch(&self, id: &str) {
        let (service, path) = {
            let mut known = self.known.borrow_mut();
            let Some(entry) = known.iter_mut().find(|known| known.id == id) else {
                return;
            };
            if entry.fetching {
                entry.again = true;
                return;
            }
            entry.fetching = true;
            (entry.service.clone(), entry.path.clone())
        };
        let Some(connection) = self.connection.borrow().clone() else {
            return;
        };
        let (me, id, generation) = (self.me.clone(), id.to_owned(), self.generation.get());
        glib::spawn_future_local(async move {
            let reply = connection
                .call_future(
                    Some(&service),
                    &path,
                    PROPERTIES,
                    "GetAll",
                    Some(&(ITEM,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            if let Some(host) = me
                .upgrade()
                .filter(|host| host.generation.get() == generation)
            {
                host.fetched(&id, reply);
            }
        });
    }

    fn fetched(&self, id: &str, reply: Result<glib::Variant, glib::Error>) {
        let again = {
            let mut known = self.known.borrow_mut();
            let Some(entry) = known.iter_mut().find(|known| known.id == id) else {
                return;
            };
            entry.fetching = false;
            match reply {
                Ok(reply) => match reply
                    .try_child_value(0)
                    .and_then(|props| sni::read(&props, WANTED_PX))
                {
                    Some(item) => entry.item = Some(item),
                    None => tracing::warn!(
                        "a tray item answered GetAll with an unexpected type; it stays hidden"
                    ),
                },
                Err(err) => tracing::warn!(error = %err, "a tray item did not give its properties"),
            }
            std::mem::take(&mut entry.again)
        };
        if again {
            self.fetch(id);
        }
        self.changed();
    }

    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.subscriptions.borrow_mut().clear();
        self.known.borrow_mut().clear();
        self.connection.take();
    }

    fn changed(&self) {
        let Some(bar) = self.bar.upgrade() else {
            return;
        };
        bar.refresh(Changed::Tray);
        if self.pending_open.get() && !self.visible().is_empty() {
            self.pending_open.set(false);
            let weak = self.bar.clone();
            glib::idle_add_local_once(move || {
                if let Some(bar) = weak.upgrade() {
                    bar.open_module(Module::Tray);
                }
            });
        }
    }

    /// The items to draw, in the order they registered. A Passive item is hidden (BR5).
    fn visible(&self) -> Vec<(String, Item)> {
        self.known
            .borrow()
            .iter()
            .filter_map(|known| Some((known.id.clone(), known.item.clone()?)))
            .filter(|(_, item)| item.status != Status::Passive)
            .collect()
    }

    fn item(&self, id: &str) -> Option<Item> {
        self.known
            .borrow()
            .iter()
            .find(|known| known.id == id)
            .and_then(|known| known.item.clone())
    }

    fn menu_target(&self, id: &str) -> Option<(gio::DBusConnection, String, String)> {
        let connection = self.connection.borrow().clone()?;
        let known = self.known.borrow();
        let entry = known.iter().find(|known| known.id == id)?;
        let path = entry.item.as_ref()?.menu.clone()?;
        Some((connection, entry.service.clone(), path))
    }

    /// Calls `method` on the item. `fallback` runs instead when the item does not implement
    /// it: an `Activate` an item leaves out means "open my menu".
    fn call_item(
        &self,
        id: &str,
        method: &'static str,
        args: glib::Variant,
        fallback: Option<Rc<dyn Fn()>>,
    ) {
        let target = self
            .known
            .borrow()
            .iter()
            .find(|known| known.id == id)
            .map(|known| (known.service.clone(), known.path.clone()));
        let (Some((service, path)), Some(connection)) = (target, self.connection.borrow().clone())
        else {
            return;
        };
        glib::spawn_future_local(async move {
            match connection
                .call_future(
                    Some(&service),
                    &path,
                    ITEM,
                    method,
                    Some(&args),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await
            {
                Ok(_) => {}
                Err(err) if err.matches(gio::DBusError::UnknownMethod) && fallback.is_some() => {
                    if let Some(open) = fallback {
                        open();
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, method, "a tray item did not take the call")
                }
            }
        });
    }

    fn open_when_listed(&self) {
        self.pending_open.set(true);
    }
}

struct Shown {
    id: String,
    button: gtk4::Button,
    image: gtk4::Image,
    menu: RefCell<Option<Rc<Menu>>>,
}

/// The item's own icon name when the theme has it, else its pixmap, else a generic icon.
fn draw(shown: &Shown, item: &Item) {
    let name = item.name();
    let tooltip = if item.tooltip.is_empty() {
        name
    } else {
        item.tooltip.as_str()
    };
    shown.button.set_tooltip_text(Some(tooltip));
    shown.button.update_property(&[Property::Label(name)]);
    let themed = item.icon.name.as_deref().filter(|icon| has_icon(icon));
    let pixmap = item
        .icon
        .pixmap
        .as_ref()
        .and_then(|pixmap| texture(pixmap.width, pixmap.height, &pixmap.rgba));
    match (themed, pixmap) {
        (Some(icon), _) => shown.image.set_icon_name(Some(icon)),
        (None, Some(paintable)) => shown.image.set_paintable(Some(&paintable)),
        (None, None) => shown.image.set_icon_name(Some(FALLBACK)),
    }
}

fn open_menu(bar: &Rc<Bar>, host: &Host, shown: &Shown) {
    match host.menu_target(&shown.id) {
        Some((connection, service, path)) => {
            let menu = shown
                .menu
                .borrow_mut()
                .get_or_insert_with(|| Menu::new(bar, &shown.button))
                .clone();
            menu.open(connection, service, path);
        }
        // No dbusmenu: the item draws its own menu, if it has one.
        None => host.call_item(&shown.id, "ContextMenu", (0i32, 0i32).to_variant(), None),
    }
}

fn item_button(bar: &Rc<Bar>, host: &Rc<Host>, id: &str) -> Rc<Shown> {
    let image = gtk4::Image::new();
    image.set_pixel_size(ICON_PX);
    let button = gtk4::Button::new();
    button.set_child(Some(&image));
    button.add_css_class("bar-button");
    let shown = Rc::new(Shown {
        id: id.to_owned(),
        button: button.clone(),
        image,
        menu: RefCell::new(None),
    });
    let open: Rc<dyn Fn()> = {
        let (bar, host, shown) = (
            Rc::downgrade(bar),
            Rc::downgrade(host),
            Rc::downgrade(&shown),
        );
        Rc::new(move || {
            if let (Some(bar), Some(host), Some(shown)) =
                (bar.upgrade(), host.upgrade(), shown.upgrade())
            {
                open_menu(&bar, &host, &shown);
            }
        })
    };

    let (weak_host, key, menu) = (Rc::downgrade(host), id.to_owned(), open.clone());
    button.connect_clicked(move |_| {
        let Some(host) = weak_host.upgrade() else {
            return;
        };
        if host.item(&key).is_some_and(|item| item.item_is_menu) {
            menu();
        } else {
            host.call_item(
                &key,
                "Activate",
                (0i32, 0i32).to_variant(),
                Some(menu.clone()),
            );
        }
    });

    let middle = gtk4::GestureClick::new();
    middle.set_button(gdk::BUTTON_MIDDLE);
    let (weak_host, key) = (Rc::downgrade(host), id.to_owned());
    middle.connect_released(move |_, _, _, _| {
        if let Some(host) = weak_host.upgrade() {
            host.call_item(&key, "SecondaryActivate", (0i32, 0i32).to_variant(), None);
        }
    });
    button.add_controller(middle);

    let secondary = gtk4::GestureClick::new();
    secondary.set_button(gdk::BUTTON_SECONDARY);
    let menu = open.clone();
    secondary.connect_pressed(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        menu();
    });
    button.add_controller(secondary);

    let keys = gtk4::ShortcutController::new();
    if let Some(trigger) = gtk4::ShortcutTrigger::parse_string("<Shift>F10|Menu") {
        let menu = open;
        let action = gtk4::CallbackAction::new(move |_, _| {
            menu();
            glib::Propagation::Stop
        });
        keys.add_shortcut(gtk4::Shortcut::new(Some(trigger), Some(action)));
    }
    button.add_controller(keys);

    let scroll = gtk4::EventControllerScroll::new(
        gtk4::EventControllerScrollFlags::BOTH_AXES | gtk4::EventControllerScrollFlags::DISCRETE,
    );
    let (weak_host, key) = (Rc::downgrade(host), id.to_owned());
    scroll.connect_scroll(move |_, dx, dy| {
        let Some(host) = weak_host.upgrade() else {
            return glib::Propagation::Proceed;
        };
        for (delta, orientation) in [(dy, "vertical"), (dx, "horizontal")] {
            if let Some(steps) = sni::scroll_delta(delta) {
                host.call_item(&key, "Scroll", (steps, orientation).to_variant(), None);
            }
        }
        glib::Propagation::Stop
    });
    button.add_controller(scroll);
    shown
}

struct TrayUi {
    row: gtk4::Box,
    host: Rc<Host>,
    shown: RefCell<Vec<Rc<Shown>>>,
}

impl ModuleUi for TrayUi {
    fn widget(&self) -> gtk4::Widget {
        self.row.clone().upcast()
    }

    fn refresh(&self, bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Tray {
            return;
        }
        let items = self.host.visible();
        let mut shown = self.shown.borrow_mut();
        shown.retain(|button| {
            let keep = items.iter().any(|(id, _)| *id == button.id);
            if !keep {
                if let Some(menu) = button.menu.borrow().as_ref() {
                    menu.popdown();
                }
                self.row.remove(&button.button);
            }
            keep
        });
        for (id, item) in &items {
            let button = match shown.iter().find(|button| button.id == *id) {
                Some(button) => button.clone(),
                None => {
                    let button = item_button(bar, &self.host, id);
                    self.row.append(&button.button);
                    shown.push(button.clone());
                    button
                }
            };
            draw(&button, item);
        }
        self.row.set_visible(!items.is_empty());
    }

    fn open(&self, bar: &Rc<Bar>) {
        let first = self.shown.borrow().first().cloned();
        match first {
            Some(button) => open_menu(bar, &self.host, &button),
            None => self.host.open_when_listed(),
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(2)
        .accessible_role(gtk4::AccessibleRole::Group)
        .build();
    row.update_property(&[Property::Label(&tr("System tray"))]);
    row.set_visible(false);
    Some(Box::new(TrayUi {
        row,
        host: bar.tray.clone(),
        shown: RefCell::new(Vec::new()),
    }))
}
