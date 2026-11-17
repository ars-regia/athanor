//! The tray of the toolkit bar (doc_bar.md BR5), as `ui/tray.rs` and `ui/menu.rs` drew the GTK
//! one: the host of `org.kde.StatusNotifierWatcher` and one button per item that is not
//! Passive, in a group named "System tray". Left click activates (or opens the menu of an
//! `ItemIsMenu` item, or of one that does not implement `Activate`), middle click is
//! `SecondaryActivate`, right click, Shift+F10 and the Menu key open the menu, the wheel is
//! `Scroll`. The menu is `com.canonical.dbusmenu`, bounded by `dbusmenu::parse_layout`, and
//! drawn with the toolkit's menu entries: checks, radios, submenus, separators and disabled
//! rows. One host per bar, shared by every surface's row.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use athanor_bar::dbusmenu::{self, Entry, Layout, Toggle};
use athanor_bar::i18n::tr;
use athanor_bar::tray::{self as sni, Item, Status, MAX_ITEMS};
use athanor_sal::{Event, Image, Kind, Theme, Ui, WidgetId};
use gio::prelude::*;

use super::{Cx, Module, Reply, Start, Wake, Wakeup, ICON};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const MENU: &str = "com.canonical.dbusmenu";
const TIMEOUT_MS: i32 = 5000;
/// A menu opens after at most four calls (AboutToShow, GetLayout, AboutToShowGroup,
/// GetLayout): a slow or hostile item holds a click for four seconds at most (ruling 9).
const MENU_TIMEOUT_MS: i32 = 1000;
/// At most one refetch per open menu in this time, however fast the item signals.
const REFETCH_GAP: Duration = Duration::from_millis(250);
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

/// Where an item's menu answers.
#[derive(Clone)]
struct Target {
    connection: gio::DBusConnection,
    service: String,
    path: String,
}

impl Target {
    async fn call(&self, method: &str, args: glib::Variant) -> Result<glib::Variant, glib::Error> {
        self.connection
            .call_future(
                Some(&self.service),
                &self.path,
                MENU,
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                MENU_TIMEOUT_MS,
            )
            .await
    }
}

async fn layout(target: &Target) -> Option<Layout> {
    let args = (0i32, -1i32, Vec::<String>::new()).to_variant();
    match target.call("GetLayout", args).await {
        Ok(reply) => {
            let layout = dbusmenu::parse_layout(&reply);
            if layout.is_none() {
                tracing::warn!(
                    reply_type = reply.type_().as_str(),
                    "a tray menu answered GetLayout with an unexpected type"
                );
            }
            layout
        }
        Err(err) => {
            tracing::warn!(error = %err, "a tray menu did not give its layout");
            None
        }
    }
}

/// The layout, fetched once more when `AboutToShowGroup` says a submenu changed.
async fn fetch(target: &Target) -> Option<Layout> {
    let first = layout(target).await?;
    if first.submenus.is_empty() {
        return Some(first);
    }
    match target
        .call("AboutToShowGroup", (first.submenus.clone(),).to_variant())
        .await
    {
        Ok(reply) => {
            let updates = reply
                .try_child_value(0)
                .and_then(|ids| ids.get::<Vec<i32>>())
                .unwrap_or_default();
            if updates.is_empty() {
                Some(first)
            } else {
                layout(target).await.or(Some(first))
            }
        }
        Err(err) => {
            tracing::debug!(error = %err, "a tray menu does not implement AboutToShowGroup");
            Some(first)
        }
    }
}

/// The menu asked for, from the request to its close.
#[derive(Default)]
struct MenuState {
    /// The item whose menu is asked for or shown.
    item: Option<String>,
    target: Option<Target>,
    /// Bumped by every request and close: a reply for an earlier one is dropped.
    generation: u64,
    /// The layout last fetched for this request.
    layout: Option<Layout>,
    /// Bumped by every layout fetched: the open menu is drawn again.
    revision: u64,
    /// The popover shows it.
    shown: bool,
    subscriptions: Vec<gio::SignalSubscription>,
    refetching: bool,
    again: bool,
}

/// The host and the menu, shared by the bar's sources and the module.
struct Host {
    me: Weak<Host>,
    wake: Wake,
    connection: RefCell<Option<gio::DBusConnection>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    known: RefCell<Vec<Known>>,
    /// Bumped on every watcher change: replies for an earlier watcher are dropped.
    generation: Cell<u64>,
    /// Bumped on every change of the items: the buttons are drawn again.
    version: Cell<u64>,
    /// The captures asked for the menu before any item was listed.
    pending_open: Cell<bool>,
    /// The menu's layout came for a press: the bar opens it (`Module::wants_open`).
    open_asked: Cell<bool>,
    menu: RefCell<MenuState>,
}

impl Host {
    fn start(wake: &Wake) -> Rc<Host> {
        Rc::new_cyclic(|me: &Weak<Host>| {
            let (appeared, vanished) = (me.clone(), me.clone());
            // The watch lives as long as the process: gio 0.22 cannot name the id's type, and
            // dropping it does not unwatch.
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
                me: me.clone(),
                wake: wake.clone(),
                connection: RefCell::new(None),
                subscriptions: RefCell::new(Vec::new()),
                known: RefCell::new(Vec::new()),
                generation: Cell::new(0),
                version: Cell::new(0),
                pending_open: Cell::new(false),
                open_asked: Cell::new(false),
                menu: RefCell::new(MenuState::default()),
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

    /// The items changed: the buttons are drawn again, an open menu follows, and a menu the
    /// captures asked for before any item was listed is asked for now.
    fn changed(&self) {
        self.version.set(self.version.get().wrapping_add(1));
        (self.wake)(Wakeup::Changed);
        if self.pending_open.get() {
            if let Some((id, _)) = self.visible().into_iter().next() {
                self.pending_open.set(false);
                self.open_menu(&id);
            }
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

    fn menu_target(&self, id: &str) -> Option<Target> {
        let connection = self.connection.borrow().clone()?;
        let known = self.known.borrow();
        let entry = known.iter().find(|known| known.id == id)?;
        let path = entry.item.as_ref()?.menu.clone()?;
        Some(Target {
            connection,
            service: entry.service.clone(),
            path,
        })
    }

    /// Calls `method` on the item. With `menu`, an item that does not implement it has its
    /// menu opened instead: an `Activate` an item leaves out means "open my menu".
    fn call_item(&self, id: &str, method: &'static str, args: glib::Variant, menu: bool) {
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
        let (me, id) = (self.me.clone(), id.to_owned());
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
                Err(err) if menu && err.matches(gio::DBusError::UnknownMethod) => {
                    if let Some(host) = me.upgrade() {
                        host.open_menu(&id);
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, method, "a tray item did not take the call")
                }
            }
        });
    }

    /// Asks the item `id` to prepare its menu and reads the layout; the bar opens the popover
    /// once it is read. An item without a dbusmenu draws its own menu, if it has one.
    fn open_menu(&self, id: &str) {
        let Some(target) = self.menu_target(id) else {
            self.call_item(id, "ContextMenu", (0i32, 0i32).to_variant(), false);
            return;
        };
        let generation = {
            let mut menu = self.menu.borrow_mut();
            menu.generation = menu.generation.wrapping_add(1);
            menu.item = Some(id.to_owned());
            menu.target = Some(target.clone());
            menu.layout = None;
            menu.generation
        };
        let me = self.me.clone();
        glib::spawn_future_local(async move {
            // Its answer only says whether to fetch again, and the layout is fetched anyway.
            if let Err(err) = target.call("AboutToShow", (0i32,).to_variant()).await {
                tracing::debug!(error = %err, "a tray menu did not answer AboutToShow");
            }
            let Some(layout) = fetch(&target).await else {
                return;
            };
            let Some(host) = me.upgrade() else {
                return;
            };
            {
                let mut menu = host.menu.borrow_mut();
                if menu.generation != generation {
                    return;
                }
                menu.layout = Some(layout);
                menu.revision = menu.revision.wrapping_add(1);
            }
            host.open_asked.set(true);
            (host.wake)(Wakeup::Changed);
        });
    }

    /// The menu shows: it follows its item's changes from now, and the item is told.
    fn shown(&self) {
        let target = {
            let mut menu = self.menu.borrow_mut();
            if menu.shown {
                return;
            }
            menu.shown = true;
            menu.target.clone()
        };
        let Some(target) = target else {
            return;
        };
        let subscriptions = ["LayoutUpdated", "ItemsPropertiesUpdated"]
            .into_iter()
            .map(|member| {
                let me = self.me.clone();
                target.connection.subscribe_to_signal(
                    Some(&target.service),
                    Some(MENU),
                    Some(member),
                    Some(&target.path),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| {
                        if let Some(host) = me.upgrade() {
                            host.refetch();
                        }
                    },
                )
            })
            .collect();
        self.menu.borrow_mut().subscriptions = subscriptions;
        self.event(0, "opened");
    }

    /// The open menu follows its item's changes, one fetch per `REFETCH_GAP` at most; the
    /// signals during the wait or the fetch make one more.
    fn refetch(&self) {
        let (target, generation) = {
            let mut menu = self.menu.borrow_mut();
            if !menu.shown {
                return;
            }
            if menu.refetching {
                menu.again = true;
                return;
            }
            let Some(target) = menu.target.clone() else {
                return;
            };
            menu.refetching = true;
            (target, menu.generation)
        };
        let me = self.me.clone();
        glib::timeout_add_local_once(REFETCH_GAP, move || {
            glib::spawn_future_local(async move {
                let layout = layout(&target).await;
                let Some(host) = me.upgrade() else {
                    return;
                };
                let again = {
                    let mut menu = host.menu.borrow_mut();
                    menu.refetching = false;
                    // A stale fetch shows nothing, but still hands on a refresh the menu
                    // opened since asked for: `refetch` returns if the menu is closed by now.
                    if let Some(layout) = layout.filter(|_| menu.generation == generation) {
                        menu.layout = Some(layout);
                        menu.revision = menu.revision.wrapping_add(1);
                    }
                    std::mem::take(&mut menu.again)
                };
                (host.wake)(Wakeup::Changed);
                if again {
                    host.refetch();
                }
            });
        });
    }

    fn event(&self, id: i32, name: &'static str) {
        let Some(target) = self.menu.borrow().target.clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            if let Err(err) = target.call("Event", dbusmenu::event(id, name)).await {
                tracing::debug!(error = %err, event = name, "a tray menu did not take the event");
            }
        });
    }

    /// The popover closed: the item is told if its menu showed, and a reply still due is
    /// dropped.
    fn closed(&self) {
        let shown = {
            let mut menu = self.menu.borrow_mut();
            menu.generation = menu.generation.wrapping_add(1);
            menu.subscriptions.clear();
            menu.again = false;
            menu.item = None;
            menu.layout = None;
            menu.shown
        };
        if shown {
            self.event(0, "closed");
        }
        let mut menu = self.menu.borrow_mut();
        menu.shown = false;
        menu.target = None;
    }
}

/// The item's button, in `row`: its own icon name when the theme has it, else its pixmap, else
/// a generic icon; named after the item, with its tooltip.
fn item_button(cx: &mut Cx, ui: &mut Ui, row: WidgetId, item: &Item) -> WidgetId {
    let name = item.name();
    let themed = item
        .icon
        .name
        .as_deref()
        .filter(|icon| cx.icons.has(icon, ICON, cx.scale));
    let pixmap = || {
        let pixmap = item.icon.pixmap.as_ref()?;
        Image::from_rgba8(pixmap.width, pixmap.height, pixmap.rgba.clone())
            .map_err(|err| tracing::warn!(error = %err, "a tray item's pixmap cannot be drawn"))
            .ok()
    };
    let button = match themed {
        Some(icon) => cx.icons.button(ui, row, icon, ICON, cx.scale, name, None),
        None => match pixmap() {
            Some(image) => ui.icon_button(row, name, image, f32::from(ICON), None),
            None => cx
                .icons
                .button(ui, row, FALLBACK, ICON, cx.scale, name, None),
        },
    };
    let tooltip = if item.tooltip.is_empty() {
        name
    } else {
        item.tooltip.as_str()
    };
    ui.set_tooltip(button, Some(tooltip));
    button
}

/// The menu of `layout` in a popover named `name`, and the dbusmenu id of each entry that
/// reports a click.
fn build(
    theme: Theme,
    rtl: bool,
    top: bool,
    name: &str,
    layout: &Layout,
) -> (Ui, Vec<(WidgetId, i32)>) {
    let (mut ui, frame) = crate::ui::look::popover(name, theme, rtl, top);
    let menu = ui.add(frame, Kind::Menu);
    let mut acts = Vec::new();
    add_entries(&mut ui, menu, &layout.entries, &mut acts);
    (ui, acts)
}

/// Adds `entries` to `menu`; a submenu is a page of its own, beside the menu.
fn add_entries(ui: &mut Ui, menu: WidgetId, entries: &[Entry], acts: &mut Vec<(WidgetId, i32)>) {
    for entry in entries {
        let Entry::Item(item) = entry else {
            ui.add(menu, Kind::Separator);
            continue;
        };
        let widget = if item.submenu {
            let (widget, page) = ui.submenu(menu, &item.label);
            add_entries(ui, page, &item.children, acts);
            widget
        } else {
            // The item owns the state: the click is reported, and a later layout shows it.
            let widget = match item.toggle {
                Toggle::None => ui.add(menu, Kind::MenuItem(item.label.clone())),
                Toggle::Check(on) => ui.menu_check(menu, &item.label, on),
                Toggle::Radio(on) => ui.menu_radio(menu, &item.label, on),
            };
            acts.push((widget, item.id));
            widget
        };
        if !item.enabled {
            ui.set_sensitive(widget, false);
        }
    }
}

/// The item buttons in the tray's group `button`: the row it holds, in the items' order.
fn item_buttons(ui: &Ui, button: WidgetId) -> &[WidgetId] {
    ui.widget(button)
        .children
        .first()
        .map_or(&[], |row| ui.widget(*row).children.as_slice())
}

struct Tray {
    host: Rc<Host>,
    /// The host's version the buttons were last drawn at.
    built: u64,
    /// The items drawn, in the order of their buttons.
    drawn: Vec<String>,
    /// The open menu's entries with their dbusmenu ids, and the revision they were drawn at.
    menu: Option<(Vec<(WidgetId, i32)>, u64)>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    Some(Box::new(Tray {
        host: Host::start(start.wake),
        built: 0,
        drawn: Vec::new(),
        menu: None,
    }))
}

impl Module for Tray {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let tray = ui.add(group, Kind::Group(tr("System tray")));
        ui.set_horizontal(tray, true);
        let row = ui.inline(tray, [0.0, 0.0]);
        let items = self.host.visible();
        for (_, item) in &items {
            item_button(cx, ui, row, item);
        }
        ui.set_visible(tray, !items.is_empty());
        self.drawn = items.into_iter().map(|(id, _)| id).collect();
        self.built = self.host.version.get();
        Some(tray)
    }

    fn stale(&self, _cx: &Cx) -> bool {
        self.built != self.host.version.get()
    }

    fn anchor(&self, ui: &Ui, button: WidgetId) -> Option<WidgetId> {
        let asked = self.host.menu.borrow().item.clone();
        let at = asked
            .and_then(|id| self.drawn.iter().position(|drawn| *drawn == id))
            .unwrap_or(0);
        item_buttons(ui, button).get(at).copied()
    }

    fn input(&mut self, ui: &Ui, button: WidgetId, widget: WidgetId, event: &Event) {
        let Some(id) = item_buttons(ui, button)
            .iter()
            .position(|b| *b == widget)
            .and_then(|at| self.drawn.get(at))
        else {
            return;
        };
        let host = &self.host;
        match *event {
            Event::Clicked(_) => {
                if host.item(id).is_some_and(|item| item.item_is_menu) {
                    host.open_menu(id);
                } else {
                    host.call_item(id, "Activate", (0i32, 0i32).to_variant(), true);
                }
            }
            Event::ContextMenu(_) => host.open_menu(id),
            Event::MiddleClick(_) => {
                host.call_item(id, "SecondaryActivate", (0i32, 0i32).to_variant(), false);
            }
            Event::Wheel { dx, dy, .. } => {
                for (delta, orientation) in [(dy, "vertical"), (dx, "horizontal")] {
                    if let Some(steps) = sni::scroll_delta(f64::from(delta)) {
                        host.call_item(id, "Scroll", (steps, orientation).to_variant(), false);
                    }
                }
            }
            _ => {}
        }
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let ready = {
            let menu = self.host.menu.borrow();
            match (&menu.item, &menu.layout) {
                (Some(id), Some(layout)) => Some((id.clone(), layout.clone(), menu.revision)),
                _ => None,
            }
        };
        let Some((id, layout, revision)) = ready else {
            // Asked for by the captures: the first item's menu, once an item is listed.
            if self.host.menu.borrow().item.is_none() {
                match self.host.visible().into_iter().next() {
                    Some((id, _)) => self.host.open_menu(&id),
                    None => self.host.pending_open.set(true),
                }
            }
            return None;
        };
        let item = self.host.item(&id)?;
        let (ui, acts) = build(cx.theme.clone(), cx.rtl, cx.top, item.name(), &layout);
        self.menu = Some((acts, revision));
        self.host.shown();
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, _ui: &mut Ui, event: Event) -> Reply {
        let (Event::Clicked(widget) | Event::Toggled(widget)) = event else {
            return Reply::Stay;
        };
        let id = self
            .menu
            .as_ref()
            .and_then(|(acts, _)| acts.iter().find(|(w, _)| *w == widget))
            .map(|(_, id)| *id);
        match id {
            Some(id) => {
                self.host.event(id, "clicked");
                Reply::Close
            }
            None => Reply::Stay,
        }
    }

    fn refresh(&mut self, _cx: &mut Cx, _ui: &mut Ui) -> Reply {
        let menu = self.host.menu.borrow();
        // The menu goes with its item, and is drawn again from a new layout.
        if !menu
            .item
            .as_ref()
            .is_some_and(|id| self.host.item(id).is_some())
        {
            return Reply::Close;
        }
        match &self.menu {
            Some((_, drawn)) if *drawn != menu.revision => Reply::Reopen,
            _ => Reply::Stay,
        }
    }

    fn wants_open(&mut self) -> bool {
        self.host.open_asked.replace(false)
    }

    fn closed(&mut self) {
        self.menu = None;
        self.host.closed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_bar::dbusmenu::Item as Row;
    use athanor_sal::Mark;

    fn row(id: i32, label: &str, toggle: Toggle) -> Row {
        Row {
            id,
            label: label.into(),
            enabled: true,
            toggle,
            submenu: false,
            children: Vec::new(),
        }
    }

    /// tray_item.py's menu: an item, a check, two radios, a disabled item and a submenu.
    fn layout() -> Layout {
        let sync = Row {
            enabled: false,
            ..row(5, "Sync now", Toggle::None)
        };
        let more = Row {
            submenu: true,
            children: vec![Entry::Item(row(7, "About", Toggle::None))],
            ..row(6, "More", Toggle::None)
        };
        Layout {
            revision: 1,
            entries: vec![
                Entry::Item(row(1, "Open window", Toggle::None)),
                Entry::Separator,
                Entry::Item(row(2, "Mute", Toggle::Check(true))),
                Entry::Separator,
                Entry::Item(row(3, "Low quality", Toggle::Radio(false))),
                Entry::Item(row(4, "High quality", Toggle::Radio(true))),
                Entry::Separator,
                Entry::Item(sync),
                Entry::Item(more),
            ],
            submenus: vec![6],
        }
    }

    #[test]
    fn the_menu_draws_each_kind_of_entry_and_reports_the_clickable_ones() {
        let (ui, acts) = build(Theme::default(), false, true, "Test item", &layout());
        let ids: Vec<i32> = acts.iter().map(|(_, id)| *id).collect();
        assert_eq!(
            ids,
            [1, 2, 3, 4, 5, 7],
            "a submenu's own entry reports no click"
        );
        let entry = |id: i32| {
            acts.iter()
                .find(|(_, i)| *i == id)
                .map(|(w, _)| *w)
                .unwrap_or_else(|| panic!("no entry {id}"))
        };
        let kind = |id| &ui.widget(entry(id)).kind;
        assert!(matches!(kind(1), Kind::MenuItem(label) if label == "Open window"));
        assert!(matches!(
            kind(2),
            Kind::MenuEntry {
                mark: Mark::Check(true),
                ..
            }
        ));
        assert!(matches!(
            kind(3),
            Kind::MenuEntry {
                mark: Mark::Radio(false),
                ..
            }
        ));
        assert!(matches!(
            kind(4),
            Kind::MenuEntry {
                mark: Mark::Radio(true),
                ..
            }
        ));
        assert!(
            !ui.is_sensitive(entry(5)),
            "a disabled entry takes no input"
        );
        assert!(ui.is_sensitive(entry(1)));
        let menu = ui.widget(entry(1)).parent.unwrap_or(Ui::ROOT);
        assert_ne!(
            ui.widget(entry(7)).parent,
            Some(menu),
            "About is on its own page"
        );
        let separators = ui
            .widget(menu)
            .children
            .iter()
            .filter(|c| matches!(ui.widget(**c).kind, Kind::Separator))
            .count();
        assert_eq!(separators, 3);
        let submenu = ui.widget(menu).children.last().copied().unwrap_or(Ui::ROOT);
        assert!(
            matches!(&ui.widget(submenu).kind, Kind::MenuEntry { label, mark: Mark::Submenu(_), .. } if label == "More")
        );
    }
}
