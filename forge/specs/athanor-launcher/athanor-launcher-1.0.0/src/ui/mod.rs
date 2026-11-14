//! The launcher's surfaces (LA5), drawn with Athanor's own toolkit: one layer surface per
//! output, kept for the output's life. Hidden, it is a 1×1 background surface that takes no
//! input and no keyboard and is out of the accessibility tree; at each Show the surface of the
//! focused output becomes an overlay over the whole output, with the panel of the query, the
//! rows and the preview. GLib's callbacks only queue a change, which `turn` applies with the
//! shell in hand.

mod actions;
mod look;
mod preview;
mod rows;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use athanor_launcher::keys::{self, Command, Key};
use athanor_launcher::list::{self, Line};
use athanor_launcher::menu::Choice;
use athanor_launcher::place;
use athanor_launcher::query::Memory;
use athanor_preview::{Card, Preview};
use athanor_sal::driver::{
    Anchor, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell, SurfaceId,
};
use athanor_sal::{Event, Key as SalKey, KeyModifiers, Kind, Ui, WidgetId, Wrap};
use athanor_search::apps::Catalog;
use athanor_search::engine::Engine;
use athanor_search::files::Files;
use athanor_search::item::Hit;
use athanor_search::providers;
use athanor_search::windows::WindowEntry;
use athanor_style::appearance::{self, Appearance};
use athanor_sulphur_client::{Client, WindowId};
use gio::glib;
use gio::prelude::*;

use crate::bus;
use crate::i18n::{is_rtl, tr, tr_with};
use look::Look;
use preview::Icons;

/// The selection settles this long before the preview draws it (LA6), so arrow keys stay
/// fluid. An estimate (open doubt 3).
const PREVIEW_DELAY: Duration = Duration::from_millis(100);
/// The layer shell namespace the compositors and the rig know the launcher by.
const NAMESPACE: &str = "athanor-launcher";
/// The panel's width, as the GTK launcher's.
const WIDTH: f32 = 760.0;
/// The icon theme the rows and the preview draw with.
const ICON_THEME: &str = "Adwaita";

pub struct Options {
    pub usage_path: PathBuf,
    pub given_up: bool,
    /// The rig's captures and launcher_e2e.py: shown at start with this query, as athanor-bar's
    /// ATHANOR_BAR_OPEN opens a popover. The rig has no way to type.
    pub show: Option<String>,
}

/// What GLib's callbacks tell the launcher, applied at the next turn.
enum Change {
    /// `os.athanor.Launcher1.Show`.
    Toggle,
    /// The engine's rows for the query.
    Lines(Vec<Line>),
    /// The preview's card for the selection.
    Card(Card),
    /// The selection settled.
    DrawPreview,
    /// The choices of the selected row, for its menu.
    Menu {
        row: WidgetId,
        hit: Hit,
        choices: Vec<Choice>,
    },
    /// A choice of the menu ran: the menu closes once its copy is served.
    CloseMenu,
    /// A copy that closes the launcher: it hides once the copy is served.
    Hide,
    Appearance(Appearance),
    Apps,
}

type Queue = Rc<RefCell<Vec<Change>>>;

/// A GLib timeout that can be replaced. The slot holds the id until the timeout fires and
/// takes it then, so a fired timeout is never removed: GLib aborts on that.
#[derive(Default)]
struct Slot(Rc<RefCell<Option<glib::SourceId>>>);

impl Slot {
    fn start(&self, after: Duration, queue: &Queue, change: impl FnOnce() -> Change + 'static) {
        self.cancel();
        let (slot, queue) = (self.0.clone(), queue.clone());
        let id = glib::timeout_add_local_once(after, move || {
            slot.borrow_mut().take();
            queue.borrow_mut().push(change());
        });
        self.0.replace(Some(id));
    }

    fn cancel(&self) {
        let pending = self.0.borrow_mut().take();
        if let Some(id) = pending {
            id.remove();
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The widgets of the panel shown, and the surface size it was laid out for.
#[derive(Clone, Copy)]
struct Panel {
    island: WidgetId,
    field: WidgetId,
    list: WidgetId,
    pane: WidgetId,
    size: (f32, f32),
}

/// The open menu of a row.
struct Menu {
    popup: SurfaceId,
    hit: Hit,
    items: Vec<(WidgetId, Choice)>,
}

pub struct Launcher {
    client: Option<Rc<Client>>,
    engine: Engine,
    preview: Rc<Preview>,
    look: Look,
    rtl: bool,
    icons: Icons,
    /// The surfaces, in the order their outputs were announced, with their outputs.
    outs: Vec<(SurfaceId, OutputInfo)>,
    /// The surface the launcher is shown on.
    shown: Option<SurfaceId>,
    panel: Option<Panel>,
    menu: Option<Menu>,
    lines: Vec<Line>,
    /// The widget of each line, at its index.
    rows: Vec<WidgetId>,
    /// The windows of the last Show, at the index their hits carry.
    windows: Vec<WindowId>,
    memory: Memory,
    queue: Queue,
    preview_timer: Slot,
    /// Bumped by every selection, so a late window capture is dropped.
    selection: Rc<Cell<u64>>,
    /// False after the crash loop gave up: no search provider is called then.
    discover_providers: bool,
    start_query: Option<String>,
    /// When the Show being drawn was asked for, logged at its first frame.
    asked: Option<Instant>,
    /// Whether a screen reader follows the selected row: from the first arrow until the query
    /// changes, as an ARIA combobox's active descendant (LA7).
    following: bool,
    /// A context singleton: it lives, and keeps its handler, only while held.
    _apps_monitor: gio::AppInfoMonitor,
    _owner: gio::OwnerId,
}

/// The size of `output` in logical pixels.
fn logical_size(output: &OutputInfo) -> (f32, f32) {
    let (width, height) = output.logical_size.unwrap_or_else(|| {
        let scale = output.scale_factor.max(1);
        output
            .modes
            .iter()
            .find(|mode| mode.current)
            .map_or((0, 0), |mode| {
                (mode.dimensions.0 / scale, mode.dimensions.1 / scale)
            })
    });
    (width as f32, height as f32)
}

fn scale(output: &OutputInfo) -> u16 {
    u16::try_from(output.scale_factor.clamp(1, 16)).unwrap_or(1)
}

/// The hidden surface: a 1×1 background surface without input or keyboard.
fn hidden() -> LayerOptions {
    let mut options = LayerOptions::new(Layer::Background);
    options.namespace = NAMESPACE.to_owned();
    options.input_region = Some(Vec::new());
    options
}

/// The shown surface: an overlay over the whole output that takes the keyboard.
fn shown() -> LayerOptions {
    let mut options = LayerOptions::new(Layer::Overlay);
    options.namespace = NAMESPACE.to_owned();
    options.anchor = Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT;
    options.keyboard = KeyboardInteractivity::Exclusive;
    options
}

/// Whether the field `field` of `ui` holds a query that is selected whole.
fn selected_whole(ui: &Ui, field: WidgetId) -> bool {
    ui.field_text(field)
        .is_some_and(|text| !text.is_empty() && ui.field_selection(field) == Some(text))
}

fn empty(look: &Look, rtl: bool) -> Ui {
    let mut ui = Ui::new("", look.panel.clone(), rtl);
    ui.min_width = 1.0;
    ui.min_height = 1.0;
    ui
}

/// The panel's widgets on a surface `size` large, the panel `height` tall at the top margin
/// and centred, with a viewport `scroll` tall onto the rows.
fn skeleton(look: &Look, rtl: bool, size: (f32, f32), height: f32, scroll: f32) -> (Ui, Panel) {
    let mut ui = Ui::new("", look.panel.clone(), rtl);
    (ui.min_width, ui.min_height) = size;
    let side = ((size.0 - WIDTH) / 2.0).max(0.0);
    let top = place::TOP_MARGIN as f32;
    let bottom = (size.1 - top - height).max(0.0);
    let island = ui.island(Ui::ROOT, true, [top, side, bottom, side]);
    let field = ui.field(island, &tr("Search"), "");
    let body = ui.add(island, Kind::Row);
    let viewport = ui.scroll(body, scroll);
    let list = ui.list(viewport, &tr("Results"));
    let pane = ui.add(body, Kind::Column);
    let footer = ui.add(
        island,
        Kind::Label(tr(
            "Enter to open · Ctrl+Enter to show in folder · Ctrl+C to copy · Tab for actions · Esc to close",
        )),
    );
    ui.set_text_style(footer, rows::caption());
    ui.set_wrap(footer, Some(Wrap::default()));
    let panel = Panel {
        island,
        field,
        list,
        pane,
        size,
    };
    (ui, panel)
}

/// The panel on a surface `size` large, `height` tall: the viewport takes what the query and
/// the footer leave, as the GTK list's scrolled window expands.
fn panel(look: &Look, rtl: bool, size: (f32, f32), height: f32) -> (Ui, Panel) {
    let (mut probe, parts) = skeleton(look, rtl, size, height, 0.0);
    probe.layout();
    let around = probe.island_fill(parts.island).map_or(0.0, |fill| fill.h);
    skeleton(look, rtl, size, height, (height - around).max(0.0))
}

/// What the launcher reads of a key of the toolkit's.
fn key(key: &SalKey) -> Key {
    match key {
        SalKey::Escape => Key::Escape,
        SalKey::Enter => Key::Enter,
        SalKey::Tab | SalKey::BackTab => Key::Tab,
        SalKey::Up => Key::Up,
        SalKey::Down => Key::Down,
        SalKey::PageUp => Key::PageUp,
        SalKey::PageDown => Key::PageDown,
        // Ctrl+C comes as the toolkit's copy.
        SalKey::Copy => Key::C,
        _ => Key::Other,
    }
}

impl Launcher {
    pub fn new(options: Options, client: Option<Client>) -> Launcher {
        let queue: Queue = Rc::default();
        // Search providers are discovered at each Show, not here.
        let files = if options.given_up {
            None
        } else {
            Some(Files::new())
        };
        let lines = queue.clone();
        let engine = Engine::new(options.usage_path, files, Vec::new(), move |rows| {
            lines.borrow_mut().push(Change::Lines(list::lines(rows)));
        });
        engine.set_catalog(Catalog::read(athanor_sulphur_client::SETTINGS_PAGE_PREFIX));
        let cards = queue.clone();
        let preview = Preview::new(move |card| cards.borrow_mut().push(Change::Card(card.clone())));
        let watched = queue.clone();
        appearance::watch(move |current| watched.borrow_mut().push(Change::Appearance(current)));
        let apps_monitor = gio::AppInfoMonitor::get();
        let watched = queue.clone();
        apps_monitor.connect_changed(move |_| watched.borrow_mut().push(Change::Apps));
        let toggles = queue.clone();
        let owner = bus::own(
            move || toggles.borrow_mut().push(Change::Toggle),
            || {
                if let Err(err) = athanor_unit::notify::notify_ready() {
                    tracing::warn!(error = %err, "cannot tell systemd the launcher is ready");
                }
            },
        );
        let look = look::look(&Appearance::read());
        Launcher {
            client: client.map(Rc::new),
            engine,
            preview,
            icons: Icons::new(ICON_THEME, look.panel.text),
            look,
            rtl: is_rtl(),
            outs: Vec::new(),
            shown: None,
            panel: None,
            menu: None,
            lines: Vec::new(),
            rows: Vec::new(),
            windows: Vec::new(),
            memory: Memory::default(),
            queue,
            preview_timer: Slot::default(),
            selection: Rc::new(Cell::new(0)),
            discover_providers: !options.given_up,
            start_query: options.show.filter(|query| !query.is_empty()),
            asked: None,
            following: false,
            _apps_monitor: apps_monitor,
            _owner: owner,
        }
    }

    /// The output `surface` is on.
    fn output_of(&self, surface: SurfaceId) -> Option<&OutputInfo> {
        self.outs
            .iter()
            .find(|(id, _)| *id == surface)
            .map(|(_, output)| output)
    }

    fn toggle(&mut self, shell: &mut Shell) {
        if self.shown.is_some() {
            self.hide(shell);
        } else {
            self.show(shell);
        }
    }

    fn show(&mut self, shell: &mut Shell) {
        let asked = Instant::now();
        let windows = self
            .client
            .as_ref()
            .map(|client| client.windows())
            .unwrap_or_default();
        let connectors: Vec<Option<String>> = self
            .outs
            .iter()
            .map(|(_, output)| output.name.clone())
            .collect();
        let activated = windows
            .iter()
            .find(|window| window.state.activated)
            .map(|window| window.outputs.as_slice());
        let Some((surface, output)) = self
            .outs
            .get(place::focused_output(activated, &connectors))
            .cloned()
        else {
            tracing::error!("no output to show the launcher on");
            return;
        };
        self.windows = windows.iter().map(|window| window.id).collect();
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
        // A few small files: an application installed or removed since the last Show adds or
        // drops its provider without a restart.
        if self.discover_providers {
            self.engine
                .set_providers(providers::discover(&providers::dirs()));
        }
        self.preview.set_scale(f64::from(scale(&output)));
        self.shown = Some(surface);
        let text = self.memory.shown(asked).unwrap_or_default();
        self.build(shell, surface, &text, logical_size(&output));
        if let Err(err) = shell.set_layer(surface, &shown()) {
            tracing::error!(error = %err, "cannot show the launcher's surface");
        }
        // The surface stays open while hidden: the frame that shows it is timed, as GTK's
        // first frame-clock tick after present.
        shell.report_next_frame(surface);
        if let Err(err) = shell.set_accessible(surface, true) {
            tracing::error!(error = %err, "the launcher's surface is not in the accessibility tree");
        }
        // The query kept from the last Show is selected, so typing replaces it.
        if let (Some(ui), Some(panel)) = (shell.ui_mut(surface), self.panel) {
            ui.set_focus(Some(panel.field));
            ui.key(SalKey::SelectAll);
        }
        if !text.is_empty() {
            self.engine.query(&text);
        }
        self.asked = Some(asked);
    }

    /// Lays the panel out on `surface`, `size` large, with `text` in the query, the rows and
    /// the card of now, and the selection kept.
    fn build(&mut self, shell: &mut Shell, surface: SurfaceId, text: &str, size: (f32, f32)) {
        let Some(output) = self.output_of(surface) else {
            return;
        };
        let (height, scale) = (
            place::panel_height(logical_size(output).1 as i32) as f32,
            scale(output),
        );
        let selected = self.selected_id(shell);
        // The menu is a popup of a row about to be replaced.
        self.close_menu(shell);
        let (mut ui, panel) = panel(&self.look, self.rtl, size, height);
        ui.set_field_text(panel.field, text);
        ui.set_focus(Some(panel.field));
        if self.following {
            ui.set_active_descendant(panel.field, Some(panel.list));
        }
        self.rows = self
            .lines
            .iter()
            .map(|line| rows::row(&mut ui, panel.list, &mut self.icons, scale, line))
            .collect();
        preview::draw(
            &mut ui,
            panel.pane,
            &self.preview.card(),
            &mut self.icons,
            scale,
        );
        ui.layout();
        if let Some(row) =
            list::reselect(&self.lines, selected.as_deref()).and_then(|index| self.rows.get(index))
        {
            ui.select_row(*row);
        }
        let Some(current) = shell.ui_mut(surface) else {
            return;
        };
        *current = ui;
        self.panel = Some(panel);
    }

    pub(super) fn hide(&mut self, shell: &mut Shell) {
        let Some(surface) = self.shown else { return };
        let text = self
            .panel
            .and_then(|panel| shell.ui(surface).and_then(|ui| ui.field_text(panel.field)))
            .unwrap_or_default();
        self.memory.hidden(&text, Instant::now());
        self.forget_shown(shell);
        if let Some(ui) = shell.ui_mut(surface) {
            *ui = empty(&self.look, self.rtl);
        }
        if let Err(err) = shell.set_layer(surface, &hidden()) {
            tracing::error!(error = %err, "cannot hide the launcher's surface");
        }
        if let Err(err) = shell.set_accessible(surface, false) {
            tracing::error!(error = %err, "the hidden launcher stays in the accessibility tree");
        }
    }

    /// Everything a hide undoes but the surface itself: the menu, the query (an empty query
    /// cancels every source still running, LA4) and the preview.
    fn forget_shown(&mut self, shell: &mut Shell) {
        self.close_menu(shell);
        self.shown = None;
        self.panel = None;
        self.asked = None;
        self.following = false;
        self.rows.clear();
        self.preview_timer.cancel();
        self.engine.query("");
        self.preview.clear();
    }

    fn focus_query(&self, shell: &mut Shell) {
        if let (Some(surface), Some(panel)) = (self.shown, self.panel) {
            if let Some(ui) = shell.ui_mut(surface) {
                ui.set_focus(Some(panel.field));
            }
        }
    }

    fn fill(&mut self, shell: &mut Shell, lines: Vec<Line>) {
        let selected = self.selected_id(shell);
        // The menu is a popup of a row about to be removed.
        self.close_menu(shell);
        self.lines = lines;
        let (Some(surface), Some(panel)) = (self.shown, self.panel) else {
            return;
        };
        let Some(scale) = self.output_of(surface).map(scale) else {
            return;
        };
        let Some(ui) = shell.ui_mut(surface) else {
            return;
        };
        ui.remove_children(panel.list);
        self.rows = self
            .lines
            .iter()
            .map(|line| rows::row(ui, panel.list, &mut self.icons, scale, line))
            .collect();
        ui.layout();
        if let Some(row) =
            list::reselect(&self.lines, selected.as_deref()).and_then(|index| self.rows.get(index))
        {
            ui.select_row(*row);
        }
        if ui
            .field_text(panel.field)
            .is_some_and(|text| !text.is_empty())
        {
            let count = self
                .lines
                .iter()
                .filter(|line| line.hit().is_some())
                .count();
            ui.announce(&tr_with("Results: {count}", "count", &count.to_string()));
        }
        // The rows were replaced: the selected one is drawn again, or the pane emptied.
        self.schedule_preview();
    }

    fn selected_index(&self, shell: &Shell) -> Option<usize> {
        let (surface, panel) = (self.shown?, self.panel?);
        let row = shell.ui(surface)?.selected(panel.list)?;
        self.rows.iter().position(|id| *id == row)
    }

    fn selected_row(&self, shell: &Shell) -> Option<WidgetId> {
        self.selected_index(shell)
            .and_then(|index| self.rows.get(index).copied())
    }

    fn selected_hit(&self, shell: &Shell) -> Option<Hit> {
        self.selected_index(shell)
            .and_then(|index| self.lines.get(index)?.hit().cloned())
    }

    /// What `list::reselect` finds the selected row by in the next answer.
    fn selected_id(&self, shell: &Shell) -> Option<String> {
        self.selected_index(shell)
            .and_then(|index| self.lines.get(index)?.id().map(str::to_owned))
    }

    fn step(&mut self, shell: &mut Shell, delta: i32) {
        let Some(target) = list::step(&self.lines, self.selected_index(shell), delta) else {
            return;
        };
        let (Some(surface), Some(panel), Some(row)) =
            (self.shown, self.panel, self.rows.get(target).copied())
        else {
            return;
        };
        let Some(ui) = shell.ui_mut(surface) else {
            return;
        };
        // The query keeps the focus, and a screen reader follows the row the arrows choose
        // until the query changes (doc_launcher.md item 12, LA7); the list scrolls it into view.
        if !self.following {
            self.following = true;
            ui.set_active_descendant(panel.field, Some(panel.list));
        }
        if ui.select_row(row).is_some() {
            // The menu belongs to the row it opened on: Enter must never run its choice for
            // another.
            self.close_menu(shell);
            self.schedule_preview();
        }
    }

    fn draw_card(&mut self, shell: &mut Shell, card: &Card) {
        let (Some(surface), Some(panel)) = (self.shown, self.panel) else {
            return;
        };
        let Some(scale) = self.output_of(surface).map(scale) else {
            return;
        };
        if let Some(ui) = shell.ui_mut(surface) {
            preview::draw(ui, panel.pane, card, &mut self.icons, scale);
            ui.layout();
        }
    }

    /// The compositor configured the surface to another size than the panel was laid out for:
    /// the panel is laid out again, centred on it.
    fn follow_size(&mut self, shell: &mut Shell) {
        let (Some(surface), Some(panel)) = (self.shown, self.panel) else {
            return;
        };
        let Some(ui) = shell.ui(surface) else { return };
        let size = (ui.min_width, ui.min_height);
        if size == panel.size {
            return;
        }
        self.rebuild(shell, surface, panel, size);
    }

    /// Lays the shown panel out again at `size`, with its query. A query selected whole, as
    /// the one kept from the last Show is, stays selected, so typing still replaces it: in a
    /// session the surface's first configure is smaller than the output, by the bar's
    /// exclusive zone, and comes after the Show.
    fn rebuild(&mut self, shell: &mut Shell, surface: SurfaceId, panel: Panel, size: (f32, f32)) {
        let Some(ui) = shell.ui(surface) else { return };
        let text = ui.field_text(panel.field).unwrap_or_default();
        let whole = selected_whole(ui, panel.field);
        self.build(shell, surface, &text, size);
        if let (true, Some(ui), Some(panel)) = (whole, shell.ui_mut(surface), self.panel) {
            ui.set_focus(Some(panel.field));
            ui.key(SalKey::SelectAll);
        }
    }

    fn apply(&mut self, shell: &mut Shell, change: Change) {
        match change {
            Change::Toggle => self.toggle(shell),
            Change::Lines(lines) => self.fill(shell, lines),
            Change::Card(card) => self.draw_card(shell, &card),
            Change::DrawPreview => self.draw_preview(shell),
            Change::Menu { row, hit, choices } => self.show_menu(shell, row, hit, choices),
            Change::CloseMenu => self.close_menu(shell),
            Change::Hide => self.hide(shell),
            Change::Appearance(current) => {
                self.look = look::look(&current);
                self.icons.reset(self.look.panel.text);
                if let (Some(surface), Some(panel)) = (self.shown, self.panel) {
                    self.rebuild(shell, surface, panel, panel.size);
                }
            }
            Change::Apps => {
                self.engine
                    .set_catalog(Catalog::read(athanor_sulphur_client::SETTINGS_PAGE_PREFIX));
                self.icons.reset(self.look.panel.text);
            }
        }
    }
}

impl Application for Launcher {
    /// Every output keeps a hidden surface for its life.
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        Some((empty(&self.look, self.rtl), hidden()))
    }

    fn output_changed(&mut self, shell: &mut Shell, surface: SurfaceId, output: &OutputInfo) {
        match self.outs.iter_mut().find(|(id, _)| *id == surface) {
            Some((_, known)) => *known = output.clone(),
            None => {
                self.outs.push((surface, output.clone()));
                // A hidden surface is out of the tree, as a hidden GTK window.
                if let Err(err) = shell.set_accessible(surface, false) {
                    tracing::warn!(error = %err, "the hidden launcher stays in the accessibility tree");
                }
            }
        }
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if let Some(menu) = self.menu.as_ref().filter(|menu| menu.popup == surface) {
            if let Event::Clicked(item) = event {
                let picked = menu
                    .items
                    .iter()
                    .find(|(id, _)| *id == item)
                    .map(|(_, choice)| choice.clone());
                if let Some(choice) = picked {
                    let hit = menu.hit.clone();
                    // Closed at the turn, once a copy of the choice's is served.
                    self.queue.borrow_mut().push(Change::CloseMenu);
                    self.run(shell, surface, hit, choice);
                }
            }
            return;
        }
        let (Some(shown), Some(panel)) = (self.shown, self.panel) else {
            return;
        };
        if surface != shown {
            return;
        }
        match event {
            Event::Edited(field) if field == panel.field => {
                // A screen reader is back on the query the user types (LA7).
                if self.following {
                    self.following = false;
                    if let Some(ui) = shell.ui_mut(surface) {
                        ui.set_active_descendant(field, None);
                    }
                }
                let text = shell
                    .ui(surface)
                    .and_then(|ui| ui.field_text(field))
                    .unwrap_or_default();
                self.engine.query(&text);
            }
            // A click on a row opens it, as the GTK list activates on a single click.
            Event::Selected(row) => {
                let hit = self
                    .rows
                    .iter()
                    .position(|id| *id == row)
                    .and_then(|index| self.lines.get(index)?.hit().cloned());
                if let Some(hit) = hit {
                    self.run(shell, surface, hit, Choice::Open);
                }
            }
            _ => {}
        }
    }

    /// A press beside the panel hides the launcher (LA5).
    fn pressed(&mut self, shell: &mut Shell, surface: SurfaceId, at: (f32, f32)) {
        let (Some(shown), Some(panel)) = (self.shown, self.panel) else {
            return;
        };
        let inside = shell
            .ui(surface)
            .and_then(|ui| ui.island_fill(panel.island))
            .is_some_and(|fill| fill.contains(at.0, at.1));
        if surface == shown && !inside {
            self.hide(shell);
        }
    }

    fn key(
        &mut self,
        shell: &mut Shell,
        surface: SurfaceId,
        pressed: &SalKey,
        modifiers: KeyModifiers,
    ) -> bool {
        if self.menu.as_ref().is_some_and(|menu| menu.popup == surface) {
            // The menu moves and runs by itself; Tab closes it, as in the GTK launcher.
            if keys::command(key(pressed), modifiers.ctrl, false, true) == Command::MenuClose {
                self.close_menu(shell);
                return true;
            }
            return false;
        }
        let (Some(shown), Some(panel)) = (self.shown, self.panel) else {
            return false;
        };
        if surface != shown {
            return false;
        }
        let selection = shell
            .ui(surface)
            .and_then(|ui| ui.field_selection(panel.field))
            .is_some_and(|text| !text.is_empty());
        match keys::command(key(pressed), modifiers.ctrl, selection, false) {
            Command::Pass | Command::MenuMove(_) | Command::MenuRun | Command::MenuClose => false,
            Command::Hide => {
                self.hide(shell);
                true
            }
            Command::Move(delta) => {
                self.step(shell, delta);
                true
            }
            Command::Menu => {
                self.open_menu(shell);
                true
            }
            Command::Run(choice) => {
                if let Some(hit) = self.selected_hit(shell) {
                    self.run(shell, surface, hit, choice);
                }
                true
            }
        }
    }

    fn turn(&mut self, shell: &mut Shell) {
        // The client reads its own connection: once it stops, the window list freezes and no
        // launch can go through the security context. End, so that systemd restarts it.
        if self
            .client
            .as_ref()
            .is_some_and(|client| client.is_closed())
        {
            shell.fail("the compositor client's connection closed".into());
            return;
        }
        self.outs.retain(|(id, _)| shell.ui(*id).is_some());
        if self.shown.is_some_and(|id| shell.ui(id).is_none()) {
            self.forget_shown(shell);
        }
        // A menu closed by Escape, a press outside it or the compositor: the query takes the
        // focus back from the row.
        if self.menu.is_some() && shell.popup_parent().is_none() {
            self.menu = None;
            self.focus_query(shell);
        }
        if !self.outs.is_empty() {
            if let Some(query) = self.start_query.take() {
                self.show(shell);
                if let (Some(surface), Some(panel)) = (self.shown, self.panel) {
                    if let Some(ui) = shell.ui_mut(surface) {
                        ui.set_field_text(panel.field, &query);
                    }
                    self.engine.query(&query);
                }
            }
        }
        // A change may queue another, as the preview's card after a selection: the queue is
        // emptied before the frame is drawn.
        loop {
            let changes = std::mem::take(&mut *self.queue.borrow_mut());
            if changes.is_empty() {
                break;
            }
            for change in changes {
                self.apply(shell, change);
            }
        }
        self.follow_size(shell);
    }

    /// §5 item 1: from the Show call to the panel's first frame, with the output it is on.
    fn drawn(&mut self, _shell: &mut Shell, surface: SurfaceId) {
        if self.shown == Some(surface) {
            if let Some(asked) = self.asked.take() {
                let output = self.output_of(surface).and_then(|output| output.name.as_deref());
                tracing::info!(
                    ms = u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX),
                    output = output.unwrap_or(""),
                    "shown"
                );
            }
        }
    }

    fn closed(&mut self, shell: &mut Shell, surface: SurfaceId) {
        self.outs.retain(|(id, _)| *id != surface);
        if self.shown == Some(surface) {
            self.forget_shown(shell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_sal::Theme;

    #[test]
    fn a_query_is_selected_whole_only_when_all_of_it_is() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let field = ui.field(Ui::ROOT, "Search", "");
        assert!(!selected_whole(&ui, field), "an empty field");
        ui.set_field_text(field, "firefox");
        ui.set_focus(Some(field));
        assert!(!selected_whole(&ui, field), "the caret only");
        ui.key(SalKey::SelectAll);
        assert!(selected_whole(&ui, field));
    }
}
