//! The toolkit dock's surfaces (doc_bar.md, BR7), as `ui/` draws the GTK dock's: one layer
//! surface per output, placed by `athanor_dock::placement::place`; the island with the
//! openers, the favourites and the running applications; their menus; reordering by drag;
//! auto-hide. Its inputs are the GTK dock's: the layout's layers and the favourites file,
//! watched live, the appearance, the installed applications and the compositor's windows.
//! GLib's callbacks only queue a change, which `turn` applies with the shell in hand.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use athanor_apps::entries::{self, Apps, State};
use athanor_apps::favorites::Store;
use athanor_apps::i18n::tr;
use athanor_apps::model;
use athanor_apps::sal::{self as row, Icons, Press, Row};
use athanor_sulphur_client::{Client, Event as Compositor, Opener};
use athanor_dock::autohide::{self, AutoHide, Event as Hide, Timer};
use athanor_dock::i18n;
use athanor_dock::placement::{self, Placement};
use athanor_layout::favorites;
use athanor_layout::loader::Source;
use athanor_layout::placement::Output;
use athanor_layout::preset::Layout;
use athanor_style::appearance::{self, Appearance};
use athanor_sal::driver::{
    Anchor as Edges, Application, LayerOptions, OutputInfo, Shell, SurfaceId,
};
use athanor_sal::{Event, Rect, Ui, WidgetId};
use gio::glib;
use gio::prelude::*;

use crate::edge;
use crate::look::{self, Look};

/// The program's name: of its failed-start notifications, as the GTK dock's.
const APP: &str = "athanor-dock";
/// Editors write a file in several steps: the dock acts this long after the last event.
const DEBOUNCE: Duration = Duration::from_millis(250);
/// The icon theme the GTK dock draws with.
const ICON_THEME: &str = "Adwaita";

/// What GLib's callbacks tell the dock, applied at the next turn.
enum Change {
    Windows,
    /// A file in a watched directory changed: a reload follows the debounce.
    Files,
    Reload,
    Appearance(Appearance),
    Apps,
    /// A surface's auto-hide timer ran out, with the generation it was started in.
    Timer(SurfaceId, u64),
}

type Queue = Rc<RefCell<Vec<Change>>>;

/// Whether `change` makes the cached images stale: a new look tints them anew, and an
/// application installed or removed can bring the icon an entry fell back from, as the GTK
/// dock follows the theme.
fn reads_icons_again(change: &Change) -> bool {
    matches!(change, Change::Appearance(_) | Change::Apps)
}

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

/// An auto-hiding surface's state.
struct Hider {
    state: AutoHide,
    timer: Slot,
    /// Bumped at each start or cancel, so a timeout that fired before is ignored.
    generation: u64,
    /// Whether the island is drawn.
    shown: bool,
    /// The surface's depth with the island, which it keeps while the island hides.
    depth: f32,
}

/// What an opener of the island opens.
type Act = Opener;

/// One output's surface.
struct Out {
    info: OutputInfo,
    placement: Option<Placement>,
    island: Option<WidgetId>,
    acts: Vec<(WidgetId, Act)>,
    row: Row,
    hider: Option<Hider>,
    /// The keyboard is on the surface; it says so only at its enter.
    keyboard: bool,
    /// The input region last set under auto-hide.
    region: Option<Vec<Rect>>,
}

impl Out {
    fn act(&self, widget: WidgetId) -> Option<Act> {
        self.acts
            .iter()
            .find(|(w, _)| *w == widget)
            .map(|(_, act)| *act)
    }

    fn entry(&self, widget: WidgetId) -> Option<&State> {
        self.row.entry(widget)
    }

    /// Shows or hides the island as the hider says, the surface keeping its depth.
    fn follow(&mut self, shell: &mut Shell, id: SurfaceId) {
        let (Some(hider), Some(island), Some(placement)) =
            (&mut self.hider, self.island, self.placement)
        else {
            return;
        };
        let shown = hider.state.shown();
        if shown == hider.shown {
            return;
        }
        hider.shown = shown;
        let Some(ui) = shell.ui_mut(id) else { return };
        let depth = if shown { 0.0 } else { hider.depth };
        if placement.anchor.vertical() {
            ui.min_width = depth;
        } else {
            ui.min_height = depth;
        }
        ui.set_visible(island, shown);
        ui.layout();
    }
}

/// The open menu of an entry.
struct Menu {
    parent: SurfaceId,
    popup: SurfaceId,
    rows: row::Menu,
}

pub struct Dock {
    /// `None` without the privileged globals: no surface at all (SH1, BR2).
    client: Option<Rc<Client>>,
    source: Source,
    layout: Layout,
    favorites: Store,
    /// Dropped when the installed applications change, and read again when next needed.
    apps: Option<Apps>,
    look: Look,
    rtl: bool,
    icons: Icons,
    outs: HashMap<SurfaceId, Out>,
    queue: Queue,
    debounce: Slot,
    menu: Option<Menu>,
    /// The surface where a drag of a pinned entry is under way.
    dragging: Option<SurfaceId>,
    /// The windows or the favourites changed while a menu or a drag held the rows.
    stale: bool,
    ready: bool,
    _watches: Vec<gio::FileMonitor>,
    _apps_monitor: gio::AppInfoMonitor,
}

impl Dock {
    pub fn new(source: Source, favorites_file: Option<PathBuf>, client: Option<Client>) -> Dock {
        let queue: Queue = Rc::default();
        let client = client.map(Rc::new);
        if let Some(client) = &client {
            let queue = queue.clone();
            client.connect_events(move |_, event| {
                if matches!(
                    event,
                    Compositor::WindowAdded(_)
                        | Compositor::WindowChanged(_)
                        | Compositor::WindowRemoved(_)
                ) {
                    queue.borrow_mut().push(Change::Windows);
                }
            });
        }
        // The layout's layers and the favourites file share these directories. Armed
        // before the layout and the favourites are read, so that a write while the dock
        // starts (the bar's first-session pick) is read or reported, never lost.
        let (layout, watches) = source.watch(|dir| {
            // A directory that does not exist yet is watched all the same.
            match gio::File::for_path(dir)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => {
                    let queue = queue.clone();
                    monitor
                        .connect_changed(move |_, _, _, _| queue.borrow_mut().push(Change::Files));
                    Some(monitor)
                }
                Err(err) => {
                    tracing::warn!(error = %err, dir = %dir.display(), "cannot watch; changes there apply at the next start");
                    None
                }
            }
        });
        tracing::info!(layout = ?layout, "layout applied");
        let watched = queue.clone();
        appearance::watch(move |current| watched.borrow_mut().push(Change::Appearance(current)));
        let apps_monitor = gio::AppInfoMonitor::get();
        let watched = queue.clone();
        apps_monitor.connect_changed(move |_| watched.borrow_mut().push(Change::Apps));
        let look = look::look(&Appearance::read());
        Dock {
            client,
            source,
            layout,
            favorites: Store::load(favorites_file),
            apps: None,
            icons: Icons::new(ICON_THEME, look.dock.text),
            look,
            rtl: i18n::is_rtl(),
            outs: HashMap::new(),
            queue,
            debounce: Slot::default(),
            menu: None,
            dragging: None,
            stale: false,
            ready: false,
            _watches: watches,
            _apps_monitor: apps_monitor,
        }
    }

    fn place(&self, info: &OutputInfo) -> Option<Placement> {
        self.client.as_ref()?;
        let (width, height) = info.logical_size.unwrap_or_else(|| {
            let scale = info.scale_factor.max(1);
            info.modes
                .iter()
                .find(|mode| mode.current)
                .map_or((0, 0), |mode| {
                    (mode.dimensions.0 / scale, mode.dimensions.1 / scale)
                })
        });
        let output = Output {
            connector: info.name.clone(),
            width,
            height,
        };
        placement::place(self.layout, &output, self.rtl)
    }

    /// The entries now, each with its desktop entry.
    fn states(&mut self) -> Vec<State> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        let windows = entries::windows(client);
        let favorites = self.favorites.ids().unwrap_or_default();
        self.apps
            .get_or_insert_with(Apps::installed)
            .states(&favorites, &windows)
    }

    /// The island of `placement`: the launcher and workspaces openers, the entries, and the
    /// application library last, as the GTK dock lines them.
    fn island(
        &mut self,
        ui: &mut Ui,
        placement: Placement,
        scale: u16,
        states: &[State],
        acts: &mut Vec<(WidgetId, Act)>,
    ) -> (WidgetId, Row) {
        let anchor = placement.anchor;
        let island = ui.island(Ui::ROOT, anchor.vertical(), edge::margin(anchor));
        let own = self.client.as_ref().is_some_and(|client| client.own_compositor());
        let opener =
            |dock: &mut Dock, ui: &mut Ui, acts: &mut Vec<(WidgetId, Act)>, opener: Opener| {
                if !opener.shown(own) {
                    return;
                }
                let (icon, name) = entries::opener(opener);
                let button =
                    dock.icons.button(ui, island, icon, row::SMALL_ICON, scale, &name, None);
                acts.push((button, opener));
            };
        opener(self, ui, acts, Opener::Launcher);
        opener(self, ui, acts, Opener::Workspaces);
        let side = edge::indicator_side(anchor);
        let row = Row::build(ui, island, states, &mut self.icons, scale, side);
        opener(self, ui, acts, Opener::AppLibrary);
        (island, row)
    }

    /// Draws `id` again from its placement and `states`. The focus stays where it was when
    /// the surface keeps its place and its buttons (`model::same_shape`).
    fn build(&mut self, shell: &mut Shell, id: SurfaceId, states: &[State], keep_focus: bool) {
        let Some(out) = self.outs.get(&id) else {
            return;
        };
        let (placement, scale) = (out.placement, out.info.scale_factor.clamp(1, 16) as u16);
        let mut ui = Ui::new(&tr("Dock"), self.look.dock.clone(), self.rtl);
        let mut acts = Vec::new();
        let (island, row) = match placement
            .map(|placement| self.island(&mut ui, placement, scale, states, &mut acts))
        {
            Some((island, row)) => (Some(island), row),
            None => (None, Row::default()),
        };
        if island.is_none() {
            // An empty surface stays mapped (see `edge::layer`): one transparent pixel.
            ui.min_width = 1.0;
            ui.min_height = 1.0;
        }
        let vertical = placement.is_some_and(|p| p.anchor.vertical());
        let (width, height) = ui.layout();
        let depth = if vertical { width } else { height };
        let options = edge::layer(placement, depth.ceil() as i32);
        let hider = placement.filter(|p| p.auto_hide).map(|_| Hider {
            state: AutoHide::default(),
            timer: Slot::default(),
            generation: 0,
            shown: true,
            depth,
        });
        if let Err(err) = shell.set_layer(id, &options) {
            tracing::error!(error = %err, "cannot place the dock surface");
        }
        let Some(current) = shell.ui_mut(id) else {
            return;
        };
        // Along a stretched axis the surface keeps the length the compositor configured.
        if options.anchor.contains(Edges::LEFT | Edges::RIGHT) {
            ui.min_width = current.min_width;
        }
        if options.anchor.contains(Edges::TOP | Edges::BOTTOM) {
            ui.min_height = current.min_height;
        }
        if keep_focus {
            ui.set_focus(current.focus());
        }
        *current = ui;
        let Some(out) = self.outs.get_mut(&id) else {
            return;
        };
        out.island = island;
        out.acts = acts;
        out.row = row;
        out.hider = hider;
        out.region = None;
        // A new auto-hiding island starts hidden, as the GTK dock's, unless it holds the keyboard.
        if let (true, Some(hider)) = (out.keyboard, &mut out.hider) {
            hider.state.feed(Hide::KeyboardIn);
        }
        out.follow(shell, id);
    }

    /// Draws every surface again: the look or the placements changed.
    fn rebuild_all(&mut self, shell: &mut Shell) {
        self.close_menu(shell);
        self.drop_drag(shell);
        let states = self.states();
        let ids: Vec<SurfaceId> = self.outs.keys().copied().collect();
        for id in ids {
            self.build(shell, id, &states, false);
        }
        self.stale = false;
    }

    /// Draws again the surfaces whose entries, or what their buttons show, changed.
    fn refresh(&mut self, shell: &mut Shell) {
        self.stale = false;
        let states = self.states();
        let entries = |states: &[State]| {
            states
                .iter()
                .map(|(entry, _)| entry.clone())
                .collect::<Vec<_>>()
        };
        let now = entries(&states);
        let changed: Vec<(SurfaceId, bool)> = self
            .outs
            .iter()
            .filter(|(_, out)| out.placement.is_some())
            .filter(|(_, out)| row::redraws(out.row.states(), &states))
            .map(|(id, out)| (*id, model::same_shape(&entries(out.row.states()), &now)))
            .collect();
        for (id, same) in changed {
            self.build(shell, id, &states, same);
        }
    }

    fn reload(&mut self, shell: &mut Shell) {
        let layout = self.source.layout();
        if layout != self.layout {
            tracing::info!(layout = ?layout, "layout applied");
            self.layout = layout;
        }
        self.favorites.reload();
        let moved: Vec<SurfaceId> = self
            .outs
            .iter()
            .filter(|(_, out)| self.place(&out.info) != out.placement)
            .map(|(id, _)| *id)
            .collect();
        if moved.is_empty() {
            self.stale = true;
            return;
        }
        for id in &moved {
            let placement = self.outs.get(id).and_then(|out| self.place(&out.info));
            if let Some(out) = self.outs.get_mut(id) {
                out.placement = placement;
            }
        }
        self.rebuild_all(shell);
    }

    /// A menu or a drag holds every auto-hiding surface: it never reveals a hidden one, and
    /// keeps a shown one shown until released.
    fn hold(&mut self, shell: &mut Shell, held: bool) {
        let ids: Vec<SurfaceId> = self.outs.keys().copied().collect();
        for id in ids {
            self.feed(shell, id, Hide::Held(held));
        }
    }

    fn feed(&mut self, shell: &mut Shell, id: SurfaceId, event: Hide) {
        let Some(out) = self.outs.get_mut(&id) else {
            return;
        };
        let Some(hider) = &mut out.hider else { return };
        match hider.state.feed(event) {
            Timer::Start(after) => {
                hider.generation += 1;
                let generation = hider.generation;
                hider
                    .timer
                    .start(after, &self.queue, move || Change::Timer(id, generation));
            }
            Timer::Cancel => {
                hider.generation += 1;
                hider.timer.cancel();
            }
            Timer::Keep => {}
        }
        out.follow(shell, id);
    }

    /// Under auto-hide the surface takes the pointer on the strip, and on the island while
    /// shown; the rest lets it through to the windows below.
    fn limit_input(&mut self, shell: &mut Shell) {
        for (id, out) in &mut self.outs {
            let (Some(hider), Some(placement), Some(ui)) =
                (&out.hider, out.placement, shell.ui(*id))
            else {
                continue;
            };
            let (width, height) = ui.size();
            let whole = autohide::Rect {
                x: 0,
                y: 0,
                w: width.round() as i32,
                h: height.round() as i32,
            };
            let island = out
                .island
                .filter(|_| hider.shown)
                .and_then(|island| ui.island_fill(island))
                .map(|r| autohide::Rect {
                    x: r.x.round() as i32,
                    y: r.y.round() as i32,
                    w: r.w.round() as i32,
                    h: r.h.round() as i32,
                });
            let region: Vec<Rect> = autohide::input_region(placement.anchor, whole, island)
                .into_iter()
                .map(|r| Rect {
                    x: r.x as f32,
                    y: r.y as f32,
                    w: r.w as f32,
                    h: r.h as f32,
                })
                .collect();
            if out.region.as_ref() == Some(&region) {
                continue;
            }
            match shell.set_input_region(*id, Some(&region)) {
                Ok(()) => out.region = Some(region),
                Err(err) => {
                    tracing::error!(error = %err, "cannot limit the dock's input to its strip")
                }
            }
        }
    }

    /// READY=1 once (Type=notify): at the first frame a surface with the island committed
    /// (`Application::drawn`, in the turn that drew it), as the GTK dock at its window's
    /// `map`, or at once when no output gets one (no client, the knob off, the `bar` preset).
    fn notify_ready(&mut self, shell: &Shell) {
        if self.ready {
            return;
        }
        let ready = match &self.client {
            None => true,
            Some(client) => {
                client
                    .outputs()
                    .iter()
                    .all(|output| placement::place(self.layout, output, self.rtl).is_none())
                    || self
                        .outs
                        .iter()
                        .any(|(id, out)| out.placement.is_some() && shell.drawn(*id))
            }
        };
        if !ready {
            return;
        }
        self.ready = true;
        if let Err(err) = athanor_unit::notify::notify_ready() {
            tracing::error!(error = %err, "cannot tell systemd the dock is ready");
        }
    }

    fn open(&self, opener: Opener) {
        if let Some(client) = &self.client {
            row::open(client, opener);
        }
    }

    fn press(&mut self, shell: &mut Shell, surface: SurfaceId, button: WidgetId) {
        let (Some(client), Some(state)) = (
            self.client.clone(),
            self.outs.get(&surface).and_then(|out| out.entry(button)),
        ) else {
            return;
        };
        if let Press::Menu = row::press(&client, state, APP) {
            self.open_menu(shell, surface, button);
        }
    }

    fn open_menu(&mut self, shell: &mut Shell, surface: SurfaceId, button: WidgetId) {
        let Some(out) = self.outs.get(&surface) else {
            return;
        };
        let (Some(state), Some(placement)) = (out.entry(button).cloned(), out.placement) else {
            return;
        };
        let scale = out.info.scale_factor.clamp(1, 16) as u16;
        let mut ui = Ui::new(&row::menu_title(&state), self.look.menu.clone(), self.rtl);
        let rows = row::menu(&mut ui, &mut self.icons, scale, &state, &self.favorites, |pinned| {
            if pinned {
                tr("Unpin from Dock")
            } else {
                tr("Pin to Dock")
            }
        });
        self.stale |= rows.stale;
        match shell.open_popup(surface, button, ui, edge::menu_side(placement.anchor)) {
            Ok(popup) => {
                self.menu = Some(Menu {
                    parent: surface,
                    popup,
                    rows,
                });
                self.hold(shell, true);
            }
            Err(err) => tracing::error!(error = %err, "cannot open the entry's menu"),
        }
    }

    /// Ends a drag: at its end, or when a rebuild or a closed surface takes away the
    /// press that would have ended it, which would otherwise hold the row and auto-hide.
    fn drop_drag(&mut self, shell: &mut Shell) {
        if self.dragging.take().is_some() {
            self.hold(shell, false);
            self.stale = true;
        }
    }

    fn close_menu(&mut self, shell: &mut Shell) {
        if self.menu.take().is_some() {
            shell.close_popup();
            self.hold(shell, false);
        }
    }

    fn menu_act(&mut self, shell: &mut Shell, act: row::MenuAct) {
        self.close_menu(shell);
        if let Some(client) = &self.client {
            row::menu_act(client, &self.favorites, act, APP);
        }
        self.stale = true;
    }

    /// The desktop id of a pinned entry's button.
    fn pinned(&self, surface: SurfaceId, button: WidgetId) -> Option<String> {
        self.outs.get(&surface)?.row.pinned(button)
    }
}

impl Application for Dock {
    /// Every output keeps a surface while the dock has a client; `output_changed`, which
    /// follows at once, places it.
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        self.client.as_ref()?;
        let mut ui = Ui::new(&tr("Dock"), self.look.dock.clone(), self.rtl);
        ui.min_width = 1.0;
        ui.min_height = 1.0;
        Some((ui, edge::layer(None, 0)))
    }

    fn output_changed(&mut self, shell: &mut Shell, surface: SurfaceId, output: &OutputInfo) {
        let placement = self.place(output);
        if let Some(out) = self.outs.get_mut(&surface) {
            let same = out.placement == placement && out.info.scale_factor == output.scale_factor;
            out.info = output.clone();
            if same {
                return;
            }
            out.placement = placement;
        } else {
            self.outs.insert(
                surface,
                Out {
                    info: output.clone(),
                    placement,
                    island: None,
                    acts: Vec::new(),
                    row: Row::default(),
                    hider: None,
                    keyboard: false,
                    region: None,
                },
            );
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.parent == surface)
        {
            self.close_menu(shell);
        }
        if self.dragging == Some(surface) {
            self.drop_drag(shell);
        }
        let states = self.states();
        self.build(shell, surface, &states, false);
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if let Some(menu) = self.menu.as_ref().filter(|menu| menu.popup == surface) {
            if let Event::Clicked(row) = event {
                if let Some(act) = menu.rows.act(row) {
                    self.menu_act(shell, act);
                }
            }
            return;
        }
        let Some(out) = self.outs.get(&surface) else {
            return;
        };
        match event {
            Event::Clicked(button) => match out.act(button) {
                Some(opener) => self.open(opener),
                None => self.press(shell, surface, button),
            },
            Event::ContextMenu(button) => {
                if out.entry(button).is_some() {
                    self.open_menu(shell, surface, button);
                }
            }
            Event::DragStarted(button) => {
                if self.pinned(surface, button).is_some() && self.dragging.is_none() {
                    self.dragging = Some(surface);
                    self.hold(shell, true);
                }
            }
            Event::DragEnded { from, to, .. } => {
                self.drop_drag(shell);
                let dragged = self.pinned(surface, from);
                let onto = to.and_then(|to| self.pinned(surface, to));
                if let (Some(dragged), Some(onto)) = (dragged, onto) {
                    if dragged != onto {
                        self.favorites
                            .change(|ids| Ok(favorites::moved(ids, &dragged, &onto)));
                    }
                }
                self.stale = true;
            }
            _ => {}
        }
    }

    fn pointer(&mut self, shell: &mut Shell, surface: SurfaceId, inside: bool) {
        self.feed(
            shell,
            surface,
            if inside {
                Hide::PointerIn
            } else {
                Hide::PointerOut
            },
        );
    }

    fn keyboard(&mut self, shell: &mut Shell, surface: SurfaceId, focused: bool) {
        if let Some(out) = self.outs.get_mut(&surface) {
            out.keyboard = focused;
        }
        self.feed(
            shell,
            surface,
            if focused {
                Hide::KeyboardIn
            } else {
                Hide::KeyboardOut
            },
        );
    }

    fn drawn(&mut self, shell: &mut Shell, _surface: SurfaceId) {
        self.notify_ready(shell);
    }

    fn turn(&mut self, shell: &mut Shell) {
        // The client reads its own connection: once it stops, the window list freezes and
        // no launch can go through the security context. End as the GTK dock does when
        // its one connection closes, so that systemd sees the failure and restarts it.
        if self.client.as_ref().is_some_and(|client| client.is_closed()) {
            shell.fail("the compositor client's connection closed".into());
            return;
        }
        self.outs.retain(|id, _| shell.ui(*id).is_some());
        if self.dragging.is_some_and(|id| !self.outs.contains_key(&id)) {
            self.drop_drag(shell);
        }
        // A menu closed by Escape, a press outside it or the compositor.
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| shell.popup_parent() != Some(menu.parent))
        {
            self.menu = None;
            self.hold(shell, false);
        }
        let changes = std::mem::take(&mut *self.queue.borrow_mut());
        for change in changes {
            if reads_icons_again(&change) {
                self.icons.reset(self.look.dock.text);
            }
            match change {
                Change::Windows => self.stale = true,
                Change::Files => self
                    .debounce
                    .start(DEBOUNCE, &self.queue, || Change::Reload),
                Change::Reload => self.reload(shell),
                Change::Appearance(current) => {
                    self.look = look::look(&current);
                    // The new look tints the images anew.
                    self.icons.reset(self.look.dock.text);
                    self.rebuild_all(shell);
                }
                Change::Apps => {
                    self.apps = None;
                    self.stale = true;
                }
                Change::Timer(id, generation) => {
                    if self
                        .outs
                        .get(&id)
                        .and_then(|out| out.hider.as_ref())
                        .is_some_and(|h| h.generation == generation)
                    {
                        self.feed(shell, id, Hide::Timer);
                    }
                }
            }
        }
        if self.stale && self.menu.is_none() && self.dragging.is_none() {
            self.refresh(shell);
        }
        self.limit_input(shell);
        self.notify_ready(shell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_look_or_a_change_of_the_installed_applications_reads_the_icons_again() {
        for change in [Change::Windows, Change::Files, Change::Reload] {
            assert!(!reads_icons_again(&change), "the images stay");
        }
        for change in [Change::Appearance(Appearance::default()), Change::Apps] {
            assert!(reads_icons_again(&change), "the images are read again");
        }
    }
}
