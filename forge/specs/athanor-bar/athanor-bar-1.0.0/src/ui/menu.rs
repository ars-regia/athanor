//! A tray item's menu (doc_bar.md BR5): `com.canonical.dbusmenu`, bounded by
//! `dbusmenu::parse_layout`, drawn as a `PopoverMenu` whose actions send `Event`. Labels are
//! plain text; an underscore is doubled so GTK draws it instead of taking it as a mnemonic.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use athanor_bar::dbusmenu::{self, Entry, Layout, Toggle};
use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::{popup, Bar};

const INTERFACE: &str = "com.canonical.dbusmenu";
/// A menu opens after at most four calls (AboutToShow, GetLayout, AboutToShowGroup,
/// GetLayout): a slow or hostile item holds a click for four seconds at most (ruling 9).
const TIMEOUT_MS: i32 = 1000;
/// At most one refetch per open menu in this time, however fast the item signals.
const REFETCH_GAP: Duration = Duration::from_millis(250);

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
                INTERFACE,
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                TIMEOUT_MS,
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

pub(super) struct Menu {
    me: Weak<Menu>,
    bar: Weak<Bar>,
    popover: gtk4::PopoverMenu,
    group: gio::SimpleActionGroup,
    target: RefCell<Option<Target>>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    /// Bumped by every open and close: a reply for an earlier one is dropped.
    generation: Cell<u64>,
    refetching: Cell<bool>,
    again: Cell<bool>,
}

impl Menu {
    pub(super) fn new(bar: &Rc<Bar>, button: &gtk4::Button) -> Rc<Menu> {
        let popover = gtk4::PopoverMenu::from_model(None::<&gio::MenuModel>);
        popup::attach_popover(bar, button, &popover);
        let group = gio::SimpleActionGroup::new();
        popover.insert_action_group("m", Some(&group));
        Rc::new_cyclic(|me: &Weak<Menu>| {
            let weak = me.clone();
            popover.connect_closed(move |_| {
                if let Some(menu) = weak.upgrade() {
                    menu.closed();
                }
            });
            Menu {
                me: me.clone(),
                bar: Rc::downgrade(bar),
                popover,
                group,
                target: RefCell::new(None),
                subscriptions: RefCell::new(Vec::new()),
                generation: Cell::new(0),
                refetching: Cell::new(false),
                again: Cell::new(false),
            }
        })
    }

    /// Asks the item to prepare, reads the layout, and pops the menu up once it is built.
    pub(super) fn open(&self, connection: gio::DBusConnection, service: String, path: String) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let target = Target {
            connection,
            service,
            path,
        };
        self.target.replace(Some(target.clone()));
        let me = self.me.clone();
        glib::spawn_future_local(async move {
            // Its answer only says whether to fetch again, and the layout is fetched anyway.
            if let Err(err) = target.call("AboutToShow", (0i32,).to_variant()).await {
                tracing::debug!(error = %err, "a tray menu did not answer AboutToShow");
            }
            let Some(layout) = fetch(&target).await else {
                return;
            };
            let Some(menu) = me.upgrade() else {
                return;
            };
            if menu.generation.get() != generation {
                return;
            }
            menu.show(&layout);
            menu.watch(&target);
            if let Some(bar) = menu.bar.upgrade() {
                bar.popover_opened(menu.popover.upcast_ref());
            }
            menu.popover.popup();
            menu.event(0, "opened");
        });
    }

    pub(super) fn popdown(&self) {
        self.popover.popdown();
    }

    fn show(&self, layout: &Layout) {
        for name in self.group.list_actions() {
            self.group.remove_action(&name);
        }
        let model = self.model(&layout.entries);
        self.popover.set_menu_model(Some(&model));
    }

    /// Separators split sections, as GMenu draws them.
    fn model(&self, entries: &[Entry]) -> gio::Menu {
        let menu = gio::Menu::new();
        let mut section = gio::Menu::new();
        for entry in entries {
            match entry {
                Entry::Separator => {
                    menu.append_section(None, &section);
                    section = gio::Menu::new();
                }
                Entry::Item(item) => section.append_item(&self.item(item)),
            }
        }
        menu.append_section(None, &section);
        menu
    }

    /// A check item is a boolean state, a radio item a string state with the target "on":
    /// that is how a `PopoverMenu` knows to draw a check or a radio.
    fn item(&self, item: &dbusmenu::Item) -> gio::MenuItem {
        let label = item.label.replace('_', "__");
        if item.submenu {
            return gio::MenuItem::new_submenu(Some(&label), &self.model(&item.children));
        }
        let name = format!("i{}", item.id);
        let action = match item.toggle {
            Toggle::None => gio::SimpleAction::new(&name, None),
            Toggle::Check(on) => gio::SimpleAction::new_stateful(&name, None, &on.to_variant()),
            Toggle::Radio(on) => gio::SimpleAction::new_stateful(
                &name,
                Some(glib::VariantTy::STRING),
                &(if on { "on" } else { "off" }).to_variant(),
            ),
        };
        action.set_enabled(item.enabled);
        let (me, id) = (self.me.clone(), item.id);
        // The item owns the state: the click is reported, and a later layout shows it.
        action.connect_activate(move |_, _| {
            if let Some(menu) = me.upgrade() {
                menu.event(id, "clicked");
            }
        });
        self.group.add_action(&action);
        let detailed = match item.toggle {
            Toggle::Radio(_) => format!("m.{name}::on"),
            Toggle::None | Toggle::Check(_) => format!("m.{name}"),
        };
        gio::MenuItem::new(Some(&label), Some(&detailed))
    }

    fn watch(&self, target: &Target) {
        let subscriptions = ["LayoutUpdated", "ItemsPropertiesUpdated"]
            .into_iter()
            .map(|member| {
                let me = self.me.clone();
                target.connection.subscribe_to_signal(
                    Some(&target.service),
                    Some(INTERFACE),
                    Some(member),
                    Some(&target.path),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| {
                        if let Some(menu) = me.upgrade() {
                            menu.refetch();
                        }
                    },
                )
            })
            .collect();
        self.subscriptions.replace(subscriptions);
    }

    /// The open menu follows its item's changes, one fetch per `REFETCH_GAP` at most; the
    /// signals during the wait or the fetch make one more.
    fn refetch(&self) {
        if !self.popover.is_visible() {
            return;
        }
        if self.refetching.replace(true) {
            self.again.set(true);
            return;
        }
        let Some(target) = self.target.borrow().clone() else {
            self.refetching.set(false);
            return;
        };
        let (me, generation) = (self.me.clone(), self.generation.get());
        glib::timeout_add_local_once(REFETCH_GAP, move || {
            glib::spawn_future_local(async move {
                let layout = layout(&target).await;
                let Some(menu) = me.upgrade() else {
                    return;
                };
                menu.refetching.set(false);
                if menu.generation.get() != generation {
                    return;
                }
                if let Some(layout) = layout {
                    menu.show(&layout);
                }
                if menu.again.replace(false) {
                    menu.refetch();
                }
            });
        });
    }

    fn event(&self, id: i32, name: &'static str) {
        let Some(target) = self.target.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            if let Err(err) = target.call("Event", dbusmenu::event(id, name)).await {
                tracing::debug!(error = %err, event = name, "a tray menu did not take the event");
            }
        });
    }

    fn closed(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.subscriptions.borrow_mut().clear();
        self.again.set(false);
        self.event(0, "closed");
    }
}
