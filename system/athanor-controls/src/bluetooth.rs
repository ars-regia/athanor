//! The Bluetooth page (doc_bar.md, BR3): the adapter's power, the paired devices, the
//! devices nearby while a popover is open, and pairing with a confirmation. The model in
//! athanor-services mirrors BlueZ, is its default agent and keeps the adapter closed to
//! bonding outside a pairing (see its documentation); this file draws its state, sends the
//! person's actions, and shows the agent's requests. Each surface has a view.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_services::bluetooth::{
    BluetoothCommand, BluetoothState, Device, PairingReply, PairingRequest,
};
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use gtk4::{glib, pango};
use tokio::sync::{mpsc, oneshot};

use crate::bridge;
use crate::i18n::{tr, tr_with};
use crate::widgets::{switch_row, Failure};
use crate::{Host, Services};

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

/// A confirmation the person has not answered.
struct Pending {
    reply: oneshot::Sender<PairingReply>,
    view: Weak<View>,
}

struct Service {
    commands: mpsc::UnboundedSender<BluetoothCommand>,
    /// The model's last state, which `bridge::follow` keeps current.
    state: RefCell<Option<BluetoothState>>,
    /// How many commands the model had counted as refused when the views last showed it.
    refused: Cell<u32>,
    views: RefCell<Vec<Weak<View>>>,
    last_opened: RefCell<Weak<View>>,
    pending: RefCell<Option<Pending>>,
    /// The person answered no to the pairing in progress: its failure is no news to them.
    declined: Cell<bool>,
    /// Pages on screen: discovery runs while at least one is.
    open_popovers: Cell<u32>,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let (states, commands) = services.bluetooth.clone();
        let requests = services.pairing_requests.borrow_mut().take();
        let service = Rc::new(Service {
            commands,
            state: RefCell::new(None),
            refused: Cell::new(0),
            views: RefCell::new(Vec::new()),
            last_opened: RefCell::new(Weak::new()),
            pending: RefCell::new(None),
            declined: Cell::new(false),
            open_popovers: Cell::new(0),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak, as the other pages hold the service: the thread-local owns it.
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state| {
            if let Some(service) = weak.upgrade() {
                service.changed(state.clone());
            }
        });
        if let Some(requests) = requests {
            let weak = Rc::downgrade(&service);
            bridge::drain(requests, move |request| {
                if let Some(service) = weak.upgrade() {
                    service.request(request);
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

    fn changed(&self, state: Option<BluetoothState>) {
        let failed = state
            .as_ref()
            .is_some_and(|state| self.refused.replace(state.refused) != state.refused);
        self.state.replace(state);
        self.show_all();
        if failed && !self.declined.replace(false) {
            self.say_failed();
        }
    }

    fn show_all(&self) {
        let state = self.state.borrow();
        for view in self.views() {
            view.show(state.as_ref());
        }
    }

    /// An action the person took failed: every view shows the mirrored state again, which
    /// puts a switch back, and the open page says so.
    fn say_failed(&self) {
        for view in self.views() {
            view.failure.show();
        }
    }

    /// What the agent asks the person to see.
    fn request(self: &Rc<Self>, request: PairingRequest) {
        match request {
            PairingRequest::Confirm { label, code, reply } => self.ask(reply, &label, &code),
            PairingRequest::Display { label, code } => {
                if let Some(view) = self.prompt_view() {
                    view.display(&label, &code);
                }
            }
            PairingRequest::Withdrawn => {
                self.cancel_pending();
                for view in self.views() {
                    view.close_page();
                }
            }
        }
    }

    fn ask(self: &Rc<Self>, reply: oneshot::Sender<PairingReply>, label: &str, code: &str) {
        // The agent has already stopped waiting when a send fails: nothing to tell it.
        let Some(view) = self
            .prompt_view()
            .filter(|_| self.pending.borrow().is_none())
        else {
            reply.send(PairingReply::Decline).ok();
            return;
        };
        self.pending.replace(Some(Pending {
            reply,
            view: Rc::downgrade(&view),
        }));
        view.confirm(label, code);
    }

    fn prompt_view(&self) -> Option<Rc<View>> {
        let usable = |view: &Rc<View>| view.host.usable();
        self.last_opened
            .borrow()
            .upgrade()
            .filter(usable)
            .or_else(|| self.views().into_iter().find(usable))
    }

    /// The person's answer to `Confirm`.
    fn answer(&self, confirmed: bool) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.declined.set(!confirmed);
        let reply = if confirmed {
            PairingReply::Accept
        } else {
            PairingReply::Decline
        };
        // The agent has already stopped waiting when a send fails: nothing to tell it.
        pending.reply.send(reply).ok();
    }

    fn cancel_pending(&self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if let Some(view) = pending.view.upgrade() {
            view.close_page();
        }
    }

    fn send(&self, command: BluetoothCommand) {
        if let Err(err) = self.commands.send(command) {
            tracing::warn!(error = %err, "the Bluetooth model did not take a command");
            // The channel is unbounded, so a send fails only when the model has ended: say
            // the action did not complete, as for a refusal.
            self.show_all();
            self.say_failed();
        }
    }

    fn pressed(&self, device: &Device) {
        let path = device.path.clone();
        if device.connected {
            self.send(BluetoothCommand::Disconnect(path));
        } else if device.paired {
            self.send(BluetoothCommand::Connect(path));
        } else {
            self.declined.set(false);
            self.send(BluetoothCommand::Pair(path));
        }
    }

    fn set_powered(&self, on: bool) {
        self.send(BluetoothCommand::Power(on));
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
            self.send_discovery(true);
        } else if before == 1 && after == 0 {
            self.send_discovery(false);
        }
    }

    /// A discovery that did not start is no action of the person's: nothing to report.
    fn send_discovery(&self, on: bool) {
        if let Err(err) = self.commands.send(BluetoothCommand::Discovery(on)) {
            tracing::warn!(error = %err, "the Bluetooth model did not take a command");
        }
    }
}

type OnShown = Box<dyn Fn(Option<&BluetoothState>)>;

struct View {
    host: Host,
    failure: Failure,
    /// Runs after the view shows a state, for the surface's own button.
    on_shown: RefCell<Option<OnShown>>,
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
    /// The page is on screen, and counted in `open_popovers`.
    counted: Cell<bool>,
    /// The devices the two lists show: they are rebuilt only when these change, not on
    /// every signal (an RSSI update while discovering), which would take the keyboard
    /// focus off a row.
    shown: RefCell<(Vec<Device>, Vec<Device>)>,
}

fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

impl View {
    fn new(host: Host) -> Rc<View> {
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
        // A screen reader reading the button reads what it confirms: the device, the digits
        // and the instruction to compare them.
        confirm.update_relation(&[Relation::DescribedBy(&[
            title.upcast_ref(),
            code.upcast_ref(),
            note.upcast_ref(),
        ])]);
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
        let failure = Failure::new(&stack);

        let view = Rc::new(View {
            host,
            failure,
            on_shown: RefCell::new(None),
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
            counted: Cell::new(false),
            shown: RefCell::new((Vec::new(), Vec::new())),
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
        self.failure.widget().connect_map(move |_| {
            if let Some(view) = weak.upgrade() {
                view.counted.set(true);
            }
            if let Some(service) = service() {
                service.last_opened.replace(weak.clone());
                service.popover_toggled(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.failure.widget().connect_unmap(move |_| {
            if let Some(view) = weak.upgrade() {
                view.counted.set(false);
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
        if let Some(state) = state {
            self.draw(state);
        }
        if let Some(shown) = &*self.on_shown.borrow() {
            shown(state);
        }
    }

    fn draw(self: &Rc<Self>, state: &BluetoothState) {
        self.updating.set(true);
        self.power.set_active(state.powered);
        self.updating.set(false);
        let changed = {
            let shown = self.shown.borrow();
            (shown.0 != state.paired, shown.1 != state.nearby)
        };
        if changed.0 {
            clear(&self.paired);
            for device in &state.paired {
                self.paired.append(&self.device_row(device));
            }
            self.shown.borrow_mut().0 = state.paired.clone();
        }
        if changed.1 {
            clear(&self.nearby);
            for device in &state.nearby {
                self.nearby.append(&self.device_row(device));
            }
            self.shown.borrow_mut().1 = state.nearby.clone();
        }
        self.paired.set_visible(state.powered);
        self.nearby_title
            .set_visible(state.powered && state.discovering);
        self.nearby.set_visible(state.powered && state.discovering);
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
        self.host.open();
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
            self.host.close();
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
        // A surface that leaves with its page on screen may never hide it: count it closed,
        // or discovery would run on with nothing to show it.
        if self.counted.get() {
            service.popover_toggled(false);
        }
    }
}

/// The Bluetooth page: one per surface, over the one service of the process.
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

    /// `shown` runs, now and after every state the page shows, with `None` while BlueZ is
    /// away.
    pub fn connect_shown(&self, shown: impl Fn(Option<&BluetoothState>) + 'static) {
        if let Some(service) = service() {
            shown(service.state.borrow().as_ref());
        }
        self.view.on_shown.replace(Some(Box::new(shown)));
    }
}
