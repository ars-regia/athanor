//! The network module (doc_bar.md, BR3): the wired state, the Wi-Fi list, joining with a
//! password, VPN and airplane mode, over NetworkManager on the system bus. One service per
//! process mirrors NetworkManager and is its secret agent; each surface has a view.
//!
//! The secret agent (`/org/freedesktop/NetworkManager/SecretAgent`, `os.athanor.Bar`)
//! answers only the current owner of `org.freedesktop.NetworkManager`, one request at a
//! time, and only for a Wi-Fi personal password the person types. The password goes from
//! the entry into the reply and nowhere else: it is never logged, never stored by the bar,
//! and the entry is cleared as the reply leaves.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use athanor_bar::network::{
    self, ConnectionInfo, Link, Network, NetworkState, PasswordPrompt, SecretsRequest, Security,
    Vpn, Wifi,
};
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use gtk4::{gio, glib, pango};

use super::bus::{self, Mirror, Source};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const ERROR: &str = "org.freedesktop.NetworkManager.SecretAgent.";
const MAX_LIST_HEIGHT: i32 = 320;
const AGENT_XML: &str = r#"<node>
  <interface name="org.freedesktop.NetworkManager.SecretAgent">
    <method name="GetSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
      <arg name="setting_name" type="s" direction="in"/>
      <arg name="hints" type="as" direction="in"/>
      <arg name="flags" type="u" direction="in"/>
      <arg name="secrets" type="a{sa{sv}}" direction="out"/>
    </method>
    <method name="CancelGetSecrets">
      <arg name="connection_path" type="o" direction="in"/>
      <arg name="setting_name" type="s" direction="in"/>
    </method>
    <method name="SaveSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
    </method>
    <method name="DeleteSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
    </method>
  </interface>
</node>"#;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

/// For [`bus::act`]: an action the person took did not complete.
fn action_failed() {
    if let Some(service) = service() {
        service.failed();
    }
}

fn error(name: &str) -> String {
    format!("{ERROR}{name}")
}

/// What the password page answers.
enum Ask {
    /// A network with no saved profile: the password goes into `AddAndActivateConnection`.
    Join { network: Network, device: String },
    /// NetworkManager's `GetSecrets`, answered through the pending invocation.
    Secrets,
}

struct Pending {
    request: SecretsRequest,
    invocation: gio::DBusMethodInvocation,
    view: Weak<View>,
}

struct Service {
    bar: Weak<Bar>,
    mirror: RefCell<Option<Rc<Mirror>>>,
    /// The mirror's generation the caches and the registration belong to.
    seen: Cell<u64>,
    connections: RefCell<BTreeMap<String, ConnectionInfo>>,
    fetching: RefCell<BTreeSet<String>>,
    state: RefCell<Option<NetworkState>>,
    views: RefCell<Vec<Weak<View>>>,
    last_opened: RefCell<Weak<View>>,
    pending: RefCell<Option<Pending>>,
    agent: RefCell<Option<gio::RegistrationId>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            mirror: RefCell::new(None),
            seen: Cell::new(0),
            connections: RefCell::new(BTreeMap::new()),
            fetching: RefCell::new(BTreeSet::new()),
            state: RefCell::new(None),
            views: RefCell::new(Vec::new()),
            last_opened: RefCell::new(Weak::new()),
            pending: RefCell::new(None),
            agent: RefCell::new(None),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        service.start();
        service
    }

    fn start(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let connection = match gio::bus_get_future(gio::BusType::System).await {
                Ok(connection) => connection,
                Err(err) => {
                    tracing::warn!(error = %err, "no system bus; the network module is hidden");
                    return;
                }
            };
            let Some(service) = weak.upgrade() else {
                return;
            };
            service.register_agent(&connection);
            let notify = weak.clone();
            let mirror = Mirror::new(
                &connection,
                network::NM,
                Source::Managed(network::NM_ROOT),
                move || {
                    if let Some(service) = notify.upgrade() {
                        service.changed();
                    }
                },
            );
            service.mirror.replace(Some(mirror));
        });
    }

    fn register_agent(self: &Rc<Self>, connection: &gio::DBusConnection) {
        let interface = gio::DBusNodeInfo::for_xml(AGENT_XML)
            .ok()
            .and_then(|node| node.lookup_interface(network::AGENT_IFACE));
        let Some(interface) = interface else {
            tracing::error!("the secret agent's interface does not parse; joining a saved network that needs a password will fail");
            return;
        };
        let weak = Rc::downgrade(self);
        let registered = connection
            .register_object(network::AGENT_PATH, &interface)
            .method_call(
                move |_, sender, _, _, method, params, invocation| match weak.upgrade() {
                    Some(service) => service.agent_call(sender, method, &params, invocation),
                    None => invocation.return_dbus_error(&error("NoSecrets"), "the agent is gone"),
                },
            )
            .build();
        match registered {
            Ok(id) => {
                self.agent.replace(Some(id));
            }
            Err(err) => {
                tracing::error!(error = %err, "cannot export the NetworkManager secret agent")
            }
        }
    }

    fn mirror(&self) -> Option<Rc<Mirror>> {
        self.mirror.borrow().clone()
    }

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    /// The mirror changed: a new owner registers the agent again, then the state is
    /// recomputed and pushed into the views when it differs.
    fn changed(self: &Rc<Self>) {
        let Some(mirror) = self.mirror() else { return };
        let generation = mirror.generation();
        if self.seen.replace(generation) != generation {
            self.connections.borrow_mut().clear();
            self.fetching.borrow_mut().clear();
            self.cancel_pending("AgentCanceled");
            if let Some(owner) = mirror.owner() {
                let args = (network::AGENT_ID,).to_variant();
                bus::spawn(
                    "register the NetworkManager secret agent",
                    bus::call(
                        mirror.connection(),
                        &owner,
                        network::AGENT_MANAGER_PATH,
                        network::AGENT_MANAGER,
                        "Register",
                        Some(&args),
                        bus::TIMEOUT_MS,
                    ),
                );
            }
        }
        self.fetch_connections(&mirror);
        let state = network::state(&mirror.objects(), &self.connections.borrow());
        if *self.state.borrow() == state {
            return;
        }
        if state.is_none() {
            self.cancel_pending("AgentCanceled");
        }
        self.state.replace(state);
        for view in self.views() {
            view.show(self.state.borrow().as_ref());
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    /// The settings of each saved profile, read once: they tell a saved network from a new
    /// one, and name the VPNs.
    // ponytail: read once per profile; follow Settings.Connection's Updated signal if renamed
    // profiles must show their new name without a restart of NetworkManager.
    fn fetch_connections(self: &Rc<Self>, mirror: &Rc<Mirror>) {
        let Some(owner) = mirror.owner() else { return };
        let paths: Vec<String> = mirror
            .objects()
            .iter()
            .filter(|(_, interfaces)| interfaces.contains_key(network::SETTINGS_CONNECTION))
            .map(|(path, _)| path.clone())
            .collect();
        self.connections
            .borrow_mut()
            .retain(|path, _| paths.contains(path));
        let generation = mirror.generation();
        for path in paths {
            if self.connections.borrow().contains_key(&path)
                || !self.fetching.borrow_mut().insert(path.clone())
            {
                continue;
            }
            let reply = bus::call(
                mirror.connection(),
                &owner,
                &path,
                network::SETTINGS_CONNECTION,
                "GetSettings",
                None,
                bus::TIMEOUT_MS,
            );
            let weak = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let reply = reply.await;
                let Some(service) = weak.upgrade() else {
                    return;
                };
                if service.seen.get() != generation {
                    return;
                }
                service.fetching.borrow_mut().remove(&path);
                match reply.as_ref().ok().and_then(network::connection_info) {
                    Some(info) => {
                        service.connections.borrow_mut().insert(path, info);
                        service.changed();
                    }
                    None => tracing::warn!(
                        path,
                        "a NetworkManager profile has unreadable settings; it is not listed"
                    ),
                }
            });
        }
    }

    fn agent_call(
        self: &Rc<Self>,
        sender: Option<&str>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        let from_owner = self
            .mirror()
            .is_some_and(|mirror| sender.is_some_and(|sender| mirror.is_owner(sender)));
        if !from_owner {
            tracing::warn!(
                method,
                sender = sender.unwrap_or(""),
                "a secret agent call from a process that is not NetworkManager was refused"
            );
            invocation.return_dbus_error(
                &error("PermissionDenied"),
                "only NetworkManager may call this agent",
            );
            return;
        }
        match method {
            "GetSecrets" => self.get_secrets(params, invocation),
            "CancelGetSecrets" => {
                if let Some((connection, setting)) = network::cancel_request(params) {
                    let matches = self.pending.borrow().as_ref().is_some_and(|pending| {
                        pending.request.connection == connection
                            && pending.request.setting == setting
                    });
                    if matches {
                        self.cancel_pending("AgentCanceled");
                    }
                }
                invocation.return_value(None);
            }
            // Nothing is ever stored here, so there is nothing to delete; and nothing is saved.
            "DeleteSecrets" => invocation.return_value(None),
            "SaveSecrets" => {
                invocation.return_dbus_error(&error("Failed"), "this agent keeps no secrets")
            }
            _ => invocation
                .return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    fn get_secrets(self: &Rc<Self>, params: &glib::Variant, invocation: gio::DBusMethodInvocation) {
        let Some(request) = network::secrets_request(params) else {
            invocation.return_dbus_error(&error("InvalidConnection"), "the request cannot be read");
            return;
        };
        let Some(prompt) = request.prompt() else {
            invocation.return_dbus_error(
                &error("NoSecrets"),
                "this agent answers only Wi-Fi personal passwords",
            );
            return;
        };
        if self.pending.borrow().is_some() {
            invocation.return_dbus_error(&error("NoSecrets"), "another request is open");
            return;
        }
        let Some(view) = self.prompt_view() else {
            invocation.return_dbus_error(&error("NoSecrets"), "the bar shows no network module");
            return;
        };
        self.pending.replace(Some(Pending {
            request,
            invocation,
            view: Rc::downgrade(&view),
        }));
        view.ask(Ask::Secrets, &prompt);
    }

    /// The view a prompt shows on: the one whose popover opened last, else the first shown.
    fn prompt_view(&self) -> Option<Rc<View>> {
        let usable = |view: &Rc<View>| view.popup.button.is_mapped();
        self.last_opened
            .borrow()
            .upgrade()
            .filter(usable)
            .or_else(|| self.views().into_iter().find(usable))
    }

    /// Answers the pending request: the password, or `UserCanceled`.
    fn answer(&self, password: Option<&str>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match password {
            Some(password) => pending
                .invocation
                .return_value(Some(&network::secrets_reply(password))),
            None => pending
                .invocation
                .return_dbus_error(&error("UserCanceled"), "the person canceled"),
        }
    }

    fn cancel_pending(&self, name: &str) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        pending
            .invocation
            .return_dbus_error(&error(name), "the request was withdrawn");
        if let Some(view) = pending.view.upgrade() {
            view.close_prompt();
        }
    }

    /// An action the person took failed: every view shows the mirrored state again, which
    /// puts a switch back, and the open popover says so.
    fn failed(&self) {
        let state = self.state.borrow();
        for view in self.views() {
            view.show(state.as_ref());
            view.popup.failed();
        }
    }

    fn manager_call(&self, method: &'static str, args: Option<glib::Variant>) {
        let (Some(mirror), Some(args)) = (self.mirror(), args) else {
            return;
        };
        bus::act(
            method,
            bus::call(
                mirror.connection(),
                network::NM,
                network::NM_PATH,
                network::NM,
                method,
                Some(&args),
                bus::INTERACTIVE_TIMEOUT_MS,
            ),
            action_failed,
        );
    }

    fn set_manager(&self, values: &[(&'static str, bool)]) {
        let Some(mirror) = self.mirror() else { return };
        for (property, value) in values {
            bus::act(
                property,
                bus::set_property(
                    mirror.connection(),
                    network::NM,
                    network::NM_PATH,
                    network::NM,
                    property,
                    value.to_variant(),
                ),
                action_failed,
            );
        }
    }

    fn pressed(&self, view: &Rc<View>, network: &Network, device: &str) {
        if let Some(active) = &network.active {
            self.manager_call("DeactivateConnection", network::deactivate(active));
        } else if let Some(saved) = &network.saved {
            self.manager_call(
                "ActivateConnection",
                network::activate(saved, device, &network.access_point),
            );
        } else {
            match network.security {
                Security::Open => self.manager_call(
                    "AddAndActivateConnection",
                    network::add_and_activate(network, None, device),
                ),
                Security::Personal { .. } => view.ask(
                    Ask::Join {
                        network: network.clone(),
                        device: device.to_owned(),
                    },
                    &PasswordPrompt {
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
            (None, true) => self.manager_call(
                "ActivateConnection",
                network::activate(&vpn.connection, "/", "/"),
            ),
            (Some(active), false) => {
                self.manager_call("DeactivateConnection", network::deactivate(active))
            }
            _ => {}
        }
    }
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
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
    /// `open` came before the module could show (the captures of BR9).
    pending_open: Cell<bool>,
    /// What the network and VPN rows show: they are rebuilt only when it changes, not on
    /// every strength update of an access point, which would take the keyboard focus off
    /// a row.
    shown: RefCell<(Option<NetworkRows>, Vec<Vpn>)>,
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
                    (icon, Network { strength: 0, ..network.clone() })
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
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("network-wireless-offline-symbolic");
        let popup = Popup::new(bar, &icon, &tr("Network"));
        popup.button.set_visible(false);

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
        popup.set_content(&stack);

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
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
            pending_open: Cell::new(false),
            shown: RefCell::new((None, Vec::new())),
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
                    service.set_manager(&[("WirelessEnabled", on)]);
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.airplane.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.set_manager(&[("WirelessEnabled", !on), ("WwanEnabled", !on)]);
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
        let popover = self.popup.popover.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_closed(move |_| {
            if let Some(view) = weak.upgrade() {
                view.abandon();
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_show(move |_| {
            if let Some(service) = service() {
                service.last_opened.replace(weak.clone());
            }
        });
    }

    fn show(self: &Rc<Self>, state: Option<&NetworkState>) {
        let Some(state) = state else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.icon.set_icon_name(Some(network::icon(state)));
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
        if self.shown.borrow().0 != rows {
            clear(&self.networks);
            if let Some(wifi) = &state.wifi {
                for network in &wifi.networks {
                    self.networks
                        .append(&self.network_row(network, &wifi.device));
                }
            }
            self.shown.borrow_mut().0 = rows;
        }
        self.scroller
            .set_visible(self.networks.first_child().is_some());
        if self.shown.borrow().1 != state.vpns {
            clear(&self.vpns);
            for vpn in &state.vpns {
                self.vpns.append(&vpn_row(vpn));
            }
            self.shown.borrow_mut().1 = state.vpns.clone();
        }
        self.vpns.set_visible(!state.vpns.is_empty());
        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
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

    /// Shows the password page, opening the popover when it is closed.
    fn ask(self: &Rc<Self>, ask: Ask, prompt: &PasswordPrompt) {
        self.asking.replace(Some(ask));
        self.security.set(prompt.security);
        self.title
            .set_text(&tr_with("Password for {network}", "network", &prompt.label));
        self.retry.set_visible(prompt.retry);
        self.entry.set_text("");
        self.connect.set_sensitive(false);
        self.stack.set_visible_child_name("password");
        if !self.popup.popover.is_visible() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
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
                Ask::Join { network, device } => service.manager_call(
                    "AddAndActivateConnection",
                    network::add_and_activate(&network, Some(password.as_str()), &device),
                ),
                Ask::Secrets => service.answer(Some(password.as_str())),
            }
        }
        self.popup.popover.popdown();
    }

    /// The popover closed with the page open: a NetworkManager request is canceled.
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
            self.popup.popover.popdown();
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

struct NetworkUi {
    view: Rc<View>,
}

impl ModuleUi for NetworkUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() {
            self.view.popup.open(bar);
        } else {
            self.view.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = Service::get(bar);
    let view = View::new(bar);
    service.views.borrow_mut().push(Rc::downgrade(&view));
    view.show(service.state.borrow().as_ref());
    Some(Box::new(NetworkUi { view }))
}
