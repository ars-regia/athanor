//! The notification popups (doc_bar.md BR4): one layer surface at the end corner on the
//! panel's side, above windows, never taking the keyboard focus (ruling 14). The newest
//! popup sits nearest the panel.

use std::rc::{Rc, Weak};

use athanor_bar::notices::Notice;
use athanor_layout::preset::PanelEdge;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::notifications::{card, Place, Service};
use super::Bar;
use crate::{i18n, layer_guard};

const MARGIN: i32 = 8;

pub(super) struct Window {
    window: gtk4::ApplicationWindow,
    cards: gtk4::Box,
}

impl Window {
    pub(super) fn new(bar: &Rc<Bar>, service: &Weak<Service>) -> Window {
        let window = gtk4::ApplicationWindow::new(&bar.app);
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!(
                "athanor-bar: the notification popups are not a layer surface: {reason}"
            );
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-notifications"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::None);
        for class in ["athanor-surface", "athanor-notifications"] {
            window.add_css_class(class);
        }
        let cards = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        window.set_child(Some(&cards));
        // The pointer over any popup pauses every countdown (ruling 4).
        let motion = gtk4::EventControllerMotion::new();
        let entered = service.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(service) = entered.upgrade() {
                service.pointer(true);
            }
        });
        let left = service.clone();
        motion.connect_leave(move |_| {
            if let Some(service) = left.upgrade() {
                service.pointer(false);
            }
        });
        window.add_controller(motion);
        Window { window, cards }
    }

    /// Shows `notices`, newest first, at the end corner of the panel's side.
    pub(super) fn show(&self, bar: &Bar, service: &Rc<Service>, notices: &[Notice]) {
        let panel = bar.layout().panel();
        let (edge, far) = match panel {
            PanelEdge::Top => (Edge::Top, Edge::Bottom),
            PanelEdge::Bottom => (Edge::Bottom, Edge::Top),
        };
        let (end, start) = if i18n::is_rtl() {
            (Edge::Left, Edge::Right)
        } else {
            (Edge::Right, Edge::Left)
        };
        self.window.set_anchor(edge, true);
        self.window.set_anchor(far, false);
        self.window.set_anchor(end, true);
        self.window.set_anchor(start, false);
        self.window.set_margin(edge, MARGIN);
        self.window.set_margin(end, MARGIN);
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        let mut ordered: Vec<&Notice> = notices.iter().collect();
        if panel == PanelEdge::Bottom {
            ordered.reverse();
        }
        for notice in ordered {
            self.cards.append(&card(service, notice, Place::Popup));
        }
        self.window.present();
    }

    pub(super) fn hide(&self) {
        self.window.set_visible(false);
    }

    pub(super) fn visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Lets the window go without destroying its layer surface (see `Surface::abandon`).
    // ponytail: the output under the surface is not known, so any output that leaves while
    // the popups show costs one empty, transparent window for the life of the process; track
    // the surface's monitor (`gdk::Surface::enter-monitor`) if that ever shows up.
    pub(super) fn abandon(self) {
        self.window.set_child(None::<&gtk4::Widget>);
    }
}
