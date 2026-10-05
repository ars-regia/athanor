//! The network page (doc_bar.md, BR3): the wired state, the Wi-Fi list, joining with a
//! password, VPN and airplane mode. The model in athanor-services mirrors NetworkManager and
//! is its secret agent; this side draws its state, sends it the person's commands and shows
//! the password pages it asks for. One service per process; each surface has a view.
//!
//! A password goes from the entry into the model's reply and nowhere else: it is never
//! logged, never stored by the bar, and the entry is cleared as the reply leaves.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_services::network::{
    self, Link, Network, NetworkCommand, NetworkState, PasswordPage, PasswordPrompt, Security, Vpn,
    Wifi,
};
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use gtk4::{glib, pango};
use tokio::sync::{mpsc, oneshot};

use crate::bridge;
use crate::i18n::{tr, tr_with};
use crate::widgets::{switch_row, Failure};
use crate::{Host, Services};

const MAX_LIST_HEIGHT: i32 = 320;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

/// What the password page answers.
enum Ask {
    /// A network with no saved profile: the password goes into `AddAndActivateConnection`.
    Join { network: Network },
    /// NetworkManager's `GetSecrets`, answered through the pending reply.
    Secrets,
}

/// A request of NetworkManager the person has not answered.
struct Pending {
    reply: oneshot::Sender<Option<String>>,
    view: Weak<View>,
}

struct Service {
    commands: mpsc::UnboundedSender<NetworkCommand>,
    /// The model's last state, which `bridge::follow` keeps current.
    state: RefCell<Option<NetworkState>>,
    /// How many commands the model had counted as refused when the views last showed it.
    refused: Cell<u32>,
    views: RefCell<Vec<Weak<View>>>,
    last_opened: RefCell<Weak<View>>,
    pending: RefCell<Option<Pending>>,
    /// Which request `pending` holds, so a withdrawal of an older one closes nothing.
    serial: Cell<u64>,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let (states, commands) = services.network.clone();
        let prompts = services.password_prompts.borrow_mut().take();
        let service = Rc::new(Service {
            commands,
            state: RefCell::new(None),
            refused: Cell::new(0),
            views: RefCell::new(Vec::new()),
            last_opened: RefCell::new(Weak::new()),
            pending: RefCell::new(None),
            serial: Cell::new(0),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak, as the other pages hold the service: the thread-local owns it.
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state| {
            if let Some(service) = weak.upgrade() {
                service.changed(state.clone());
            }
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

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    /// The model published a state: the views show it, and an action it counts as refused
    /// is said in the open page.
    fn changed(&self, state: Option<NetworkState>) {
        let failed = state
            .as_ref()
            .is_some_and(|state| self.refused.replace(state.refused) != state.refused);
        self.state.replace(state);
        let state = self.state.borrow();
        for view in self.views() {
            view.show(state.as_ref());
            if failed {
                view.failure.show();
            }
        }
    }

    fn send(&self, command: NetworkCommand) {
        // The model has ended when the send fails: the page shows its last state.
        self.commands.send(command).ok();
    }

    /// NetworkManager asks for a password: shows the page, unless the surface cannot.
    fn prompt(self: &Rc<Self>, prompt: PasswordPrompt) {
        let PasswordPrompt {
            request,
            reply,
            withdrawn,
        } = prompt;
        // Dropping `reply` answers NoSecrets.
        let Some(page) = request.prompt() else { return };
        if self.pending.borrow().is_some() {
            return;
        }
        let Some(view) = self.prompt_view() else {
            return;
        };
        // The page may hold the password of a network the user is joining: replacing it would
        // drop that join and send what they are typing as another network's password.
        if view.asking.borrow().is_some() {
            return;
        }
        let serial = self.serial.get() + 1;
        self.serial.set(serial);
        self.pending.replace(Some(Pending {
            reply,
            view: Rc::downgrade(&view),
        }));
        view.ask(Ask::Secrets, &page);
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

    /// The view a prompt shows on: the one shown last, else the first usable.
    fn prompt_view(&self) -> Option<Rc<View>> {
        let usable = |view: &Rc<View>| view.host.usable();
        self.last_opened
            .borrow()
            .upgrade()
            .filter(usable)
            .or_else(|| self.views().into_iter().find(usable))
    }

    /// Answers the pending request: the password, or a cancel.
    fn answer(&self, password: Option<&str>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        // The model has stopped waiting when the send fails.
        pending.reply.send(password.map(str::to_owned)).ok();
    }

    /// The model no longer waits for request `serial`: closes its page.
    fn withdraw(&self, serial: u64) {
        if self.serial.get() != serial {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        if let Some(view) = pending.view.upgrade() {
            view.close_prompt();
        }
    }

    fn pressed(&self, view: &Rc<View>, network: &Network, device: &str) {
        if let Some(active) = &network.active {
            self.send(NetworkCommand::Deactivate(active.clone()));
        } else if let Some(saved) = &network.saved {
            self.send(NetworkCommand::Activate {
                connection: saved.clone(),
                device: device.to_owned(),
                specific: network.access_point.clone(),
            });
        } else {
            match network.security {
                Security::Open => self.send(NetworkCommand::Join {
                    network: network.clone(),
                    password: None,
                }),
                Security::Personal { .. } => view.ask(
                    Ask::Join {
                        network: network.clone(),
                    },
                    &PasswordPage {
                        label: network.label.clone(),
                        security: network.security,
                        retry: false,
                    },
                ),
                Security::Other => {}
            }
        }
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

type OnShown = Box<dyn Fn(Option<&NetworkState>)>;

struct View {
    host: Host,
    failure: Failure,
    /// Runs after the view shows a state, for the surface's own button.
    shown: RefCell<Option<OnShown>>,
    stack: gtk4::Stack,
    wired: gtk4::Label,
    wifi_row: gtk4::Box,
    wifi: gtk4::Switch,
    scroller: gtk4::ScrolledWindow,
    networks: gtk4::Box,
    vpns: gtk4::Box,
    airplane: gtk4::Switch,
    title: gtk4::Label,
    retry: gtk4::Label,
    entry: gtk4::PasswordEntry,
    connect: gtk4::Button,
    asking: RefCell<Option<Ask>>,
    security: Cell<Security>,
    /// A switch is being set from NetworkManager, not by the person.
    updating: Cell<bool>,
    /// What the network and VPN rows show: they are rebuilt only when it changes, not on
    /// every strength update of an access point, which would take the keyboard focus off
    /// a row.
    rows: RefCell<(Option<NetworkRows>, Vec<Vpn>)>,
}

/// What the network rows show: the device, and each network with its strength replaced by
/// the signal icon it picks. A row keeps the network it was built from for its click, whose
/// strength it does not read.
#[derive(PartialEq)]
struct NetworkRows {
    device: String,
    networks: Vec<(&'static str, Network)>,
}

impl NetworkRows {
    fn of(wifi: &Wifi) -> NetworkRows {
        NetworkRows {
            device: wifi.device.clone(),
            networks: wifi
                .networks
                .iter()
                .map(|network| {
                    let icon = network::signal_icon(network.strength);
                    (
                        icon,
                        Network {
                            strength: 0,
                            ..network.clone()
                        },
                    )
                })
                .collect(),
        }
    }
}

fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

impl View {
    fn new(host: Host) -> Rc<View> {
        let heading = gtk4::Label::new(Some(&tr("Network")));
        heading.add_css_class("bar-popover-title");
        heading.set_xalign(0.0);
        let wired = gtk4::Label::new(None);
        wired.add_css_class("bar-popover-note");
        wired.set_xalign(0.0);
        let (wifi_row, wifi) = switch_row(&tr("Wi-Fi"));
        let networks = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let scroller = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .max_content_height(MAX_LIST_HEIGHT)
            .propagate_natural_height(true)
            .child(&networks)
            .build();
        let vpns = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let (airplane_row, airplane) = switch_row(&tr("Airplane mode"));
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        for widget in [
            heading.upcast_ref::<gtk4::Widget>(),
            wired.upcast_ref(),
            wifi_row.upcast_ref(),
            scroller.upcast_ref(),
            vpns.upcast_ref(),
            airplane_row.upcast_ref(),
        ] {
            list.append(widget);
        }

        let title = gtk4::Label::new(None);
        title.add_css_class("bar-popover-title");
        title.set_wrap(true);
        title.set_xalign(0.0);
        let retry = gtk4::Label::new(Some(&tr("The password was not accepted. Try again.")));
        retry.add_css_class("bar-popover-note");
        retry.set_wrap(true);
        retry.set_xalign(0.0);
        let entry = gtk4::PasswordEntry::new();
        entry.set_show_peek_icon(true);
        entry.update_relation(&[Relation::LabelledBy(&[title.upcast_ref()])]);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        cancel.add_css_class("bar-row");
        let connect = gtk4::Button::with_label(&tr("Connect"));
        connect.add_css_class("bar-confirm");
        connect.set_sensitive(false);
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.append(&cancel);
        buttons.append(&connect);
        let password = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        password.append(&title);
        password.append(&retry);
        password.append(&entry);
        password.append(&buttons);

        let stack = gtk4::Stack::new();
        stack.add_named(&list, Some("list"));
        stack.add_named(&password, Some("password"));
        let failure = Failure::new(&stack);

        let view = Rc::new(View {
            host,
            failure,
            shown: RefCell::new(None),
            stack,
            wired,
            wifi_row,
            wifi,
            scroller,
            networks,
            vpns,
            airplane,
            title,
            retry,
            entry,
            connect,
            asking: RefCell::new(None),
            security: Cell::new(Security::Open),
            updating: Cell::new(false),
            rows: RefCell::new((None, Vec::new())),
        });
        view.connect_handlers(&cancel);
        view
    }

    /// Every handler holds the view weakly: the view owns the widgets, and a strong
    /// reference from a widget would be a cycle.
    fn connect_handlers(self: &Rc<Self>, cancel: &gtk4::Button) {
        let weak = Rc::downgrade(self);
        self.wifi.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.send(NetworkCommand::Wireless(on));
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.airplane.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.send(NetworkCommand::Airplane(on));
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_changed(move |entry| {
            if let Some(view) = weak.upgrade() {
                view.connect.set_sensitive(network::password_acceptable(
                    view.security.get(),
                    &entry.text(),
                ));
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(view) = weak.upgrade() {
                view.submit();
            }
        });
        let weak = Rc::downgrade(self);
        self.connect.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.submit();
            }
        });
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.host.close();
            }
        });
        // The page is hidden: a request of NetworkManager that it showed is canceled.
        let weak = Rc::downgrade(self);
        self.failure.widget().connect_unmap(move |_| {
            if let Some(view) = weak.upgrade() {
                view.abandon();
            }
        });
        let weak = Rc::downgrade(self);
        self.failure.widget().connect_map(move |_| {
            if let Some(service) = service() {
                service.last_opened.replace(weak.clone());
            }
        });
    }

    fn show(self: &Rc<Self>, state: Option<&NetworkState>) {
        if let Some(state) = state {
            self.draw(state);
        }
        if let Some(shown) = &*self.shown.borrow() {
            shown(state);
        }
    }

    fn draw(self: &Rc<Self>, state: &NetworkState) {
        match state.wired {
            Some(true) => self.wired.set_text(&tr("Wired: connected")),
            Some(false) => self.wired.set_text(&tr("Wired: not connected")),
            None => {}
        }
        self.wired.set_visible(state.wired.is_some());
        self.updating.set(true);
        self.wifi_row.set_visible(state.wifi.is_some());
        self.wifi
            .set_active(state.wifi.as_ref().is_some_and(|wifi| wifi.enabled));
        self.airplane.set_active(state.airplane);
        self.updating.set(false);
        let rows = state.wifi.as_ref().map(NetworkRows::of);
        if self.rows.borrow().0 != rows {
            clear(&self.networks);
            if let Some(wifi) = &state.wifi {
                for network in &wifi.networks {
                    self.networks
                        .append(&self.network_row(network, &wifi.device));
                }
            }
            self.rows.borrow_mut().0 = rows;
        }
        self.scroller
            .set_visible(self.networks.first_child().is_some());
        if self.rows.borrow().1 != state.vpns {
            clear(&self.vpns);
            for vpn in &state.vpns {
                self.vpns.append(&vpn_row(vpn));
            }
            self.rows.borrow_mut().1 = state.vpns.clone();
        }
        self.vpns.set_visible(!state.vpns.is_empty());
    }

    fn network_row(self: &Rc<Self>, network: &Network, device: &str) -> gtk4::Button {
        let usable = network.security != Security::Other
            || network.saved.is_some()
            || network.active.is_some();
        let name = match (network.link, network.security) {
            (Link::Connected, _) => tr_with("{network}, connected", "network", &network.label),
            (Link::Connecting, _) => tr_with("{network}, connecting", "network", &network.label),
            _ if !usable => tr_with("{network}, needs Settings", "network", &network.label),
            (_, Security::Personal { .. }) => {
                tr_with("{network}, secured", "network", &network.label)
            }
            _ => network.label.clone(),
        };
        let icon = gtk4::Image::from_icon_name(network::signal_icon(network.strength));
        let text = gtk4::Label::new(Some(&name));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(pango::EllipsizeMode::End);
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        content.append(&icon);
        content.append(&text);
        let button = gtk4::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("bar-row");
        button.update_property(&[Property::Label(&name)]);
        button.set_sensitive(usable);
        let (weak, network, device) = (Rc::downgrade(self), network.clone(), device.to_owned());
        button.connect_clicked(move |_| {
            if let (Some(view), Some(service)) = (weak.upgrade(), service()) {
                service.pressed(&view, &network, &device);
            }
        });
        button
    }

    /// Shows the password page, opening the surface when it is closed.
    fn ask(self: &Rc<Self>, ask: Ask, prompt: &PasswordPage) {
        self.asking.replace(Some(ask));
        self.security.set(prompt.security);
        self.title
            .set_text(&tr_with("Password for {network}", "network", &prompt.label));
        self.retry.set_visible(prompt.retry);
        self.entry.set_text("");
        self.connect.set_sensitive(false);
        self.stack.set_visible_child_name("password");
        self.host.open();
        self.entry.grab_focus();
    }

    fn submit(self: &Rc<Self>) {
        let password = self.entry.text();
        if !network::password_acceptable(self.security.get(), &password) {
            return;
        }
        let Some(ask) = self.asking.take() else {
            return;
        };
        self.entry.set_text("");
        self.stack.set_visible_child_name("list");
        if let Some(service) = service() {
            match ask {
                Ask::Join { network } => service.send(NetworkCommand::Join {
                    network,
                    password: Some(password.to_string()),
                }),
                Ask::Secrets => service.answer(Some(password.as_str())),
            }
        }
        self.host.close();
    }

    /// The page was hidden while it asked: a NetworkManager request is canceled.
    fn abandon(&self) {
        let asked = self.asking.take();
        self.entry.set_text("");
        self.stack.set_visible_child_name("list");
        if matches!(asked, Some(Ask::Secrets)) {
            if let Some(service) = service() {
                service.answer(None);
            }
        }
    }

    /// NetworkManager withdrew the request, or left.
    fn close_prompt(&self) {
        if self.asking.take().is_some() {
            self.entry.set_text("");
            self.stack.set_visible_child_name("list");
            self.host.close();
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // A rebuild removed the surface while NetworkManager waited for this view's answer.
        if matches!(self.asking.get_mut(), Some(Ask::Secrets)) {
            if let Some(service) = service() {
                service.answer(None);
            }
        }
    }
}

fn vpn_row(vpn: &Vpn) -> gtk4::Box {
    let (row, switch) = switch_row(&vpn.label);
    switch.set_active(vpn.link != Link::Idle);
    let vpn = vpn.clone();
    switch.connect_state_set(move |_, on| {
        if let Some(service) = service() {
            service.switch_vpn(&vpn, on);
        }
        glib::Propagation::Proceed
    });
    row
}

/// The network page: one per surface, over the one service of the process.
pub struct Page {
    view: Rc<View>,
}

impl Page {
    pub fn new(services: &Services, host: Host) -> Page {
        let service = Service::get(services);
        let view = View::new(host);
        service.views.borrow_mut().push(Rc::downgrade(&view));
        view.show(service.state.borrow().as_ref());
        Page { view }
    }

    pub fn widget(&self) -> gtk4::Widget {
        self.view.failure.widget()
    }

    /// `shown` runs, now and after every state the page shows, with `None` while NetworkManager
    /// is away.
    pub fn connect_shown(&self, shown: impl Fn(Option<&NetworkState>) + 'static) {
        if let Some(service) = service() {
            shown(service.state.borrow().as_ref());
        }
        self.view.shown.replace(Some(Box::new(shown)));
    }
}
