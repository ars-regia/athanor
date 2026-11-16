//! The Bluetooth page drawn with athanor-sal (doc_bar.md, BR3): the adapter's power, the paired
//! devices to connect and disconnect, and, on the page of the bar, which owns the one agent of
//! the session, the devices nearby while the page is on screen and pairing with a confirmation,
//! as the GTK page. The control center's page, [`Page::new`], pairs nothing: a pairing needs a
//! bondable adapter and an agent, which only the bar's process has.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_sal::{Align, Event, Kind, Ui, WidgetId, Wrap};
use athanor_services::bluetooth::{
    BluetoothCommand, BluetoothState, Device, PairingReply, PairingRequest,
};
use tokio::sync::{mpsc, oneshot};

use super::widgets::{column, heading, switch_row, Failure, Look};
use super::Turn;
use crate::bridge;
use crate::i18n::{tr, tr_with};
use crate::Services;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

/// What the agent asks the person to see, until it is answered or withdrawn.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Prompt {
    label: String,
    /// The six digits both devices show, or the code to type on the other device.
    code: String,
    /// `RequestConfirmation`, answered through [`Service::pending`]; otherwise a code to type
    /// on the other device, with nothing to answer.
    confirm: bool,
}

/// The Bluetooth model of the process, which every page shows, and the agent's requests.
struct Service {
    commands: mpsc::UnboundedSender<BluetoothCommand>,
    /// The model's last state, which `bridge::follow` keeps current; `None` while BlueZ is
    /// away.
    state: RefCell<Option<BluetoothState>>,
    /// Counts the changes a page shows: a state, a request, a request answered or withdrawn.
    states: Cell<u64>,
    /// Counts the actions that did not complete: the model refused them or had ended.
    failures: Cell<u64>,
    /// The request of the agent a pairing page shows.
    prompt: RefCell<Option<Prompt>>,
    /// The reply to the confirmation `prompt` asks, until the person answers.
    pending: RefCell<Option<oneshot::Sender<PairingReply>>>,
    /// Whether [`Page::wants_open`] has said so for `prompt`.
    announced: Cell<bool>,
    /// The person answered no to the pairing in progress: its failure is no news to them.
    declined: Cell<bool>,
    /// The pages that pair: a request with none is declined.
    pairing: Cell<u32>,
    /// The pages that pair on screen: discovery runs while at least one is.
    open: Cell<u32>,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = SERVICE.with(|cell| cell.borrow().clone()) {
            return service;
        }
        let (states, commands) = services.bluetooth.clone();
        let requests = services.pairing_requests.borrow_mut().take();
        let service = Rc::new(Service::new(commands));
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak: the thread-local owns the service.
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state: &Option<BluetoothState>| {
            if let Some(service) = weak.upgrade() {
                service.follow(state);
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

    fn new(commands: mpsc::UnboundedSender<BluetoothCommand>) -> Service {
        Service {
            commands,
            state: RefCell::new(None),
            states: Cell::new(0),
            failures: Cell::new(0),
            prompt: RefCell::new(None),
            pending: RefCell::new(None),
            announced: Cell::new(false),
            declined: Cell::new(false),
            pairing: Cell::new(0),
            open: Cell::new(0),
        }
    }

    /// A state of the model; a refusal it counted is a failure the page shows, but for a
    /// pairing the person declined.
    fn follow(&self, state: &Option<BluetoothState>) {
        let refused = |s: &Option<BluetoothState>| s.as_ref().map(|s| s.refused);
        if let (Some(now), Some(before)) = (refused(state), refused(&self.state.borrow())) {
            if now != before && !self.declined.replace(false) {
                self.failed();
            }
        }
        self.state.replace(state.clone());
        self.changed();
    }

    fn changed(&self) {
        self.states.set(self.states.get() + 1);
    }

    fn failed(&self) {
        self.failures.set(self.failures.get() + 1);
    }

    fn send(&self, command: BluetoothCommand) {
        if let Err(err) = self.commands.send(command) {
            tracing::warn!(error = %err, "the Bluetooth model did not take a command");
            // The channel is unbounded, so a send fails only when the model has ended: the
            // action did not complete, as for a refusal, and the page shows the state again.
            self.failed();
            self.changed();
        }
    }

    /// A device's row was pressed: a connected one disconnects, a paired one connects, a
    /// nearby one pairs.
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

    /// What the agent asks the person to see: kept for a pairing page to show. A confirmation
    /// with no pairing page, or while another waits, is declined.
    fn request(&self, request: PairingRequest) {
        let prompt = match request {
            PairingRequest::Confirm { label, code, reply } => {
                if self.pairing.get() == 0 || self.pending.borrow().is_some() {
                    // The agent has already stopped waiting when a send fails: nothing to
                    // tell it.
                    reply.send(PairingReply::Decline).ok();
                    return;
                }
                self.pending.replace(Some(reply));
                Prompt {
                    label,
                    code,
                    confirm: true,
                }
            }
            PairingRequest::Display { label, code } => {
                if self.pairing.get() == 0 {
                    return;
                }
                Prompt {
                    label,
                    code,
                    confirm: false,
                }
            }
            PairingRequest::Withdrawn => {
                // A dropped reply is a decline, which the agent no longer waits for.
                self.pending.take();
                self.prompt.take();
                self.changed();
                return;
            }
        };
        self.prompt.replace(Some(prompt));
        self.announced.set(false);
        self.changed();
    }

    /// The person's answer to the prompt: a confirmation's reply, or a code they have read.
    fn answer(&self, confirmed: bool) {
        self.prompt.take();
        if let Some(sender) = self.pending.take() {
            self.declined.set(!confirmed);
            let reply = if confirmed {
                PairingReply::Accept
            } else {
                PairingReply::Decline
            };
            // The agent has already stopped waiting when a send fails: nothing to tell it.
            sender.send(reply).ok();
        }
        self.changed();
    }

    /// A pairing page came on screen or left it: discovery runs while one is.
    fn discovery(&self, shown: bool) {
        let before = self.open.get();
        let after = if shown {
            before + 1
        } else {
            before.saturating_sub(1)
        };
        self.open.set(after);
        if (before == 0) != (after == 0) {
            // A discovery that did not start is no action of the person's: nothing to report.
            if let Err(err) = self.commands.send(BluetoothCommand::Discovery(after > 0)) {
                tracing::warn!(error = %err, "the Bluetooth model did not take a command");
            }
        }
    }
}

/// A device's row name: its label, and that it is connected.
fn device_name(device: &Device) -> String {
    if device.connected {
        tr_with("{device}, connected", "device", &device.label)
    } else {
        device.label.clone()
    }
}

/// The widgets of the page in the surface's [`Ui`].
struct Ids {
    list: WidgetId,
    power: WidgetId,
    paired: WidgetId,
    nearby_title: WidgetId,
    nearby: WidgetId,
    /// Each device's row, in the order of the state the page was built with: the paired ones,
    /// then the nearby ones.
    rows: Vec<(WidgetId, Device)>,
    /// How many of `rows` are paired.
    paired_rows: usize,
    prompt: WidgetId,
    title: WidgetId,
    code: WidgetId,
    note: WidgetId,
    cancel: WidgetId,
    confirm: WidgetId,
    failure: Failure,
}

/// The Bluetooth page: one per surface, over the one model of the process. The surface adds it
/// to its widgets with [`Page::build`], calls [`Page::turn`] in each turn of its loop and hands
/// it every event with [`Page::event`]. A pairing page asks [`Page::wants_open`] while it is
/// hidden, so that the agent's request opens it, and [`Page::wants_close`] after each event,
/// to close the surface once the person confirmed a pairing.
pub struct Page {
    service: Rc<Service>,
    pairing: bool,
    /// The state and the failure the page shows last.
    seen: (u64, u64),
    ids: Option<Ids>,
    /// Whether the page is on screen: built, and not closed since.
    shown: bool,
    close: bool,
}

impl Page {
    /// The page of a surface that does not pair: the paired devices to connect, disconnect and
    /// switch, no nearby list and no discovery.
    pub fn new(services: &Services) -> Page {
        Page::with(Service::get(services), false)
    }

    /// The page of the surface that pairs, the bar's: the devices nearby while it is on screen,
    /// and the agent's requests. The process registers the agent itself.
    pub fn with_pairing(services: &Services) -> Page {
        Page::with(Service::get(services), true)
    }

    fn with(service: Rc<Service>, pairing: bool) -> Page {
        if pairing {
            service.pairing.set(service.pairing.get() + 1);
        }
        Page {
            service,
            pairing,
            seen: (0, 0),
            ids: None,
            shown: false,
            close: false,
        }
    }

    /// Whether the model changed since the page last showed it: the surface calls
    /// [`Page::turn`] then.
    pub fn changed(&self) -> bool {
        (self.service.states.get(), self.service.failures.get()) != self.seen
    }

    /// Whether the surface should open the page: the agent asked the person while the page
    /// was hidden. True once for each request; the page built next shows it.
    pub fn wants_open(&mut self) -> bool {
        let service = &self.service;
        let wants = self.pairing
            && !self.shown
            && service.prompt.borrow().is_some()
            && !service.announced.get();
        if wants {
            service.announced.set(true);
        }
        wants
    }

    /// Whether the surface should close, as the GTK page's host: the person confirmed a
    /// pairing. True once.
    pub fn wants_close(&mut self) -> bool {
        std::mem::take(&mut self.close)
    }

    /// Adds the page's widgets as a column under `parent`, showing the model's state, and
    /// returns the column, its rows with `look`'s icons. A page built again, in a new [`Ui`],
    /// leaves the old one's widgets; a request it was showing stays.
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, look: &mut Look) -> WidgetId {
        let page = column(ui, parent);
        let list = column(ui, page);
        let power = switch_row(ui, list, &tr("Bluetooth"), false);
        ui.set_text_style(power, heading());
        let paired = column(ui, list);
        let nearby_title = ui.add(list, Kind::Label(tr("Nearby devices")));
        look.note(ui, nearby_title);
        let nearby = column(ui, list);
        let (paired_devices, nearby_devices) = self
            .service
            .state
            .borrow()
            .as_ref()
            .map(|state| (state.paired.clone(), state.nearby.clone()))
            .unwrap_or_default();
        let paired_rows = paired_devices.len();
        let nearby_devices = nearby_devices.into_iter().filter(|_| self.pairing);
        let rows = (paired_devices.into_iter().map(|device| (paired, device)))
            .chain(nearby_devices.map(|device| (nearby, device)))
            .map(|(list, device)| {
                // The kind's icon, then the name, as the GTK row's box.
                let name = device_name(&device);
                let icon = (look.icon)(device.icon);
                let row = ui.labelled_button(list, &name, icon, look.icon_size, &name);
                (row, device)
            })
            .collect();

        let prompt = column(ui, page);
        let title = ui.add(prompt, Kind::Label(String::new()));
        ui.set_text_style(title, heading());
        ui.set_wrap(title, Some(Wrap::default()));
        // The digits in the title's style: large enough to compare across the room.
        let code = ui.add(prompt, Kind::Label(String::new()));
        ui.set_text_style(code, heading());
        let note = ui.add(prompt, Kind::Label(String::new()));
        look.note(ui, note);
        ui.set_wrap(note, Some(Wrap::default()));
        let buttons = ui.add(prompt, Kind::Row);
        ui.set_align(buttons, [Align::End, Align::Fill]);
        let cancel = ui.add(buttons, Kind::Button(tr("Cancel")));
        let confirm = ui.add(buttons, Kind::Button(tr("Pair")));
        ui.set_visible(prompt, false);

        let failure = Failure::new(ui, page, look);
        self.ids = Some(Ids {
            list,
            power,
            paired,
            nearby_title,
            nearby,
            rows,
            paired_rows,
            prompt,
            title,
            code,
            note,
            cancel,
            confirm,
            failure,
        });
        if !self.shown && self.pairing {
            self.service.discovery(true);
        }
        self.shown = true;
        self.show(ui);
        page
    }

    /// Shows what changed since the last call: a new state or request, and a failure as the
    /// note under the page. A device that came, went or changed asks for the page to be built
    /// again.
    pub fn turn(&mut self, ui: &mut Ui) -> Turn {
        let now = (self.service.states.get(), self.service.failures.get());
        if now == self.seen {
            return Turn::Unchanged;
        }
        let rebuild = self.ids.as_ref().is_some_and(|ids| {
            let state = self.service.state.borrow();
            let (paired, nearby) = state.as_ref().map_or((&[][..], &[][..]), |s| {
                (s.paired.as_slice(), s.nearby.as_slice())
            });
            let nearby = if self.pairing { nearby } else { &[] };
            ids.paired_rows != paired.len()
                || !ids
                    .rows
                    .iter()
                    .map(|(_, d)| d)
                    .eq(paired.iter().chain(nearby))
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

    /// The page left the screen: a note says nothing of the actions to come, a confirmation it
    /// showed is declined and discovery stops with the last pairing page.
    pub fn closed(&mut self, ui: &mut Ui) {
        if let Some(ids) = &self.ids {
            ids.failure.hide(ui);
        }
        self.left();
        self.show(ui);
    }

    /// The surface dropped the [`Ui`] that held the page, as a popover that closed: the page
    /// leaves the screen as [`Page::closed`] says, with no widgets left to set.
    pub fn dropped(&mut self) {
        self.ids = None;
        self.left();
    }

    /// Takes `event` if it is one of the page's widgets': whether it was.
    pub fn event(&mut self, ui: &mut Ui, event: &Event) -> bool {
        let Some(ids) = &self.ids else {
            return false;
        };
        match *event {
            Event::Toggled(id) if id == ids.power => {
                self.service.send(BluetoothCommand::Power(ui.is_active(id)));
            }
            Event::Clicked(id) if id == ids.confirm => self.finish(ui, true),
            Event::Clicked(id) if id == ids.cancel => self.finish(ui, false),
            Event::Clicked(id) => match ids.rows.iter().find(|(row, _)| *row == id) {
                Some((_, device)) => self.service.pressed(device),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// The person answered the prompt: back to the list, and the surface closes once a pairing
    /// is confirmed.
    fn finish(&mut self, ui: &mut Ui, confirmed: bool) {
        let confirm = self.service.prompt.borrow().as_ref().map(|p| p.confirm);
        let Some(confirm) = confirm else {
            return;
        };
        self.service.answer(confirmed);
        self.close = confirm && confirmed;
        self.show(ui);
    }

    /// The page is off screen: a confirmation it showed is declined, a code it showed is let
    /// go, and discovery stops with the last pairing page.
    fn left(&mut self) {
        if !std::mem::take(&mut self.shown) || !self.pairing {
            return;
        }
        if self.service.prompt.borrow().is_some() {
            self.service.answer(false);
        }
        self.service.discovery(false);
    }

    fn show(&mut self, ui: &mut Ui) {
        self.seen = (self.service.states.get(), self.service.failures.get());
        let Some(ids) = &self.ids else {
            return;
        };
        let state = self.service.state.borrow();
        // Nothing is nearby before the first state, and never on a page that does not pair.
        let finding = self.pairing
            && state
                .as_ref()
                .is_some_and(|state| state.powered && state.discovering);
        ui.set_visible(ids.nearby_title, finding);
        ui.set_visible(ids.nearby, finding);
        if let Some(state) = state.as_ref() {
            ui.set_active(ids.power, state.powered);
            ui.set_visible(ids.paired, state.powered);
        }
        let prompt = self
            .service
            .prompt
            .borrow()
            .clone()
            .filter(|_| self.pairing);
        let was_asking = ui.is_visible(ids.prompt);
        ui.set_visible(ids.list, prompt.is_none());
        ui.set_visible(ids.prompt, prompt.is_some());
        let Some(prompt) = prompt else {
            return;
        };
        let device = &prompt.label;
        let (title, note) = if prompt.confirm {
            (
                tr_with("Pair with {device}?", "device", device),
                tr_with(
                    "Pair only if {device} shows the same number.",
                    "device",
                    device,
                ),
            )
        } else {
            (
                tr_with("Pairing with {device}", "device", device),
                tr_with(
                    "Type this code on {device}, then press Enter there.",
                    "device",
                    device,
                ),
            )
        };
        // A screen reader reading Pair reads what it confirms: the device, the digits and the
        // instruction to compare them, as the GTK button's DescribedBy.
        let description = format!("{title} {} {note}", prompt.code);
        ui.set_text(ids.title, &title);
        ui.set_text(ids.code, &prompt.code);
        ui.set_text(ids.note, &note);
        ui.set_description(ids.confirm, Some(&description));
        ui.set_visible(ids.confirm, prompt.confirm);
        if !was_asking {
            ui.set_focus(Some(if prompt.confirm {
                ids.confirm
            } else {
                ids.cancel
            }));
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // A surface that leaves with its page on screen may never close it: count it closed,
        // or discovery would run on with nothing to show it.
        self.left();
        if self.pairing {
            let pairing = &self.service.pairing;
            pairing.set(pairing.get().saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use athanor_sal::Theme;

    use super::*;

    fn device(label: &str, connected: bool) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{label}"),
            label: label.to_owned(),
            icon: "audio-headphones-symbolic",
            connected,
            paired: true,
        }
    }

    fn nearby(label: &str) -> Device {
        Device {
            paired: false,
            ..device(label, false)
        }
    }

    fn state(paired: Vec<Device>) -> BluetoothState {
        BluetoothState {
            adapter: "/org/bluez/hci0".to_owned(),
            powered: true,
            discovering: false,
            pairable: false,
            discoverable: false,
            paired,
            nearby: Vec::new(),
            refused: 0,
        }
    }

    fn service(state: BluetoothState) -> (Rc<Service>, mpsc::UnboundedReceiver<BluetoothCommand>) {
        let (commands, received) = mpsc::unbounded_channel();
        let service = Rc::new(Service::new(commands));
        service.state.replace(Some(state));
        service.changed();
        (service, received)
    }

    fn page(state: BluetoothState) -> (Page, mpsc::UnboundedReceiver<BluetoothCommand>) {
        let (service, received) = service(state);
        (Page::with(service, false), received)
    }

    fn ids(page: &Page) -> &Ids {
        match &page.ids {
            Some(ids) => ids,
            None => unreachable!("built"),
        }
    }

    fn confirm(service: &Service, label: &str) -> oneshot::Receiver<PairingReply> {
        let (reply, answer) = oneshot::channel();
        service.request(PairingRequest::Confirm {
            label: label.to_owned(),
            code: "123456".to_owned(),
            reply,
        });
        answer
    }

    #[test]
    fn the_page_powers_the_adapter_and_connects_or_disconnects_a_device() {
        let state = state(vec![device("Headphones", true), device("Keyboard", false)]);
        let (mut page, mut received) = page(state);
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        let ids = ids(&page);
        let (power, headphones, keyboard) = (ids.power, ids.rows[0].0, ids.rows[1].0);
        assert!(ui.is_active(power));
        assert!(
            matches!(&ui.widget(headphones).kind, Kind::LabelledButton { text, .. } if text == "Headphones, connected")
        );
        let toggled = ui.activate(power);
        assert!(toggled.is_some_and(|event| page.event(&mut ui, &event)));
        assert!(matches!(
            received.try_recv(),
            Ok(BluetoothCommand::Power(false))
        ));
        for (row, connected) in [(headphones, true), (keyboard, false)] {
            let clicked = ui.activate(row);
            assert!(clicked.is_some_and(|event| page.event(&mut ui, &event)));
            match received.try_recv() {
                Ok(BluetoothCommand::Disconnect(_)) => assert!(connected),
                Ok(BluetoothCommand::Connect(_)) => assert!(!connected),
                other => unreachable!("a connection command, not {other:?}"),
            }
        }
    }

    #[test]
    fn a_device_that_comes_or_goes_rebuilds_the_page_and_a_refusal_shows_the_note() {
        let (mut page, _received) = page(state(vec![device("Keyboard", false)]));
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        assert_eq!(page.turn(&mut ui), Turn::Unchanged);
        assert!(!page.changed());
        page.service.failed();
        assert!(page.changed());
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(ui.is_visible(ids(&page).failure.note));
        page.service
            .state
            .replace(Some(state(vec![device("Keyboard", true)])));
        page.service.changed();
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
    }

    #[test]
    fn a_pairing_page_discovers_while_on_screen_and_pairs_a_nearby_device() {
        let mut discovering = state(vec![device("Keyboard", false)]);
        discovering.discovering = true;
        discovering.nearby = vec![nearby("Speaker")];
        let (service, mut received) = service(discovering);
        let mut pairing = Page::with(service.clone(), true);
        let mut ui = Ui::new("t", Theme::default(), false);
        pairing.build(&mut ui, Ui::ROOT, &mut Look::plain());
        assert!(matches!(
            received.try_recv(),
            Ok(BluetoothCommand::Discovery(true))
        ));
        let shown = ids(&pairing);
        assert!(ui.is_visible(shown.nearby_title) && ui.is_visible(shown.nearby));
        let speaker = shown.rows[1].0;
        let clicked = ui.activate(speaker);
        assert!(clicked.is_some_and(|event| pairing.event(&mut ui, &event)));
        assert!(matches!(received.try_recv(), Ok(BluetoothCommand::Pair(_))));

        // Built again while on screen, as a popover built anew: discovery is not asked twice.
        pairing.build(&mut ui, Ui::ROOT, &mut Look::plain());
        assert!(received.try_recv().is_err());
        pairing.dropped();
        assert!(matches!(
            received.try_recv(),
            Ok(BluetoothCommand::Discovery(false))
        ));

        // The control center's page shows no nearby device and starts no discovery.
        let mut plain = Page::with(service, false);
        let mut ui = Ui::new("t", Theme::default(), false);
        plain.build(&mut ui, Ui::ROOT, &mut Look::plain());
        assert_eq!(ids(&plain).rows.len(), 1);
        assert!(!ui.is_visible(ids(&plain).nearby));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn a_confirmation_opens_the_hidden_page_once_and_pair_accepts_and_closes() {
        let (service, _received) = service(state(Vec::new()));
        let mut page = Page::with(service.clone(), true);
        let mut answer = confirm(&service, "Headphones");
        assert!(page.wants_open());
        assert!(!page.wants_open(), "once for each request");
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        let ids = ids(&page);
        let (list, prompt, pair) = (ids.list, ids.prompt, ids.confirm);
        assert!(!ui.is_visible(list) && ui.is_visible(prompt));
        assert_eq!(ui.focus(), Some(pair));
        assert_eq!(
            ui.description(pair),
            Some("Pair with Headphones? 123456 Pair only if Headphones shows the same number.")
        );
        let clicked = ui.activate(pair);
        assert!(clicked.is_some_and(|event| page.event(&mut ui, &event)));
        assert_eq!(answer.try_recv(), Ok(PairingReply::Accept));
        assert!(page.wants_close());
        assert!(ui.is_visible(list) && !ui.is_visible(prompt));
    }

    #[test]
    fn cancel_or_closing_declines_and_a_declined_pairing_shows_no_failure() {
        let (service, _received) = service(state(Vec::new()));
        let mut page = Page::with(service.clone(), true);
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        let mut answer = confirm(&service, "Headphones");
        assert!(!page.wants_open(), "the page is on screen");
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        let cancel = ids(&page).cancel;
        let clicked = ui.activate(cancel);
        assert!(clicked.is_some_and(|event| page.event(&mut ui, &event)));
        assert_eq!(answer.try_recv(), Ok(PairingReply::Decline));
        assert!(!page.wants_close());
        // BlueZ counts the declined pairing as refused: no news to the person.
        let mut refused = state(Vec::new());
        refused.refused = 1;
        service.follow(&Some(refused));
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(!ui.is_visible(ids(&page).failure.note));

        let mut answer = confirm(&service, "Headphones");
        page.dropped();
        assert_eq!(answer.try_recv(), Ok(PairingReply::Decline));
    }

    #[test]
    fn a_request_without_a_pairing_page_or_while_one_waits_is_declined() {
        let (service, _received) = service(state(Vec::new()));
        let mut alone = confirm(&service, "Headphones");
        assert_eq!(alone.try_recv(), Ok(PairingReply::Decline));
        let _page = Page::with(service.clone(), true);
        let _first = confirm(&service, "Headphones");
        let mut second = confirm(&service, "Keyboard");
        assert_eq!(second.try_recv(), Ok(PairingReply::Decline));
    }

    #[test]
    fn a_code_to_type_shows_without_pair_and_a_withdrawal_closes_the_prompt() {
        let (service, _received) = service(state(Vec::new()));
        let mut page = Page::with(service.clone(), true);
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        service.request(PairingRequest::Display {
            label: "Keyboard".to_owned(),
            code: "000042".to_owned(),
        });
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        let ids = ids(&page);
        let (prompt, pair, cancel, title) = (ids.prompt, ids.confirm, ids.cancel, ids.title);
        assert!(ui.is_visible(prompt) && !ui.is_visible(pair));
        assert_eq!(ui.focus(), Some(cancel));
        assert!(
            matches!(&ui.widget(title).kind, Kind::Label(text) if text == "Pairing with Keyboard")
        );
        service.request(PairingRequest::Withdrawn);
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(!ui.is_visible(prompt));
    }
}
