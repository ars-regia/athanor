//! The launcher's content: the query, the rows and the preview, moved onto the surface of
//! the focused output at each Show (LA5).

mod actions;
mod rows;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_compositor_client::{outputs, theme, Client, WindowId};
use athanor_launcher::keys::{self, Command, Key};
use athanor_launcher::list::{self, Line};
use athanor_launcher::place;
use athanor_launcher::query::Memory;
use athanor_preview::Preview;
use athanor_search::apps::Catalog;
use athanor_search::engine::Engine;
use athanor_search::files::Files;
use athanor_search::providers;
use athanor_search::windows::WindowEntry;
use athanor_style::calmo;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::i18n::{is_rtl, tr, tr_with};
use crate::surface::Surface;

/// The selection settles this long before the preview draws it (LA6), so arrow keys stay
/// fluid. An estimate (open doubt 3).
const PREVIEW_DELAY: Duration = Duration::from_millis(100);

const CSS: &str = "
.launcher-surface { background: transparent; }
.launcher { border-radius: 16px; padding: 12px; }
.launcher-query { font-size: 1.4em; min-height: 44px; }
.launcher-results { min-width: 420px; }
.preview { min-width: 320px; padding: 12px; }
.section-title { font-size: 0.85em; opacity: 0.7; padding: 8px 8px 2px; }
.row-subtitle { font-size: 0.85em; opacity: 0.7; }
.launcher-footer { font-size: 0.85em; opacity: 0.7; padding-top: 8px; }
";

pub struct Options {
    pub usage_path: PathBuf,
    pub given_up: bool,
}

pub struct Launcher {
    app: gtk4::Application,
    display: gdk::Display,
    client: Option<Client>,
    engine: Engine,
    preview: Rc<Preview>,
    surfaces: RefCell<Vec<Surface>>,
    /// The monitor of the surface the launcher is shown on.
    shown: RefCell<Option<gdk::Monitor>>,
    content: gtk4::Box,
    entry: gtk4::Entry,
    list: gtk4::ListBox,
    scroll: gtk4::ScrolledWindow,
    menu: RefCell<Option<gtk4::Popover>>,
    lines: RefCell<Vec<Line>>,
    /// The windows of the last Show, at the index their hits carry.
    windows: RefCell<Vec<WindowId>>,
    memory: RefCell<Memory>,
    pending_preview: RefCell<Option<glib::SourceId>>,
    /// Bumped by every selection, so a late window capture is dropped.
    selection: Cell<u64>,
    watches: RefCell<Vec<gio::FileMonitor>>,
    /// A context singleton: it lives, and keeps its handler, only while held.
    apps_monitor: gio::AppInfoMonitor,
}

pub fn start(app: &gtk4::Application, options: &Options) -> Rc<Launcher> {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("athanor-launcher: no display");
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
    gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    let client = match Client::connect(&display) {
        Ok(client) => Some(client),
        Err(err) => {
            tracing::error!(error = %err, "no compositor client; windows, launching and the focused output are unavailable");
            None
        }
    };
    let (files, search_providers) = if options.given_up {
        (None, Vec::new())
    } else {
        (Some(Files::new()), providers::discover(&providers::dirs()))
    };

    let entry = gtk4::Entry::builder().placeholder_text(tr("Search")).hexpand(true).build();
    entry.add_css_class("launcher-query");
    entry.update_property(&[gtk4::accessible::Property::Label(&tr("Search"))]);
    let list = gtk4::ListBox::builder().selection_mode(gtk4::SelectionMode::Single).activate_on_single_click(true).build();
    list.add_css_class("launcher-results");
    list.update_property(&[gtk4::accessible::Property::Label(&tr("Results"))]);
    let scroll = gtk4::ScrolledWindow::builder().child(&list).hscrollbar_policy(gtk4::PolicyType::Never).vexpand(true).build();
    let preview = Preview::new();
    let body = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    body.append(&scroll);
    body.append(preview.widget());
    let footer = gtk4::Label::new(Some(&tr("Enter to open · Ctrl+Enter to show in folder · Ctrl+C to copy · Tab for actions · Esc to close")));
    footer.add_css_class("launcher-footer");
    footer.set_xalign(0.0);
    let content = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(8)
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Start)
        .margin_top(place::TOP_MARGIN)
        .width_request(760)
        .height_request(place::MAX_HEIGHT)
        .build();
    content.add_css_class("launcher");
    content.add_css_class("background");
    content.append(&entry);
    content.append(&body);
    content.append(&footer);

    let launcher = Rc::new_cyclic(|weak: &std::rc::Weak<Launcher>| {
        let weak = weak.clone();
        let engine = Engine::new(options.usage_path.clone(), files, search_providers, move |rows| {
            if let Some(launcher) = weak.upgrade() {
                launcher.fill(&list::lines(rows));
            }
        });
        engine.set_catalog(Catalog::read(athanor_compositor_client::SETTINGS_PAGE_PREFIX));
        Launcher {
            app: app.clone(),
            display: display.clone(),
            client,
            engine,
            preview,
            surfaces: RefCell::default(),
            shown: RefCell::default(),
            content,
            entry,
            list,
            scroll,
            menu: RefCell::default(),
            lines: RefCell::default(),
            windows: RefCell::default(),
            memory: RefCell::default(),
            pending_preview: RefCell::default(),
            selection: Cell::new(0),
            watches: RefCell::default(),
            apps_monitor: gio::AppInfoMonitor::get(),
        }
    });
    launcher.connect();
    launcher.sync_surfaces();
    // The rig's captures and launcher_e2e.py (Task 16): shown at start with this query, as
    // athanor-bar's ATHANOR_BAR_OPEN opens a popover. The rig has no way to type.
    if let Some(query) = std::env::var("ATHANOR_LAUNCHER_SHOW").ok().filter(|query| !query.is_empty()) {
        launcher.show();
        launcher.entry.set_text(&query);
        launcher.entry.set_position(-1);
    }
    launcher
}

fn key(value: gdk::Key) -> Key {
    match value {
        gdk::Key::Escape => Key::Escape,
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => Key::Enter,
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => Key::Tab,
        gdk::Key::Up | gdk::Key::KP_Up => Key::Up,
        gdk::Key::Down | gdk::Key::KP_Down => Key::Down,
        gdk::Key::Page_Up | gdk::Key::KP_Page_Up => Key::PageUp,
        gdk::Key::Page_Down | gdk::Key::KP_Page_Down => Key::PageDown,
        gdk::Key::c | gdk::Key::C => Key::C,
        _ => Key::Other,
    }
}

impl Launcher {
    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_changed(move |entry| {
            if let Some(launcher) = weak.upgrade() {
                launcher.engine.query(&entry.text());
            }
        });
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, value, _, state| {
            let Some(launcher) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            match keys::command(key(value), ctrl, launcher.entry.selection_bounds().is_some(), launcher.menu_open()) {
                Command::Pass => glib::Propagation::Proceed,
                Command::Hide => {
                    launcher.hide();
                    glib::Propagation::Stop
                }
                Command::Move(delta) => {
                    launcher.step(delta);
                    glib::Propagation::Stop
                }
                Command::Menu => {
                    launcher.open_menu();
                    glib::Propagation::Stop
                }
                Command::MenuMove(delta) => {
                    launcher.move_menu(delta);
                    glib::Propagation::Stop
                }
                Command::MenuRun => {
                    launcher.run_menu();
                    glib::Propagation::Stop
                }
                Command::MenuClose => {
                    launcher.close_menu();
                    glib::Propagation::Stop
                }
                Command::Run(choice) => {
                    if let Some(hit) = launcher.selected_hit() {
                        launcher.run(hit, choice);
                    }
                    glib::Propagation::Stop
                }
            }
        });
        self.content.add_controller(keys);
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(launcher) = weak.upgrade() else { return };
            let hit = usize::try_from(row.index()).ok().and_then(|index| launcher.lines.borrow().get(index).and_then(Line::hit).cloned());
            if let Some(hit) = hit {
                launcher.run(hit, athanor_launcher::menu::Choice::Open);
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, _| {
            if let Some(launcher) = weak.upgrade() {
                // The menu belongs to the row it opened on: Enter must never run its choice for another.
                launcher.close_menu();
                launcher.schedule_preview();
            }
        });
        let weak = Rc::downgrade(self);
        self.apps_monitor.connect_changed(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.engine.set_catalog(Catalog::read(athanor_compositor_client::SETTINGS_PAGE_PREFIX));
            }
        });
        let weak = Rc::downgrade(self);
        outputs::watch(&self.display, move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.sync_surfaces();
            }
        });
        let display = self.display.clone();
        self.watches.borrow_mut().extend(theme::watch(move |cosmic| {
            calmo::load(&display, cosmic.variant());
            theme::load_accent(&display, &cosmic);
        }));
    }

    fn monitors(&self) -> Vec<gdk::Monitor> {
        let monitors = self.display.monitors();
        (0..monitors.n_items()).filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>()).collect()
    }

    /// One surface per output, reused on the same monitor object; a surface whose output
    /// left is abandoned, and hides the launcher if it showed it (Review Focus 4).
    fn sync_surfaces(self: &Rc<Self>) {
        let mut old = self.surfaces.take();
        let mut surfaces = Vec::new();
        for monitor in self.monitors() {
            match old.iter().position(|surface| surface.monitor == monitor && surface.alive()) {
                Some(pos) => surfaces.push(old.remove(pos)),
                None => surfaces.push(self.surface(&monitor)),
            }
        }
        for leftover in old {
            if self.shown.borrow().as_ref() == Some(&leftover.monitor) {
                self.forget_shown();
            }
            leftover.abandon();
        }
        self.surfaces.replace(surfaces);
    }

    fn surface(self: &Rc<Self>, monitor: &gdk::Monitor) -> Surface {
        let surface = Surface::new(&self.app, monitor);
        // A click outside the panel hides the launcher (LA5). A hidden surface has an empty
        // input region and receives none.
        let click = gtk4::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |gesture, _, x, y| {
            let (Some(launcher), Some(window)) = (weak.upgrade(), gesture.widget()) else { return };
            let inside = window
                .pick(x, y, gtk4::PickFlags::DEFAULT)
                .is_some_and(|picked| picked == *launcher.content.upcast_ref::<gtk4::Widget>() || picked.is_ancestor(&launcher.content));
            if !inside {
                launcher.hide();
            }
        });
        surface.window.add_controller(click);
        surface
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.shown.borrow().is_some() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn show(self: &Rc<Self>) {
        let asked = Instant::now();
        let windows = self.client.as_ref().map(Client::windows).unwrap_or_default();
        let surfaces = self.surfaces.borrow();
        let connectors: Vec<Option<String>> = surfaces.iter().map(|surface| surface.monitor.connector().map(|name| name.to_string())).collect();
        let activated = windows.iter().find(|window| window.state.activated).map(|window| window.outputs.as_slice());
        let Some(surface) = surfaces.get(place::focused_output(activated, &connectors)) else {
            tracing::error!("no output to show the launcher on");
            return;
        };
        self.windows.replace(windows.iter().map(|window| window.id).collect());
        self.engine.set_windows(
            windows
                .iter()
                .enumerate()
                .map(|(index, window)| {
                    let info = gio_unix::DesktopAppInfo::new(&format!("{}.desktop", window.app_id));
                    WindowEntry {
                        index,
                        title: window.title.clone(),
                        app_id: window.app_id.clone(),
                        app_name: info.as_ref().map(|info| info.name().to_string()),
                        icon: info.and_then(|info| info.icon()),
                    }
                })
                .collect(),
        );
        self.engine.reload_usage();
        self.content.set_height_request(place::panel_height(surface.monitor.geometry().height()));
        surface.show(self.content.upcast_ref());
        self.shown.replace(Some(surface.monitor.clone()));
        let text = self.memory.borrow_mut().shown(asked).unwrap_or_default();
        self.entry.set_text(&text);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        // §5 item 1: from the Show call to the first frame-clock tick after present, which is
        // before the frame is painted.
        surface.window.add_tick_callback(move |_, _| {
            tracing::info!(ms = u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX), "shown");
            glib::ControlFlow::Break
        });
    }

    pub fn hide(&self) {
        let Some(monitor) = self.shown.borrow().clone() else { return };
        self.memory.borrow_mut().hidden(&self.entry.text(), Instant::now());
        self.forget_shown();
        if let Some(surface) = self.surfaces.borrow().iter().find(|surface| surface.monitor == monitor) {
            surface.hide();
        }
    }

    /// Everything a hide undoes, except the surface itself: the menu, the query (an empty
    /// query cancels every source still running, LA4) and the preview.
    fn forget_shown(&self) {
        self.shown.replace(None);
        if let Some(menu) = self.menu.take() {
            menu.popdown();
        }
        if let Some(source) = self.pending_preview.take() {
            source.remove();
        }
        self.entry.set_text("");
        self.preview.clear();
    }

    fn fill(&self, lines: &[Line]) {
        // The popover is parented to a row that is about to be removed.
        if let Some(menu) = self.menu.take() {
            menu.popdown();
            menu.unparent();
        }
        let selected = self.selected_hit().map(|hit| if hit.key.is_empty() { hit.title } else { hit.key });
        self.list.remove_all();
        for line in lines {
            self.list.append(&rows::row(line));
        }
        self.lines.replace(lines.to_vec());
        if let Some(row) = list::reselect(lines, selected.as_deref()).and_then(|index| self.list.row_at_index(index as i32)) {
            self.list.select_row(Some(&row));
        }
        let count = lines.iter().filter(|line| line.hit().is_some()).count();
        if !self.entry.text().is_empty() {
            self.list.announce(
                &tr_with("Results: {count}", "count", &count.to_string()),
                gtk4::AccessibleAnnouncementPriority::Medium,
            );
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.list.selected_row().and_then(|row| usize::try_from(row.index()).ok())
    }

    fn selected_hit(&self) -> Option<athanor_search::item::Hit> {
        self.selected_index().and_then(|index| self.lines.borrow().get(index).and_then(Line::hit).cloned())
    }

    fn step(&self, delta: i32) {
        let target = list::step(&self.lines.borrow(), self.selected_index(), delta);
        let Some(row) = target.and_then(|index| self.list.row_at_index(index as i32)) else { return };
        self.list.select_row(Some(&row));
        // The entry keeps the focus, so the list does not scroll by itself.
        if let Some(bounds) = row.compute_bounds(&self.list) {
            let adjustment = self.scroll.vadjustment();
            let (top, bottom) = (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
            if top < adjustment.value() {
                adjustment.set_value(top);
            } else if bottom > adjustment.value() + adjustment.page_size() {
                adjustment.set_value(bottom - adjustment.page_size());
            }
        }
    }
}
