//! One output's dock surface (doc_bar.md, BR7): a layer surface on one edge holding the
//! island (the launcher and workspaces openers, the applications row, the application
//! library opener). Under auto-hide it reserves no space and shows a strip on its edge
//! until the pointer rests there (`athanor_dock::autohide`).

use std::cell::RefCell;
use std::rc::Rc;

use athanor_apps::openers;
use athanor_apps::row::Row;
use athanor_compositor_client::Opener;
use athanor_dock::autohide::{AutoHide, Event, Timer, STRIP_PX};
use athanor_dock::placement::{Anchor, Placement};
use gtk4::accessible::Property;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::Dock;
use crate::i18n::tr;
use crate::layer_guard;

/// The auto-hide state of one surface and its one timer.
struct Hider {
    window: gtk4::ApplicationWindow,
    edge: Edge,
    stack: gtk4::Stack,
    state: RefCell<AutoHide>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Hider {
    fn feed(self: &Rc<Self>, event: Event) {
        let timer = self.state.borrow_mut().feed(event);
        match timer {
            Timer::Keep => {}
            Timer::Cancel => self.cancel(),
            Timer::Start(delay) => {
                self.cancel();
                let weak = Rc::downgrade(self);
                let id = glib::timeout_add_local_once(delay, move || {
                    if let Some(hider) = weak.upgrade() {
                        // Fired: the id is spent, and removing it again would abort.
                        hider.timer.take();
                        hider.feed(Event::Timer);
                    }
                });
                self.timer.replace(Some(id));
            }
        }
        let shown = self.state.borrow().shown();
        self.show(shown);
    }

    /// Hidden, the surface is anchored to both ends of its edge, so the strip spans the
    /// whole edge and the pointer finds it anywhere along it; shown, it holds the island
    /// alone, centred. The anchors change in place: the layer surface is never recreated.
    fn show(&self, shown: bool) {
        self.stack
            .set_visible_child_name(if shown { "island" } else { "strip" });
        for side in across(self.edge) {
            self.window.set_anchor(side, !shown);
        }
    }

    fn cancel(&self) {
        if let Some(pending) = self.timer.take() {
            pending.remove();
        }
    }
}

impl Drop for Hider {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The two edges at the ends of `edge`.
fn across(edge: Edge) -> [Edge; 2] {
    match edge {
        Edge::Left | Edge::Right => [Edge::Top, Edge::Bottom],
        _ => [Edge::Left, Edge::Right],
    }
}

/// Feeds the surface's hider, if it has one. The hider is cloned out first: its timer
/// and the stack it switches may call back in.
fn feed(hider: &RefCell<Option<Rc<Hider>>>, event: Event) {
    let current = hider.borrow().clone();
    if let Some(hider) = current {
        hider.feed(event);
    }
}

pub struct Surface {
    window: gtk4::ApplicationWindow,
    /// The output this surface is on; see the bar's `Surface::monitor`.
    monitor: gdk::Monitor,
    /// The monitor's `invalidate` handler, which rebuilds the surfaces when the output
    /// leaves (see `new`).
    invalidated: glib::SignalHandlerId,
    /// Shared with the pointer handlers, which exist before any placement.
    hider: Rc<RefCell<Option<Rc<Hider>>>>,
    row: Option<Row>,
    /// `None`: the window is not on screen.
    placement: Option<Placement>,
}

impl Surface {
    /// A layer surface on `monitor`, not on screen until `place` gives it an edge.
    pub fn new(dock: &Rc<Dock>, monitor: &gdk::Monitor) -> Surface {
        let window = gtk4::ApplicationWindow::new(&dock.app);
        athanor_apps::timing::watch_layer(&window, "dock");
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!("athanor-dock: not a layer surface: {reason}");
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-dock"));
        window.set_layer(Layer::Top);
        window.set_monitor(Some(monitor));
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.set_resizable(false);
        for class in ["athanor-surface", "athanor-dock"] {
            window.add_css_class(class);
        }
        let hider: Rc<RefCell<Option<Rc<Hider>>>> = Rc::new(RefCell::new(None));
        let motion = gtk4::EventControllerMotion::new();
        let over = hider.clone();
        motion.connect_enter(move |_, _, _| feed(&over, Event::PointerIn));
        let over = hider.clone();
        motion.connect_leave(move |_| feed(&over, Event::PointerOut));
        window.add_controller(motion);
        let weak = Rc::downgrade(dock);
        window.connect_map(move |_| {
            if let Some(dock) = weak.upgrade() {
                dock.mapped();
            }
        });
        // As in the bar: gtk4-layer-shell answers an output's `invalidate` by destroying
        // the layer surface, which makes cosmic-comp close the connection. The emission
        // stops here and the rebuild, a debounce later, releases the surface.
        let weak = Rc::downgrade(dock);
        let invalidated = monitor.connect_invalidate(move |monitor| {
            monitor.stop_signal_emission_by_name("invalidate");
            if let Some(dock) = weak.upgrade() {
                tracing::info!("an output left; the dock surfaces are rebuilt");
                dock.schedule();
            }
        });
        Surface {
            window,
            monitor: monitor.clone(),
            invalidated,
            hider,
            row: None,
            placement: None,
        }
    }

    /// Moves the surface to `placement`, or off screen for `None`. The window and its
    /// monitor stay; an unchanged placement keeps the island and its open menu.
    pub fn place(&mut self, dock: &Rc<Dock>, placement: Option<Placement>) {
        if placement == self.placement {
            return;
        }
        self.placement = placement;
        self.hider.replace(None);
        self.row = None;
        for class in ["edge-bottom", "edge-left", "edge-right", "auto-hide"] {
            self.window.remove_css_class(class);
        }
        let Some(placement) = placement else {
            // cosmic-comp closes the connection of a client that unmaps a layer surface and
            // maps it again, so a surface on screen stays mapped: empty, one pixel, no
            // exclusive zone and no input on its child. This departs from BR7's "no
            // surface" (a declared limit in doc_bar.md) until cosmic-comp tolerates that.
            if self.window.is_mapped() {
                self.window.set_exclusive_zone(0);
                let empty = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
                empty.set_size_request(1, 1);
                empty.set_can_target(false);
                self.window.set_child(Some(&empty));
            }
            return;
        };
        let vertical = placement.anchor.vertical();
        let (edge, class, menu_position) = match placement.anchor {
            Anchor::Bottom => (Edge::Bottom, "edge-bottom", gtk4::PositionType::Top),
            Anchor::Left => (Edge::Left, "edge-left", gtk4::PositionType::Right),
            Anchor::Right => (Edge::Right, "edge-right", gtk4::PositionType::Left),
        };
        for anchor in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            self.window.set_anchor(anchor, anchor == edge);
        }
        self.window.add_css_class(class);
        if placement.auto_hide {
            self.window.add_css_class("auto-hide");
            self.window.set_exclusive_zone(0);
        } else {
            self.window.auto_exclusive_zone_enable();
        }
        let orientation = if vertical {
            gtk4::Orientation::Vertical
        } else {
            gtk4::Orientation::Horizontal
        };
        let island = gtk4::Box::new(orientation, 2);
        island.add_css_class("dock-island");
        island.update_property(&[Property::Label(&tr("Dock"))]);
        for opener in [Opener::Launcher, Opener::Workspaces] {
            if let Some(button) = openers::button(dock, opener) {
                island.append(&button);
            }
        }
        let row = Row::new(dock, orientation, menu_position);
        if let Some(row) = &row {
            island.append(&row.widget());
        }
        if let Some(button) = openers::button(dock, Opener::AppLibrary) {
            island.append(&button);
        }
        // The strip is STRIP_PX deep; hidden, the surface stretches it along the whole edge.
        let strip = gtk4::Box::new(orientation, 0);
        strip.add_css_class("dock-strip");
        if vertical {
            strip.set_size_request(STRIP_PX, -1);
        } else {
            strip.set_size_request(-1, STRIP_PX);
        }
        let stack = gtk4::Stack::new();
        stack.set_hhomogeneous(!vertical);
        stack.set_vhomogeneous(vertical);
        stack.add_named(&island, Some("island"));
        stack.add_named(&strip, Some("strip"));
        stack.set_visible_child_name("island");
        if placement.auto_hide {
            let hider = Rc::new(Hider {
                window: self.window.clone(),
                edge,
                stack: stack.clone(),
                state: RefCell::new(AutoHide::default()),
                timer: RefCell::new(None),
            });
            hider.show(false);
            self.hider.replace(Some(hider));
        }
        self.window.set_child(Some(&stack));
        self.row = row;
        self.window.present();
    }

    pub fn refresh(&self, dock: &Rc<Dock>) {
        if let Some(row) = &self.row {
            row.refresh(dock);
        }
    }

    pub fn hold(&self, held: bool) {
        feed(&self.hider, Event::Held(held));
    }

    pub fn placed(&self) -> bool {
        self.placement.is_some()
    }

    pub fn on(&self, monitor: &gdk::Monitor) -> bool {
        self.monitor == *monitor
    }

    /// A surface off screen is always reusable; one on screen only while realized.
    pub fn alive(&self) -> bool {
        !self.window.is_mapped() || self.window.is_realized()
    }

    /// Lets go of a surface whose output left, or that was torn down. One still on screen
    /// is emptied, not destroyed: cosmic-comp 1.8 closes the connection of a client that
    /// destroys a layer surface whose output left (see the bar's `Surface::abandon`).
    // ponytail: one empty window per output removal for the life of the process; destroy
    // it instead once cosmic-comp tolerates that.
    pub fn release(self) {
        self.hider.replace(None);
        self.monitor.disconnect(self.invalidated);
        if self.window.is_mapped() && self.window.is_realized() {
            self.window.set_child(None::<&gtk4::Widget>);
        } else {
            self.window.destroy();
        }
    }
}
