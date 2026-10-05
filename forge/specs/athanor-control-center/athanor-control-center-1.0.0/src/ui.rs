//! The panel on the surface of the focused output, placed against the bar (CC9). The control
//! pages are placeholders, a label per page id, until the later tasks fill them; the
//! notification panel is the real one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_compositor_client::{outputs, theme, Client};
use athanor_control_center::{
    anchor, focused_output, notifications_height, parse_target, toggle, Page, Panel, Side, Target,
    GAP, NOTIFICATIONS_WIDTH,
};
use athanor_layout::loader::{Paths, Source};
use athanor_services::Buses;
use athanor_style::calmo;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use tokio::runtime::Handle;

use crate::bus::Bus;
use crate::i18n::is_rtl;
use crate::notifications;
use crate::surface::Surface;

const CSS: &str = "
.control-center-surface { background: transparent; }
.control-center { border-radius: 16px; padding: 12px; }
.control-center .notification-card { padding: 10px; border-radius: 14px; box-shadow: none; }
.control-center .nc-heading { font-weight: bold; }
.control-center .nc-dim { opacity: 0.7; }
.control-center .nc-read .notification-title { font-weight: normal; }
.control-center .nc-unavailable { opacity: 0.5; }
";

/// The stack child that is the panel itself.
const PANEL: &str = "panel";
/// The stack child of the notification center (NC1).
const NOTIFICATIONS: &str = "notifications";
/// The control panel's width; the notification panel's is `NOTIFICATIONS_WIDTH`.
const CONTROLS_WIDTH: i32 = 360;

pub struct ControlCenter {
    app: gtk4::Application,
    display: gdk::Display,
    client: Option<Rc<Client>>,
    layout: Source,
    bus: Rc<Bus>,
    surfaces: RefCell<Vec<Surface>>,
    /// The monitor of the surface the panel is shown on.
    shown: RefCell<Option<gdk::Monitor>>,
    /// Which of the two panels is shown; `Some` exactly when `shown` is.
    panel: Cell<Option<Panel>>,
    /// The surface has had the keyboard since the panel was shown, so losing it now is a
    /// loss of focus and not the keyboard not having arrived yet.
    focused: Cell<bool>,
    content: gtk4::Box,
    stack: gtk4::Stack,
    notifications: Rc<notifications::Panel>,
    watches: RefCell<Vec<gtk4::gio::FileMonitor>>,
}

/// `services` is the runtime the models of athanor-services run on; without it the
/// notification panel says it is unavailable.
pub fn start(
    app: &gtk4::Application,
    bus: Rc<Bus>,
    layout: Source,
    services: Option<(Handle, Buses)>,
) -> Rc<ControlCenter> {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("athanor-control-center: no display");
        std::process::exit(1);
    };
    if is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }
    let cosmic = theme::read();
    calmo::load(&display, cosmic.variant());
    theme::load_accent(&display, &cosmic);
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let client = match Client::connect(&display) {
        Ok(client) => Some(Rc::new(client)),
        Err(err) => {
            tracing::error!(error = %err, "no compositor client; the panel opens on the first output");
            None
        }
    };
    let stack = gtk4::Stack::new();
    stack.add_named(&gtk4::Label::new(Some(PANEL)), Some(PANEL));
    let notifications = notifications::new(services, client.clone());
    stack.add_named(&notifications.root, Some(NOTIFICATIONS));
    for page in Page::ALL {
        stack.add_named(&gtk4::Label::new(Some(page.id())), Some(page.id()));
    }
    let content = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .width_request(CONTROLS_WIDTH)
        .build();
    content.add_css_class("control-center");
    content.add_css_class("background");
    content.append(&stack);
    let center = Rc::new(ControlCenter {
        app: app.clone(),
        display: display.clone(),
        client,
        layout,
        bus,
        surfaces: RefCell::default(),
        shown: RefCell::default(),
        panel: Cell::new(None),
        focused: Cell::new(false),
        content,
        stack,
        notifications,
        watches: RefCell::default(),
    });
    // The panel is as tall as its content: it asks again when the content changes.
    let weak = Rc::downgrade(&center);
    center.notifications.connect_resized(move || {
        if let Some(center) = weak.upgrade() {
            center.relimit();
        }
    });
    let weak = Rc::downgrade(&center);
    outputs::watch(&display, move |_| {
        if let Some(center) = weak.upgrade() {
            center.sync_surfaces();
        }
    });
    let watched = display.clone();
    center
        .watches
        .borrow_mut()
        .extend(theme::watch(move |cosmic| {
            calmo::load(&watched, cosmic.variant());
            theme::load_accent(&watched, &cosmic);
        }));
    center.sync_surfaces();
    center
}

impl ControlCenter {
    fn monitors(&self) -> Vec<gdk::Monitor> {
        let monitors = self.display.monitors();
        (0..monitors.n_items())
            .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
            .collect()
    }

    /// One surface per output, reused on the same monitor object; a surface whose output left
    /// is abandoned, and closes the panel if it showed it.
    fn sync_surfaces(self: &Rc<Self>) {
        let mut old = self.surfaces.take();
        let mut surfaces = Vec::new();
        for monitor in self.monitors() {
            match old
                .iter()
                .position(|surface| surface.monitor == monitor && surface.alive())
            {
                Some(pos) => surfaces.push(old.remove(pos)),
                None => surfaces.push(self.surface(&monitor)),
            }
        }
        for leftover in old {
            if self.shown.borrow().as_ref() == Some(&leftover.monitor) {
                self.shown.replace(None);
                self.panel.set(None);
                self.notifications.closed();
                self.focused.set(false);
                self.bus.set_open(false);
            }
            leftover.abandon();
        }
        self.surfaces.replace(surfaces);
    }

    fn surface(self: &Rc<Self>, monitor: &gdk::Monitor) -> Surface {
        let surface = Surface::new(&self.app, monitor);
        // A click outside the panel closes it (CC9). A hidden surface has an empty input
        // region and receives none.
        let click = gtk4::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |gesture, _, x, y| {
            let (Some(center), Some(window)) = (weak.upgrade(), gesture.widget()) else {
                return;
            };
            let inside = window
                .pick(x, y, gtk4::PickFlags::DEFAULT)
                .is_some_and(|picked| {
                    picked == *center.content.upcast_ref::<gtk4::Widget>()
                        || picked.is_ancestor(&center.content)
                });
            if !inside {
                center.hide();
            }
        });
        surface.window.add_controller(click);
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, value, _, _| match (value, weak.upgrade()) {
            (gdk::Key::Escape, Some(center)) => {
                center.hide();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        surface.window.add_controller(keys);
        // The output's size changes the most the notification panel may take.
        let weak = Rc::downgrade(self);
        let geometry = monitor.connect_notify_local(Some("geometry"), move |_, _| {
            if let Some(center) = weak.upgrade() {
                center.relimit();
            }
        });
        surface.geometry.replace(Some(geometry));
        // The loss of focus closes it too, once the surface has had the keyboard.
        let weak = Rc::downgrade(self);
        let monitor = monitor.clone();
        surface.window.connect_is_active_notify(move |window| {
            let Some(center) = weak.upgrade() else { return };
            if center.shown.borrow().as_ref() != Some(&monitor) {
                return;
            }
            if window.is_active() {
                center.focused.set(true);
            } else if center.focused.get() {
                center.hide();
            }
        });
        surface
    }

    /// Opens the asked panel, switches to it from the other, or closes it (NC1).
    pub fn toggle(self: &Rc<Self>, asked: Panel) {
        match toggle(self.panel.get(), asked) {
            None => self.hide(),
            Some(Panel::Controls) => self.show(Target::Controls(None)),
            Some(Panel::Notifications) => self.show(Target::Notifications(None)),
        }
    }

    /// Shows the panel, or the page `id` names; a shown panel moves to that page.
    pub fn show_page(self: &Rc<Self>, id: &str) -> Result<(), String> {
        let target = parse_target(id)?;
        self.show(target);
        if let Target::Notifications(Some(row)) = target {
            self.notifications.focus_reply(row);
        }
        Ok(())
    }

    fn show(self: &Rc<Self>, target: Target) {
        let (panel, child, width) = match target {
            Target::Controls(page) => (Panel::Controls, page.map_or(PANEL, Page::id), CONTROLS_WIDTH),
            Target::Notifications(_) => (Panel::Notifications, NOTIFICATIONS, NOTIFICATIONS_WIDTH),
        };
        self.stack.set_visible_child_name(child);
        self.content.set_width_request(width);
        // Both panels share the one surface and the one anchor (the bar's end edge), so
        // switching moves nothing but the size.
        self.content.set_height_request(-1);
        match panel {
            Panel::Notifications if self.panel.get() != Some(Panel::Notifications) => {
                self.notifications.opened()
            }
            Panel::Notifications => {}
            Panel::Controls => self.notifications.closed(),
        }
        if let (Panel::Notifications, Some(monitor)) = (panel, self.shown.borrow().as_ref()) {
            self.limit_height(monitor);
        }
        if self.panel.replace(Some(panel)).is_some() {
            return;
        }
        let windows = self
            .client
            .as_ref()
            .map(|client| client.windows())
            .unwrap_or_default();
        let surfaces = self.surfaces.borrow();
        let connectors: Vec<Option<String>> = surfaces
            .iter()
            .map(|surface| surface.monitor.connector().map(|name| name.to_string()))
            .collect();
        let activated = windows
            .iter()
            .find(|window| window.state.activated)
            .map(|window| window.outputs.as_slice());
        let Some(surface) = surfaces.get(focused_output(activated, &connectors)) else {
            tracing::error!("no output to show the control center on");
            return;
        };
        let place = anchor(self.layout.layout().panel(), is_rtl());
        let align = |side| match side {
            Side::Start => gtk4::Align::Start,
            Side::End => gtk4::Align::End,
        };
        self.content.set_valign(align(place.vertical));
        self.content.set_halign(align(place.horizontal));
        self.content.set_margin_top(GAP);
        self.content.set_margin_bottom(GAP);
        self.content.set_margin_start(GAP);
        self.content.set_margin_end(GAP);
        self.focused.set(false);
        if panel == Panel::Notifications {
            self.limit_height(&surface.monitor);
        }
        self.shown.replace(Some(surface.monitor.clone()));
        surface.show(self.content.upcast_ref());
        self.bus.set_open(true);
    }

    /// The notification panel is as tall as its content, at most 80% of the output (NC11).
    /// A minimum height does not cap a natural one, so what exceeds the cap is taken from the
    /// list, which scrolls. The measure includes the margins, which the request does not.
    fn limit_height(&self, monitor: &gdk::Monitor) {
        let margins = 2 * GAP;
        self.notifications.cap_list(0);
        self.content.set_height_request(-1);
        let natural = self
            .content
            .measure(gtk4::Orientation::Vertical, NOTIFICATIONS_WIDTH + margins)
            .1
            - margins;
        let height = notifications_height(natural, monitor.geometry().height());
        self.notifications.cap_list(natural - height);
        self.content.set_height_request(height);
    }

    /// Sizes the shown notification panel again, after its content or the output changed.
    fn relimit(&self) {
        if self.panel.get() != Some(Panel::Notifications) {
            return;
        }
        if let Some(monitor) = self.shown.borrow().as_ref() {
            self.limit_height(monitor);
        }
    }

    pub fn hide(&self) {
        let Some(monitor) = self.shown.take() else {
            return;
        };
        self.panel.set(None);
        self.notifications.closed();
        self.focused.set(false);
        if let Some(surface) = self
            .surfaces
            .borrow()
            .iter()
            .find(|surface| surface.monitor == monitor)
        {
            surface.hide();
        }
        self.bus.set_open(false);
    }
}

/// The layout source of a live session: the three layers of athanor-layout.
pub fn live_layout(config: &std::path::Path) -> Source {
    Source::Live(Paths::for_config_home(config))
}
