//! The Bluetooth module (doc_bar.md, BR3): the adapter's power, the paired devices, the
//! devices nearby while a popover is open, and pairing with a confirmation. One service per
//! process mirrors BlueZ and is its default agent; each surface has a view.
//!
//! The agent answers only the current owner of `org.bluez`, and only about the device whose
//! pairing the person started from the bar: every other request is rejected before anything
//! shows. It never types a code: the capability is DisplayYesNo, so the person compares six
//! digits on both screens.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_bar::bluetooth::{self, BluetoothState, Device};
use athanor_bar::props;
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gio, glib, pango};

use super::bus::{self, Mirror, Source};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const MAX_CODE_CHARS: usize = 16;
const AGENT_XML: &str = r#"<node>
  <interface name="org.bluez.Agent1">
    <method name="Release"/>
    <method name="RequestPinCode">
      <arg type="o" direction="in"/><arg type="s" direction="out"/>
    </method>
    <method name="DisplayPinCode">
      <arg type="o" direction="in"/><arg type="s" direction="in"/>
    </method>
    <method name="RequestPasskey">
      <arg type="o" direction="in"/><arg type="u" direction="out"/>
    </method>
    <method name="DisplayPasskey">
      <arg type="o" direction="in"/><arg type="u" direction="in"/><arg type="q" direction="in"/>
    </method>
    <method name="RequestConfirmation">
      <arg type="o" direction="in"/><arg type="u" direction="in"/>
    </method>
    <method name="RequestAuthorization">
      <arg type="o" direction="in"/>
    </method>
    <method name="AuthorizeService">
      <arg type="o" direction="in"/><arg type="s" direction="in"/>
    </method>
    <method name="Cancel"/>
  </interface>
</node>"#;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

struct Pending {
    invocation: gio::DBusMethodInvocation,
    view: Weak<View>,
}

struct Service {
    bar: Weak<Bar>,
    mirror: RefCell<Option<Rc<Mirror>>>,
    seen: Cell<u64>,
    state: RefCell<Option<BluetoothState>>,
    views: RefCell<Vec<Weak<View>>>,
    last_opened: RefCell<Weak<View>>,
    pending: RefCell<Option<Pending>>,
    /// The device the person pressed to pair, until `Pair` returns.
    pairing: RefCell<Option<String>>,
    /// Open popovers: discovery runs while at least one is open.
    open_popovers: Cell<u32>,
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
            state: RefCell::new(None),
            views: RefCell::new(Vec::new()),
            last_opened: RefCell::new(Weak::new()),
            pending: RefCell::new(None),
            pairing: RefCell::new(None),
            open_popovers: Cell::new(0),
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
                    tracing::warn!(error = %err, "no system bus; the Bluetooth module is hidden");
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
                bluetooth::BLUEZ,
                Source::Managed(bluetooth::ROOT),
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
            .and_then(|node| node.lookup_interface(bluetooth::AGENT_IFACE));
        let Some(interface) = interface else {
            tracing::error!("the Bluetooth agent's interface does not parse; pairing will fail");
            return;
        };
        let weak = Rc::downgrade(self);
        let registered = connection
            .register_object(bluetooth::AGENT_PATH, &interface)
            .method_call(
                move |_, sender, _, _, method, params, invocation| match weak.upgrade() {
                    Some(service) => service.agent_call(sender, method, &params, invocation),
                    None => invocation.return_dbus_error(bluetooth::REJECTED, "the agent is gone"),
                },
            )
            .build();
        match registered {
            Ok(id) => {
                self.agent.replace(Some(id));
            }
            Err(err) => tracing::error!(error = %err, "cannot export the Bluetooth agent"),
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

    fn changed(self: &Rc<Self>) {
        let Some(mirror) = self.mirror() else { return };
        let generation = mirror.generation();
        if self.seen.replace(generation) != generation {
            self.cancel_pending();
            self.pairing.replace(None);
            if let Some(owner) = mirror.owner() {
                register_with(mirror.connection().clone(), owner);
                if self.open_popovers.get() > 0 {
                    self.discovery(true);
                }
            }
        }
        let state = bluetooth::state(&mirror.objects());
        if *self.state.borrow() == state {
            return;
        }
        if state.is_none() {
            self.cancel_pending();
        }
        // bluetoothd stops discovery when the adapter powers off: start it again when the
        // adapter comes back on under an open popover.
        let powered_on = state.as_ref().is_some_and(|state| state.powered)
            && !self
                .state
                .borrow()
                .as_ref()
                .is_some_and(|state| state.powered);
        self.state.replace(state);
        if powered_on && self.open_popovers.get() > 0 {
            self.discovery(true);
        }
        for view in self.views() {
            view.show(self.state.borrow().as_ref());
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    fn agent_call(
        self: &Rc<Self>,
        sender: Option<&str>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        let Some(mirror) = self.mirror() else {
            invocation.return_dbus_error(bluetooth::REJECTED, "BlueZ is not running");
            return;
        };
        if !sender.is_some_and(|sender| mirror.is_owner(sender)) {
            tracing::warn!(
                method,
                sender = sender.unwrap_or(""),
                "a Bluetooth agent call from a process that is not BlueZ was refused"
            );
            invocation.return_dbus_error(bluetooth::REJECTED, "only BlueZ may call this agent");
            return;
        }
        match method {
            "RequestConfirmation"
            | "DisplayPasskey"
            | "DisplayPinCode"
            | "RequestAuthorization"
            | "AuthorizeService"
                if !self.is_bar_pairing(params) =>
            {
                tracing::info!(
                    method,
                    "a Bluetooth request the person did not start from the bar was rejected"
                );
                invocation.return_dbus_error(
                    bluetooth::REJECTED,
                    "the person did not start this pairing from the bar",
                );
            }
            "RequestConfirmation" => {
                let request =
                    bluetooth::device_and_passkey(params).and_then(|(device, passkey)| {
                        Some((
                            bluetooth::device_label(&mirror.objects(), &device)?,
                            bluetooth::passkey_label(passkey)?,
                        ))
                    });
                let Some((label, code)) = request else {
                    invocation.return_dbus_error(
                        bluetooth::REJECTED,
                        "unknown device or invalid passkey",
                    );
                    return;
                };
                self.ask(invocation, &label, &code);
            }
            "DisplayPasskey" | "DisplayPinCode" => {
                let code = if method == "DisplayPasskey" {
                    bluetooth::device_and_passkey(params)
                        .and_then(|(_, passkey)| bluetooth::passkey_label(passkey))
                } else if props::has_type(params, "(os)") {
                    params.try_child_value(1).and_then(|pin| {
                        pin.str().map(|pin| {
                            pin.chars()
                                .filter(char::is_ascii_alphanumeric)
                                .take(MAX_CODE_CHARS)
                                .collect::<String>()
                        })
                    })
                } else {
                    None
                };
                let label = bluetooth::device_of(params)
                    .or_else(|| bluetooth::device_and_passkey(params).map(|(device, _)| device))
                    .and_then(|device| bluetooth::device_label(&mirror.objects(), &device));
                if let (Some(label), Some(code), Some(view)) = (label, code, self.prompt_view()) {
                    view.display(&label, &code);
                }
                invocation.return_value(None);
            }
            // The guard above admitted only the device of the pairing in progress.
            "RequestAuthorization" | "AuthorizeService" => invocation.return_value(None),
            "RequestPinCode" | "RequestPasskey" => {
                invocation.return_dbus_error(bluetooth::REJECTED, "this agent does not type codes")
            }
            "Cancel" => {
                self.cancel_pending();
                for view in self.views() {
                    view.close_page();
                }
                invocation.return_value(None);
            }
            "Release" => invocation.return_value(None),
            _ => invocation
                .return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    /// Whether a request names the device of the pairing the person started from the bar
    /// (maintainer decision D8). Every agent request names its device first: `(o)`, `(os)`,
    /// `(ou)` or `(ouq)`.
    fn is_bar_pairing(&self, params: &glib::Variant) -> bool {
        bluetooth::device_of(params)
            .or_else(|| bluetooth::device_and_passkey(params).map(|(device, _)| device))
            .is_some_and(|device| self.pairing.borrow().as_deref() == Some(device.as_str()))
    }

    fn ask(self: &Rc<Self>, invocation: gio::DBusMethodInvocation, label: &str, code: &str) {
        if self.pending.borrow().is_some() {
            invocation.return_dbus_error(bluetooth::REJECTED, "another request is open");
            return;
        }
        let Some(view) = self.prompt_view() else {
            invocation.return_dbus_error(bluetooth::REJECTED, "the bar shows no Bluetooth module");
            return;
        };
        self.pending.replace(Some(Pending {
            invocation,
            view: Rc::downgrade(&view),
        }));
        view.confirm(label, code);
    }

    fn prompt_view(&self) -> Option<Rc<View>> {
        let usable = |view: &Rc<View>| view.popup.button.is_mapped();
        self.last_opened
            .borrow()
            .upgrade()
            .filter(usable)
            .or_else(|| self.views().into_iter().find(usable))
    }

    /// The person's answer to `RequestConfirmation`.
    fn answer(&self, confirmed: bool) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if confirmed {
            pending.invocation.return_value(None);
        } else {
            pending
                .invocation
                .return_dbus_error(bluetooth::REJECTED, "the person declined");
        }
    }

    fn cancel_pending(&self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        pending
            .invocation
            .return_dbus_error(bluetooth::CANCELED, "the request was withdrawn");
        if let Some(view) = pending.view.upgrade() {
            view.close_page();
        }
    }

    fn device_call(&self, device: &str, method: &'static str) {
        let Some(mirror) = self.mirror() else { return };
        bus::spawn(
            method,
            bus::call(
                mirror.connection(),
                bluetooth::BLUEZ,
                device,
                bluetooth::DEVICE,
                method,
                None,
                bus::TIMEOUT_MS,
            ),
        );
    }

    fn pressed(self: &Rc<Self>, device: &Device) {
        if device.connected {
            self.device_call(&device.path, "Disconnect");
        } else if device.paired {
            self.device_call(&device.path, "Connect");
        } else {
            self.pair(device.path.clone());
        }
    }

    /// Pairs, trusts, connects: the order GNOME and COSMIC use. Trusting lets the device
    /// reconnect later without asking again.
    fn pair(self: &Rc<Self>, device: String) {
        let Some(mirror) = self.mirror() else { return };
        if self.pairing.borrow().is_some() {
            return;
        }
        self.pairing.replace(Some(device.clone()));
        let connection = mirror.connection().clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let paired = bus::call(
                &connection,
                bluetooth::BLUEZ,
                &device,
                bluetooth::DEVICE,
                "Pair",
                None,
                bus::INTERACTIVE_TIMEOUT_MS,
            )
            .await;
            if let Some(service) = weak.upgrade() {
                service.pairing.replace(None);
            }
            if let Err(err) = paired {
                tracing::warn!(error = %err, "pairing failed");
                return;
            }
            let trusted = bus::set_property(
                &connection,
                bluetooth::BLUEZ,
                &device,
                bluetooth::DEVICE,
                "Trusted",
                true.to_variant(),
            )
            .await;
            if let Err(err) = trusted {
                tracing::warn!(error = %err, "the paired device could not be trusted");
            }
            bus::spawn(
                "Connect",
                bus::call(
                    &connection,
                    bluetooth::BLUEZ,
                    &device,
                    bluetooth::DEVICE,
                    "Connect",
                    None,
                    bus::TIMEOUT_MS,
                ),
            );
        });
    }

    fn set_powered(&self, on: bool) {
        let (Some(mirror), Some(adapter)) = (
            self.mirror(),
            self.state.borrow().as_ref().map(|s| s.adapter.clone()),
        ) else {
            return;
        };
        bus::spawn(
            "Powered",
            bus::set_property(
                mirror.connection(),
                bluetooth::BLUEZ,
                &adapter,
                bluetooth::ADAPTER,
                "Powered",
                on.to_variant(),
            ),
        );
    }

    fn discovery(&self, on: bool) {
        let Some(mirror) = self.mirror() else { return };
        let Some(adapter) = self
            .state
            .borrow()
            .as_ref()
            .filter(|state| state.powered)
            .map(|state| state.adapter.clone())
        else {
            return;
        };
        let method = if on {
            "StartDiscovery"
        } else {
            "StopDiscovery"
        };
        bus::spawn(
            method,
            bus::call(
                mirror.connection(),
                bluetooth::BLUEZ,
                &adapter,
                bluetooth::ADAPTER,
                method,
                None,
                bus::TIMEOUT_MS,
            ),
        );
    }

    fn popover_toggled(&self, open: bool) {
        let before = self.open_popovers.get();
        let after = if open {
            before + 1
        } else {
            before.saturating_sub(1)
        };
        self.open_popovers.set(after);
        if before == 0 && after == 1 {
            self.discovery(true);
        } else if before == 1 && after == 0 {
            self.discovery(false);
        }
    }
}

/// `RegisterAgent`, then `RequestDefaultAgent`, to this owner.
fn register_with(connection: gio::DBusConnection, owner: String) {
    glib::spawn_future_local(async move {
        // A constant, valid path: the `else` is unreachable, and it is not an unwrap.
        let Ok(path) = glib::variant::ObjectPath::try_from(bluetooth::AGENT_PATH) else {
            return;
        };
        let register = (path.clone(), bluetooth::CAPABILITY).to_variant();
        if let Err(err) = bus::call(
            &connection,
            &owner,
            bluetooth::AGENT_MANAGER_PATH,
            bluetooth::AGENT_MANAGER,
            "RegisterAgent",
            Some(&register),
            bus::TIMEOUT_MS,
        )
        .await
        {
            tracing::warn!(error = %err, "BlueZ refused the pairing agent; pairing from the bar will fail");
            return;
        }
        let default = (path,).to_variant();
        bus::spawn(
            "RequestDefaultAgent",
            bus::call(
                &connection,
                &owner,
                bluetooth::AGENT_MANAGER_PATH,
                bluetooth::AGENT_MANAGER,
                "RequestDefaultAgent",
                Some(&default),
                bus::TIMEOUT_MS,
            ),
        );
    });
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    stack: gtk4::Stack,
    power: gtk4::Switch,
    paired: gtk4::Box,
    nearby_title: gtk4::Label,
    nearby: gtk4::Box,
    title: gtk4::Label,
    code: gtk4::Label,
    note: gtk4::Label,
    confirm: gtk4::Button,
    /// The page shows a `RequestConfirmation` this view must answer.
    asking: Cell<bool>,
    updating: Cell<bool>,
    pending_open: Cell<bool>,
}

fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("bluetooth-symbolic");
        let popup = Popup::new(bar, &icon, &tr("Bluetooth"));
        popup.button.set_visible(false);

        let (power_row, power) = switch_row(&tr("Bluetooth"));
        power_row.add_css_class("bar-popover-title");
        let paired = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let nearby_title = gtk4::Label::new(Some(&tr("Nearby devices")));
        nearby_title.add_css_class("bar-popover-note");
        nearby_title.set_xalign(0.0);
        let nearby = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        list.append(&power_row);
        list.append(&paired);
        list.append(&nearby_title);
        list.append(&nearby);

        let title = gtk4::Label::new(None);
        title.add_css_class("bar-popover-title");
        title.set_wrap(true);
        title.set_xalign(0.0);
        let code = gtk4::Label::new(None);
        // The digits in the title's size: large enough to compare across the room, with
        // no new class in the shared stylesheet (athanor-style), which 2c also edits.
        code.add_css_class("bar-popover-title");
        let note = gtk4::Label::new(None);
        note.add_css_class("bar-popover-note");
        note.set_wrap(true);
        note.set_xalign(0.0);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        cancel.add_css_class("bar-row");
        let confirm = gtk4::Button::with_label(&tr("Pair"));
        confirm.add_css_class("bar-confirm");
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        buttons.append(&cancel);
        buttons.append(&confirm);
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        page.append(&title);
        page.append(&code);
        page.append(&note);
        page.append(&buttons);

        let stack = gtk4::Stack::new();
        stack.add_named(&list, Some("list"));
        stack.add_named(&page, Some("confirm"));
        popup.popover.set_child(Some(&stack));

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            stack,
            power,
            paired,
            nearby_title,
            nearby,
            title,
            code,
            note,
            confirm,
            asking: Cell::new(false),
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers(&cancel);
        view
    }

    fn connect_handlers(self: &Rc<Self>, cancel: &gtk4::Button) {
        let weak = Rc::downgrade(self);
        self.power.connect_state_set(move |_, on| {
            if weak.upgrade().is_some_and(|view| !view.updating.get()) {
                if let Some(service) = service() {
                    service.set_powered(on);
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        self.confirm.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.finish(true);
            }
        });
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.finish(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_show(move |_| {
            if let Some(service) = service() {
                service.last_opened.replace(weak.clone());
                service.popover_toggled(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_closed(move |_| {
            if let Some(view) = weak.upgrade() {
                if view.asking.get() {
                    view.finish(false);
                }
                view.stack.set_visible_child_name("list");
            }
            if let Some(service) = service() {
                service.popover_toggled(false);
            }
        });
    }

    fn show(self: &Rc<Self>, state: Option<&BluetoothState>) {
        let Some(state) = state else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.icon.set_icon_name(Some(bluetooth::module_icon(state)));
        self.updating.set(true);
        self.power.set_active(state.powered);
        self.updating.set(false);
        clear(&self.paired);
        for device in &state.paired {
            self.paired.append(&self.device_row(device));
        }
        clear(&self.nearby);
        for device in &state.nearby {
            self.nearby.append(&self.device_row(device));
        }
        self.paired.set_visible(state.powered);
        self.nearby_title
            .set_visible(state.powered && state.discovering);
        self.nearby.set_visible(state.powered && state.discovering);
        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }

    fn device_row(self: &Rc<Self>, device: &Device) -> gtk4::Button {
        let name = if device.connected {
            tr_with("{device}, connected", "device", &device.label)
        } else {
            device.label.clone()
        };
        let text = gtk4::Label::new(Some(&name));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(pango::EllipsizeMode::End);
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        content.append(&gtk4::Image::from_icon_name(device.icon));
        content.append(&text);
        let button = gtk4::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("bar-row");
        button.update_property(&[Property::Label(&name)]);
        let device = device.clone();
        button.connect_clicked(move |_| {
            if let Some(service) = service() {
                service.pressed(&device);
            }
        });
        button
    }

    fn open_page(&self) {
        self.stack.set_visible_child_name("confirm");
        if !self.popup.popover.is_visible() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }

    /// `RequestConfirmation`: the six digits both devices show, and Pair or Cancel.
    fn confirm(&self, label: &str, code: &str) {
        self.asking.set(true);
        self.title
            .set_text(&tr_with("Pair with {device}?", "device", label));
        self.code.set_text(code);
        self.note.set_text(&tr_with(
            "Pair only if {device} shows the same number.",
            "device",
            label,
        ));
        self.confirm.set_visible(true);
        self.open_page();
        self.confirm.grab_focus();
    }

    /// `DisplayPasskey` or `DisplayPinCode`: a code to type on the other device.
    fn display(&self, label: &str, code: &str) {
        self.asking.set(false);
        self.title
            .set_text(&tr_with("Pairing with {device}", "device", label));
        self.code.set_text(code);
        self.note.set_text(&tr_with(
            "Type this code on {device}, then press Enter there.",
            "device",
            label,
        ));
        self.confirm.set_visible(false);
        self.open_page();
    }

    fn finish(&self, confirmed: bool) {
        if self.asking.replace(false) {
            if let Some(service) = service() {
                service.answer(confirmed);
            }
        }
        self.stack.set_visible_child_name("list");
        if confirmed {
            self.popup.popover.popdown();
        }
    }

    fn close_page(&self) {
        self.asking.set(false);
        self.stack.set_visible_child_name("list");
    }
}

impl Drop for View {
    fn drop(&mut self) {
        let Some(service) = service() else { return };
        if self.asking.get() {
            service.answer(false);
        }
        // A surface that leaves with its popover open never emits `closed`: count it closed,
        // or discovery would run on with no popover to show it.
        if self.popup.popover.is_visible() {
            service.popover_toggled(false);
        }
    }
}

struct BluetoothUi {
    view: Rc<View>,
}

impl ModuleUi for BluetoothUi {
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
    Some(Box::new(BluetoothUi { view }))
}
