//! The bar's notifications (doc_bar.md BR3, BR4), as the GTK bar's `ui/notifications.rs` and
//! `ui/popups.rs` drew them: the service that talks to athanor-shelld's private interface, the
//! card every notification is drawn as, the list popover with "Clear all" and do not disturb,
//! and the popups. One service for the bar: the button on every surface reads it, so two
//! outputs never mean two `List` calls.
//!
//! The popups are layer surfaces of the module's own, one per output, at the end corner on the
//! panel's side, above windows, never taking the keyboard (ruling 14), the newest nearest the
//! panel. A surface opens on the output its first popup goes to and is kept: with nothing to
//! show it holds no widget, takes no input and leaves the accessibility tree, as the GTK bar
//! kept its windows mapped.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use athanor_apps::sal::entry_icon;
use athanor_bar::control_center;
use athanor_bar::i18n::{tr, tr_with};
use athanor_bar::notices::{self, Held, Notice, Picture, WIRE_SIGNATURE};
use athanor_bar::popups::{place, target_output, Popups};
use athanor_sal::driver::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Shell, SurfaceId};
use athanor_sal::{
    Align, Event, IconTheme, Image, Kind, Rgba, TextStyle, Theme, Ui, WidgetId, Wrap,
};
use athanor_style::appearance::Appearance;
use athanor_style::sal;
use athanor_sulphur_client::Client;
use gio::{glib, prelude::*};
use gio_unix::DesktopAppInfo;

use super::{Around, Cx, Module, Reply, Start, Wake, Wakeup, ICON};
use crate::ui::look;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/os/athanor/Notifications1";
const INTERFACE: &str = "os.athanor.Notifications1";
const TIMEOUT_MS: i32 = 5000;
/// A `List` that failed for another reason than a refusal (a slow daemon at session start)
/// is asked once more after this.
const LIST_RETRY: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(250);
/// Signals that arrive between the admission and the `List` reply (ruling 13).
const EARLY_SIGNALS: usize = 256;
const EXPIRED: u32 = 1;
const DISMISSED: u32 = 2;
const PICTURE: u16 = 32;
const CARD_WIDTH: f32 = 360.0;
/// A popup card's corners and padding, as calmo's `.notification-card`; the list's cards take
/// the popover's.
const CARD_RADIUS: f32 = 18.0;
const CARD_PADDING: f32 = 12.0;
const TEXT_CHARS: u32 = 36;
const LIST_HEIGHT: f32 = 480.0;
const FALLBACK_ICON: &str = "dialog-information-symbolic";
/// The icon theme the GTK bar drew the cards with.
const ICON_THEME: &str = "Adwaita";
const NAMESPACE: &str = "athanor-notifications";
/// The popups' distance from the panel's edge and from the end edge.
const MARGIN: i32 = 8;
/// A popup's margin around its card, as `.athanor-notifications .notification-card`: top,
/// right, bottom, left, the room of its shadow in GTK.
const POPUP_MARGIN: [f32; 4] = [6.0, 14.0, 14.0, 14.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No daemon on the bus, or it failed: the button is hidden (SH1).
    Absent,
    /// `List` is in flight; signals are queued.
    Listing,
    /// The daemon refused the bar: not in `athanor-bar.service` (BR1).
    Refused,
    Live,
}

struct Service {
    wake: Wake,
    client: Option<Rc<Client>>,
    me: Weak<Service>,
    state: Cell<State>,
    /// The connection and the daemon's unique name, while it owns the name.
    peer: RefCell<Option<(gio::DBusConnection, String)>>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    /// Bumped on every owner change: a `List` reply from a previous owner is dropped.
    generation: Cell<u64>,
    /// The one `List` retry for this owner is spent.
    retried: Cell<bool>,
    early: RefCell<VecDeque<(String, glib::Variant)>>,
    held: RefCell<Held>,
    /// Bumped whenever `held` changes: the cards drawn from it are drawn again.
    revision: Cell<u64>,
    popups: RefCell<Popups>,
    dnd: Cell<bool>,
    ticking: Cell<bool>,
    last_tick: Cell<Option<Instant>>,
    /// A popover of the bar is open, or the pointer is over a popup: set by the module at
    /// every turn of the bar.
    paused: Cell<bool>,
    /// The control center's `Open` (CC9): false while it has no owner. The popups stay
    /// hidden while it is true.
    control_center_open: Cell<bool>,
    control_center_subscription: RefCell<Option<gio::SignalSubscription>>,
    /// Bumped on every owner change of the control center: a `GetAll` reply from a previous
    /// owner is dropped.
    control_center_generation: Cell<u64>,
}

impl Service {
    fn start(wake: Wake, client: Option<Rc<Client>>) -> Rc<Service> {
        Rc::new_cyclic(|me: &Weak<Service>| {
            let (appeared, vanished) = (me.clone(), me.clone());
            // The watches live as long as the process: the service is the bar's. Their ids
            // are not kept, since gio 0.22 cannot name the type (`dbus::WatcherId` is
            // shadowed by `dbus_connection::WatcherId`) and dropping one does not unwatch.
            let _watch = gio::bus_watch_name(
                gio::BusType::Session,
                NAME,
                gio::BusNameWatcherFlags::NONE,
                move |connection, _, owner| {
                    if let Some(service) = appeared.upgrade() {
                        service.appeared(connection, owner);
                    }
                },
                move |_, _| {
                    if let Some(service) = vanished.upgrade() {
                        service.vanished();
                    }
                },
            );
            let (cc_appeared, cc_vanished) = (me.clone(), me.clone());
            let _control_center = gio::bus_watch_name(
                gio::BusType::Session,
                control_center::NAME,
                gio::BusNameWatcherFlags::NONE,
                move |connection, _, owner| {
                    if let Some(service) = cc_appeared.upgrade() {
                        service.control_center_appeared(connection, owner);
                    }
                },
                move |_, _| {
                    if let Some(service) = cc_vanished.upgrade() {
                        service.control_center_vanished();
                    }
                },
            );
            Service {
                wake,
                client,
                me: me.clone(),
                state: Cell::new(State::Absent),
                peer: RefCell::new(None),
                subscription: RefCell::new(None),
                generation: Cell::new(0),
                retried: Cell::new(false),
                early: RefCell::new(VecDeque::new()),
                held: RefCell::new(Held::default()),
                revision: Cell::new(0),
                popups: RefCell::new(Popups::default()),
                dnd: Cell::new(false),
                ticking: Cell::new(false),
                last_tick: Cell::new(None),
                paused: Cell::new(false),
                control_center_open: Cell::new(false),
                control_center_subscription: RefCell::new(None),
                control_center_generation: Cell::new(0),
            }
        })
    }

    /// The control center owns its name: follow `Open`. The signal is subscribed before
    /// `GetAll` is asked, so no change falls between them.
    fn control_center_appeared(&self, connection: gio::DBusConnection, owner: &str) {
        let generation = self.control_center_generation.get().wrapping_add(1);
        self.control_center_generation.set(generation);
        let me = self.me.clone();
        let subscription = connection.subscribe_to_signal(
            Some(owner),
            Some("org.freedesktop.DBus.Properties"),
            Some("PropertiesChanged"),
            Some(control_center::PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let (Some(service), Some(open)) = (
                    me.upgrade(),
                    control_center::open_in_change(signal.parameters),
                ) {
                    service.set_control_center_open(open);
                }
            },
        );
        self.control_center_subscription.replace(Some(subscription));
        let me = self.me.clone();
        let owner = owner.to_owned();
        glib::spawn_future_local(async move {
            let reply = athanor_bar::bus::call(
                &connection,
                &owner,
                control_center::PATH,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                Some(&(control_center::NAME,).to_variant()),
                athanor_bar::bus::TIMEOUT_MS,
            )
            .await;
            let Some(service) = me
                .upgrade()
                .filter(|service| service.control_center_generation.get() == generation)
            else {
                return;
            };
            match reply {
                Ok(reply) => {
                    if let Some(open) = control_center::open_in_get_all(&reply) {
                        service.set_control_center_open(open);
                    }
                }
                Err(err) => tracing::warn!(
                    error = %err,
                    "cannot read Open of the control center; the popups follow its signals"
                ),
            }
        });
    }

    /// Nobody owns the name: nothing is open.
    fn control_center_vanished(&self) {
        self.control_center_generation
            .set(self.control_center_generation.get().wrapping_add(1));
        self.control_center_subscription.take();
        self.set_control_center_open(false);
    }

    fn set_control_center_open(&self, open: bool) {
        if self.control_center_open.replace(open) != open {
            self.changed();
        }
    }

    /// A daemon owns the name: subscribe to its private signals first, so none is lost
    /// between the admission and the reply, then ask for the list.
    fn appeared(&self, connection: gio::DBusConnection, owner: &str) {
        self.reset();
        self.state.set(State::Listing);
        let me = self.me.clone();
        let subscription = connection.subscribe_to_signal(
            Some(owner),
            Some(INTERFACE),
            None,
            Some(PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some(service) = me.upgrade() {
                    service.signal(signal.signal_name, signal.parameters);
                }
            },
        );
        self.subscription.replace(Some(subscription));
        self.peer.replace(Some((connection, owner.to_owned())));
        self.list();
    }

    /// Asks the daemon for its notifications. A reply for an earlier owner is dropped.
    fn list(&self) {
        let Some((connection, owner)) = self.peer.borrow().clone() else {
            return;
        };
        let (me, generation) = (self.me.clone(), self.generation.get());
        glib::spawn_future_local(async move {
            let reply = connection
                .call_future(
                    Some(&owner),
                    PATH,
                    INTERFACE,
                    "List",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            if let Some(service) = me.upgrade() {
                if service.generation.get() == generation {
                    service.listed(reply);
                }
            }
        });
    }

    fn listed(&self, reply: Result<glib::Variant, glib::Error>) {
        let reply = match reply {
            Ok(reply) => reply,
            Err(err) if err.matches(gio::DBusError::AccessDenied) => {
                tracing::error!(
                    "athanor-shelld refused the bar's List; notifications stay hidden (is the bar running in athanor-bar.service?)"
                );
                self.stop(State::Refused);
                return;
            }
            Err(err) => {
                if self.retried.replace(true) {
                    tracing::warn!(error = %err, "athanor-shelld did not list its notifications twice; they stay hidden");
                    self.stop(State::Absent);
                    return;
                }
                // Still Listing: the signals that arrive meanwhile queue as before.
                tracing::info!(error = %err, "athanor-shelld did not list its notifications; asking once more");
                let (me, generation) = (self.me.clone(), self.generation.get());
                glib::timeout_add_local_once(LIST_RETRY, move || {
                    if let Some(service) = me.upgrade() {
                        if service.generation.get() == generation {
                            service.list();
                        }
                    }
                });
                return;
            }
        };
        let Some((dnd, list)) = decode_list(&reply) else {
            tracing::error!(
                reply_type = reply.type_().as_str(),
                "athanor-shelld answered List with an unexpected type; notifications stay hidden"
            );
            self.stop(State::Absent);
            return;
        };
        self.dnd.set(dnd);
        self.popups.borrow_mut().clear();
        let count = list.len();
        // Held and Live before any popup is placed: a transient notice whose popup cannot
        // show (do not disturb, or its time ran out while no bar ran) is closed at once
        // (ruling 6), `close` reaches the daemon only in Live, and its removal from `held`
        // must not be undone by a later `replace_all`.
        self.held.borrow_mut().replace_all(list.clone());
        self.bump();
        self.state.set(State::Live);
        for notice in &list {
            self.place(notice);
        }
        tracing::info!("listed {count} notifications from athanor-shelld");
        let early: Vec<(String, glib::Variant)> = self.early.borrow_mut().drain(..).collect();
        for (name, parameters) in early {
            self.live_signal(&name, &parameters);
        }
        self.ensure_ticking();
        self.changed();
    }

    fn signal(&self, name: &str, parameters: &glib::Variant) {
        match self.state.get() {
            State::Listing => {
                let mut early = self.early.borrow_mut();
                if early.len() == EARLY_SIGNALS {
                    // ponytail: a fixed bound; the list reply carries the state these
                    // signals describe, so only an older transition is lost.
                    tracing::warn!("too many notification signals before the list arrived; the oldest is dropped");
                    early.pop_front();
                }
                early.push_back((name.to_owned(), parameters.clone()));
            }
            State::Live => {
                self.live_signal(name, parameters);
                self.ensure_ticking();
                self.changed();
            }
            State::Absent | State::Refused => {}
        }
    }

    fn live_signal(&self, name: &str, parameters: &glib::Variant) {
        match name {
            "Added" | "Replaced" => match parameters
                .try_child_value(0)
                .and_then(|value| Notice::decode(&value))
            {
                Some(notice) => self.arrived(notice),
                None => tracing::warn!(
                    signal = name,
                    "athanor-shelld sent a notification the bar cannot read; it is skipped"
                ),
            },
            "Closed" => match parameters.get::<(u32, u32)>() {
                Some((id, _reason)) => self.closed(id),
                None => tracing::warn!("athanor-shelld sent Closed with an unexpected type"),
            },
            _ => {}
        }
    }

    fn arrived(&self, notice: Notice) {
        self.place(&notice);
        let evicted = self.held.borrow_mut().arrived(notice);
        self.bump();
        for id in evicted {
            self.popups.borrow_mut().remove(id);
        }
    }

    /// Shows `notice`'s popup for the time the daemon says is left. A transient notice
    /// whose popup does not show is closed as expired at once (ruling 6).
    fn place(&self, notice: &Notice) {
        let shown =
            self.popups
                .borrow_mut()
                .show(notice.id, notice.popup_ms_left, notice.critical());
        if !shown && notice.transient {
            self.close(notice.id, EXPIRED);
        }
    }

    /// The popups in `ids` ended: their notices move to the list, a transient one closes.
    fn ended(&self, ids: &[u32]) {
        let transient: Vec<u32> = {
            let held = self.held.borrow();
            ids.iter()
                .copied()
                .filter(|id| held.get(*id).is_some_and(|notice| notice.transient))
                .collect()
        };
        for id in transient {
            self.close(id, EXPIRED);
        }
    }

    fn closed(&self, id: u32) {
        if self.held.borrow_mut().closed(id) {
            self.bump();
        }
        self.popups.borrow_mut().remove(id);
    }

    fn ensure_ticking(&self) {
        if self.ticking.get() || !self.popups.borrow().counting() {
            return;
        }
        self.ticking.set(true);
        self.last_tick.set(Some(Instant::now()));
        let me = self.me.clone();
        glib::timeout_add_local(TICK, move || match me.upgrade() {
            Some(service) => service.tick(),
            None => glib::ControlFlow::Break,
        });
    }

    fn tick(&self) -> glib::ControlFlow {
        let now = Instant::now();
        let elapsed = self
            .last_tick
            .replace(Some(now))
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
        // While a popover of the bar or the control center is open the popups are hidden and
        // their time stands still (BR6, "Stacking", ruling 3); the pointer over them pauses
        // it too (ruling 4).
        if !self.control_center_open.get() && !self.paused.get() {
            let elapsed = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
            let ended = self.popups.borrow_mut().tick(elapsed);
            if !ended.is_empty() {
                self.ended(&ended);
                self.changed();
            }
        }
        if self.popups.borrow().counting() {
            glib::ControlFlow::Continue
        } else {
            self.ticking.set(false);
            glib::ControlFlow::Break
        }
    }

    fn call(&self, method: &'static str, args: glib::Variant) {
        self.call_then(method, args, |_| {});
    }

    /// `call`, then `done` once the daemon accepted it, unless the daemon changed since. A
    /// refused call redraws, so a switch shows the service's state, not the refused wish.
    fn call_then(
        &self,
        method: &'static str,
        args: glib::Variant,
        done: impl FnOnce(&Service) + 'static,
    ) {
        if self.state.get() != State::Live {
            return;
        }
        let Some((connection, owner)) = self.peer.borrow().clone() else {
            return;
        };
        let (me, generation) = (self.me.clone(), self.generation.get());
        glib::spawn_future_local(async move {
            let reply = connection
                .call_future(
                    Some(&owner),
                    PATH,
                    INTERFACE,
                    method,
                    Some(&args),
                    None,
                    gio::DBusCallFlags::NONE,
                    TIMEOUT_MS,
                )
                .await;
            let Some(service) = me
                .upgrade()
                .filter(|service| service.generation.get() == generation)
            else {
                return;
            };
            match reply {
                Ok(_) => done(&service),
                Err(err) => {
                    tracing::warn!(error = %err, method, "athanor-shelld did not accept the call");
                    service.changed();
                }
            }
        });
    }

    /// Closes `id` here at once and asks the daemon to close it; its `Closed` then finds
    /// nothing to remove. The caller redraws.
    fn close(&self, id: u32, reason: u32) {
        self.call("Close", (id, reason).to_variant());
        self.closed(id);
    }

    fn dismiss(&self, id: u32) {
        self.close(id, DISMISSED);
        self.changed();
    }

    /// An action button, or a click on the text for "default". The token lets the
    /// application raise its window (BR4, "Actions"); without one the call still goes.
    fn invoke(&self, id: u32, key: &str) {
        let token = self
            .client
            .as_ref()
            .and_then(|client| client.activation_token(None))
            .unwrap_or_default();
        self.call("InvokeAction", (id, key, token).to_variant());
    }

    /// The daemon sends no signal for do not disturb, so the bar takes the new state only
    /// once the daemon accepted it; a refused call leaves the switch as the daemon has it.
    fn set_dnd(&self, on: bool) {
        self.call_then("SetDoNotDisturb", (on,).to_variant(), move |service| {
            service.dnd.set(on);
            if on {
                let ended = service.popups.borrow_mut().end_non_critical();
                service.ended(&ended);
            }
            service.changed();
        });
    }

    fn clear_all(&self) {
        let ids: Vec<u32> = self
            .held
            .borrow()
            .all()
            .iter()
            .map(|notice| notice.id)
            .collect();
        for id in ids {
            self.close(id, DISMISSED);
        }
        self.changed();
    }

    fn vanished(&self) {
        if self.state.get() != State::Absent {
            tracing::info!("athanor-shelld left the bus; notifications are hidden");
        }
        self.stop(State::Absent);
    }

    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.retried.set(false);
        self.subscription.take();
        self.peer.take();
        self.early.borrow_mut().clear();
        self.held.borrow_mut().replace_all(Vec::new());
        self.bump();
        self.popups.borrow_mut().clear();
    }

    fn stop(&self, state: State) {
        self.reset();
        self.state.set(state);
        self.changed();
    }

    fn bump(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    /// The bar asks the buttons whether they are stale at its next turn, the open list
    /// follows, and the popups are drawn again.
    fn changed(&self) {
        (self.wake)(Wakeup::Changed);
    }

    fn live(&self) -> bool {
        self.state.get() == State::Live
    }

    /// What the button shows; `None` while it is hidden.
    fn face(&self) -> Option<Face> {
        self.live().then(|| Face {
            dnd: self.dnd.get(),
            waiting: self.popups.borrow().waiting(),
        })
    }

    /// The popups to show now, newest first: none unless live, nor while the control center
    /// is open.
    fn visible(&self) -> Vec<u32> {
        if self.live() && !self.control_center_open.get() {
            self.popups.borrow().visible()
        } else {
            Vec::new()
        }
    }

    fn run(&self, act: &Act) {
        match act {
            Act::Close(id) => self.dismiss(*id),
            Act::Invoke(id, key) => self.invoke(*id, key),
            Act::ClearAll => self.clear_all(),
        }
    }
}

/// The `List` reply: do not disturb, and the last `CAPACITY` notifications, oldest first.
fn decode_list(reply: &glib::Variant) -> Option<(bool, Vec<Notice>)> {
    if reply.type_().as_str() != format!("(ba{WIRE_SIGNATURE})") {
        return None;
    }
    let dnd = reply.try_child_value(0)?.get::<bool>()?;
    let list = reply.try_child_value(1)?;
    let count = list.n_children();
    let notices = (count.saturating_sub(notices::CAPACITY)..count)
        .filter_map(|index| list.try_child_value(index))
        .filter_map(|value| Notice::decode(&value))
        .collect();
    Some((dnd, notices))
}

fn app_name(notice: &Notice) -> String {
    if notice.app_name.is_empty() {
        tr("Unknown application")
    } else {
        notice.app_name.clone()
    }
}

/// The card's name and title: the summary, else the application's name.
fn title(notice: &Notice) -> String {
    if notice.summary.is_empty() {
        app_name(notice)
    } else {
        notice.summary.clone()
    }
}

/// What the button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Face {
    dnd: bool,
    waiting: usize,
}

/// What a widget of a card or of the list does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    Close(u32),
    Invoke(u32, String),
    ClearAll,
}

/// The widgets of one surface's cards that answer.
#[derive(Default)]
struct Acts {
    buttons: Vec<(WidgetId, Act)>,
    /// The text of each card whose application offers "default": a click on it invokes it.
    texts: Vec<(WidgetId, u32)>,
}

impl Acts {
    fn act(&self, widget: WidgetId) -> Option<Act> {
        self.buttons
            .iter()
            .find(|(id, _)| *id == widget)
            .map(|(_, act)| act.clone())
    }

    /// The notification whose text is under `at`, beside every focusable widget, inside
    /// `within` when the cards scroll.
    fn text_at(&self, ui: &Ui, within: Option<WidgetId>, at: (f32, f32)) -> Option<u32> {
        let (x, y) = at;
        if ui.hit(x, y).is_some() || within.is_some_and(|w| !ui.widget(w).rect.contains(x, y)) {
            return None;
        }
        self.texts
            .iter()
            .find(|(text, _)| ui.widget(*text).rect.contains(x, y))
            .map(|(_, id)| *id)
    }
}

/// The type and ink of the cards, as calmo's `bar-popover-title` and `bar-popover-note`.
struct Style {
    title: TextStyle,
    note: TextStyle,
    secondary: Rgba,
    /// A critical card's outline.
    critical: Rgba,
    /// `.bar-badge`: the small size, semibold.
    badge: TextStyle,
}

impl Style {
    fn of(appearance: &Appearance) -> Style {
        let palette = sal::palette(appearance);
        let note = TextStyle {
            scale: sal::size::CAPTION.scale,
            weight: sal::size::CAPTION.weight,
            ..TextStyle::default()
        };
        Style {
            title: TextStyle {
                weight: 600,
                ..TextStyle::default()
            },
            badge: TextStyle {
                weight: 600,
                ..note.clone()
            },
            note,
            secondary: Rgba(palette.ink2),
            critical: Rgba(palette.warn),
        }
    }
}

/// Where a card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Popup,
    List,
}

/// The images of the cards, from the notification or the icon theme.
struct Pictures {
    theme: IconTheme,
    /// Themed icons by name, size, scale and ink; `None` for one the theme lacks.
    cache: HashMap<(String, u16, u16, [u8; 4]), Option<Image>>,
}

impl Pictures {
    fn icon(&mut self, name: &str, size: u16, scale: u16, ink: Rgba) -> Option<Image> {
        let theme = &self.theme;
        self.cache
            .entry((name.to_owned(), size, scale, ink.0))
            .or_insert_with(|| match theme.image(name, size, scale) {
                Ok(image) if name.ends_with("-symbolic") => Some(image.tinted(ink)),
                Ok(image) => Some(image),
                // A name the theme lacks falls back, as GTK's `has_icon` did, without a line
                // the session log would count as an error.
                Err(err) => {
                    tracing::debug!(error = %err, icon = name, "no such icon");
                    None
                }
            })
            .clone()
    }

    /// The image data, else the file, else the icon name, else the application's icon, else
    /// a generic one (BR4, "Images").
    fn picture(&mut self, notice: &Notice, scale: u16, ink: Rgba) -> Option<Image> {
        let own = match &notice.picture {
            Picture::Pixels {
                width,
                height,
                rgba,
            } => Image::from_rgba8(*width, *height, rgba.clone()).ok(),
            Picture::File(path) => {
                notices::read_icon_file(path).and_then(|bytes| Image::from_png(&bytes).ok())
            }
            Picture::Name(name) if notices::is_icon_name(name) => {
                self.icon(name, PICTURE, scale, ink)
            }
            Picture::Name(_) | Picture::None => None,
        };
        own.or_else(|| {
            let entry = notice.desktop_entry.as_deref()?;
            let info = DesktopAppInfo::new(&format!("{entry}.desktop"))?;
            self.icon(&entry_icon(Some(&info)), PICTURE, scale, ink)
        })
        .or_else(|| self.icon(FALLBACK_ICON, PICTURE, scale, ink))
    }
}

/// A column of `spacing` under `parent`, as a vertical GTK box.
fn column(ui: &mut Ui, parent: WidgetId, spacing: f32) -> WidgetId {
    let id = ui.add(parent, Kind::Column);
    ui.set_spacing(id, Some(spacing));
    id
}

/// An inline of `spacing` under `parent`, as a horizontal GTK box.
fn row(ui: &mut Ui, parent: WidgetId, spacing: f32) -> WidgetId {
    let id = ui.inline(parent, [0.0, 0.0]);
    ui.set_spacing(id, Some(spacing));
    id
}

/// A label of a card: plain text, so markup shows as text (SH12), wrapped at the card's
/// width and cut after `lines`.
fn text_label(
    ui: &mut Ui,
    parent: WidgetId,
    text: &str,
    lines: u32,
    style: &TextStyle,
) -> WidgetId {
    let label = ui.add(parent, Kind::Label(text.to_owned()));
    let mut wrap = Wrap::default();
    wrap.max_width_chars = NonZeroU32::new(TEXT_CHARS);
    wrap.lines = NonZeroU32::new(lines);
    ui.set_wrap(label, Some(wrap));
    ui.set_text_style(label, style.clone());
    label
}

/// What a card is drawn with.
struct Paint<'a> {
    style: &'a Style,
    picture: Option<Image>,
    close: Option<Image>,
}

/// One notification under `parent`. A popup is an alert, so a screen reader reads it (BR4);
/// in the list it is a group. Its name is the title.
fn card(
    ui: &mut Ui,
    parent: WidgetId,
    notice: &Notice,
    place: Place,
    paint: Paint,
    acts: &mut Acts,
) {
    let title = title(notice);
    let (holder, margin) = match place {
        Place::Popup => (ui.alert(parent, &title), POPUP_MARGIN),
        Place::List => (ui.add(parent, Kind::Group(title.clone())), [0.0; 4]),
    };
    let island = ui.island(holder, true, margin);
    // GTK's size request of 360 counts the popup's margin in, as Sal's least width does.
    ui.set_min_width(island, CARD_WIDTH);
    if notice.critical() {
        ui.set_ink(island, Some(paint.style.critical));
    }
    let content = column(ui, island, 8.0);
    let top = row(ui, content, 10.0);
    if let Some(picture) = paint.picture {
        // In a column, so that it keeps its size at the top; decoration: the title says what
        // the notification is.
        let side = ui.add(top, Kind::Column);
        let image = ui.image(side, picture, f32::from(PICTURE), "");
        ui.set_align(image, [Align::Start, Align::Start]);
    }
    let text = column(ui, top, 2.0);
    ui.set_fill(text, true);
    if place == Place::Popup {
        let app = text_label(ui, text, &app_name(notice), 1, &paint.style.note);
        ui.set_ink(app, Some(paint.style.secondary));
    }
    text_label(ui, text, &title, 2, &paint.style.title);
    if !notice.body.is_empty() {
        text_label(ui, text, &notice.body, 4, &TextStyle::default());
    }
    if notice.has_default {
        acts.texts.push((text, notice.id));
    }
    let end = ui.add(top, Kind::Column);
    let close_name = tr_with("Close {title}", "title", &title);
    let close = match paint.close {
        Some(image) => ui.icon_button(end, &close_name, image, f32::from(ICON), None),
        None => ui.add(end, Kind::Button(close_name.clone())),
    };
    ui.set_align(close, [Align::Start, Align::Start]);
    ui.set_tooltip(close, Some(&close_name));
    acts.buttons.push((close, Act::Close(notice.id)));
    if !notice.actions.is_empty() {
        let actions = row(ui, content, 6.0);
        ui.set_align(actions, [Align::End, Align::Start]);
        for action in &notice.actions {
            let button = ui.labelled_button(actions, &action.label, None, 0.0, &action.label);
            acts.buttons
                .push((button, Act::Invoke(notice.id, action.key.clone())));
        }
    }
}

/// The open list and what it was drawn from.
struct List {
    acts: Acts,
    scroll: Option<WidgetId>,
    switch: WidgetId,
    revision: u64,
}

/// The list popover: the header with "Clear all", the cards by application, newest first,
/// and do not disturb. The cards scroll once they are taller than `LIST_HEIGHT`.
fn build_list(cx: &mut Cx, service: &Service, pictures: &mut Pictures, style: &Style) -> (Ui, List) {
    let name = tr("Notifications");
    let (mut ui, frame) = look::popover(&name, cx.theme.clone(), cx.rtl, cx.top);
    let ink = cx.theme.text;
    let content = column(&mut ui, frame, 10.0);
    let header = row(&mut ui, content, 8.0);
    let heading = ui.add(header, Kind::Label(name));
    ui.set_text_style(heading, style.title.clone());
    ui.set_fill(heading, true);
    let clear_name = tr("Clear all");
    let clear = ui.labelled_button(header, &clear_name, None, 0.0, &clear_name);
    let mut acts = Acts::default();
    acts.buttons.push((clear, Act::ClearAll));
    // A transient notification never reaches the list (BR4).
    let listed: Vec<Notice> = service
        .held
        .borrow()
        .all()
        .iter()
        .filter(|notice| !notice.transient)
        .cloned()
        .collect();
    ui.set_sensitive(clear, !listed.is_empty());
    let mut viewport = None;
    if listed.is_empty() {
        let empty = ui.add(content, Kind::Label(tr("No notifications")));
        ui.set_text_style(empty, style.note.clone());
        ui.set_ink(empty, Some(style.secondary));
    } else {
        let scroll = ui.scroll(content, LIST_HEIGHT);
        ui.set_natural_height(scroll, true);
        viewport = Some(scroll);
        let groups = column(&mut ui, scroll, 12.0);
        ui.set_min_width(groups, CARD_WIDTH);
        let close = pictures.icon("window-close-symbolic", ICON, cx.scale, ink);
        for group in notices::groups(&listed) {
            let Some(first) = group.first() else {
                continue;
            };
            let name = app_name(first);
            let holder = ui.add(groups, Kind::Group(name.clone()));
            ui.set_spacing(holder, Some(6.0));
            let heading = ui.add(holder, Kind::Label(name));
            ui.set_text_style(heading, style.note.clone());
            ui.set_ink(heading, Some(style.secondary));
            for notice in group {
                let paint = Paint {
                    style,
                    picture: pictures.picture(notice, cx.scale, ink),
                    close: close.clone(),
                };
                card(&mut ui, holder, notice, Place::List, paint, &mut acts);
            }
        }
    }
    let separator = ui.add(content, Kind::Separator);
    ui.set_fill(separator, true);
    let switch = ui.switch(content, &tr("Do not disturb"), service.dnd.get());
    let list = List {
        acts,
        scroll: viewport,
        switch,
        revision: service.revision.get(),
    };
    (ui, list)
}

/// One output's popups surface.
struct Window {
    id: SurfaceId,
    acts: Acts,
    /// What it shows: `None` while hidden.
    shown: Option<Shown>,
    /// The pointer is over it (ruling 4).
    inside: bool,
}

/// What a popups surface was drawn from.
#[derive(Clone, PartialEq)]
struct Shown {
    ids: Vec<u32>,
    revision: u64,
    theme: Theme,
    top: bool,
    rtl: bool,
}

/// The popups surface on the panel's edge, at its end corner: shown, it takes the pointer,
/// hidden none of it. Never the keyboard (ruling 14), no exclusive zone.
fn options(top: bool, rtl: bool, shown: bool) -> LayerOptions {
    let mut options = LayerOptions::new(Layer::Top);
    options.namespace = NAMESPACE.to_owned();
    // The margins run top, right, bottom, left.
    let (edge, edge_margin) = if top {
        (Anchor::TOP, 0)
    } else {
        (Anchor::BOTTOM, 2)
    };
    let (end, end_margin) = if rtl {
        (Anchor::LEFT, 3)
    } else {
        (Anchor::RIGHT, 1)
    };
    options.anchor = edge | end;
    options.margin[edge_margin] = MARGIN;
    options.margin[end_margin] = MARGIN;
    options.keyboard = KeyboardInteractivity::None;
    options.input_region = (!shown).then(Vec::new);
    options
}

/// The popups' look: the popover's, with the card's corners and padding and no arrow.
fn popups_theme(theme: &Theme) -> Theme {
    let mut theme = theme.clone();
    theme.island_radius = CARD_RADIUS;
    theme.island_padding = CARD_PADDING;
    theme.island_arrow = 0.0;
    theme
}

/// A hidden surface's widgets: none, 1 by 1.
fn hidden(theme: &Theme, rtl: bool) -> Ui {
    let mut ui = Ui::new("", popups_theme(theme), rtl);
    ui.min_width = 1.0;
    ui.min_height = 1.0;
    ui
}

struct Notifications {
    service: Rc<Service>,
    pictures: Pictures,
    /// What the buttons show.
    drawn: Option<Face>,
    list: Option<List>,
    /// The popups surfaces, by output name.
    windows: HashMap<String, Window>,
    /// The output each visible popup was put on.
    placed: HashMap<u32, String>,
}

pub fn new(start: &Start) -> Option<Box<dyn Module>> {
    let client = start.client.cloned();
    Some(Box::new(Notifications {
        service: Service::start(start.wake.clone(), client),
        pictures: Pictures {
            theme: IconTheme::system(ICON_THEME),
            cache: HashMap::new(),
        },
        drawn: None,
        list: None,
        windows: HashMap::new(),
        placed: HashMap::new(),
    }))
}

impl Notifications {
    /// The output of the activated window, else the first (BR4).
    fn target(&self, outputs: &[String]) -> Option<usize> {
        let activated = self.service.client.as_ref().and_then(|client| {
            client
                .windows()
                .into_iter()
                .find(|window| window.state.activated)
                .map(|window| window.outputs)
        });
        target_output(activated.as_deref(), outputs)
    }

    /// The cards of `ids`, newest nearest the panel.
    fn popups(&mut self, around: &Around, ids: &[u32], scale: u16) -> (Ui, Acts) {
        let theme = popups_theme(around.theme);
        let ink = theme.text;
        let mut ui = Ui::new(&tr("Notifications"), theme, around.rtl);
        ui.set_spacing(Ui::ROOT, Some(0.0));
        let style = Style::of(&Appearance::read());
        let close = self
            .pictures
            .icon("window-close-symbolic", ICON, scale, ink);
        let mut ordered: Vec<Notice> = {
            let held = self.service.held.borrow();
            ids.iter().filter_map(|id| held.get(*id).cloned()).collect()
        };
        if !around.top {
            ordered.reverse();
        }
        let mut acts = Acts::default();
        for notice in &ordered {
            let paint = Paint {
                style: &style,
                picture: self.pictures.picture(notice, scale, ink),
                close: close.clone(),
            };
            card(&mut ui, Ui::ROOT, notice, Place::Popup, paint, &mut acts);
        }
        (ui, acts)
    }

    fn window(&self, surface: SurfaceId) -> Option<&Window> {
        self.windows.values().find(|window| window.id == surface)
    }
}

impl Module for Notifications {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        let face = self.service.face();
        self.drawn = face;
        let face = face?;
        let icon = if face.dnd {
            "notifications-disabled-symbolic"
        } else {
            "preferences-system-notifications-symbolic"
        };
        let name = if face.waiting > 0 {
            tr_with(
                "Notifications, {count} waiting",
                "count",
                &face.waiting.to_string(),
            )
        } else {
            tr("Notifications")
        };
        let button = match cx.icons.image(icon, ICON, cx.scale) {
            Some(image) if face.waiting > 0 => {
                let badge = format!("+{}", face.waiting);
                let button = ui.labelled_button(group, &name, Some(image), f32::from(ICON), &badge);
                ui.set_text_style(button, Style::of(&Appearance::read()).badge);
                button
            }
            _ => cx
                .icons
                .button(ui, group, icon, ICON, cx.scale, &name, None),
        };
        ui.set_tooltip(button, Some(&name));
        Some(button)
    }

    fn stale(&self, _cx: &Cx) -> bool {
        self.drawn != self.service.face()
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        if !self.service.live() {
            return None;
        }
        let style = Style::of(&Appearance::read());
        let (ui, list) = build_list(cx, &self.service, &mut self.pictures, &style);
        self.list = Some(list);
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        let Some(list) = &self.list else {
            return Reply::Stay;
        };
        match event {
            Event::Toggled(widget) if widget == list.switch => {
                self.service.set_dnd(ui.is_active(widget));
            }
            Event::Clicked(widget) => {
                if let Some(act) = list.acts.act(widget) {
                    self.service.run(&act);
                }
            }
            _ => {}
        }
        Reply::Stay
    }

    /// The list follows the notifications; the switch follows do not disturb in place.
    fn refresh(&mut self, _cx: &mut Cx, ui: &mut Ui) -> Reply {
        let Some(list) = &self.list else {
            return Reply::Stay;
        };
        if !self.service.live() {
            return Reply::Close;
        }
        if list.revision != self.service.revision.get() {
            return Reply::Reopen;
        }
        let dnd = self.service.dnd.get();
        if ui.is_active(list.switch) != dnd {
            ui.set_active(list.switch, dnd);
        }
        Reply::Stay
    }

    fn closed(&mut self) {
        self.list = None;
    }

    /// The popups, each on the output it was put on (BR4): a new popup goes to the output of
    /// the activated window and stays there when the focus moves, so nothing is drawn twice.
    /// The output with an open popover of the bar shows none, and a new popup waits for it
    /// (BR6, "Stacking"). An output with nothing to show keeps its surface, emptied.
    fn surfaces(&mut self, shell: &mut Shell, around: &Around) {
        self.windows
            .retain(|_, window| shell.ui(window.id).is_some());
        let inside = self.windows.values().any(|window| window.inside);
        self.service.paused.set(around.popover || inside);
        let visible = self.service.visible();
        let outputs: Vec<(String, u16)> = shell
            .outputs()
            .filter_map(|info| {
                let scale = u16::try_from(info.scale_factor.clamp(1, 16)).unwrap_or(1);
                Some((info.name.clone()?, scale))
            })
            .collect();
        let names: Vec<String> = outputs.iter().map(|(name, _)| name.clone()).collect();
        let covered = around.covered.map(str::to_owned);
        let target = self.target(&names);
        let plan = place(&mut self.placed, &visible, &names, target, covered.as_ref());
        for ((output, scale), ids) in outputs.into_iter().zip(plan) {
            let shown = (!ids.is_empty()).then(|| Shown {
                ids,
                revision: self.service.revision.get(),
                theme: around.theme.clone(),
                top: around.top,
                rtl: around.rtl,
            });
            let open = self.windows.get(&output).map(|window| window.id);
            if open.is_none() && shown.is_none()
                || self
                    .windows
                    .get(&output)
                    .is_some_and(|window| window.shown == shown)
            {
                continue;
            }
            let (ui, acts) = match &shown {
                Some(shown) => self.popups(around, &shown.ids, scale),
                None => (hidden(around.theme, around.rtl), Acts::default()),
            };
            let layer = options(around.top, around.rtl, shown.is_some());
            let id = match open {
                Some(id) => {
                    if let Some(drawn) = shell.ui_mut(id) {
                        *drawn = ui;
                    }
                    if let Err(err) = shell.set_layer(id, &layer) {
                        tracing::error!(error = %err, output, "cannot place the notification popups");
                    }
                    if let Err(err) = shell.set_accessible(id, shown.is_some()) {
                        tracing::error!(error = %err, output, "cannot show or hide the notification popups");
                    }
                    id
                }
                None => match shell.open_layer(ui, layer, Some(&output)) {
                    Ok(id) => id,
                    Err(err) => {
                        tracing::error!(error = %err, output, "cannot open the notification popups");
                        continue;
                    }
                },
            };
            // A surface that stops taking input never sees the pointer leave.
            let inside = shown.is_some()
                && self
                    .windows
                    .get(&output)
                    .is_some_and(|window| window.inside);
            self.windows.insert(
                output,
                Window {
                    id,
                    acts,
                    shown,
                    inside,
                },
            );
        }
    }

    fn surface_event(&mut self, surface: SurfaceId, event: &Event) {
        let Event::Clicked(widget) = event else {
            return;
        };
        if let Some(act) = self
            .window(surface)
            .and_then(|window| window.acts.act(*widget))
        {
            self.service.run(&act);
        }
    }

    fn pointer(&mut self, surface: SurfaceId, inside: bool) {
        if let Some(window) = self
            .windows
            .values_mut()
            .find(|window| window.id == surface)
        {
            window.inside = inside;
        }
    }

    fn pressed(&mut self, ui: &Ui, surface: Option<SurfaceId>, at: (f32, f32)) {
        let id = match surface {
            None => self
                .list
                .as_ref()
                .and_then(|list| list.acts.text_at(ui, list.scroll, at)),
            Some(surface) => self
                .window(surface)
                .and_then(|window| window.acts.text_at(ui, None, at)),
        };
        if let Some(id) = id {
            self.service.invoke(id, "default");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popups_sit_on_the_panels_edge_at_the_end_corner() {
        let shown = options(true, false, true);
        assert_eq!(shown.anchor, Anchor::TOP | Anchor::RIGHT);
        assert_eq!(shown.margin, [MARGIN, MARGIN, 0, 0]);
        assert_eq!(shown.input_region, None);
        assert!(matches!(shown.keyboard, KeyboardInteractivity::None));
        let hidden = options(false, true, false);
        assert_eq!(hidden.anchor, Anchor::BOTTOM | Anchor::LEFT);
        assert_eq!(hidden.margin, [0, 0, MARGIN, MARGIN]);
        assert_eq!(hidden.input_region, Some(Vec::new()));
    }
}
