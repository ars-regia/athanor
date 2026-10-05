//! The notifications module (doc_bar.md BR3, BR4): the service that talks to
//! athanor-shelld's private interface, the card every popup is drawn as, and the button
//! that shows the unread count and opens the notification center of athanor-control-center.
//! One service per bar (`Bar::notifications`): the button on every surface reads it, so two
//! outputs never mean two `List` calls.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use athanor_bar::control_center;
use athanor_bar::fullscreen::report;
use athanor_bar::badge;
use athanor_bar::notices::{self, Held, Notice, Picture, WIRE_SIGNATURE};
use athanor_bar::popups::{target_output, Popups};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};

use super::control_center::toggle_notifications;
use super::popups::Window;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

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
const PICTURE_PX: i32 = 32;
const CARD_WIDTH: i32 = 360;
const TEXT_CHARS: i32 = 36;
const FALLBACK_ICON: &str = "dialog-information-symbolic";

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

pub struct Service {
    bar: Weak<Bar>,
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
    popups: RefCell<Popups>,
    dnd: Cell<bool>,
    /// The last `(available, active)` sent to the daemon; `None` until the first send for
    /// this owner.
    fullscreen: Cell<Option<(bool, bool)>>,
    ticking: Cell<bool>,
    last_tick: Cell<Option<Instant>>,
    /// One popups window per output, made when that output first shows a popup, then only
    /// emptied, never moved (see `super::popups`).
    windows: RefCell<Vec<Window>>,
    /// The output each visible popup was put on.
    placed: RefCell<HashMap<u32, gdk::Monitor>>,
    /// The control center's `Open` (CC9): false while it has no owner. The popups stay
    /// hidden while it is true.
    control_center_open: Cell<bool>,
    control_center_subscription: RefCell<Option<gio::SignalSubscription>>,
    /// Bumped on every owner change of the control center: a `GetAll` reply from a previous
    /// owner is dropped.
    control_center_generation: Cell<u64>,
}

impl Service {
    pub(super) fn start(bar: &Weak<Bar>) -> Rc<Service> {
        Rc::new_cyclic(|me: &Weak<Service>| {
            let (appeared, vanished) = (me.clone(), me.clone());
            // The watch lives as long as the process: the service is the bar's. Its id is
            // not kept, since gio 0.22 cannot name the type (`dbus::WatcherId` is shadowed
            // by `dbus_connection::WatcherId`) and dropping it does not unwatch.
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
                bar: bar.clone(),
                me: me.clone(),
                state: Cell::new(State::Absent),
                peer: RefCell::new(None),
                subscription: RefCell::new(None),
                generation: Cell::new(0),
                retried: Cell::new(false),
                early: RefCell::new(VecDeque::new()),
                held: RefCell::new(Held::default()),
                popups: RefCell::new(Popups::default()),
                dnd: Cell::new(false),
                fullscreen: Cell::new(None),
                ticking: Cell::new(false),
                last_tick: Cell::new(None),
                windows: RefCell::new(Vec::new()),
                placed: RefCell::new(HashMap::new()),
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
            let reply = super::bus::call(
                &connection,
                &owner,
                control_center::PATH,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                Some(&(control_center::NAME,).to_variant()),
                super::bus::TIMEOUT_MS,
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
                Err(err) => {
                    tracing::warn!(error = %err, "cannot read Open of the control center; the popups follow its signals")
                }
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
            self.redraw_popups();
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
        let Some(list) = decode_list(&reply) else {
            tracing::error!(
                reply_type = reply.type_().as_str(),
                "athanor-shelld answered List with an unexpected type; notifications stay hidden"
            );
            self.stop(State::Absent);
            return;
        };
        self.popups.borrow_mut().clear();
        let count = list.len();
        // Held and Live before any popup is placed: a transient notice whose popup cannot
        // show (do not disturb, or its time ran out while no bar ran) is closed at once
        // (ruling 6), `close` reaches the daemon only in Live, and its removal from `held`
        // must not be undone by a later `replace_all`.
        self.held.borrow_mut().replace_all(list.clone());
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
        self.read_dnd();
        self.fullscreen.set(None);
        self.report_fullscreen();
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
            "Read" => match parameters.get::<(Vec<u32>,)>() {
                // A notification read anywhere leaves the unread list and its popup.
                Some((ids,)) => ids.into_iter().for_each(|id| self.closed(id)),
                None => tracing::warn!("athanor-shelld sent Read with an unexpected type"),
            },
            "DoNotDisturbChanged" => match parameters.get::<(bool, String, i64)>() {
                Some((on, _reason, _until)) => self.apply_dnd(on),
                None => tracing::warn!(
                    "athanor-shelld sent DoNotDisturbChanged with an unexpected type"
                ),
            },
            _ => {}
        }
    }

    fn arrived(&self, notice: Notice) {
        self.place(&notice);
        let evicted = self.held.borrow_mut().arrived(notice);
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
        self.held.borrow_mut().closed(id);
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
        if !self.paused() {
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

    /// While a popover of the bar or the control center is open the popups are hidden, and their time stands
    /// still (BR6 "Stacking", ruling 3); the pointer over them pauses it too (ruling 4).
    fn paused(&self) -> bool {
        self.control_center_open.get()
            || self.windows.borrow().iter().any(Window::pointer_inside)
            || self.bar.upgrade().is_some_and(|bar| bar.popover_is_open())
    }

    fn call(&self, method: &'static str, args: glib::Variant) {
        self.call_with(method, args, |_| {}, |_| {});
    }

    /// `call`, then `done` once the daemon accepted it, unless the daemon changed since, and
    /// `failed` when it refused or did not answer. A refused call redraws.
    fn call_with(
        &self,
        method: &'static str,
        args: glib::Variant,
        done: impl FnOnce(&Service) + 'static,
        failed: impl FnOnce(&Service) + 'static,
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
                    failed(&service);
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

    pub(super) fn dismiss(&self, id: u32) {
        self.close(id, DISMISSED);
        self.changed();
    }

    /// An action button, or a click on the text for "default". The token lets the
    /// application raise its window (BR4, "Actions"); without one the call still goes.
    pub(super) fn invoke(&self, id: u32, key: &str) {
        let token = self
            .bar
            .upgrade()
            .and_then(|bar| {
                bar.client()
                    .and_then(|client| client.activation_token(None))
            })
            .unwrap_or_default();
        self.call("InvokeAction", (id, key, token).to_variant());
    }

    /// The do-not-disturb state the daemon reports: the switch, and the popups it ends.
    fn apply_dnd(&self, on: bool) {
        self.dnd.set(on);
        if on {
            let ended = self.popups.borrow_mut().end_non_critical();
            self.ended(&ended);
        }
        self.changed();
    }

    /// Asks the daemon whether do not disturb is on: after each list, since the list no
    /// longer carries it and a change made while no bar ran sent it no signal.
    fn read_dnd(&self) {
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
                    "DoNotDisturb",
                    None,
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
            match reply
                .as_ref()
                .ok()
                .filter(|reply| reply.type_().as_str() == "(bsxas)")
                .and_then(|reply| reply.try_child_value(0)?.get::<bool>())
            {
                Some(on) => service.apply_dnd(on),
                None => tracing::warn!(
                    "athanor-shelld did not report do not disturb; the switch stays as it was"
                ),
            }
        });
    }

    /// Tells the daemon whether a fullscreen window has the focus, when that changed since
    /// the last report to this owner. A compositor that withholds the toplevel list is
    /// reported once as unobservable.
    pub(super) fn report_fullscreen(&self) {
        let Some(bar) = self.bar.upgrade() else {
            return;
        };
        let client = bar.client();
        let value = report(
            client.is_some_and(|c| c.toplevel_info_available()),
            client
                .map(|c| c.windows())
                .unwrap_or_default()
                .into_iter()
                .map(|window| window.state),
        );
        if self.state.get() != State::Live || self.fullscreen.get() == Some(value) {
            return;
        }
        self.fullscreen.set(Some(value));
        // A refused call forgets the value, so the next window event sends it again.
        self.call_with(
            "ReportFullscreen",
            value.to_variant(),
            |_| {},
            |service| service.fullscreen.set(None),
        );
    }

    fn vanished(&self) {
        if self.state.get() != State::Absent {
            tracing::info!("athanor-shelld left the bus; notifications are hidden");
        }
        self.stop(State::Absent);
    }

    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.fullscreen.set(None);
        self.retried.set(false);
        self.subscription.take();
        self.peer.take();
        self.early.borrow_mut().clear();
        self.held.borrow_mut().replace_all(Vec::new());
        self.popups.borrow_mut().clear();
    }

    fn stop(&self, state: State) {
        self.reset();
        self.state.set(state);
        self.changed();
    }

    fn changed(&self) {
        self.redraw_popups();
        if let Some(bar) = self.bar.upgrade() {
            bar.refresh(Changed::Notifications);
        }
    }

    /// Draws the visible popups, each on the output it was put on (BR4). A new popup goes to
    /// the output of the activated window and stays there when the focus moves, so nothing
    /// is drawn twice. The output with an open popover of the bar shows none and takes no new
    /// one, which waits (BR6, "Stacking"). An output with nothing to show keeps its window,
    /// emptied.
    pub(super) fn redraw_popups(&self) {
        let (Some(bar), Some(me)) = (self.bar.upgrade(), self.me.upgrade()) else {
            return;
        };
        let visible = if self.live() && !self.control_center_open.get() {
            self.popups.borrow().visible()
        } else {
            Vec::new()
        };
        let monitors: Vec<gdk::Monitor> = bar
            .monitors()
            .into_iter()
            .filter(|monitor| monitor.is_valid())
            .collect();
        let covered = bar.popover_output();
        let plan: Vec<(gdk::Monitor, Vec<Notice>)> = {
            let mut placed = self.placed.borrow_mut();
            placed.retain(|id, monitor| visible.contains(id) && monitors.contains(monitor));
            if let Some(target) =
                target_monitor(&bar, &monitors).filter(|target| covered.as_ref() != Some(target))
            {
                for id in &visible {
                    placed.entry(*id).or_insert_with(|| target.clone());
                }
            }
            let held = self.held.borrow();
            monitors
                .into_iter()
                .map(|monitor| {
                    let shown = visible
                        .iter()
                        .filter(|id| placed.get(id) == Some(&monitor))
                        .filter_map(|id| held.get(*id).cloned())
                        .collect();
                    (monitor, shown)
                })
                .collect()
        };
        let mut windows = self.windows.borrow_mut();
        for (monitor, shown) in plan {
            match windows.iter().find(|window| window.on(&monitor)) {
                Some(window) if shown.is_empty() => window.hide(),
                Some(window) => window.show(&bar, &me, &shown),
                None if shown.is_empty() => {}
                None => {
                    let window = Window::new(&bar, &monitor);
                    window.show(&bar, &me, &shown);
                    windows.push(window);
                }
            }
        }
    }

    /// `monitor` left: its popups window is abandoned, and its popups go to an output still
    /// there at the next redraw, on the next idle (Review Focus 4).
    pub(super) fn output_left(&self, monitor: &gdk::Monitor) {
        let gone = {
            let mut windows = self.windows.borrow_mut();
            windows
                .iter()
                .position(|window| window.on(monitor))
                .map(|index| windows.swap_remove(index))
        };
        if let Some(window) = gone {
            window.abandon();
        }
        self.placed
            .borrow_mut()
            .retain(|_, placed| placed != monitor);
        let me = self.me.clone();
        glib::idle_add_local_once(move || {
            if let Some(service) = me.upgrade() {
                service.redraw_popups();
            }
        });
    }

    pub(super) fn live(&self) -> bool {
        self.state.get() == State::Live
    }

    pub(super) fn dnd(&self) -> bool {
        self.dnd.get()
    }

    /// Marks `id` read. The daemon answers with `Read`, which takes it off the unread list
    /// and off the screen.
    pub(super) fn mark_read(&self, id: u32) {
        self.call("MarkRead", (vec![id],).to_variant());
    }

    /// How many notifications await reading: the daemon's unread list, less the transient
    /// ones, which never reach the notification center (BR4).
    pub(super) fn unread(&self) -> usize {
        self.held
            .borrow()
            .all()
            .iter()
            .filter(|notice| !notice.transient)
            .count()
    }
}

/// The `List` reply: do not disturb, and the last `CAPACITY` notifications, oldest first.
/// The output new popups go to: the one the activated window is on. With no window
/// activated, or no compositor client, the first output is the fallback (BR4).
fn target_monitor(bar: &Bar, monitors: &[gdk::Monitor]) -> Option<gdk::Monitor> {
    let activated = bar
        .client()
        .and_then(|client| {
            client
                .windows()
                .into_iter()
                .find(|window| window.state.activated)
        })
        .map(|window| window.outputs);
    let connectors: Vec<String> = monitors
        .iter()
        .map(|monitor| monitor.connector().map(String::from).unwrap_or_default())
        .collect();
    target_output(activated.as_deref(), &connectors).and_then(|index| monitors.get(index).cloned())
}

fn decode_list(reply: &glib::Variant) -> Option<Vec<Notice>> {
    if reply.type_().as_str() != format!("(a{WIRE_SIGNATURE})") {
        return None;
    }
    let list = reply.try_child_value(0)?;
    let count = list.n_children();
    Some(
        (count.saturating_sub(notices::CAPACITY)..count)
            .filter_map(|index| list.try_child_value(index))
            .filter_map(|value| Notice::decode(&value))
            .collect(),
    )
}

pub(super) fn app_name(notice: &Notice) -> String {
    if notice.app_name.is_empty() {
        tr("Unknown application")
    } else {
        notice.app_name.clone()
    }
}

/// A texture from straight RGBA. `None` for a zero side or a length that does not match:
/// `MemoryTexture::new` would abort on either.
pub(super) fn texture(width: u32, height: u32, rgba: &[u8]) -> Option<gdk::Texture> {
    let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
        return None;
    };
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let expected = stride.checked_mul(usize::try_from(height).ok()?)?;
    if w <= 0 || h <= 0 || rgba.len() != expected {
        return None;
    }
    let bytes = glib::Bytes::from(rgba);
    Some(gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8a8, &bytes, stride).upcast())
}

pub(super) fn has_icon(name: &str) -> bool {
    gdk::Display::default()
        .is_some_and(|display| gtk4::IconTheme::for_display(&display).has_icon(name))
}

/// The image data, else the file, else the icon name, else the application's icon, else a
/// generic one (BR4, "Images").
fn picture(notice: &Notice) -> gtk4::Image {
    let from_texture = |texture: gdk::Texture| gtk4::Image::from_paintable(Some(&texture));
    let image = match &notice.picture {
        Picture::Pixels {
            width,
            height,
            rgba,
        } => texture(*width, *height, rgba).map(from_texture),
        Picture::File(path) => athanor_unit::icon::read_icon_file(path)
            .and_then(|bytes| gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok())
            .map(from_texture),
        Picture::Name(name) if has_icon(name) => Some(gtk4::Image::from_icon_name(name)),
        Picture::Name(_) | Picture::None => None,
    }
    .or_else(|| {
        notice
            .desktop_entry
            .as_deref()
            .and_then(|entry| gio_unix::DesktopAppInfo::new(&format!("{entry}.desktop")))
            .and_then(|info| info.icon())
            .map(|icon| gtk4::Image::from_gicon(&icon))
    })
    .unwrap_or_else(|| gtk4::Image::from_icon_name(FALLBACK_ICON));
    image.set_pixel_size(PICTURE_PX);
    image.set_valign(gtk4::Align::Start);
    image
}

fn text_label(text: &str, lines: i32) -> gtk4::Label {
    // A plain label: `use-markup` stays false, so markup in the text shows as text (SH12).
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::WordChar);
    label.set_max_width_chars(TEXT_CHARS);
    label.set_lines(lines);
    label.set_ellipsize(pango::EllipsizeMode::End);
    label
}

/// One notification as a popup. It is an `alert`, so a screen reader reads it (BR4); its
/// name is the summary. A click on it marks it read.
pub(super) fn card(service: &Rc<Service>, notice: &Notice) -> gtk4::Box {
    let card = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(8)
        .accessible_role(gtk4::AccessibleRole::Alert)
        .build();
    card.add_css_class("notification-card");
    if notice.critical() {
        card.add_css_class("critical");
    }
    card.set_size_request(CARD_WIDTH, -1);
    let title = if notice.summary.is_empty() {
        app_name(notice)
    } else {
        notice.summary.clone()
    };
    card.update_property(&[Property::Label(&title)]);

    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let app = text_label(&app_name(notice), 1);
    app.add_css_class("bar-popover-note");
    text.append(&app);
    let summary = text_label(&title, 2);
    summary.add_css_class("bar-popover-title");
    text.append(&summary);
    if !notice.body.is_empty() {
        text.append(&text_label(&notice.body, 4));
    }
    let id = notice.id;
    let read = gtk4::GestureClick::new();
    let weak = Rc::downgrade(service);
    read.connect_released(move |_, _, _, _| {
        if let Some(service) = weak.upgrade() {
            service.mark_read(id);
        }
    });
    card.add_controller(read);
    if notice.has_default {
        let click = gtk4::GestureClick::new();
        let weak = Rc::downgrade(service);
        click.connect_released(move |_, _, _, _| {
            if let Some(service) = weak.upgrade() {
                service.invoke(id, "default");
            }
        });
        text.add_controller(click);
    }

    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.set_valign(gtk4::Align::Start);
    let close_name = tr_with("Close {title}", "title", &title);
    close.set_tooltip_text(Some(&close_name));
    close.update_property(&[Property::Label(&close_name)]);
    let weak = Rc::downgrade(service);
    close.connect_clicked(move |_| {
        if let Some(service) = weak.upgrade() {
            service.dismiss(id);
        }
    });

    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    top.append(&picture(notice));
    top.append(&text);
    top.append(&close);
    card.append(&top);

    if !notice.actions.is_empty() {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        row.set_halign(gtk4::Align::End);
        for action in &notice.actions {
            let button = gtk4::Button::with_label(&action.label);
            button.add_css_class("bar-row");
            let (weak, key) = (Rc::downgrade(service), action.key.clone());
            button.connect_clicked(move |_| {
                if let Some(service) = weak.upgrade() {
                    service.invoke(id, &key);
                }
            });
            row.append(&button);
        }
        card.append(&row);
    }
    card
}

struct NotificationsUi {
    button: gtk4::Button,
    image: gtk4::Image,
    badge: gtk4::Label,
    service: Rc<Service>,
}

impl ModuleUi for NotificationsUi {
    fn widget(&self) -> gtk4::Widget {
        self.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Notifications {
            return;
        }
        let live = self.service.live();
        self.button.set_visible(live);
        if !live {
            return;
        }
        self.image.set_icon_name(Some(if self.service.dnd() {
            "notifications-disabled-symbolic"
        } else {
            "preferences-system-notifications-symbolic"
        }));
        let unread = self.service.unread();
        let text = badge::badge_text(unread);
        self.badge.set_visible(text.is_some());
        self.badge.set_text(text.as_deref().unwrap_or_default());
        let name = badge::accessible_name(unread, &tr);
        self.button.set_tooltip_text(Some(&name));
        self.button.update_property(&[Property::Label(&name)]);
    }

    fn open(&self, _bar: &Rc<Bar>) {
        toggle_notifications();
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = bar.notifications.clone();
    let image = gtk4::Image::from_icon_name("preferences-system-notifications-symbolic");
    let badge = gtk4::Label::new(None);
    badge.add_css_class("bar-badge");
    badge.set_visible(false);
    let face = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    face.append(&image);
    face.append(&badge);
    let button = gtk4::Button::new();
    button.set_child(Some(&face));
    button.add_css_class("bar-button");
    let name = tr("Notifications");
    button.set_tooltip_text(Some(&name));
    button.update_property(&[Property::Label(&name)]);
    button.set_visible(false);
    button.connect_clicked(|_| toggle_notifications());
    Some(Box::new(NotificationsUi {
        button,
        image,
        badge,
        service,
    }))
}
