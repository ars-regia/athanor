//! One layer surface per output, created once, pinned to its output and never destroyed,
//! unmapped or moved (Global Constraints; Task 2). Hidden, it is a 1×1 background surface
//! with no child, no input and no keyboard; shown, an overlay over the whole output that
//! takes the keyboard.

use gtk4::prelude::*;
use gtk4::{cairo, gdk};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::layer_guard;

const EDGES: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

pub struct Surface {
    pub window: gtk4::ApplicationWindow,
    /// Kept only while this very monitor object is listed (see athanor-bar's ui/mod.rs).
    pub monitor: gdk::Monitor,
}

/// No input at all: a hidden surface takes no click and no touch.
fn empty_input(window: &gtk4::ApplicationWindow) {
    if let Some(surface) = window.surface() {
        surface.set_input_region(Some(&cairo::Region::create()));
    }
}

impl Surface {
    pub fn new(app: &gtk4::Application, monitor: &gdk::Monitor) -> Surface {
        // When an output leaves, gtk4-layer-shell answers its monitor's `invalidate` by destroying
        // the layer surface and creating another on the default output. cosmic-comp 1.8 closes
        // the connection of a client that destroys a layer surface whose output left (found by
        // the dev VM hotplug stage, with the launcher shown on that output), so the emission
        // stops here. The handler is never disconnected: the emission can follow the monitor
        // list's change, which has already abandoned the surface (see `abandon`).
        monitor.connect_invalidate(|monitor| monitor.stop_signal_emission_by_name("invalidate"));
        let window = gtk4::ApplicationWindow::new(app);
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!("athanor-launcher: not a layer surface: {reason}");
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-launcher"));
        window.set_monitor(Some(monitor));
        window.add_css_class("launcher-surface");
        let surface = Surface { window, monitor: monitor.clone() };
        // The input region belongs to the GdkSurface, which exists once the window is
        // realized; before that, `window.surface()` is None and the call would do nothing.
        surface.window.connect_realize(|window| {
            if window.child().is_none() {
                empty_input(window);
            }
        });
        surface.hide();
        surface.window.present();
        surface
    }

    pub fn alive(&self) -> bool {
        self.window.is_realized()
    }

    pub fn hide(&self) {
        self.window.set_child(None::<&gtk4::Widget>);
        self.window.set_layer(Layer::Background);
        self.window.set_keyboard_mode(KeyboardMode::None);
        for edge in EDGES {
            self.window.set_anchor(edge, false);
        }
        self.window.set_default_size(1, 1);
        empty_input(&self.window);
    }

    pub fn show(&self, content: &gtk4::Widget) {
        self.window.set_child(Some(content));
        self.window.set_layer(Layer::Overlay);
        for edge in EDGES {
            self.window.set_anchor(edge, true);
        }
        self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        if let Some(surface) = self.window.surface() {
            surface.set_input_region(None);
        }
    }

    /// The output left: the window is emptied and kept, never destroyed (the bar's
    /// `abandon`, for the same cosmic-comp 1.8 behaviour). A window that has shown keeps its
    /// renderer state, about 3.4 MB (measured: 23155 to 26613 kB PSS), because it is never
    /// unrealized; the dev VM hotplug stage's 8 MB gate therefore allows two such removals.
    // ponytail: one empty window per output removal for the life of the process; destroy
    // it instead once cosmic-comp tolerates that.
    pub fn abandon(self) {
        self.window.set_child(None::<&gtk4::Widget>);
    }
}
