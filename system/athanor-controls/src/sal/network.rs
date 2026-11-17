//! The network page drawn with athanor-sal (doc_bar.md, BR3): the wired state, the Wi-Fi list,
//! joining with a password, VPN and airplane mode, as the GTK page shows them. The model in
//! athanor-services mirrors NetworkManager and is its secret agent; this side draws its state,
//! sends it the person's commands and shows the password page it asks for.
//!
//! A password goes from the masked field into the model's reply and nowhere else: it is read
//! from the field only as it is sent, moved into the command or the reply without a copy that
//! stays behind, never logged, and the field is cleared as it leaves.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_sal::{Event, Kind, Ui, WidgetId};
use athanor_services::network::{
    self, Link, Network, NetworkCommand, NetworkState, PasswordPage, PasswordPrompt, Security, Vpn,
    Zeroizing,
};
use tokio::sync::{mpsc, oneshot};

use super::widgets::{column, heading, switch_row, Failure, Look};
use super::Turn;
use crate::bridge;
use crate::i18n::{tr, tr_with};
use crate::Services;

/// The height of the Wi-Fi list's viewport, in logical pixels.
const MAX_LIST_HEIGHT: f32 = 320.0;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

/// The network model of the process, which every page shows, and NetworkManager's requests.
struct Service {
    commands: mpsc::UnboundedSender<NetworkCommand>,
    /// The model's last state, which `bridge::follow` keeps current; `None` while
    /// NetworkManager is away.
    state: RefCell<Option<NetworkState>>,
    /// Counts the changes a page shows: a state, a request, a request withdrawn.
    states: Cell<u64>,
    /// Counts the actions that did not complete: the model refused them or had ended.
    failures: Cell<u64>,
    /// A request of NetworkManager the person has not answered.
    pending: RefCell<Option<oneshot::Sender<Option<Zeroizing<String>>>>>,
    /// The password page of `pending`, until a page takes it up.
    offered: RefCell<Option<PasswordPage>>,
    /// Whether [`Page::wants_open`] has said so for `offered`.
    announced: Cell<bool>,
    /// Which request `pending` holds, so a withdrawal of an older one closes nothing.
    serial: Cell<u64>,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = SERVICE.with(|cell| cell.borrow().clone()) {
            return service;
        }
        let (states, commands) = services.network.clone();
        let prompts = services.password_prompts.borrow_mut().take();
        let service = Rc::new(Service::new(commands));
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak: the thread-local owns the service.
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state: &Option<NetworkState>| {
            let Some(service) = weak.upgrade() else {
                return;
            };
            let refused = |s: &Option<NetworkState>| s.as_ref().map(|s| s.refused);
            if let (Some(now), Some(before)) = (refused(state), refused(&service.state.borrow())) {
                if now != before {
                    service.failed();
                }
            }
            service.state.replace(state.clone());
            service.changed();
        });
        if let Some(prompts) = prompts {
            let weak = Rc::downgrade(&service);
            bridge::drain(prompts, move |prompt| {
                if let Some(service) = weak.upgrade() {
                    service.prompt(prompt);
                }
            });
        }
        service
    }

    fn new(commands: mpsc::UnboundedSender<NetworkCommand>) -> Service {
        Service {
            commands,
            state: RefCell::new(None),
            states: Cell::new(0),
            failures: Cell::new(0),
            pending: RefCell::new(None),
            offered: RefCell::new(None),
            announced: Cell::new(false),
            serial: Cell::new(0),
        }
    }

    fn changed(&self) {
        self.states.set(self.states.get() + 1);
    }

    fn failed(&self) {
        self.failures.set(self.failures.get() + 1);
    }

    fn send(&self, command: NetworkCommand) {
        if let Err(err) = self.commands.send(command) {
            // `SendError` carries the command, which may hold a password: say only that it
            // failed.
            tracing::warn!(
                "the network model did not take a command ({})",
                err.to_string()
            );
            // The channel is unbounded, so a send fails only when the model has ended: the
            // action did not complete, as for a refusal, and the page shows the state again.
            self.failed();
            self.changed();
        }
    }

    /// NetworkManager asks for a password: keeps the request for a page to show.
    fn prompt(self: &Rc<Self>, prompt: PasswordPrompt) {
        let PasswordPrompt {
            request,
            reply,
            withdrawn,
        } = prompt;
        // Dropping `reply` answers NoSecrets.
        let Some(page) = request.prompt() else { return };
        let Some(serial) = self.offer(page, reply) else {
            return;
        };
        // The model withdraws the request when NetworkManager cancels it or leaves, or when no
        // one answers in time; a request that was answered ends this wait with an error.
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if withdrawn.await.is_ok() {
                if let Some(service) = weak.upgrade() {
                    service.withdraw(serial);
                }
            }
        });
    }

    /// Holds a request for a page, unless one is held: its serial.
    fn offer(
        &self,
        page: PasswordPage,
        reply: oneshot::Sender<Option<Zeroizing<String>>>,
    ) -> Option<u64> {
        if self.pending.borrow().is_some() {
            return None;
        }
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        self.pending.replace(Some(reply));
        self.offered.replace(Some(page));
        self.announced.set(false);
        self.changed();
        Some(serial)
    }

    /// Answers the pending request: the password, or a cancel.
    fn answer(&self, password: Option<Zeroizing<String>>) {
        let Some(reply) = self.pending.take() else {
            return;
        };
        // The model stopped waiting, as its timeout ended the request: the password comes back
        // in the error and is overwritten as it drops.
        if reply.send(password).is_err() {
            tracing::warn!("the network model no longer waited for the password page's answer");
        }
    }

    /// The model no longer waits for request `serial`: the page closes its prompt.
    fn withdraw(&self, serial: u64) {
        if self.serial.get() != serial || self.pending.take().is_none() {
            return;
        }
        self.offered.take();
        self.changed();
    }

    fn switch_vpn(&self, vpn: &Vpn, on: bool) {
        match (&vpn.active, on) {
            (None, true) => self.send(NetworkCommand::Activate {
                connection: vpn.connection.clone(),
                device: "/".to_owned(),
                specific: "/".to_owned(),
            }),
            (Some(active), false) => self.send(NetworkCommand::Deactivate(active.clone())),
            _ => {}
        }
    }
}

/// What the password page answers.
enum Ask {
    /// A network with no saved profile: the password goes into `AddAndActivateConnection`.
    Join { network: Network },
    /// NetworkManager's `GetSecrets`, answered through the pending reply.
    Secrets,
}

/// The password page while it asks.
struct Asked {
    ask: Ask,
    label: String,
    security: Security,
    /// NetworkManager asks again because the password it had was refused.
    retry: bool,
}

/// A network row's name: its label, and what the person needs to know before pressing it.
fn network_name(network: &Network) -> String {
    let usable =
        network.security != Security::Other || network.saved.is_some() || network.active.is_some();
    match (network.link, network.security) {
        (Link::Connected, _) => tr_with("{network}, connected", "network", &network.label),
        (Link::Connecting, _) => tr_with("{network}, connecting", "network", &network.label),
        _ if !usable => tr_with("{network}, needs Settings", "network", &network.label),
        (_, Security::Personal { .. }) => tr_with("{network}, secured", "network", &network.label),
        _ => network.label.clone(),
    }
}

/// The networks the rows show, each with the icon of its strength and the strength left out: a
/// row is built again when its icon or anything else changes, not on every strength update of
/// an access point, which would take the keyboard focus off a row.
fn shape(state: Option<&NetworkState>) -> (String, Vec<(&'static str, Network)>) {
    let Some(wifi) = state.and_then(|state| state.wifi.as_ref()) else {
        return (String::new(), Vec::new());
    };
    let networks = wifi.networks.iter().map(|network| {
        let icon = network::signal_icon(network.strength);
        (
            icon,
            Network {
                strength: 0,
                ..network.clone()
            },
        )
    });
    (wifi.device.clone(), networks.collect())
}

/// The widgets of the page in the surface's [`Ui`].
struct Ids {
    list: WidgetId,
    wired: WidgetId,
    wifi: WidgetId,
    scroll: WidgetId,
    vpns: WidgetId,
    /// `None` on a page whose surface has its own airplane control.
    airplane: Option<WidgetId>,
    /// The Wi-Fi device and each network's row, in the order of the state the page was built
    /// with.
    device: String,
    rows: Vec<(WidgetId, Network)>,
    /// The icon of each row's strength.
    icons: Vec<&'static str>,
    vpn_rows: Vec<(WidgetId, Vpn)>,
    password: WidgetId,
    title: WidgetId,
    retry: WidgetId,
    field: WidgetId,
    cancel: WidgetId,
    connect: WidgetId,
    failure: Failure,
}

/// What an event of the page's widgets asks for.
enum Hit {
    Wifi(WidgetId),
    Airplane(WidgetId),
    Vpn(Vpn, WidgetId),
    Network(Network, String),
    Field,
    Connect,
    Cancel,
}

/// The network page: one per surface, over the one model of the process. The surface adds it
/// to its widgets with [`Page::build`], calls [`Page::turn`] in each turn of its loop and hands
/// it every event with [`Page::event`]. It asks [`Page::wants_open`] while the page is hidden,
/// so that a password prompt of NetworkManager opens it, and [`Page::wants_close`] after each
/// event and turn, to close the surface once the prompt is done.
pub struct Page {
    service: Rc<Service>,
    airplane_switch: bool,
    /// The state and the failure the page shows last.
    seen: (u64, u64),
    ids: Option<Ids>,
    /// Whether the page is on screen: built, and not closed since.
    shown: bool,
    asking: Option<Asked>,
    /// What the prompt's field holds, put back when the page is built again in a new [`Ui`],
    /// as GTK's entry kept its text while the page updated in place. Overwritten when dropped.
    typed: Zeroizing<String>,
    close: bool,
}

impl Page {
    /// The page with its airplane switch.
    pub fn new(services: &Services) -> Page {
        Page::with(Service::get(services), true)
    }

    /// The page of a surface that has its own airplane control, as the control center: this
    /// switch only turns off Wi-Fi and mobile data through NetworkManager, where a panel's tile
    /// blocks every radio.
    pub fn without_airplane(services: &Services) -> Page {
        Page::with(Service::get(services), false)
    }

    fn with(service: Rc<Service>, airplane_switch: bool) -> Page {
        Page {
            service,
            airplane_switch,
            seen: (0, 0),
            ids: None,
            shown: false,
            asking: None,
            typed: Zeroizing::default(),
            close: false,
        }
    }

    /// Whether the model changed since the page last showed it: the surface calls
    /// [`Page::turn`] then.
    pub fn changed(&self) -> bool {
        (self.service.states.get(), self.service.failures.get()) != self.seen
    }

    /// Whether the surface should open the page: NetworkManager asked for a password while the
    /// page was hidden. True once for each request; the page built next shows the prompt.
    pub fn wants_open(&mut self) -> bool {
        let service = &self.service;
        let wants = !self.shown && service.offered.borrow().is_some() && !service.announced.get();
        if wants {
            service.announced.set(true);
        }
        wants
    }

    /// Whether the surface should close, as the GTK page's host: the password was sent, the
    /// person cancelled, or NetworkManager withdrew the request. True once.
    pub fn wants_close(&mut self) -> bool {
        std::mem::take(&mut self.close)
    }

    /// Adds the page's widgets as a column under `parent`, showing the model's state, and
    /// returns the column, its rows with `look`'s icons. A page built again, in a new [`Ui`],
    /// leaves the old one's widgets; a password prompt it was showing stays, with what was
    /// typed in its field.
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, look: &mut Look) -> WidgetId {
        let page = column(ui, parent);
        let list = column(ui, page);
        let title = ui.add(list, Kind::Label(tr("Network")));
        ui.set_text_style(title, heading());
        let wired = ui.add(list, Kind::Label(String::new()));
        look.note(ui, wired);
        let wifi = switch_row(ui, list, &tr("Wi-Fi"), false);
        let scroll = ui.scroll(list, MAX_LIST_HEIGHT);
        ui.set_natural_height(scroll, true);
        let networks = column(ui, scroll);
        let vpns = column(ui, list);
        let airplane = self
            .airplane_switch
            .then(|| switch_row(ui, list, &tr("Airplane mode"), false));
        ui.set_visible(wired, false);
        ui.set_visible(wifi, false);

        let state = self.service.state.borrow();
        let (device, wanted) = shape(state.as_ref());
        let icons = wanted.iter().map(|(icon, _)| *icon).collect();
        let rows = wanted
            .into_iter()
            .map(|(icon, network)| {
                // The strength's icon, then the name, as the GTK row's box.
                let name = network_name(&network);
                let icon = (look.icon)(icon);
                let row = ui.labelled_button(networks, &name, icon, look.icon_size, &name);
                let usable = network.security != Security::Other
                    || network.saved.is_some()
                    || network.active.is_some();
                ui.set_sensitive(row, usable);
                (row, network)
            })
            .collect();
        let vpn_rows = state
            .as_ref()
            .map(|state| state.vpns.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|vpn| (switch_row(ui, vpns, &vpn.label, vpn.link != Link::Idle), vpn))
            .collect();
        drop(state);

        let password = column(ui, page);
        let title = ui.add(password, Kind::Label(String::new()));
        ui.set_text_style(title, heading());
        let retry = ui.add(
            password,
            Kind::Label(tr("The password was not accepted. Try again.")),
        );
        look.note(ui, retry);
        let field = ui.password(password, &tr("Password"));
        if self.asking.is_some() {
            ui.set_field_text(field, &self.typed);
            // A single-line field's caret stays at its start; what is typed next goes after.
            ui.set_field_range(field, (usize::MAX, usize::MAX));
        }
        let buttons = ui.add(password, Kind::Row);
        let cancel = ui.add(buttons, Kind::Button(tr("Cancel")));
        let connect = ui.add(buttons, Kind::Button(tr("Connect")));
        ui.set_visible(password, false);

        let failure = Failure::new(ui, page, look);
        self.ids = Some(Ids {
            list,
            wired,
            wifi,
            scroll,
            vpns,
            airplane,
            device,
            rows,
            icons,
            vpn_rows,
            password,
            title,
            retry,
            field,
            cancel,
            connect,
            failure,
        });
        self.shown = true;
        self.take_offer();
        self.show(ui);
        page
    }

    /// Shows what changed since the last call: a new state or request, and a failure as the
    /// note under the page. A network that came, went or changed, or a VPN that came or went,
    /// asks for the page to be built again; a VPN that connects or disconnects only sets its
    /// switch, which keeps the keyboard focus on the switch the person just pressed.
    pub fn turn(&mut self, ui: &mut Ui) -> Turn {
        let now = (self.service.states.get(), self.service.failures.get());
        if now == self.seen {
            return Turn::Unchanged;
        }
        if self.shown && self.ids.is_some() {
            self.take_offer();
            // The request was withdrawn, or answered elsewhere: the prompt closes.
            if matches!(&self.asking, Some(Asked { ask: Ask::Secrets, .. }))
                && self.service.pending.borrow().is_none()
            {
                self.asking = None;
                self.clear_field(ui);
                self.close = true;
            }
        }
        let rebuild = self.ids.as_ref().is_some_and(|ids| {
            let state = self.service.state.borrow();
            let (device, wanted) = shape(state.as_ref());
            let vpns = state.as_ref().map_or(&[][..], |s| s.vpns.as_slice());
            ids.device != device
                || !ids.rows.iter().map(|(_, n)| n).zip(&ids.icons).eq(wanted.iter().map(|(i, n)| (n, i)))
                || !ids
                    .vpn_rows
                    .iter()
                    .map(|(_, v)| (&v.connection, &v.label))
                    .eq(vpns.iter().map(|v| (&v.connection, &v.label)))
        });
        if rebuild {
            return Turn::Rebuild;
        }
        let failed = now.1 != self.seen.1;
        self.show(ui);
        if let (true, Some(ids)) = (failed, &self.ids) {
            ids.failure.show(ui);
        }
        Turn::Updated
    }

    /// The page left the screen: a request of NetworkManager that it showed is canceled, and a
    /// note says nothing of the actions to come.
    pub fn closed(&mut self, ui: &mut Ui) {
        self.shown = false;
        if let Some(ids) = &self.ids {
            ids.failure.hide(ui);
        }
        self.abandon(ui);
    }

    /// The surface dropped the [`Ui`] that held the page, as a popover that closed: the page
    /// leaves the screen as [`Page::closed`] says, with no widgets left to set.
    pub fn dropped(&mut self) {
        self.shown = false;
        self.ids = None;
        self.typed = Zeroizing::default();
        if matches!(self.asking.take(), Some(Asked { ask: Ask::Secrets, .. })) {
            self.service.answer(None);
        }
    }

    /// Takes `event` if it is one of the page's widgets': whether it was.
    pub fn event(&mut self, ui: &mut Ui, event: &Event) -> bool {
        let Some(ids) = &self.ids else {
            return false;
        };
        let hit = match *event {
            Event::Toggled(id) if id == ids.wifi => Hit::Wifi(id),
            Event::Toggled(id) if Some(id) == ids.airplane => Hit::Airplane(id),
            Event::Toggled(id) => match ids.vpn_rows.iter().find(|(row, _)| *row == id) {
                Some((_, vpn)) => Hit::Vpn(vpn.clone(), id),
                None => return false,
            },
            Event::Edited(id) if id == ids.field => Hit::Field,
            // Enter in the field connects, as the GTK entry's `activate` did.
            Event::Activated(id) if id == ids.field => Hit::Connect,
            Event::Clicked(id) if id == ids.connect => Hit::Connect,
            Event::Clicked(id) if id == ids.cancel => Hit::Cancel,
            Event::Clicked(id) => match ids.rows.iter().find(|(row, _)| *row == id) {
                Some((_, network)) => Hit::Network(network.clone(), ids.device.clone()),
                None => return false,
            },
            _ => return false,
        };
        match hit {
            Hit::Wifi(id) => {
                self.service.send(NetworkCommand::Wireless(ui.is_active(id)));
            }
            Hit::Airplane(id) => {
                self.service.send(NetworkCommand::Airplane(ui.is_active(id)));
            }
            Hit::Vpn(vpn, id) => self.service.switch_vpn(&vpn, ui.is_active(id)),
            Hit::Network(network, device) => self.pressed(ui, &network, &device),
            Hit::Field => {
                let typed = self.ids.as_ref().and_then(|ids| ui.password_text(ids.field));
                self.typed = Zeroizing::new(typed.unwrap_or_default().to_owned());
                self.show_prompt(ui);
            }
            Hit::Connect => self.submit(ui),
            Hit::Cancel => {
                self.abandon(ui);
                self.close = true;
            }
        }
        true
    }

    /// A network's row was pressed: a connected one disconnects, a saved one activates, an open
    /// one joins, a secured one asks for its password.
    fn pressed(&mut self, ui: &mut Ui, network: &Network, device: &str) {
        let service = &self.service;
        if let Some(active) = &network.active {
            service.send(NetworkCommand::Deactivate(active.clone()));
        } else if let Some(saved) = &network.saved {
            service.send(NetworkCommand::Activate {
                connection: saved.clone(),
                device: device.to_owned(),
                specific: network.access_point.clone(),
            });
        } else {
            match network.security {
                Security::Open => service.send(NetworkCommand::Join {
                    network: network.clone(),
                    password: None,
                }),
                Security::Personal { .. } => {
                    self.clear_field(ui);
                    self.asking = Some(Asked {
                        ask: Ask::Join {
                            network: network.clone(),
                        },
                        label: network.label.clone(),
                        security: network.security,
                        retry: false,
                    });
                    self.show_prompt(ui);
                }
                Security::Other => {}
            }
        }
    }

    /// Takes the request of NetworkManager that waits for a page. The prompt of a network the
    /// person is joining stays: the request's reply is dropped, which NetworkManager hears as
    /// NoSecrets, as the GTK page's `return` dropped it; the person canceled nothing.
    fn take_offer(&mut self) {
        let Some(page) = self.service.offered.take() else {
            return;
        };
        if self.asking.is_some() {
            self.service.pending.take();
            return;
        }
        self.asking = Some(Asked {
            ask: Ask::Secrets,
            label: page.label,
            security: page.security,
            retry: page.retry,
        });
    }

    /// Sends the password if the security accepts it: a join, or the answer to NetworkManager.
    fn submit(&mut self, ui: &mut Ui) {
        let (Some(ids), Some(asked)) = (&self.ids, &self.asking) else {
            return;
        };
        let field = ids.field;
        let acceptable = ui
            .password_text(field)
            .is_some_and(|password| network::password_acceptable(asked.security, password));
        if !acceptable {
            return;
        }
        let Some(asked) = self.asking.take() else {
            return;
        };
        // The one owned copy, which moves into the command or the reply and is overwritten
        // wherever it drops.
        let password = ui.password_text(field).map(|p| Zeroizing::new(p.to_owned()));
        self.clear_field(ui);
        match asked.ask {
            Ask::Join { network } => self.service.send(NetworkCommand::Join { network, password }),
            Ask::Secrets => self.service.answer(password),
        }
        self.close = true;
        self.show_prompt(ui);
    }

    /// The page was hidden or cancelled while it asked: a NetworkManager request is canceled.
    fn abandon(&mut self, ui: &mut Ui) {
        let asked = self.asking.take();
        self.clear_field(ui);
        if matches!(asked, Some(Asked { ask: Ask::Secrets, .. })) {
            self.service.answer(None);
        }
        self.show_prompt(ui);
    }

    fn clear_field(&mut self, ui: &mut Ui) {
        self.typed = Zeroizing::default();
        if let Some(ids) = &self.ids {
            ui.set_field_text(ids.field, "");
        }
    }

    /// Shows the password page in place of the list while the page asks.
    fn show_prompt(&self, ui: &mut Ui) {
        let Some(ids) = &self.ids else {
            return;
        };
        let was_asking = ui.is_visible(ids.password);
        ui.set_visible(ids.list, self.asking.is_none());
        ui.set_visible(ids.password, self.asking.is_some());
        let Some(asked) = &self.asking else {
            ui.set_sensitive(ids.connect, false);
            return;
        };
        ui.set_text(
            ids.title,
            &tr_with("Password for {network}", "network", &asked.label),
        );
        ui.set_visible(ids.retry, asked.retry);
        let acceptable = ui
            .password_text(ids.field)
            .is_some_and(|password| network::password_acceptable(asked.security, password));
        ui.set_sensitive(ids.connect, acceptable);
        if !was_asking {
            ui.set_focus(Some(ids.field));
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        self.seen = (self.service.states.get(), self.service.failures.get());
        if let Some(ids) = &mut self.ids {
            if let Some(state) = self.service.state.borrow().as_ref() {
                match state.wired {
                    Some(true) => ui.set_text(ids.wired, &tr("Wired: connected")),
                    Some(false) => ui.set_text(ids.wired, &tr("Wired: not connected")),
                    None => None,
                };
                ui.set_visible(ids.wired, state.wired.is_some());
                ui.set_visible(ids.wifi, state.wifi.is_some());
                ui.set_active(ids.wifi, state.wifi.as_ref().is_some_and(|w| w.enabled));
                if let Some(airplane) = ids.airplane {
                    ui.set_active(airplane, state.airplane);
                }
                ui.set_visible(ids.scroll, !ids.rows.is_empty());
                ui.set_visible(ids.vpns, !ids.vpn_rows.is_empty());
                // The rows are those of the state's VPNs, in its order: `turn` asks for a
                // rebuild otherwise. A switch press reads the active connection kept here.
                for ((row, shown), vpn) in ids.vpn_rows.iter_mut().zip(&state.vpns) {
                    ui.set_active(*row, vpn.link != Link::Idle);
                    *shown = vpn.clone();
                }
            }
        }
        self.show_prompt(ui);
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // The surface was removed while NetworkManager waited for this page's answer.
        if matches!(&self.asking, Some(Asked { ask: Ask::Secrets, .. })) {
            self.service.answer(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use athanor_sal::{Image, Theme};
    use athanor_services::network::Wifi;

    use super::*;

    fn network(label: &str, security: Security) -> Network {
        Network {
            ssid: label.as_bytes().to_vec(),
            label: label.to_owned(),
            strength: 70,
            security,
            access_point: format!("/ap/{label}"),
            link: Link::Idle,
            active: None,
            saved: None,
        }
    }

    fn state(networks: Vec<Network>, vpns: Vec<Vpn>) -> NetworkState {
        NetworkState {
            wired: Some(false),
            wifi: Some(Wifi {
                device: "/dev/wlan0".to_owned(),
                enabled: true,
                networks,
            }),
            vpns,
            airplane: false,
            refused: 0,
        }
    }

    fn vpn(label: &str, active: Option<&str>) -> Vpn {
        Vpn {
            connection: format!("/vpn/{label}"),
            label: label.to_owned(),
            active: active.map(str::to_owned),
            link: if active.is_some() {
                Link::Connected
            } else {
                Link::Idle
            },
        }
    }

    fn page(state: NetworkState) -> (Page, mpsc::UnboundedReceiver<NetworkCommand>, Ui) {
        let (commands, received) = mpsc::unbounded_channel();
        let service = Service::new(commands);
        service.state.replace(Some(state));
        service.changed();
        let mut page = Page::with(Rc::new(service), true);
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        (page, received, ui)
    }

    fn click(page: &mut Page, ui: &mut Ui, id: WidgetId) {
        let event = ui.activate(id);
        assert!(event.is_some_and(|event| page.event(ui, &event)));
    }

    fn ids(page: &Page) -> &Ids {
        match &page.ids {
            Some(ids) => ids,
            None => unreachable!("built"),
        }
    }

    fn type_password(page: &mut Page, ui: &mut Ui, text: &str) {
        let field = ids(page).field;
        ui.set_focus(Some(field));
        let edited = ui.commit(text);
        assert!(edited.is_some_and(|event| page.event(ui, &event)));
    }

    #[test]
    fn a_row_shows_the_icon_of_its_strength_before_its_name() {
        let mut strong = network("Athanor Lab", Security::Open);
        strong.strength = 90;
        let (mut page, _received, _) = page(state(vec![strong], vec![]));
        let asked = Rc::new(RefCell::new(Vec::new()));
        let mut look = Look {
            icon: Box::new({
                let asked = asked.clone();
                move |name| {
                    asked.borrow_mut().push(name);
                    Image::from_rgba8(1, 1, vec![0; 4]).ok()
                }
            }),
            ..Look::plain()
        };
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut look);
        assert_eq!(*asked.borrow(), ["network-wireless-signal-excellent-symbolic"]);
        let row = ids(&page).rows[0].0;
        assert!(matches!(
            &ui.widget(row).kind,
            Kind::LabelledButton { image: Some(_), text, .. } if text == "Athanor Lab"
        ));
    }

    #[test]
    fn the_rows_are_built_from_the_state_in_its_order() {
        let mut lab = network("Athanor Lab", Security::Personal { sae: false });
        lab.link = Link::Connected;
        lab.active = Some("/active/lab".to_owned());
        let state = state(
            vec![
                lab,
                network("Corner Café", Security::Open),
                network("Home Network", Security::Personal { sae: false }),
                network("Odd", Security::Other),
            ],
            vec![vpn("Office VPN", None)],
        );
        let (page, _received, mut ui) = page(state);
        let ids = ids(&page);
        let names: Vec<_> = ids
            .rows
            .iter()
            .map(|(row, _)| match &ui.widget(*row).kind {
                Kind::LabelledButton { label, text, .. } if label == text => text.clone(),
                _ => unreachable!("a button named by its text"),
            })
            .collect();
        assert_eq!(
            names,
            [
                "Athanor Lab, connected",
                "Corner Café",
                "Home Network, secured",
                "Odd, needs Settings"
            ]
        );
        assert!(!ui.is_sensitive(ids.rows[3].0));
        assert!(ui.is_visible(ids.wifi) && ui.is_active(ids.wifi));
        assert!(ui.is_visible(ids.wired) && ui.is_visible(ids.list));
        assert_eq!(ui.widget(ids.wired).text_style(), &Look::plain().caption);
        ui.layout();
        assert!(ui.widget(ids.scroll).rect.h < MAX_LIST_HEIGHT, "as tall as its rows");
        assert!(!ui.is_visible(ids.password));
        assert!(matches!(&ui.widget(ids.vpn_rows[0].0).kind, Kind::Switch { label, .. } if label == "Office VPN"));
    }

    #[test]
    fn a_press_on_an_open_saved_or_connected_network_sends_the_command() {
        let mut saved = network("Saved", Security::Personal { sae: false });
        saved.saved = Some("/conn/saved".to_owned());
        let mut up = network("Up", Security::Open);
        up.active = Some("/active/up".to_owned());
        let (mut page, mut received, mut ui) =
            page(state(vec![network("Cafe", Security::Open), saved, up], vec![]));
        let rows: Vec<_> = ids(&page).rows.iter().map(|(row, _)| *row).collect();
        click(&mut page, &mut ui, rows[0]);
        assert!(matches!(
            received.try_recv(),
            Ok(NetworkCommand::Join { password: None, .. })
        ));
        click(&mut page, &mut ui, rows[1]);
        match received.try_recv() {
            Ok(NetworkCommand::Activate {
                connection,
                device,
                specific,
            }) => assert_eq!(
                (connection.as_str(), device.as_str(), specific.as_str()),
                ("/conn/saved", "/dev/wlan0", "/ap/Saved")
            ),
            other => unreachable!("an activation, not {other:?}"),
        }
        click(&mut page, &mut ui, rows[2]);
        assert!(matches!(
            received.try_recv(),
            Ok(NetworkCommand::Deactivate(active)) if active == "/active/up"
        ));
    }

    #[test]
    fn a_secured_network_asks_and_connect_sends_the_typed_password_then_clears_the_field() {
        let (mut page, mut received, mut ui) = page(state(
            vec![network("Home Network", Security::Personal { sae: false })],
            vec![],
        ));
        let (row, field, connect) = {
            let ids = ids(&page);
            (ids.rows[0].0, ids.field, ids.connect)
        };
        click(&mut page, &mut ui, row);
        assert!(received.try_recv().is_err(), "nothing is sent before the password");
        assert!(ui.is_visible(ids(&page).password) && !ui.is_visible(ids(&page).list));
        assert!(matches!(
            &ui.widget(ids(&page).title).kind,
            Kind::Label(t) if t == "Password for Home Network"
        ));
        assert!(!ui.is_visible(ids(&page).retry));
        assert!(!ui.is_sensitive(connect), "an empty password is not acceptable");
        type_password(&mut page, &mut ui, "short");
        assert!(!ui.is_sensitive(connect), "a short WPA password is not acceptable");
        type_password(&mut page, &mut ui, "correct horse");
        assert!(ui.is_sensitive(connect));
        click(&mut page, &mut ui, connect);
        match received.try_recv() {
            Ok(NetworkCommand::Join { network, password }) => {
                assert_eq!(network.label, "Home Network");
                assert_eq!(password.as_ref().map(|p| p.as_str()), Some("shortcorrect horse"));
            }
            other => unreachable!("a join, not {other:?}"),
        }
        assert_eq!(ui.password_text(field), Some(""));
        assert!(ui.is_visible(ids(&page).list));
        assert!(page.wants_close() && !page.wants_close());
    }

    #[test]
    fn enter_in_the_password_field_connects_once_the_password_is_acceptable() {
        let (mut page, mut received, mut ui) = page(state(
            vec![network("Home Network", Security::Personal { sae: false })],
            vec![],
        ));
        let row = ids(&page).rows[0].0;
        click(&mut page, &mut ui, row);
        type_password(&mut page, &mut ui, "short");
        let enter = ui.key(athanor_sal::Key::Enter);
        assert_eq!(enter, Some(Event::Activated(ids(&page).field)));
        assert!(enter.is_some_and(|event| page.event(&mut ui, &event)));
        assert!(received.try_recv().is_err(), "a short WPA password is not sent");
        assert!(ui.is_visible(ids(&page).password), "the prompt stays");
        type_password(&mut page, &mut ui, " horse");
        let enter = ui.key(athanor_sal::Key::Enter);
        assert!(enter.is_some_and(|event| page.event(&mut ui, &event)));
        match received.try_recv() {
            Ok(NetworkCommand::Join { network, password }) => {
                assert_eq!(network.label, "Home Network");
                assert_eq!(password.as_ref().map(|p| p.as_str()), Some("short horse"));
            }
            other => unreachable!("a join, not {other:?}"),
        }
        assert!(page.wants_close());
    }

    fn secrets_page() -> (Page, Ui, oneshot::Receiver<Option<Zeroizing<String>>>) {
        let (mut page, _received, mut ui) = page(state(vec![], vec![]));
        let (reply, answer) = oneshot::channel();
        let offered = PasswordPage {
            label: "Home Network".to_owned(),
            security: Security::Personal { sae: false },
            retry: true,
        };
        assert_eq!(page.service.offer(offered, reply), Some(1));
        assert!(page.service.offer(
            PasswordPage {
                label: "Other".to_owned(),
                security: Security::Open,
                retry: false
            },
            oneshot::channel().0
        )
        .is_none());
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        (page, ui, answer)
    }

    /// Counts the warnings logged while it is the default subscriber.
    struct Warnings(std::sync::atomic::AtomicUsize);

    impl tracing::Subscriber for Warnings {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            if *event.metadata().level() == tracing::Level::WARN {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[test]
    fn an_answer_the_model_no_longer_waits_for_is_logged_and_the_prompt_closes() {
        let (mut page, mut ui, answer) = secrets_page();
        // The model's timeout ended the request before the person connected.
        drop(answer);
        type_password(&mut page, &mut ui, "correct horse");
        let warnings = std::sync::Arc::new(Warnings(Default::default()));
        let connect = ids(&page).connect;
        tracing::subscriber::with_default(std::sync::Arc::clone(&warnings), || {
            click(&mut page, &mut ui, connect);
        });
        assert_eq!(warnings.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(page.service.pending.take().is_none());
        assert!(page.wants_close());
    }

    #[test]
    fn a_request_while_the_person_joins_a_network_is_dropped_not_canceled() {
        let (mut page, _received, mut ui) = page(state(
            vec![network("Home Network", Security::Personal { sae: false })],
            vec![],
        ));
        let row = ids(&page).rows[0].0;
        click(&mut page, &mut ui, row);
        type_password(&mut page, &mut ui, "correct");
        let (reply, mut answer) = oneshot::channel();
        let offered = PasswordPage {
            label: "Other".to_owned(),
            security: Security::Personal { sae: false },
            retry: false,
        };
        assert_eq!(page.service.offer(offered, reply), Some(1));
        page.turn(&mut ui);
        // Dropped, as the GTK page did: NetworkManager hears NoSecrets, not UserCanceled.
        assert!(matches!(
            answer.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert_eq!(ui.password_text(ids(&page).field), Some("correct"), "the join's prompt stays");
    }

    #[test]
    fn a_rebuild_while_the_prompt_is_open_keeps_what_was_typed() {
        let (mut page, mut received, mut ui) = page(state(
            vec![network("Home Network", Security::Personal { sae: false })],
            vec![],
        ));
        let row = ids(&page).rows[0].0;
        click(&mut page, &mut ui, row);
        type_password(&mut page, &mut ui, "correct horse");
        // A network came: the surface builds the page again in a new Ui.
        page.service.state.replace(Some(state(
            vec![
                network("Home Network", Security::Personal { sae: false }),
                network("Lab", Security::Open),
            ],
            vec![],
        )));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        let field = ids(&page).field;
        assert!(ui.is_visible(ids(&page).password), "the prompt stays");
        assert_eq!(ui.password_text(field), Some("correct horse"));
        // Typing goes on at the end of what was kept, not before it.
        type_password(&mut page, &mut ui, "!");
        assert_eq!(ui.password_text(field), Some("correct horse!"));
        let connect = ids(&page).connect;
        click(&mut page, &mut ui, connect);
        match received.try_recv() {
            Ok(NetworkCommand::Join { password, .. }) => {
                assert_eq!(password.as_ref().map(|p| p.as_str()), Some("correct horse!"));
            }
            other => unreachable!("a join, not {other:?}"),
        }
        // Sent: the next prompt starts empty.
        assert!(page.typed.is_empty());
    }

    #[test]
    fn a_request_of_networkmanager_shows_the_prompt_and_connect_answers_it() {
        let (mut page, mut ui, mut answer) = secrets_page();
        let (connect, retry) = (ids(&page).connect, ids(&page).retry);
        assert!(ui.is_visible(retry), "the retry text shows on a second ask");
        assert!(matches!(
            &ui.widget(retry).kind,
            Kind::Label(t) if t == "The password was not accepted. Try again."
        ));
        type_password(&mut page, &mut ui, "correct horse");
        click(&mut page, &mut ui, connect);
        assert!(matches!(answer.try_recv(), Ok(Some(p)) if p.as_str() == "correct horse"));
        assert_eq!(ui.password_text(ids(&page).field), Some(""));
        assert!(page.wants_close());
    }

    #[test]
    fn cancel_answers_none_and_closes() {
        let (mut page, mut ui, mut answer) = secrets_page();
        let cancel = ids(&page).cancel;
        type_password(&mut page, &mut ui, "correct horse");
        click(&mut page, &mut ui, cancel);
        assert!(matches!(answer.try_recv(), Ok(None)));
        assert_eq!(ui.password_text(ids(&page).field), Some(""));
        assert!(ui.is_visible(ids(&page).list));
        assert!(page.wants_close());
    }

    #[test]
    fn a_withdrawn_request_closes_the_prompt() {
        let (mut page, mut ui, mut answer) = secrets_page();
        page.service.withdraw(99);
        assert!(page.service.pending.borrow().is_some(), "an older serial closes nothing");
        page.service.withdraw(1);
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(ui.is_visible(ids(&page).list) && !ui.is_visible(ids(&page).password));
        assert!(page.wants_close());
        assert!(answer.try_recv().is_err(), "the model was told nothing: it withdrew");
    }

    #[test]
    fn a_request_while_the_page_is_hidden_asks_the_surface_to_open_it_once() {
        let (commands, _received) = mpsc::unbounded_channel();
        let service = Rc::new(Service::new(commands));
        let mut page = Page::with(service.clone(), false);
        assert!(!page.wants_open());
        let offered = PasswordPage {
            label: "Home".to_owned(),
            security: Security::Personal { sae: true },
            retry: false,
        };
        let (reply, mut answer) = oneshot::channel();
        service.offer(offered, reply);
        assert!(page.wants_open() && !page.wants_open());
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        assert!(ui.is_visible(ids(&page).password));
        assert!(ids(&page).airplane.is_none());
        page.closed(&mut ui);
        assert!(matches!(answer.try_recv(), Ok(None)), "hiding the page cancels the request");
    }

    #[test]
    fn a_page_dropped_with_its_popover_cancels_the_request_and_forgets_what_was_typed() {
        let (mut page, mut ui, mut answer) = secrets_page();
        type_password(&mut page, &mut ui, "correct horse");
        assert!(!page.changed());
        page.dropped();
        assert!(matches!(answer.try_recv(), Ok(None)));
        assert!(page.typed.is_empty() && page.ids.is_none());
    }

    #[test]
    fn the_switches_and_the_vpn_check_boxes_send_their_commands() {
        let (mut page, mut received, mut ui) = page(state(
            vec![],
            vec![vpn("Office VPN", None), vpn("Home VPN", Some("/active/home"))],
        ));
        let (wifi, airplane, off, on) = {
            let ids = ids(&page);
            let airplane = ids.airplane;
            (ids.wifi, airplane, ids.vpn_rows[0].0, ids.vpn_rows[1].0)
        };
        click(&mut page, &mut ui, wifi);
        assert!(matches!(received.try_recv(), Ok(NetworkCommand::Wireless(false))));
        if let Some(airplane) = airplane {
            click(&mut page, &mut ui, airplane);
            assert!(matches!(received.try_recv(), Ok(NetworkCommand::Airplane(true))));
        }
        click(&mut page, &mut ui, off);
        assert!(matches!(
            received.try_recv(),
            Ok(NetworkCommand::Activate { connection, device, specific })
                if connection == "/vpn/Office VPN" && device == "/" && specific == "/"
        ));
        click(&mut page, &mut ui, on);
        assert!(matches!(
            received.try_recv(),
            Ok(NetworkCommand::Deactivate(active)) if active == "/active/home"
        ));
    }

    #[test]
    fn a_strength_update_keeps_the_rows_and_a_new_network_rebuilds_them() {
        let (mut page, _received, mut ui) =
            page(state(vec![network("Cafe", Security::Open)], vec![]));
        assert_eq!(page.turn(&mut ui), Turn::Unchanged);
        let mut stronger = network("Cafe", Security::Open);
        stronger.strength = 60;
        page.service.state.replace(Some(state(vec![stronger], vec![])));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        let mut weaker = network("Cafe", Security::Open);
        weaker.strength = 20;
        page.service.state.replace(Some(state(vec![weaker], vec![])));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Rebuild, "the row's icon changed");
        page.service.state.replace(Some(state(
            vec![network("Cafe", Security::Open), network("New", Security::Open)],
            vec![],
        )));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
        page.service.failed();
        page.service.changed();
        let mut fresh = Ui::new("t", Theme::default(), false);
        page.build(&mut fresh, Ui::ROOT, &mut Look::plain());
        assert_eq!(page.turn(&mut fresh), Turn::Unchanged);
    }

    #[test]
    fn a_vpn_that_connects_keeps_its_row_and_a_new_vpn_rebuilds_them() {
        let (mut page, mut received, mut ui) = page(state(vec![], vec![vpn("Office VPN", None)]));
        let row = ids(&page).vpn_rows[0].0;
        click(&mut page, &mut ui, row);
        assert!(matches!(received.try_recv(), Ok(NetworkCommand::Activate { .. })));
        page.service
            .state
            .replace(Some(state(vec![], vec![vpn("Office VPN", Some("/active/office"))])));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Updated, "only the VPN's link changed");
        assert!(ui.is_active(row));
        // The switch turns off the active connection the update brought.
        click(&mut page, &mut ui, row);
        assert!(matches!(
            received.try_recv(),
            Ok(NetworkCommand::Deactivate(active)) if active == "/active/office"
        ));
        page.service.state.replace(Some(state(vec![], vec![vpn("Office VPN", None)])));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(!ui.is_active(row));
        page.service.state.replace(Some(state(
            vec![],
            vec![vpn("Home VPN", None), vpn("Office VPN", None)],
        )));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
    }
}
