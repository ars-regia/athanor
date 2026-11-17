//! The bar's Bluetooth (doc_bar.md, BR3), as the GTK bar's `ui/bluetooth.rs` was: a button with
//! the model's icon, hidden until BlueZ's first state, and athanor-controls' Bluetooth page in
//! its popover, with the devices nearby while it is open and the confirmation of a pairing,
//! which opens the popover by itself. The bar owns the session's one pairing agent.

use std::cell::RefCell;
use std::rc::Rc;

use athanor_bar::i18n::tr;
use athanor_controls::bridge;
use athanor_controls::sal::bluetooth::Page;
use athanor_controls::sal::Turn;
use athanor_sal::{Event, Ui, WidgetId};
use athanor_services::bluetooth::{self, BluetoothState};

use super::{controls, Cx, Module, Reply, Start, ICON};
use crate::ui::look;

struct Bluetooth {
    page: Page,
    /// The model's last state; `None` while BlueZ is away, and the button is hidden.
    state: Rc<RefCell<Option<BluetoothState>>>,
    /// The button's icon, as last drawn.
    drawn: Option<&'static str>,
}

pub fn new(_start: &Start) -> Option<Box<dyn Module>> {
    let Some(services) = bridge::services() else {
        tracing::warn!("no runtime for the models; the Bluetooth module is hidden");
        return None;
    };
    // The bar owns the one default pairing agent of the session (doc_control_center.md).
    services.register_bluetooth_agent();
    let state = Rc::new(RefCell::new(None));
    let followed = state.clone();
    bridge::follow(
        services.bluetooth.0.clone(),
        move |now: &Option<BluetoothState>| {
            followed.replace(now.clone());
        },
    );
    Some(Box::new(Bluetooth {
        page: Page::with_pairing(&services),
        state,
        drawn: None,
    }))
}

impl Bluetooth {
    /// The button's icon now; `None` while the module is hidden.
    fn icon(&self) -> Option<&'static str> {
        self.state.borrow().as_ref().map(bluetooth::module_icon)
    }

    /// Closes the popover once the person confirmed a pairing.
    fn reply(&mut self) -> Reply {
        if self.page.wants_close() {
            Reply::Close
        } else {
            Reply::Stay
        }
    }
}

impl Module for Bluetooth {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        self.drawn = self.icon();
        let icon = self.drawn?;
        Some(
            cx.icons
                .button(ui, group, icon, ICON, cx.scale, &tr("Bluetooth"), None),
        )
    }

    fn stale(&self, _cx: &Cx) -> bool {
        self.icon() != self.drawn
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let (mut ui, frame) = look::popover(&tr("Bluetooth"), controls::rows(cx.theme), cx.rtl, cx.top);
        let mut drawn_with = controls::look(cx.icons, cx.scale, cx.rtl, cx.secondary);
        self.page.build(&mut ui, frame, &mut drawn_with);
        Some(ui)
    }

    fn event(&mut self, _cx: &mut Cx, ui: &mut Ui, event: Event) -> Reply {
        self.page.event(ui, &event);
        self.reply()
    }

    fn refresh(&mut self, _cx: &mut Cx, ui: &mut Ui) -> Reply {
        match self.page.turn(ui) {
            Turn::Rebuild => Reply::Reopen,
            Turn::Updated | Turn::Unchanged => self.reply(),
        }
    }

    fn wants_open(&mut self) -> bool {
        self.page.wants_open()
    }

    fn changed(&self) -> bool {
        self.page.changed()
    }

    fn closed(&mut self) {
        // The popover's widgets went with it: a confirmation it showed is declined, and
        // discovery stops.
        self.page.dropped();
    }
}
