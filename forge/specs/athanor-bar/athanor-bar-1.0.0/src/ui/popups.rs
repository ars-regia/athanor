//! The notification popups (doc_bar.md BR4, NC12): one layer surface per output, at the corner
//! the settings name (by default the end corner on the panel's side), above windows, never
//! taking the keyboard focus (ruling 14). The newest popup sits nearest the anchored edge.
//!
//! Once mapped, the surface stays mapped for the life of the process: cosmic-comp 1.8.0
//! drops the Wayland connection of a gtk4-layer-shell client that destroys a mapped layer
//! surface, which is what hiding the window does. With no popup to show, the window holds
//! no card and takes no input, so it is neither seen nor in the pointer's way.
//!
//! A window is pinned to the output it was made on, and is never moved: gtk4-layer-shell 1.3
//! remaps a layer window that has no monitor, or changes monitor, by destroying its surface,
//! and cosmic-comp then drops the connection. It is let go when that output leaves.

use std::cell::Cell;
use std::rc::Rc;

use athanor_bar::notices::Notice;
use athanor_bar::popups::{corner_edges, Look};
use gtk4::prelude::*;
use gtk4::{cairo, gdk};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::notifications::{card, Service};
use super::Bar;
use crate::{i18n, layer_guard};

const MARGIN: i32 = 8;

pub(super) struct Window {
    window: gtk4::ApplicationWindow,
    cards: gtk4::Box,
    monitor: gdk::Monitor,
    /// The pointer is over this window's popups (ruling 4).
    inside: Rc<Cell<bool>>,
}

impl Window {
    pub(super) fn new(bar: &Rc<Bar>, monitor: &gdk::Monitor) -> Window {
        let window = gtk4::ApplicationWindow::new(&bar.app);
        window.init_layer_shell();
        athanor_apps::timing::watch_layer(&window, "bar-popups");
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!(
                "athanor-bar: the notification popups are not a layer surface: {reason}"
            );
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-notifications"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_monitor(Some(monitor));
        for class in ["athanor-surface", "athanor-notifications"] {
            window.add_css_class(class);
        }
        let cards = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        window.set_child(Some(&cards));
        // The pointer over any popup pauses every countdown (ruling 4).
        let inside = Rc::new(Cell::new(false));
        let motion = gtk4::EventControllerMotion::new();
        let entered = Rc::clone(&inside);
        motion.connect_enter(move |_, _, _| entered.set(true));
        let left = Rc::clone(&inside);
        motion.connect_leave(move |_| left.set(false));
        window.add_controller(motion);
        Window {
            window,
            cards,
            monitor: monitor.clone(),
            inside,
        }
    }

    /// Shows `notices`, newest first, at the corner `look` names (the end corner of the
    /// panel's side by default), on the overlay layer unless the fullscreen trigger is on.
    pub(super) fn show(&self, bar: &Bar, service: &Rc<Service>, notices: &[Notice], look: Look) {
        let (edge, end) = corner_edges(look.corner, bar.layout().panel(), i18n::is_rtl());
        for side in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let taken = side == edge || side == end;
            self.window.set_anchor(side, taken);
            self.window.set_margin(side, if taken { MARGIN } else { 0 });
        }
        // The layer is set only when it changes: a protocol too old to move a mapped
        // surface would make gtk4-layer-shell remap it (see the module's documentation).
        let layer = if look.trigger_fullscreen {
            Layer::Top
        } else {
            Layer::Overlay
        };
        if self.window.layer() != layer {
            self.window.set_layer(layer);
        }
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        let mut ordered: Vec<&Notice> = notices.iter().collect();
        if edge == Edge::Bottom {
            ordered.reverse();
        }
        for notice in ordered {
            self.cards.append(&card(service, notice, look.private));
        }
        self.window.present();
        self.set_input(true);
    }

    /// Takes every popup off the screen without unmapping the surface (see the module's
    /// documentation): no card, and an empty input region.
    pub(super) fn hide(&self) {
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        // A window with nothing to draw yields no render node, GTK commits no frame, and the
        // compositor keeps showing the last one: the ended card stayed on screen. A cleared
        // pixel makes GTK commit a transparent frame.
        let blank = gtk4::DrawingArea::builder()
            .content_width(1)
            .content_height(1)
            .can_target(false)
            .accessible_role(gtk4::AccessibleRole::Presentation)
            .build();
        blank.set_draw_func(|_, cr, _, _| {
            cr.set_operator(cairo::Operator::Clear);
            if let Err(err) = cr.paint() {
                tracing::warn!(error = %err, "cannot clear the empty notification popups");
            }
        });
        self.cards.append(&blank);
        self.set_input(false);
        // A surface that stops taking input may never see the pointer's `leave`.
        self.inside.set(false);
    }

    /// The pointer is over a popup of this window.
    pub(super) fn pointer_inside(&self) -> bool {
        self.inside.get()
    }

    /// The whole surface takes the pointer, or none of it does.
    // ponytail: GTK resets the input region of client-decorated windows only; a layer
    // surface is not one, so the region set here holds until the next call.
    fn set_input(&self, taking: bool) {
        let Some(surface) = self.window.surface() else {
            return;
        };
        // No region is the whole surface.
        let empty = cairo::Region::create();
        surface.set_input_region((!taking).then_some(&empty));
    }

    /// The output the window is pinned to.
    pub(super) fn on(&self, monitor: &gdk::Monitor) -> bool {
        self.monitor == *monitor
    }

    /// Lets the window go without destroying its layer surface (see `Surface::abandon`).
    /// The bar's own `invalidate` handler on the same monitor stops gtk4-layer-shell's.
    pub(super) fn abandon(self) {
        // Hidden first: a surface left mapped on another output must not keep the input
        // region its last `show` gave it.
        self.hide();
        self.window.set_child(None::<&gtk4::Widget>);
    }
}
