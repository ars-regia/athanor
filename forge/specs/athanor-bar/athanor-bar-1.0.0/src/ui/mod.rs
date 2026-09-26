//! The bar's surfaces and modules (doc_bar.md, BR3, BR6, BR7). Everything here runs on the
//! GTK main thread. One layer surface per output; the preset and the panel edge come from
//! the layout document, resolved by the loader the chooser and the translator use, so a
//! key the policy marks mandatory holds here too. Every change applies live.

mod clock;
mod openers;
mod popup;

use std::cell::{Cell, RefCell};
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use athanor_bar::order::{self, Module};
use athanor_compositor_client::{outputs, theme, Client, Event, Opener};
use athanor_layout::favorites;
use athanor_layout::loader::{self, Paths};
use athanor_layout::placement::Output;
use athanor_layout::preset::{Layout, PanelEdge};
use athanor_style::calmo;
use athanor_unit::notify;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::i18n;
use crate::layer_guard;

/// Where the layout comes from: the three layers, or the vendor layer alone after a
/// crash-loop give-up (SH8).
#[derive(Clone)]
pub enum Source {
    Live(Paths),
    Vendor(PathBuf),
}

impl Source {
    fn layout(&self) -> Layout {
        match self {
            Source::Live(paths) => loader::resolve(paths).layout,
            Source::Vendor(dir) => loader::vendor_layout(dir),
        }
    }

    /// The directories whose changes can change the layout. The user's directory also
    /// holds the favourites file.
    fn watched(&self) -> Vec<PathBuf> {
        match self {
            Source::Live(paths) => {
                let mut dirs = vec![paths.vendor_dir.clone(), paths.policy_dir.clone()];
                dirs.extend(paths.user_file.parent().map(Path::to_path_buf));
                dirs
            }
            Source::Vendor(dir) => vec![dir.clone()],
        }
    }
}

/// What changed, so that each module refreshes only for what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Changed {
    Windows,
    Workspaces,
    Keyboard,
    Accessibility,
    Favorites,
    /// Once a second: the clock, and a time zone that changed.
    Tick,
}

impl Changed {
    pub const ALL: [Changed; 6] = [
        Changed::Windows,
        Changed::Workspaces,
        Changed::Keyboard,
        Changed::Accessibility,
        Changed::Favorites,
        Changed::Tick,
    ];
}

/// One module on one surface. A module whose source is absent hides its widget (SH1).
pub trait ModuleUi {
    fn widget(&self) -> gtk4::Widget;
    fn refresh(&self, bar: &Rc<Bar>, changed: Changed);
    /// Opens the module's popover, for the captures of BR9 (`ATHANOR_BAR_OPEN`).
    fn open(&self, _bar: &Rc<Bar>) {}
}

enum Favorites {
    // ponytail: no payload until Task 8's `Bar::favorites()` reads it (deferred-code.md);
    // carrying the ids here now would be a write-only field, which clippy already caught once.
    Loaded,
    /// A rejected file, a give-up, or no configuration directory: nothing is pinned or
    /// unpinned, and the file is never replaced.
    Unavailable,
}

struct Surface {
    window: gtk4::ApplicationWindow,
    groups: [gtk4::Box; 3],
    modules: Vec<(Module, Box<dyn ModuleUi>)>,
}

impl Surface {
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
    favorites_file: Option<PathBuf>,
    layout: Cell<Layout>,
    outputs: RefCell<Vec<Output>>,
    favorites: RefCell<Favorites>,
    surfaces: RefCell<Vec<Surface>>,
    watches: RefCell<Vec<gio::FileMonitor>>,
    debounce: RefCell<Option<glib::SourceId>>,
    open_popover: RefCell<Option<gtk4::Popover>>,
    ready: Cell<bool>,
    open_on_start: Option<Module>,
}

pub fn start(app: &gtk4::Application, source: Source, favorites_file: Option<PathBuf>) {
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
    let favorites = load_favorites(favorites_file.as_deref());
    let bar = Rc::new(Bar {
        app: app.clone(),
        _hold: app.hold(),
        outputs: RefCell::new(outputs::current(&display)),
        display,
        client,
        layout: Cell::new(source.layout()),
        source,
        favorites_file,
        favorites: RefCell::new(favorites),
        surfaces: RefCell::new(Vec::new()),
        watches: RefCell::new(Vec::new()),
        debounce: RefCell::new(None),
        open_popover: RefCell::new(None),
        ready: Cell::new(false),
        open_on_start: env::var("ATHANOR_BAR_OPEN")
            .ok()
            .and_then(|id| Module::from_id(&id)),
    });
    bar.rebuild();
    if bar.surfaces.borrow().is_empty() {
        // No output yet: ready all the same, and the surfaces come with the first output.
        bar.mapped();
    }
    bar.watch();
}

/// The favourites at start. `Favorites::Unavailable` when there is no configuration
/// directory, the file is rejected, or a rejected file cannot be told from one whose
/// import failed to save: either way nothing is pinned or unpinned, and the file on disk
/// is left exactly as `favorites::load_or_import` left it.
fn load_favorites(file: Option<&Path>) -> Favorites {
    let Some(file) = file else {
        return Favorites::Unavailable;
    };
    match favorites::load_or_import(
        file,
        athanor_compositor_client::favorites::cosmic_favorites,
        Path::new(favorites::VENDOR_FILE),
    ) {
        Ok(_ids) => Favorites::Loaded,
        Err(err) => {
            tracing::error!(error = %err, file = %file.display(), "the favourites are unavailable; the file is left as it is");
            Favorites::Unavailable
        }
    }
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
fn build(module: Module, bar: &Rc<Bar>, _connector: Option<&str>) -> Option<Box<dyn ModuleUi>> {
    match module {
        Module::Launcher => openers::new(bar, Opener::Launcher),
        Module::AppLibrary => openers::new(bar, Opener::AppLibrary),
        Module::Workspaces => openers::new(bar, Opener::Workspaces),
        Module::Clock => clock::new(bar),
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

    pub fn refresh(self: &Rc<Self>, changed: Changed) {
        for surface in self.surfaces.borrow().iter() {
            for (_, module) in &surface.modules {
                module.refresh(self, changed);
            }
            surface.fit_groups();
        }
    }

    fn rebuild(self: &Rc<Self>) {
        self.open_popover.take();
        for surface in self.surfaces.take() {
            surface.window.destroy();
        }
        let layout = self.layout.get();
        let monitors = self.display.monitors();
        let mut surfaces = Vec::new();
        for index in 0..monitors.n_items() {
            let Some(monitor) = monitors.item(index).and_downcast::<gdk::Monitor>() else {
                continue;
            };
            let connector = monitor.connector().map(|name| name.to_string());
            surfaces.push(self.surface(&monitor, connector.as_deref(), layout));
        }
        self.surfaces.replace(surfaces);
        for changed in Changed::ALL {
            self.refresh(changed);
        }
    }

    fn surface(
        self: &Rc<Self>,
        monitor: &gdk::Monitor,
        connector: Option<&str>,
        layout: Layout,
    ) -> Surface {
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
        window.set_child(Some(&centre_box));
        let weak = Rc::downgrade(self);
        window.connect_map(move |_| {
            if let Some(bar) = weak.upgrade() {
                bar.mapped();
            }
        });
        window.present();
        Surface {
            window,
            groups,
            modules,
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
                let Some(bar) = weak.upgrade() else { return };
                let surfaces = bar.surfaces.borrow();
                let target = surfaces
                    .first()
                    .and_then(|surface| surface.modules.iter().find(|(m, _)| *m == module));
                match target {
                    Some((_, ui)) => ui.open(&bar),
                    None => tracing::warn!(
                        module = module.id(),
                        "ATHANOR_BAR_OPEN names a module the bar does not show"
                    ),
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
        let moved = layout != self.layout.get() || outputs != *self.outputs.borrow();
        self.layout.set(layout);
        self.outputs.replace(outputs);
        if let Some(file) = &self.favorites_file {
            let now = match favorites::read(file) {
                Ok(_ids) => Favorites::Loaded,
                Err(err) => {
                    tracing::error!(error = %err, file = %file.display(), "the favourites are unavailable; the file is left as it is");
                    Favorites::Unavailable
                }
            };
            self.favorites.replace(now);
        }
        if moved {
            self.rebuild();
        } else {
            self.refresh(Changed::Favorites);
        }
    }
}
