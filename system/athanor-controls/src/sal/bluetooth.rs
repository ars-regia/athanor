//! The Bluetooth page drawn with athanor-sal (doc_bar.md, BR3), as the control center shows it:
//! the adapter's power and the paired devices to connect and disconnect, as the GTK page's
//! `without_pairing`. Pairing needs a bondable adapter and an agent, which only the bar's
//! process has; its page adds them when the bar moves to athanor-sal.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_sal::{Event, Ui, WidgetId};
use athanor_services::bluetooth::{BluetoothCommand, BluetoothState, Device};
use tokio::sync::mpsc;

use super::widgets::{column, heading, switch_row, Failure, Look};
use super::Turn;
use crate::bridge;
use crate::i18n::{tr, tr_with};
use crate::Services;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

/// The Bluetooth model of the process, which every page shows.
struct Service {
    commands: mpsc::UnboundedSender<BluetoothCommand>,
    /// The model's last state, which `bridge::follow` keeps current; `None` while BlueZ is
    /// away.
    state: RefCell<Option<BluetoothState>>,
    /// Counts the states, so that a page shows one it has not seen.
    states: Cell<u64>,
    /// Counts the actions that did not complete: the model refused them or had ended.
    failures: Cell<u64>,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = SERVICE.with(|cell| cell.borrow().clone()) {
            return service;
        }
        let (states, commands) = services.bluetooth.clone();
        let service = Rc::new(Service {
            commands,
            state: RefCell::new(None),
            states: Cell::new(0),
            failures: Cell::new(0),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak: the thread-local owns the service.
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state: &Option<BluetoothState>| {
            let Some(service) = weak.upgrade() else {
                return;
            };
            let refused = |s: &Option<BluetoothState>| s.as_ref().map(|s| s.refused);
            if let (Some(now), Some(before)) = (refused(state), refused(&service.state.borrow())) {
                if now != before {
                    service.failed();
                }
            }
            service.state.replace(state.clone());
            service.states.set(service.states.get() + 1);
        });
        service
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
            self.states.set(self.states.get() + 1);
        }
    }

    /// A device's row was pressed: a connected one disconnects, a paired one connects.
    fn pressed(&self, device: &Device) {
        let path = device.path.clone();
        self.send(if device.connected {
            BluetoothCommand::Disconnect(path)
        } else {
            BluetoothCommand::Connect(path)
        });
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
    power: WidgetId,
    paired: WidgetId,
    /// Each device's row, in the order of the state the page was built with.
    rows: Vec<(WidgetId, Device)>,
    failure: Failure,
}

/// The Bluetooth page: one per surface, over the one model of the process. The surface adds it
/// to its widgets with [`Page::build`], calls [`Page::turn`] in each turn of its loop and hands
/// it every event with [`Page::event`].
pub struct Page {
    service: Rc<Service>,
    /// The state and the failure the page shows last.
    seen: (u64, u64),
    ids: Option<Ids>,
}

impl Page {
    pub fn new(services: &Services) -> Page {
        Page {
            service: Service::get(services),
            seen: (0, 0),
            ids: None,
        }
    }

    /// Adds the page's widgets as a column under `parent`, showing the model's state, and
    /// returns the column, its rows with `look`'s icons. A page built again, in a new [`Ui`],
    /// leaves the old one's widgets.
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, look: &mut Look) -> WidgetId {
        let page = column(ui, parent);
        let power = switch_row(ui, page, &tr("Bluetooth"), false);
        ui.set_text_style(power, heading());
        let paired = column(ui, page);
        let devices = self
            .service
            .state
            .borrow()
            .as_ref()
            .map(|state| state.paired.clone())
            .unwrap_or_default();
        let rows = devices
            .into_iter()
            .map(|device| {
                // The kind's icon, then the name, as the GTK row's box.
                let name = device_name(&device);
                let icon = (look.icon)(device.icon);
                let row = ui.labelled_button(paired, &name, icon, look.icon_size, &name);
                (row, device)
            })
            .collect();
        let failure = Failure::new(ui, page, look);
        self.ids = Some(Ids {
            power,
            paired,
            rows,
            failure,
        });
        self.show(ui);
        page
    }

    /// Shows what changed since the last call: a new state, and a failure as the note under
    /// the page. A device that came, went or changed asks for the page to be built again.
    pub fn turn(&mut self, ui: &mut Ui) -> Turn {
        let now = (self.service.states.get(), self.service.failures.get());
        if now == self.seen {
            return Turn::Unchanged;
        }
        let rebuild = self.ids.as_ref().is_some_and(|ids| {
            let state = self.service.state.borrow();
            let paired = state.as_ref().map_or(&[][..], |s| s.paired.as_slice());
            !ids.rows.iter().map(|(_, d)| d).eq(paired.iter())
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

    /// The page left the screen: a note says nothing of the actions to come.
    pub fn closed(&mut self, ui: &mut Ui) {
        if let Some(ids) = &self.ids {
            ids.failure.hide(ui);
        }
    }

    /// Takes `event` if it is one of the page's widgets': whether it was.
    pub fn event(&mut self, ui: &mut Ui, event: &Event) -> bool {
        let Some(ids) = &self.ids else {
            return false;
        };
        match *event {
            Event::Toggled(id) if id == ids.power => {
                self.service.send(BluetoothCommand::Power(ui.is_active(id)));
                true
            }
            Event::Clicked(id) => match ids.rows.iter().find(|(row, _)| *row == id) {
                Some((_, device)) => {
                    self.service.pressed(device);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        self.seen = (self.service.states.get(), self.service.failures.get());
        let (Some(ids), state) = (&self.ids, self.service.state.borrow()) else {
            return;
        };
        let Some(state) = state.as_ref() else {
            return;
        };
        ui.set_active(ids.power, state.powered);
        ui.set_visible(ids.paired, state.powered);
    }
}

#[cfg(test)]
mod tests {
    use athanor_sal::{Kind, Theme};

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

    fn page(state: BluetoothState) -> (Page, mpsc::UnboundedReceiver<BluetoothCommand>) {
        let (commands, received) = mpsc::unbounded_channel();
        let service = Rc::new(Service {
            commands,
            state: RefCell::new(Some(state)),
            states: Cell::new(1),
            failures: Cell::new(0),
        });
        let page = Page {
            service,
            seen: (0, 0),
            ids: None,
        };
        (page, received)
    }

    #[test]
    fn the_page_powers_the_adapter_and_connects_or_disconnects_a_device() {
        let state = state(vec![device("Headphones", true), device("Keyboard", false)]);
        let (mut page, mut received) = page(state);
        let mut ui = Ui::new("t", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &mut Look::plain());
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
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
        page.service.failed();
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        assert!(ui.is_visible(ids.failure.note));
        page.service.state.replace(Some(state(vec![device("Keyboard", true)])));
        page.service.states.set(page.service.states.get() + 1);
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
    }
}
