//! The panel on the surface of the focused output, placed against the bar (CC9). The pages
//! are placeholders: a label per page id, until the later tasks fill them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_compositor_client::{outputs, theme, Client};
use athanor_control_center::{anchor, focused_output, parse_page, Page, Side, GAP};
use athanor_layout::loader::{Paths, Source};
use athanor_style::calmo;
use gtk4::prelude::*;
use gtk4::{gdk, glib};

use crate::bus::Bus;
use crate::i18n::is_rtl;
use crate::panel::{Hooks, Panel};
use crate::surface::Surface;

const CSS: &str = "
.control-center-surface { background: transparent; }
.control-center { border-radius: 16px; padding: 12px; }
";


pub struct ControlCenter {
    app: gtk4::Application,
    display: gdk::Display,
    client: Option<Rc<Client>>,
    layout: Source,
    bus: Rc<Bus>,
    surfaces: RefCell<Vec<Surface>>,
    /// The monitor of the surface the panel is shown on.
    shown: RefCell<Option<gdk::Monitor>>,
    /// The surface has had the keyboard since the panel was shown, so losing it now is a
    /// loss of focus and not the keyboard not having arrived yet.
    focused: Cell<bool>,
    content: gtk4::Box,
    panel: Rc<Panel>,
    watches: RefCell<Vec<gtk4::gio::FileMonitor>>,
}

pub fn start(
    app: &gtk4::Application,
    bus: Rc<Bus>,
    layout: Source,
    rfkill: Option<Rc<crate::panel::Rfkill>>,
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
    let panel = Panel::new(athanor_controls::bridge::services(), rfkill, hooks(client.clone()));
    panel.set_dark(cosmic.is_dark);
    let content = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .width_request(360)
        .build();
    content.add_css_class("control-center");
    content.add_css_class("background");
    content.append(&panel.widget());
    let center = Rc::new(ControlCenter {
        app: app.clone(),
        display: display.clone(),
        client,
        layout,
        bus,
        surfaces: RefCell::default(),
        shown: RefCell::default(),
        focused: Cell::new(false),
        content,
        panel,
        watches: RefCell::default(),
    });
    let weak = Rc::downgrade(&center);
    outputs::watch(&display, move |_| {
        if let Some(center) = weak.upgrade() {
            center.sync_surfaces();
        }
    });
    let watched = display.clone();
    let weak = Rc::downgrade(&center);
    center
        .watches
        .borrow_mut()
        .extend(theme::watch(move |cosmic| {
            calmo::load(&watched, cosmic.variant());
            theme::load_accent(&watched, &cosmic);
            if let Some(center) = weak.upgrade() {
                center.panel.set_dark(cosmic.is_dark);
            }
        }));
    let weak = Rc::downgrade(&center);
    center.panel.set_closer(move || {
        if let Some(center) = weak.upgrade() {
            center.hide();
        }
    });
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
                // Exclusive keyboard mode never reports a loss of focus, so once the surface
                // has the keyboard it asks for it on demand instead (CC9).
                crate::surface::relax_to_on_demand(window);
            } else if center.focused.get() {
                center.hide();
            }
        });
        surface
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.shown.borrow().is_some() {
            self.hide();
        } else {
            self.show(None);
        }
    }

    /// Shows the panel, or the page `id` names; a shown panel moves to that page.
    pub fn show_page(self: &Rc<Self>, id: &str) -> Result<(), String> {
        let page = parse_page(id)?;
        self.show(page);
        Ok(())
    }

    fn show(self: &Rc<Self>, page: Option<Page>) {
        self.panel.show(page);
        if self.shown.borrow().is_some() {
            return;
        }
        let windows = self
            .client
            .as_deref()
            .map(Client::windows)
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
        self.shown.replace(Some(surface.monitor.clone()));
        surface.show(self.content.upcast_ref());
        self.panel.set_shown(true);
        if page.is_none() {
            self.panel.focus_first();
        }
        self.bus.set_open(true);
    }

    pub fn hide(&self) {
        let Some(monitor) = self.shown.take() else {
            return;
        };
        self.focused.set(false);
        self.panel.set_shown(false);
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

/// What the panel asks of the program: Settings through the compositor client, and the
/// theme mode written where cosmic-settings writes it.
fn hooks(client: Option<Rc<Client>>) -> Hooks {
    Hooks {
        open_settings: Box::new(move || {
            let Some(client) = client.clone() else {
                tracing::warn!("no compositor client; Settings is not opened");
                return;
            };
            let Some(app) = gio_unix::DesktopAppInfo::new(SETTINGS) else {
                tracing::warn!("{SETTINGS} is not installed");
                return;
            };
            glib::spawn_future_local(async move {
                if let Err(err) = client.launch(&app).await {
                    tracing::warn!(error = %err, "Settings did not start");
                }
            });
        }),
        set_dark: Box::new(theme::set_dark),
    }
}

const SETTINGS: &str = "com.system76.CosmicSettings.desktop";
