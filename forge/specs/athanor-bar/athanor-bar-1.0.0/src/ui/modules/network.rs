//! The bar's network (doc_bar.md, BR3), as the GTK bar's `ui/network.rs` was: a button with the
//! model's icon, hidden until NetworkManager's first state, and athanor-controls' network page
//! in its popover, which opens by itself when NetworkManager asks for a password. The bar owns
//! the session's one secret agent.

use std::cell::RefCell;
use std::rc::Rc;

use athanor_bar::i18n::tr;
use athanor_controls::bridge;
use athanor_controls::sal::network::Page;
use athanor_controls::sal::Turn;
use athanor_sal::{Event, Ui, WidgetId};
use athanor_services::network::{self, NetworkState};

use super::{Cx, Module, Reply, Start, ICON};
use crate::ui::look;

struct Network {
    page: Page,
    /// The model's last state; `None` while NetworkManager is away, and the button is hidden.
    state: Rc<RefCell<Option<NetworkState>>>,
    /// The button's icon, as last drawn.
    drawn: Option<&'static str>,
}

pub fn new(_start: &Start) -> Option<Box<dyn Module>> {
    let Some(services) = bridge::services() else {
        tracing::warn!("no runtime for the models; the network module is hidden");
        return None;
    };
    // The bar owns the one secret agent of the session (doc_control_center.md).
    services.register_network_agent();
    let state = Rc::new(RefCell::new(None));
    let followed = state.clone();
    bridge::follow(
        services.network.0.clone(),
        move |now: &Option<NetworkState>| {
            followed.replace(now.clone());
        },
    );
    Some(Box::new(Network {
        page: Page::new(&services),
        state,
        drawn: None,
    }))
}

impl Network {
    /// The button's icon now; `None` while the module is hidden.
    fn icon(&self) -> Option<&'static str> {
        self.state.borrow().as_ref().map(network::icon)
    }

    /// Closes the popover once the page is done: a password sent, cancelled or withdrawn.
    fn reply(&mut self) -> Reply {
        if self.page.wants_close() {
            Reply::Close
        } else {
            Reply::Stay
        }
    }
}

impl Module for Network {
    fn button(&mut self, cx: &mut Cx, ui: &mut Ui, group: WidgetId) -> Option<WidgetId> {
        self.drawn = self.icon();
        let icon = self.drawn?;
        Some(
            cx.icons
                .button(ui, group, icon, ICON, cx.scale, &tr("Network"), None),
        )
    }

    fn stale(&self, _cx: &Cx) -> bool {
        self.icon() != self.drawn
    }

    fn open(&mut self, cx: &mut Cx) -> Option<Ui> {
        let (mut ui, frame) = look::popover(&tr("Network"), cx.theme.clone(), cx.rtl, cx.top);
        let mut controls = look::controls(cx.icons, cx.scale, cx.secondary);
        self.page.build(&mut ui, frame, &mut controls);
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
        // The popover's widgets went with it: a password request it showed is cancelled.
        self.page.dropped();
    }
}
