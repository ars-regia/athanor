//! The dock's surfaces (doc_bar.md, BR7): one per output, placed by
//! `athanor_dock::placement::place` and rebuilt when the layout or the outputs change;
//! the favourites and the windows only refresh the rows.

mod surface;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use athanor_apps::favorites::Store;
use athanor_apps::Host;
use athanor_compositor_client::{outputs, theme, Client, Event};
use athanor_dock::placement;
use athanor_layout::loader::Source;
use athanor_layout::placement::Output;
use athanor_layout::preset::Layout;
use athanor_style::calmo;
use athanor_unit::notify;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::i18n::{self, tr};
use surface::Surface;

pub struct Dock {
    app: gtk4::Application,
    _hold: gio::ApplicationHoldGuard,
    display: gdk::Display,
    client: Option<Client>,
    source: Source,
    favorites: Store,
    layout: Cell<Layout>,
    outputs: RefCell<Vec<Output>>,
    surfaces: RefCell<Vec<Surface>>,
    watches: RefCell<Vec<gio::FileMonitor>>,
    debounce: RefCell<Option<glib::SourceId>>,
    open_popover: RefCell<Option<gtk4::Popover>>,
    ready: Cell<bool>,
}

pub fn start(app: &gtk4::Application, source: Source, favorites_file: Option<PathBuf>) -> Rc<Dock> {
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
    let layout = source.layout();
    tracing::info!(layout = ?layout, "layout applied");
    let client = match Client::connect(&display) {
        Ok(client) => Some(client),
        Err(err) => {
            // No windows to list and no secure launch (SH1, BR2): no dock surface at all.
            tracing::error!(error = %err, "no compositor client; the dock shows no surface");
            None
        }
    };
    let dock = Rc::new(Dock {
        app: app.clone(),
        _hold: app.hold(),
        outputs: RefCell::new(outputs::current(&display)),
        display,
        client,
        layout: Cell::new(layout),
        source,
        favorites: Store::load(favorites_file),
        surfaces: RefCell::new(Vec::new()),
        watches: RefCell::new(Vec::new()),
        debounce: RefCell::new(None),
        open_popover: RefCell::new(None),
        ready: Cell::new(false),
    });
    dock.rebuild();
    if !dock.surfaces.borrow().iter().any(Surface::placed) {
        // Nothing to map (the knob is off, `bar` preset, no output, no client): ready all
        // the same, and a surface comes with the next change that places one.
        dock.mapped();
    }
    dock.watch();
    dock
}

impl Dock {
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
                .all(|(surface, monitor)| surface.on(monitor) && surface.alive())
    }

    /// Places a surface per output. A surface is reused on the same monitor object while
    /// alive, and reconfigured in place (`Surface::place`); one whose output left is
    /// released, never destroyed while realized (`Surface::release`).
    fn rebuild(self: &Rc<Self>) {
        self.open_popover.take();
        let layout = self.layout.get();
        let rtl = i18n::is_rtl();
        // Both read GDK's one monitor list back to back, with no event dispatched between
        // them: the n-th output is the n-th monitor.
        let outputs = outputs::current(&self.display);
        let monitors = self.monitors();
        let mut old = self.surfaces.take();
        let mut surfaces = Vec::new();
        for (monitor, output) in monitors.iter().zip(&outputs) {
            let placement = self
                .client
                .as_ref()
                .and_then(|_| placement::place(layout, output, rtl));
            let reused = old
                .iter()
                .position(|surface| surface.on(monitor) && surface.alive())
                .map(|pos| old.remove(pos));
            let mut surface = reused.unwrap_or_else(|| Surface::new(self, monitor));
            surface.place(self, placement);
            surfaces.push(surface);
        }
        for leftover in old {
            leftover.release();
        }
        self.surfaces.replace(surfaces);
        self.refresh_rows();
    }

    /// READY=1 once, at the first surface on screen (Type=notify).
    fn mapped(&self) {
        if self.ready.replace(true) {
            return;
        }
        if let Err(err) = notify::notify_ready() {
            tracing::error!(error = %err, "cannot tell systemd the dock is ready");
        }
    }

    fn watch(self: &Rc<Self>) {
        if let Some(client) = &self.client {
            let weak = Rc::downgrade(self);
            client.connect_events(move |_, event| {
                let windows = matches!(
                    event,
                    Event::WindowAdded(_) | Event::WindowChanged(_) | Event::WindowRemoved(_)
                );
                if let Some(dock) = weak.upgrade().filter(|_| windows) {
                    dock.refresh_rows();
                }
            });
        }
        let weak = Rc::downgrade(self);
        outputs::watch(&self.display, move |_| {
            if let Some(dock) = weak.upgrade() {
                dock.schedule();
            }
        });
        // The layout's layers and the favourites file share these directories.
        for dir in self.source.watched() {
            // A directory that does not exist yet is watched all the same.
            match gio::File::for_path(&dir)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => {
                    let weak = Rc::downgrade(self);
                    monitor.connect_changed(move |_, _, _, _| {
                        if let Some(dock) = weak.upgrade() {
                            dock.schedule();
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
    }

    /// Editors write a file in several steps: act 250 ms after the last event.
    fn schedule(self: &Rc<Self>) {
        if let Some(pending) = self.debounce.take() {
            pending.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(dock) = weak.upgrade() {
                // Fired: the id is spent, and removing it again would abort.
                dock.debounce.take();
                dock.reload();
            }
        });
        self.debounce.replace(Some(id));
    }

    fn reload(self: &Rc<Self>) {
        let layout = self.source.layout();
        if layout != self.layout.get() {
            tracing::info!(layout = ?layout, "layout applied");
        }
        let outputs = outputs::current(&self.display);
        // The snapshot catches a change of size, rotation or connector; `surfaces_current`
        // an output that left and came back within the debounce.
        let moved = layout != self.layout.get()
            || outputs != *self.outputs.borrow()
            || !self.surfaces_current();
        self.layout.set(layout);
        self.outputs.replace(outputs);
        self.favorites.reload();
        if moved {
            self.rebuild();
        } else {
            self.refresh_rows();
        }
    }
}

impl Host for Dock {
    const APP: &'static str = "athanor-dock";
    const REORDER: bool = true;

    fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    fn favorites(&self) -> &Store {
        &self.favorites
    }

    fn pin_label(&self, pinned: bool) -> String {
        if pinned {
            tr("Unpin from Dock")
        } else {
            tr("Pin to Dock")
        }
    }

    /// BR6: at most one popover of the dock is open; opening one closes the other.
    fn menu_opened(&self, menu: &gtk4::Popover) {
        let previous = self.open_popover.replace(Some(menu.clone()));
        if let Some(previous) = previous.filter(|previous| previous != menu) {
            previous.popdown();
        }
    }

    fn refresh_rows(self: &Rc<Self>) {
        for surface in self.surfaces.borrow().iter() {
            surface.refresh(self);
        }
    }

    /// A menu or a drag holds every auto-hiding surface: it never reveals a hidden one,
    /// and keeps a shown one shown until released.
    fn hold(&self, held: bool) {
        for surface in self.surfaces.borrow().iter() {
            surface.hold(held);
        }
    }
}
