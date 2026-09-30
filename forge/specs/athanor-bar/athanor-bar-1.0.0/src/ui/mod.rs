//! The bar's surfaces and modules (doc_bar.md, BR3, BR6, BR7). Everything here runs on the
//! GTK main thread. One layer surface per output; the preset and the panel edge come from
//! the layout document, resolved by the loader the chooser and the translator use, so a
//! key the policy marks mandatory holds here too. Every change applies live.

mod accessibility;
mod bus;
mod clock;
mod input;
mod logind;
mod menu;
mod network;
mod notifications;
mod openers;
mod popup;
mod popups;
mod power;
mod running;
mod tiling;
mod tray;

use std::cell::{Cell, RefCell};
use std::env;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use athanor_apps::favorites::Store;
use athanor_apps::Host;
use athanor_bar::order::{self, Module};
use athanor_compositor_client::{outputs, theme, Client, Event, Opener};
use athanor_layout::loader::Source;
use athanor_layout::placement::Output;
use athanor_layout::preset::{Layout, PanelEdge};
use athanor_style::calmo;
use athanor_unit::notify;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::i18n::{self, tr};
use crate::layer_guard;

/// What changed, so that each module refreshes only for what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Changed {
    Windows,
    Workspaces,
    Keyboard,
    Accessibility,
    Favorites,
    /// The notification service: its state, the list, the popups.
    Notifications,
    /// The tray: an item came, left, or changed.
    Tray,
    /// Once a second: the clock, and a time zone that changed.
    Tick,
}

impl Changed {
    pub const ALL: [Changed; 8] = [
        Changed::Windows,
        Changed::Workspaces,
        Changed::Keyboard,
        Changed::Accessibility,
        Changed::Favorites,
        Changed::Tick,
        Changed::Notifications,
        Changed::Tray,
    ];
}

/// One module on one surface. A module whose source is absent hides its widget (SH1).
pub trait ModuleUi {
    fn widget(&self) -> gtk4::Widget;
    fn refresh(&self, bar: &Rc<Bar>, changed: Changed);
    /// Opens the module's popover, for the captures of BR9 (`ATHANOR_BAR_OPEN`).
    fn open(&self, _bar: &Rc<Bar>) {}
}

/// One surface's content, before it is handed to a window: the centre box to set as the
/// child, its three groups (`fit_groups` walks them), and the flat module list (`Surface`
/// keeps it, `refresh` walks it).
type Content = (
    gtk4::CenterBox,
    [gtk4::Box; 3],
    Vec<(Module, Box<dyn ModuleUi>)>,
);

struct Surface {
    window: gtk4::ApplicationWindow,
    /// The output this surface is on. `rebuild` keeps a surface only while this very
    /// monitor object is still listed: an output that leaves and comes back, even under the
    /// same connector, is a new `GdkMonitor` and a new `wl_output`.
    monitor: gdk::Monitor,
    /// The output's connector, for the modules that follow one output (tiling); `None`
    /// when GDK cannot name it.
    connector: Option<String>,
    groups: [gtk4::Box; 3],
    modules: Vec<(Module, Box<dyn ModuleUi>)>,
    /// The monitor's `invalidate` handler, which rebuilds the surfaces when the output
    /// leaves (see `surface`).
    invalidated: glib::SignalHandlerId,
}

impl Surface {
    /// Still realized: a window torn down behind the bar's back is replaced, never reused.
    fn alive(&self) -> bool {
        self.window.is_realized()
    }

    fn destroy(self) {
        self.monitor.disconnect(self.invalidated);
        self.window.destroy();
    }

    /// Empties the window of an output that left and lets it go without destroying it:
    /// cosmic-comp 1.8 closes the connection of a client that destroys a layer surface whose
    /// output left, whenever it does so (seen in the dev VM with a bare gtk4-layer-shell
    /// client too). GTK's list of toplevels keeps the window.
    // ponytail: one empty window per output removal for the life of the process; destroy
    // it instead once cosmic-comp tolerates that.
    fn abandon(self) {
        self.monitor.disconnect(self.invalidated);
        self.window.set_child(None::<&gtk4::Widget>);
    }

    /// A group with nothing visible in it is not drawn: an empty island is a facade.
    fn fit_groups(&self) {
        for group in &self.groups {
            let mut child = group.first_child();
            let mut any = false;
            while let Some(widget) = child {
                any |= widget.is_visible();
                child = widget.next_sibling();
            }
            group.set_visible(any);
        }
    }
}

pub struct Bar {
    app: gtk4::Application,
    _hold: gio::ApplicationHoldGuard,
    display: gdk::Display,
    client: Option<Client>,
    source: Source,
    layout: Cell<Layout>,
    outputs: RefCell<Vec<Output>>,
    favorites: Store,
    surfaces: RefCell<Vec<Surface>>,
    watches: RefCell<Vec<gio::FileMonitor>>,
    debounce: RefCell<Option<glib::SourceId>>,
    open_popover: RefCell<Option<gtk4::Popover>>,
    ready: Cell<bool>,
    open_on_start: Option<Module>,
    /// One service per bar, not per surface: every surface's button reads it.
    notifications: Rc<notifications::Service>,
    /// One tray host per bar: the watcher knows the bar as a single host.
    tray: Rc<tray::Host>,
    /// The bar itself, for the `&self` methods that defer work to an idle.
    me: Weak<Bar>,
}

pub fn start(app: &gtk4::Application, source: Source, favorites_file: Option<PathBuf>) -> Rc<Bar> {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("no display");
        std::process::exit(1);
    };
    if i18n::is_rtl() {
        gtk4::Widget::set_default_direction(gtk4::TextDirection::Rtl);
    }
    let cosmic = theme::read();
    calmo::load(&display, cosmic.variant());
    theme::load_accent(&display, &cosmic);
    let client = match Client::connect(&display) {
        Ok(client) => Some(client),
        Err(err) => {
            tracing::error!(error = %err, "no compositor client; the modules that need it stay hidden");
            None
        }
    };
    let bar = Rc::new_cyclic(|weak: &Weak<Bar>| Bar {
        app: app.clone(),
        _hold: app.hold(),
        outputs: RefCell::new(outputs::current(&display)),
        display,
        client,
        layout: Cell::new(source.layout()),
        source,
        favorites: Store::load(favorites_file),
        surfaces: RefCell::new(Vec::new()),
        watches: RefCell::new(Vec::new()),
        debounce: RefCell::new(None),
        open_popover: RefCell::new(None),
        ready: Cell::new(false),
        open_on_start: env::var("ATHANOR_BAR_OPEN")
            .ok()
            .and_then(|id| Module::from_id(&id)),
        me: weak.clone(),
        notifications: notifications::Service::start(weak),
        tray: tray::Host::start(weak),
    });
    bar.rebuild();
    if bar.surfaces.borrow().is_empty() {
        // No output yet: ready all the same, and the surfaces come with the first output.
        bar.mapped();
    }
    bar.watch();
    bar
}

fn changed_by(event: &Event) -> Changed {
    match event {
        Event::WindowAdded(_) | Event::WindowChanged(_) | Event::WindowRemoved(_) => {
            Changed::Windows
        }
        Event::WorkspaceAdded(_) | Event::WorkspaceChanged(_) | Event::WorkspaceRemoved(_) => {
            Changed::Workspaces
        }
        Event::KeyboardLayouts(_) | Event::KeyboardGroup(_) => Changed::Keyboard,
        Event::Accessibility(_) => Changed::Accessibility,
    }
}

/// Builds `module` for the surface on `connector`; `None` when it is not built, or its
/// source is absent.
fn build(module: Module, bar: &Rc<Bar>, connector: Option<&str>) -> Option<Box<dyn ModuleUi>> {
    match module {
        Module::Launcher => openers::new(bar, Opener::Launcher),
        Module::AppLibrary => openers::new(bar, Opener::AppLibrary),
        Module::Workspaces => openers::new(bar, Opener::Workspaces),
        Module::Clock => clock::new(bar),
        Module::Power => power::new(bar),
        Module::InputSource => input::new(bar),
        Module::Tiling => tiling::new(bar, connector),
        Module::Accessibility => accessibility::new(bar),
        Module::RunningApps => running::new(bar),
        Module::Notifications => notifications::new(bar),
        Module::Tray => tray::new(bar),
        Module::Network => network::new(bar),
        // Later tasks of this plan, and the plans of 2b.3 to 2b.5.
        _ => None,
    }
}

impl Bar {
    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    pub fn layout(&self) -> Layout {
        self.layout.get()
    }

    /// BR6: at most one popover of the bar is open; opening one closes the other.
    pub fn popover_opened(&self, popover: &gtk4::Popover) {
        let previous = self.open_popover.replace(Some(popover.clone()));
        if let Some(previous) = previous.filter(|previous| previous != popover) {
            previous.popdown();
        }
    }

    /// A popover of the bar is on screen (BR6, "Stacking").
    pub fn popover_is_open(&self) -> bool {
        self.open_popover
            .borrow()
            .as_ref()
            .is_some_and(|popover| popover.is_visible())
    }

    /// Opens `module`'s popover on the first surface, for the captures of BR9
    /// (`ATHANOR_BAR_OPEN`). A module whose source answers later opens itself then.
    pub fn open_module(self: &Rc<Self>, module: Module) {
        let surfaces = self.surfaces.borrow();
        let target = surfaces
            .first()
            .and_then(|surface| surface.modules.iter().find(|(m, _)| *m == module));
        match target {
            Some((_, ui)) => ui.open(self),
            None => tracing::warn!(
                module = module.id(),
                "ATHANOR_BAR_OPEN names a module the bar does not show"
            ),
        }
    }

    /// A popover of the bar opened or closed: the notification popups hide or come back
    /// (BR6, "Stacking"). The shield sheet of 2b.5 calls it too (item 13).
    pub fn popovers_changed(&self) {
        self.notifications.redraw_popups();
    }

    /// `popovers_changed` on the next idle, once GTK has settled the visibility that
    /// `popover_is_open` reads: a popover about to pop up is not visible yet in this turn.
    pub fn popovers_changed_later(&self) {
        let bar = self.me.clone();
        glib::idle_add_local_once(move || {
            if let Some(bar) = bar.upgrade() {
                bar.popovers_changed();
            }
        });
    }

    pub fn refresh(self: &Rc<Self>, changed: Changed) {
        for surface in self.surfaces.borrow().iter() {
            for (_, module) in &surface.modules {
                module.refresh(self, changed);
            }
            surface.fit_groups();
        }
    }

    /// Refits every surface's groups after a module showed or hid itself outside `refresh`:
    /// the system modules follow their service's signals, not the bar's events.
    pub fn fit_groups(&self) {
        for surface in self.surfaces.borrow().iter() {
            surface.fit_groups();
        }
    }

    /// The outputs now, as GDK's monitor objects.
    fn monitors(&self) -> Vec<gdk::Monitor> {
        let monitors = self.display.monitors();
        (0..monitors.n_items())
            .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
            .collect()
    }

    /// One live surface per current output, each on that very monitor object.
    fn surfaces_current(&self) -> bool {
        let monitors = self.monitors();
        let surfaces = self.surfaces.borrow();
        surfaces.len() == monitors.len()
            && surfaces
                .iter()
                .zip(&monitors)
                .all(|(surface, monitor)| surface.monitor == *monitor && surface.alive())
    }

    /// Diffs the outputs against the live surfaces by monitor object, so a layout or
    /// module change (by far the common case a live reload applies) reconfigures the
    /// existing layer surfaces in place instead of destroying and recreating them: a
    /// compositor that tears its own reservation down before the destroy is acknowledged
    /// closes the connection outright, and this was the reload's untested path (the `Rc`
    /// that used to keep the bar alive was dropped before any second rebuild could ever
    /// run - see `start`). A surface is reused only on the same monitor object and while
    /// still on screen; a new output, or one that came back as a new monitor, gets a new
    /// surface, and a surface whose output left goes.
    fn rebuild(self: &Rc<Self>) {
        self.open_popover.take();
        let layout = self.layout.get();
        let mut old = self.surfaces.take();
        let mut surfaces = Vec::new();
        for monitor in self.monitors() {
            let reused = old
                .iter()
                .position(|surface| surface.monitor == monitor && surface.alive())
                .map(|pos| old.remove(pos));
            surfaces.push(match reused {
                Some(surface) => self.update_surface(surface, layout),
                None => self.surface(&monitor, layout),
            });
        }
        // Every surface on a listed monitor and still realized was reused: one still
        // realized here is on an output that left.
        for leftover in old {
            if leftover.alive() {
                leftover.abandon();
            } else {
                leftover.destroy();
            }
        }
        self.surfaces.replace(surfaces);
        for changed in Changed::ALL {
            self.refresh(changed);
        }
        self.popovers_changed();
    }

    /// The module row for one surface, in visual order (`order::visual`): the left,
    /// centre and right groups plus the flat module list `fit_groups` and `refresh` walk.
    fn build_content(self: &Rc<Self>, connector: Option<&str>, layout: Layout) -> Content {
        let row = order::visual(layout.preset(), i18n::is_rtl());
        let mut modules = Vec::new();
        let mut group = |list: &[Module], name: &str| {
            let group = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
            // The row is already in visual order (order::visual): GTK must not mirror it again.
            group.set_direction(gtk4::TextDirection::Ltr);
            group.add_css_class("bar-group");
            group.add_css_class(name);
            for &module in list {
                if let Some(ui) = build(module, self, connector) {
                    group.append(&ui.widget());
                    modules.push((module, ui));
                }
            }
            group
        };
        let groups = [
            group(&row.left, "left"),
            group(&row.centre, "centre"),
            group(&row.right, "right"),
        ];
        let centre_box = gtk4::CenterBox::new();
        centre_box.set_direction(gtk4::TextDirection::Ltr);
        centre_box.set_start_widget(Some(&groups[0]));
        centre_box.set_center_widget(Some(&groups[1]));
        centre_box.set_end_widget(Some(&groups[2]));
        (centre_box, groups, modules)
    }

    fn surface(self: &Rc<Self>, monitor: &gdk::Monitor, layout: Layout) -> Surface {
        let connector = monitor.connector().map(|name| name.to_string());
        let window = gtk4::ApplicationWindow::new(&self.app);
        window.init_layer_shell();
        if let Err(reason) = layer_guard::require_layer_surface(&window) {
            tracing::error!("athanor-bar: not a layer surface: {reason}");
            std::process::exit(1);
        }
        window.set_namespace(Some("athanor-bar"));
        window.set_layer(Layer::Top);
        window.set_monitor(Some(monitor));
        let (edge, edge_class) = match layout.panel() {
            PanelEdge::Top => (Edge::Top, "edge-top"),
            PanelEdge::Bottom => (Edge::Bottom, "edge-bottom"),
        };
        for anchor in [edge, Edge::Left, Edge::Right] {
            window.set_anchor(anchor, true);
        }
        window.auto_exclusive_zone_enable();
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        for class in ["athanor-surface", "athanor-bar", edge_class] {
            window.add_css_class(class);
        }
        window.add_css_class(&format!("preset-{}", layout.preset().id()));

        let (centre_box, groups, modules) = self.build_content(connector.as_deref(), layout);
        window.set_child(Some(&centre_box));
        let weak = Rc::downgrade(self);
        window.connect_map(move |_| {
            if let Some(bar) = weak.upgrade() {
                bar.mapped();
            }
        });
        // When an output leaves, gtk4-layer-shell 1.3 answers its monitor's `invalidate`
        // (connected after, so that the application goes first) by moving the window to no
        // monitor: it destroys the layer surface and creates another one on the default
        // output. cosmic-comp closes the connection of a client that destroys a layer
        // surface of an output that left (see `Surface::abandon`), so the emission stops
        // here and the rebuild, a debounce later, lets the surface go.
        let weak = Rc::downgrade(self);
        let invalidated = monitor.connect_invalidate(move |monitor| {
            monitor.stop_signal_emission_by_name("invalidate");
            if let Some(bar) = weak.upgrade() {
                tracing::info!("an output left; the bar surfaces are rebuilt");
                bar.notifications.output_left();
                bar.schedule();
            }
        });
        window.present();
        Surface {
            window,
            monitor: monitor.clone(),
            connector,
            groups,
            modules,
            invalidated,
        }
    }

    /// Reconfigures a surface already on screen for a new layout: the panel edge (the
    /// only anchor that ever changes), the preset's CSS class, and the module row. The
    /// window, its monitor and its layer surface are never touched, which is the point
    /// (see `rebuild`): gtk4-layer-shell recreates the layer surface for a new monitor.
    fn update_surface(self: &Rc<Self>, surface: Surface, layout: Layout) -> Surface {
        let Surface {
            window,
            monitor,
            connector,
            invalidated,
            ..
        } = surface;
        let (edge, edge_class) = match layout.panel() {
            PanelEdge::Top => (Edge::Top, "edge-top"),
            PanelEdge::Bottom => (Edge::Bottom, "edge-bottom"),
        };
        window.set_anchor(Edge::Top, edge == Edge::Top);
        window.set_anchor(Edge::Bottom, edge == Edge::Bottom);
        window.auto_exclusive_zone_enable();
        window.remove_css_class("edge-top");
        window.remove_css_class("edge-bottom");
        window.add_css_class(edge_class);
        for class in window.css_classes() {
            if class.starts_with("preset-") {
                window.remove_css_class(&class);
            }
        }
        window.add_css_class(&format!("preset-{}", layout.preset().id()));

        let (centre_box, groups, modules) = self.build_content(connector.as_deref(), layout);
        window.set_child(Some(&centre_box));
        Surface {
            window,
            monitor,
            connector,
            groups,
            modules,
            invalidated,
        }
    }

    /// READY=1 once, at the first surface on screen (Type=notify), then the popover the
    /// captures asked for.
    fn mapped(self: &Rc<Self>) {
        if self.ready.replace(true) {
            return;
        }
        if let Err(err) = notify::notify_ready() {
            tracing::error!(error = %err, "cannot tell systemd the bar is ready");
        }
        if let Some(module) = self.open_on_start {
            let weak = Rc::downgrade(self);
            glib::idle_add_local_once(move || {
                if let Some(bar) = weak.upgrade() {
                    bar.open_module(module);
                }
            });
        }
    }

    fn watch(self: &Rc<Self>) {
        if let Some(client) = &self.client {
            let weak = Rc::downgrade(self);
            client.connect_events(move |_, event| {
                if let Some(bar) = weak.upgrade() {
                    bar.refresh(changed_by(event));
                }
            });
        }
        let weak = Rc::downgrade(self);
        outputs::watch(&self.display, move |_| {
            if let Some(bar) = weak.upgrade() {
                bar.schedule();
            }
        });
        for dir in self.source.watched() {
            // A directory that does not exist yet is watched all the same.
            match gio::File::for_path(&dir)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => {
                    let weak = Rc::downgrade(self);
                    monitor.connect_changed(move |_, _, _, _| {
                        if let Some(bar) = weak.upgrade() {
                            bar.schedule();
                        }
                    });
                    self.watches.borrow_mut().push(monitor);
                }
                Err(err) => {
                    tracing::warn!(error = %err, dir = %dir.display(), "cannot watch; changes there apply at the next start");
                }
            }
        }
        let display = self.display.clone();
        let theme_watches = theme::watch(move |cosmic| {
            calmo::load(&display, cosmic.variant());
            theme::load_accent(&display, &cosmic);
        });
        self.watches.borrow_mut().extend(theme_watches);

        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(1, move || match weak.upgrade() {
            Some(bar) => {
                bar.refresh(Changed::Tick);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    /// Editors write a file in several steps: act 250 ms after the last event.
    fn schedule(self: &Rc<Self>) {
        if let Some(pending) = self.debounce.take() {
            pending.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(bar) = weak.upgrade() {
                // Fired: the id is spent, and removing it again would abort.
                bar.debounce.take();
                bar.reload();
            }
        });
        self.debounce.replace(Some(id));
    }

    fn reload(self: &Rc<Self>) {
        let layout = self.source.layout();
        let outputs = outputs::current(&self.display);
        // The snapshot catches a change of size or connector; `surfaces_current` an output
        // that left and came back within the debounce, and a surface whose output left.
        let moved = layout != self.layout.get()
            || outputs != *self.outputs.borrow()
            || !self.surfaces_current();
        self.layout.set(layout);
        self.outputs.replace(outputs);
        self.favorites.reload();
        if moved {
            self.rebuild();
        } else {
            self.refresh(Changed::Favorites);
        }
    }
}

impl Host for Bar {
    const APP: &'static str = "athanor-bar";
    const REORDER: bool = false;

    fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    fn favorites(&self) -> &Store {
        &self.favorites
    }

    fn pin_label(&self, pinned: bool) -> String {
        if pinned {
            tr("Unpin from Bar")
        } else {
            tr("Pin to Bar")
        }
    }

    fn menu_opened(&self, menu: &gtk4::Popover) {
        self.popover_opened(menu);
        // The row calls this before `popup()`: the menu is visible only after this turn.
        self.popovers_changed_later();
    }

    fn hold(&self, held: bool) {
        // A row's menu closed (the row calls this from an idle after `closed`) or a drag
        // ended: the popups come back unless another popover is open.
        if !held {
            self.popovers_changed();
        }
    }

    fn refresh_rows(self: &Rc<Self>) {
        self.refresh(Changed::Favorites);
    }
}
