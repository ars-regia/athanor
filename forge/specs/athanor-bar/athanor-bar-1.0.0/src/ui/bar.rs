//! The bar's surfaces (doc_bar.md, BR3, BR6, BR7): one layer surface per output along the
//! layout's panel edge, the preset's three groups in a centre box in the order of
//! `order::groups`, which the toolkit mirrors under right-to-left text as `order::visual`
//! does, and one popover open at a time. Its inputs are the layout's layers and the
//! favourites file, watched live, the appearance, the installed applications and the
//! compositor's windows. GLib's callbacks only queue a change, which `turn` applies with the
//! shell in hand.
//!
//! The launcher, application-library and workspaces openers, the running applications with
//! their menus and the control center's button are drawn here; every module with a popover is
//! a [`modules::Module`] of its own file, registered in `modules::new`. A module not drawn yet
//! shows no button, and a preset's group keeps its place without it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use athanor_apps::entries::{self, Apps, State};
use athanor_apps::favorites::Store;
use athanor_apps::sal::{self as row, Icons, Press, Row};
use athanor_bar::control_center::{NAME, PATH};
use athanor_bar::i18n::{self, tr};
use athanor_bar::order::{self, Module};
use athanor_bar::{bus, start};
use athanor_layout::loader::{Paths, Source};
use athanor_layout::placement::Output;
use athanor_layout::preset::{Layout, PanelEdge, Preset};
use athanor_sal::driver::{
    Anchor as Edges, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell,
    Side, SurfaceId,
};
use athanor_sal::{Event, Priority, Ui, WidgetId};
use athanor_style::appearance::{self, Appearance};
use athanor_sulphur_client::{Client, Event as Compositor, Opener};
use gio::glib;
use gio::prelude::*;

use crate::ui::look::{self, Look};
use crate::ui::modules::{self, Around, Cx, Reply, Wake, Wakeup, ICON};

/// The program's name: of its failed-start notifications, as the GTK bar's (`Host::APP`).
const APP: &str = "athanor-bar";
/// The layer shell namespace cosmic-comp and the rig know the bar by.
const NAMESPACE: &str = "athanor-bar";
/// Editors write a file in several steps: the bar acts this long after the last event.
const DEBOUNCE: Duration = Duration::from_millis(250);
/// The icon theme the GTK bar draws with.
const ICON_THEME: &str = "Adwaita";

/// What GLib's callbacks tell the bar, applied at the next turn.
enum Change {
    Windows,
    /// A file in a watched directory changed: a reload follows the debounce.
    Files,
    Reload,
    Appearance(Appearance),
    Apps,
    /// Once a second: a turn, at which the clock's button reads the time.
    Tick,
    /// Whether the control center can be started on the session bus (CC1).
    ControlCenter(bool),
    /// A change of the compositor's other than of the windows: the modules hear it.
    Compositor(Compositor),
    /// What a module's source asks.
    Module(Wakeup),
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

/// What a module's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Open(Opener),
    /// The button opens the module's popover.
    Popover(Module),
    ControlCenter,
}

/// One output's surface.
struct Out {
    info: OutputInfo,
    acts: Vec<(WidgetId, Act)>,
    row: Row,
    /// The output's connector, whose active workspace the tiling switch changes.
    connector: Option<String>,
}

impl Out {
    fn act(&self, widget: WidgetId) -> Option<Act> {
        self.acts
            .iter()
            .find(|(w, _)| *w == widget)
            .map(|(_, act)| *act)
    }

    fn scale(&self) -> u16 {
        self.info.scale_factor.clamp(1, 16) as u16
    }

    /// The button of `act` on this surface.
    fn button(&self, act: Act) -> Option<WidgetId> {
        self.acts
            .iter()
            .find(|(_, a)| *a == act)
            .map(|(widget, _)| *widget)
    }
}

/// What the open popover is.
enum Open {
    /// A running entry's menu.
    Entry(row::Menu),
    Module(Module),
}

/// The open popover, and the surface and the button it opened from.
struct Popover {
    parent: SurfaceId,
    anchor: WidgetId,
    popup: SurfaceId,
    open: Open,
}

pub struct Bar {
    /// `None` without the privileged globals: the modules that need it stay hidden (SH1).
    client: Option<Rc<Client>>,
    source: Source,
    layout: Layout,
    favorites: Store,
    /// Dropped when the installed applications change, and read again when next needed.
    apps: Option<Apps>,
    look: Look,
    appearance: Appearance,
    rtl: bool,
    icons: Icons,
    outs: HashMap<SurfaceId, Out>,
    queue: Queue,
    debounce: Slot,
    /// The modules with a popover, in the order of `Module::ALL`.
    modules: Vec<(Module, Box<dyn modules::Module>)>,
    /// The activation tokens asked for and not answered yet, with the module each goes to.
    tokens: Vec<(u64, Module)>,
    popover: Option<Popover>,
    control_center: bool,
    /// The windows or the favourites changed while a popover held the rows.
    stale: bool,
    /// A module's button went stale while a popover was open: the surfaces are drawn again
    /// once it closes.
    modules_stale: bool,
    ready: bool,
    /// The popover the captures of BR9 ask for (`ATHANOR_BAR_OPEN`), opened once the bar is
    /// ready and shows the module's button.
    open_on_start: Option<Module>,
    /// The module and the surface of the last input on a module's own widgets, where the
    /// popover it makes ready opens.
    input_from: Option<(Module, SurfaceId)>,
    /// The layout paths and the state directory of the first-session pick, until it is done.
    first_session: Option<(Paths, PathBuf)>,
    _watches: Vec<gio::FileMonitor>,
    _apps_monitor: gio::AppInfoMonitor,
}

/// The output `info` as the layout's placement reads it.
fn output(info: &OutputInfo) -> Output {
    let (width, height) = info.logical_size.unwrap_or_else(|| {
        let scale = info.scale_factor.max(1);
        info.modes
            .iter()
            .find(|mode| mode.current)
            .map_or((0, 0), |mode| {
                (mode.dimensions.0 / scale, mode.dimensions.1 / scale)
            })
    });
    Output {
        connector: info.name.clone(),
        width,
        height,
    }
}

/// The layer surface of `layout`, `depth` logical pixels deep: along its panel edge and
/// across the whole output, its depth kept free of windows.
fn layer(layout: Layout, depth: i32) -> LayerOptions {
    let mut options = LayerOptions::new(Layer::Top);
    options.namespace = NAMESPACE.to_owned();
    let edge = match layout.panel() {
        PanelEdge::Top => Edges::TOP,
        PanelEdge::Bottom => Edges::BOTTOM,
    };
    options.anchor = edge | Edges::LEFT | Edges::RIGHT;
    options.exclusive_zone = depth;
    options.keyboard = KeyboardInteractivity::OnDemand;
    options
}

/// The side a popover opens on: towards the inside of the screen.
fn inside(layout: Layout) -> Side {
    match layout.panel() {
        PanelEdge::Top => Side::Bottom,
        PanelEdge::Bottom => Side::Top,
    }
}

/// Asks the session bus whether the control center can be started (CC1): its button shows
/// only then.
fn ask_control_center(queue: &Queue) {
    let queue = queue.clone();
    glib::spawn_future_local(async move {
        let activatable = async {
            let bus = gio::bus_get_future(gio::BusType::Session).await?;
            let reply = bus::call(
                &bus,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ListActivatableNames",
                None,
                bus::TIMEOUT_MS,
            )
            .await?;
            Ok::<_, glib::Error>(
                reply
                    .get::<(Vec<String>,)>()
                    .is_some_and(|(names,)| names.iter().any(|name| name == NAME)),
            )
        }
        .await;
        match activatable {
            Ok(present) => queue.borrow_mut().push(Change::ControlCenter(present)),
            Err(err) => {
                tracing::warn!(error = %err, "cannot tell whether the control center is installed; its button stays hidden")
            }
        }
    });
}

/// Shows or hides the control center (CC1), which answers on the session bus.
fn toggle_control_center() {
    glib::spawn_future_local(async {
        let result = async {
            let bus = gio::bus_get_future(gio::BusType::Session).await?;
            bus::call(&bus, NAME, PATH, NAME, "Toggle", None, bus::TIMEOUT_MS).await
        }
        .await;
        if let Err(err) = result {
            tracing::error!(error = %err, "the control center did not toggle");
        }
    });
}

impl Bar {
    pub fn new(
        source: Source,
        favorites_file: Option<PathBuf>,
        first_session: Option<(Paths, PathBuf)>,
        client: Option<Client>,
    ) -> Bar {
        let queue: Queue = Rc::default();
        let client = client.map(Rc::new);
        if let Some(client) = &client {
            let queue = queue.clone();
            client.connect_events(move |_, event| {
                let change = match event {
                    Compositor::WindowAdded(_)
                    | Compositor::WindowChanged(_)
                    | Compositor::WindowRemoved(_) => Change::Windows,
                    _ => Change::Compositor(event.clone()),
                };
                queue.borrow_mut().push(change);
            });
        }
        let wake: Wake = {
            let queue = queue.clone();
            Rc::new(move |wakeup| queue.borrow_mut().push(Change::Module(wakeup)))
        };
        let start = modules::Start {
            client: client.as_ref(),
            wake: &wake,
        };
        let modules = Module::ALL
            .into_iter()
            .filter_map(|module| Some((module, modules::new(module, &start)?)))
            .collect();
        // The layout's layers and the favourites file share these directories. Armed before
        // the layout and the favourites are read, so that a write while the bar starts is
        // read or reported, never lost.
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
        let ticked = queue.clone();
        glib::timeout_add_seconds_local(1, move || {
            ticked.borrow_mut().push(Change::Tick);
            glib::ControlFlow::Continue
        });
        ask_control_center(&queue);
        let current = Appearance::read();
        let look = look::look(&current, layout.preset());
        Bar {
            client,
            source,
            layout,
            favorites: Store::load(favorites_file),
            apps: None,
            icons: Icons::new(ICON_THEME, look.bar.text),
            look,
            appearance: current,
            rtl: i18n::is_rtl(),
            outs: HashMap::new(),
            queue,
            debounce: Slot::default(),
            modules,
            tokens: Vec::new(),
            popover: None,
            control_center: false,
            stale: false,
            modules_stale: false,
            ready: false,
            open_on_start: std::env::var("ATHANOR_BAR_OPEN")
                .ok()
                .and_then(|id| Module::from_id(&id)),
            input_from: None,
            first_session,
            _watches: watches,
            _apps_monitor: apps_monitor,
        }
    }

    /// The running entries now, each with its desktop entry.
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

    /// Adds `module`'s widgets to `group`; false when it is not drawn, without its source or
    /// not yet ported.
    fn module(
        &mut self,
        ui: &mut Ui,
        group: WidgetId,
        module: Module,
        scale: u16,
        states: &[State],
        out: &mut Out,
    ) -> bool {
        let own = self.client.as_ref().map(|client| client.own_compositor());
        let opener = match module {
            Module::Launcher => Some(Opener::Launcher),
            Module::AppLibrary => Some(Opener::AppLibrary),
            Module::Workspaces => Some(Opener::Workspaces),
            _ => None,
        };
        if let Some(opener) = opener {
            // Without the compositor client (SH1), and for an opener hidden under the own
            // compositor (doc_own_stack.md, decision 28), no button.
            if !own.is_some_and(|own| opener.shown(own)) {
                return false;
            }
            let (icon, name) = entries::opener(opener);
            let button = self.icons.button(ui, group, icon, ICON, scale, &name, None);
            out.acts.push((button, Act::Open(opener)));
            return true;
        }
        match module {
            Module::RunningApps => {
                if self.client.is_none() {
                    return false;
                }
                // The running line under an active entry, whatever the panel edge, as
                // `button.bar-button.running.active`.
                out.row = Row::build(ui, group, states, &mut self.icons, scale, Side::Bottom);
                true
            }
            Module::ControlCenter => {
                if !self.control_center {
                    return false;
                }
                let button = self.icons.button(
                    ui,
                    group,
                    "preferences-system-symbolic",
                    ICON,
                    scale,
                    &tr("Control center"),
                    None,
                );
                out.acts.push((button, Act::ControlCenter));
                true
            }
            _ => {
                let top = self.layout.panel() == PanelEdge::Top;
                let Some((_, drawn)) = self.modules.iter_mut().find(|(m, _)| *m == module) else {
                    return false;
                };
                let mut cx = Cx {
                    icons: &mut self.icons,
                    theme: &self.look.popover,
                    secondary: self.look.secondary,
                    rtl: self.rtl,
                    top,
                    scale,
                    connector: out.connector.as_deref(),
                };
                let Some(button) = drawn.button(&mut cx, ui, group) else {
                    return false;
                };
                if drawn.anchor(ui, button) == Some(button) {
                    ui.set_has_popup(button, true);
                }
                out.acts.push((button, Act::Popover(module)));
                true
            }
        }
    }

    /// Draws `id` again from the layout and `states`.
    fn build(&mut self, shell: &mut Shell, id: SurfaceId, states: &[State]) {
        let Some(info) = self.outs.get(&id).map(|out| out.info.clone()) else {
            return;
        };
        let scale = info.scale_factor.clamp(1, 16) as u16;
        let preset = self.layout.preset();
        let mut ui = Ui::new(&tr("Bar"), self.look.bar.clone(), self.rtl);
        let connector = info.name.clone();
        let mut out = Out {
            info,
            acts: Vec::new(),
            row: Row::default(),
            connector,
        };
        let line = ui.add(Ui::ROOT, athanor_sal::Kind::CenterBox);
        let groups = order::groups(preset);
        let top = self.layout.panel() == PanelEdge::Top;
        for modules in [&groups.start, &groups.centre, &groups.end] {
            // A float group is an island on the transparent surface.
            let holder = if preset == Preset::Float {
                ui.island(line, false, look::island_margin(top))
            } else {
                line
            };
            let group = ui.inline(holder, look::group_padding(preset));
            let mut any = false;
            for &module in modules {
                any |= self.module(&mut ui, group, module, scale, states, &mut out);
            }
            // A group with nothing in it is not drawn: an empty island is a facade.
            // The centre box keeps the group's place.
            if !any {
                ui.set_visible(group, false);
                if holder != line {
                    ui.set_visible(holder, false);
                }
            }
        }
        let (_, height) = ui.layout();
        let options = layer(self.layout, height.ceil() as i32);
        if let Err(err) = shell.set_layer(id, &options) {
            tracing::error!(error = %err, "cannot place the bar surface");
        }
        let Some(current) = shell.ui_mut(id) else {
            return;
        };
        // Across the output the surface keeps the width the compositor configured.
        ui.min_width = current.min_width;
        *current = ui;
        self.outs.insert(id, out);
    }

    /// Draws every surface again: the look, the layout or a module's source changed.
    fn rebuild_all(&mut self, shell: &mut Shell) {
        self.close_popover(shell);
        self.modules_stale = false;
        let states = self.states();
        let ids: Vec<SurfaceId> = self.outs.keys().copied().collect();
        for id in ids {
            self.build(shell, id, &states);
        }
        self.stale = false;
    }

    /// Draws again the surfaces whose running entries, or what their buttons show, changed.
    fn refresh(&mut self, shell: &mut Shell) {
        self.stale = false;
        let states = self.states();
        let changed: Vec<SurfaceId> = self
            .outs
            .iter()
            .filter(|(_, out)| row::redraws(out.row.states(), &states))
            .map(|(id, _)| *id)
            .collect();
        for id in changed {
            self.build(shell, id, &states);
        }
    }

    fn reload(&mut self, shell: &mut Shell) {
        let layout = self.source.layout();
        self.favorites.reload();
        if layout != self.layout {
            tracing::info!(layout = ?layout, "layout applied");
            self.layout = layout;
            self.look = look::look(&self.appearance, layout.preset());
            self.rebuild_all(shell);
        } else {
            self.stale = true;
        }
    }

    /// Sets each module's stale buttons to their sources in place, or marks the surfaces to be
    /// drawn again where a button cannot be set so.
    fn update_buttons(&mut self, shell: &mut Shell) {
        let top = self.layout.panel() == PanelEdge::Top;
        for (id, out) in &self.outs {
            let Some(ui) = shell.ui_mut(*id) else {
                continue;
            };
            for (module, drawn) in &mut self.modules {
                let mut cx = Cx {
                    icons: &mut self.icons,
                    theme: &self.look.popover,
                    secondary: self.look.secondary,
                    rtl: self.rtl,
                    top,
                    scale: out.scale(),
                    connector: out.connector.as_deref(),
                };
                if !drawn.stale(&cx) {
                    continue;
                }
                match out.button(Act::Popover(*module)) {
                    Some(button) if drawn.update(&mut cx, ui, button) => {}
                    _ => self.modules_stale = true,
                }
            }
        }
    }

    /// SH10: the first-session pick, on the outputs the shell knows, until it is done.
    fn pick_first_layout(&mut self, shell: &Shell) {
        let Some((paths, state)) = &self.first_session else {
            return;
        };
        let outputs: Vec<Output> = shell.outputs().map(output).collect();
        if start::pick_first_layout(paths, &start::first_session_marker(state), &outputs) {
            self.first_session = None;
        }
    }

    /// READY=1 once (Type=notify): at the first frame of a surface, as the GTK bar at its
    /// window's `map`. The first turns run before the outputs are announced, so an empty output
    /// list is not a reason to be ready.
    fn notify_ready(&mut self, shell: &mut Shell) {
        if self.ready {
            return;
        }
        if !self.outs.keys().any(|id| shell.drawn(*id)) {
            return;
        }
        self.ready = true;
        if let Err(err) = athanor_unit::notify::notify_ready() {
            tracing::error!(error = %err, "cannot tell systemd the bar is ready");
        }
        if let Some(module) = self.open_on_start {
            if !self.shows(module) {
                tracing::info!(
                    module = module.id(),
                    "the popover asked for opens once the module shows its button"
                );
            }
        }
    }

    /// Whether a surface shows `module`'s button.
    fn shows(&self, module: Module) -> bool {
        self.outs
            .values()
            .any(|out| out.button(Act::Popover(module)).is_some())
    }

    /// The popover the captures ask for, once the bar is ready and the module shows its
    /// button with its content final (`Module::ready`), as the GTK bar's modules kept the
    /// request until then: the notifications show theirs only once the daemon listed them.
    fn open_asked(&mut self, shell: &mut Shell) {
        let Some(module) = self.open_on_start.filter(|_| self.ready) else {
            return;
        };
        let ready = self
            .modules
            .iter()
            .find(|(m, _)| *m == module)
            .is_none_or(|(_, drawn)| drawn.ready());
        if !ready || !self.shows(module) {
            return;
        }
        self.open_on_start = None;
        self.open_module(shell, module);
    }

    /// The modules' own surfaces, after the bar's.
    fn module_surfaces(&mut self, shell: &mut Shell) {
        let covered = self
            .popover
            .as_ref()
            .and_then(|p| self.outs.get(&p.parent))
            .and_then(|out| out.info.name.clone());
        let around = Around {
            theme: &self.look.popover,
            rtl: self.rtl,
            top: self.layout.panel() == PanelEdge::Top,
            covered: covered.as_deref(),
            popover: self.popover.is_some(),
        };
        for (_, drawn) in &mut self.modules {
            drawn.surfaces(shell, &around);
        }
    }

    /// Opens `module`'s popover on the surface of the first output by name, for the captures
    /// of BR9 and for a module's source that asks.
    fn open_module(&mut self, shell: &mut Shell, module: Module) {
        let first = self
            .outs
            .iter()
            .min_by(|a, b| a.1.info.name.cmp(&b.1.info.name))
            .map(|(id, _)| *id);
        let found =
            first.and_then(|surface| Some((surface, self.anchor(shell, surface, module)?)));
        match found {
            Some((surface, button)) => self.open_popover(shell, surface, button, module),
            None => tracing::warn!(
                module = module.id(),
                "a module's popover was asked for, but the bar does not show the module"
            ),
        }
    }

    /// The widget `module`'s popover opens from on `surface`: the one the module names, else
    /// its button.
    fn anchor(&self, shell: &Shell, surface: SurfaceId, module: Module) -> Option<WidgetId> {
        let button = self.outs.get(&surface)?.button(Act::Popover(module))?;
        let (_, drawn) = self.modules.iter().find(|(m, _)| *m == module)?;
        drawn.anchor(shell.ui(surface)?, button).or(Some(button))
    }

    /// Opens the popover `module` has ready, on the surface of its last input, or on the
    /// first output's when it was asked for by the captures.
    fn open_ready(&mut self, shell: &mut Shell, module: Module) {
        let from = self
            .input_from
            .filter(|(m, surface)| *m == module && self.outs.contains_key(surface));
        let Some((_, surface)) = from else {
            self.open_module(shell, module);
            return;
        };
        if let Some(anchor) = self.anchor(shell, surface, module) {
            self.open_popover(shell, surface, anchor, module);
        }
    }

    /// Hands an input on a widget inside a module's button to the module, when the button is
    /// a row of widgets of its own (the tray's items); false for any other widget. A press on
    /// the widget whose popover is open closes it, as a second press on a popover's button.
    fn module_input(
        &mut self,
        shell: &mut Shell,
        surface: SurfaceId,
        widget: WidgetId,
        event: &Event,
    ) -> bool {
        let (Some(out), Some(ui)) = (self.outs.get(&surface), shell.ui(surface)) else {
            return false;
        };
        // Down from the button, whose widgets are the surface's: `widget` came with the event.
        let inside = |button: WidgetId| {
            let mut stack = vec![button];
            while let Some(at) = stack.pop() {
                let children = &ui.widget(at).children;
                if children.contains(&widget) {
                    return true;
                }
                stack.extend(children);
            }
            false
        };
        let Some((module, button)) = out.acts.iter().find_map(|(button, act)| match act {
            Act::Popover(module) if *button != widget && inside(*button) => {
                Some((*module, *button))
            }
            _ => None,
        }) else {
            return false;
        };
        if matches!(event, Event::Clicked(_))
            && self
                .popover
                .as_ref()
                .is_some_and(|p| p.anchor == widget && p.parent == surface)
        {
            self.close_popover(shell);
            return true;
        }
        self.input_from = Some((module, surface));
        let (Some(ui), Some((_, drawn))) = (
            shell.ui(surface),
            self.modules.iter_mut().find(|(m, _)| *m == module),
        ) else {
            return true;
        };
        drawn.input(ui, button, widget, event);
        true
    }

    fn close_popover(&mut self, shell: &mut Shell) {
        if let Some(popover) = self.popover.take() {
            shell.close_popup();
            self.closed(popover.open);
        }
    }

    /// Tells the module whose popover closed.
    fn closed(&mut self, open: Open) {
        if let Open::Module(module) = open {
            if let Some((_, drawn)) = self.modules.iter_mut().find(|(m, _)| *m == module) {
                drawn.closed();
            }
        }
    }

    /// Opens `module`'s popover from its `button` on `surface`, replacing an open one.
    fn open_popover(
        &mut self,
        shell: &mut Shell,
        surface: SurfaceId,
        button: WidgetId,
        module: Module,
    ) {
        // The popover the new one replaces is told it closed, unless it is the module's own,
        // which a second page opens again.
        let reopening = self
            .popover
            .as_ref()
            .is_some_and(|p| matches!(p.open, Open::Module(m) if m == module));
        if !reopening {
            if let Some(popover) = self.popover.take() {
                self.closed(popover.open);
            }
        }
        let top = self.layout.panel() == PanelEdge::Top;
        let (Some(out), Some((_, drawn))) = (
            self.outs.get(&surface),
            self.modules.iter_mut().find(|(m, _)| *m == module),
        ) else {
            return;
        };
        let mut cx = Cx {
            icons: &mut self.icons,
            theme: &self.look.popover,
            secondary: self.look.secondary,
            rtl: self.rtl,
            top,
            scale: out.scale(),
            connector: out.connector.as_deref(),
        };
        let Some(ui) = drawn.open(&mut cx) else {
            return;
        };
        self.show_popover(shell, surface, button, ui, Open::Module(module));
    }

    fn show_popover(
        &mut self,
        shell: &mut Shell,
        surface: SurfaceId,
        button: WidgetId,
        ui: Ui,
        open: Open,
    ) {
        match shell.open_popup(surface, button, ui, inside(self.layout)) {
            Ok(popup) => {
                self.popover = Some(Popover {
                    parent: surface,
                    anchor: button,
                    popup,
                    open,
                });
            }
            Err(err) => {
                tracing::error!(error = %err, "cannot open the popover");
                self.popover = None;
                self.closed(open);
            }
        }
    }

    /// Acts on `event` of the open popover's widgets.
    fn popover_event(&mut self, shell: &mut Shell, event: Event) {
        let Some(popover) = &self.popover else {
            return;
        };
        let (parent, anchor) = (popover.parent, popover.anchor);
        let module = match (&popover.open, &event) {
            (Open::Entry(rows), Event::Clicked(widget)) => {
                if let Some(act) = rows.act(*widget) {
                    self.close_popover(shell);
                    if let Some(client) = &self.client {
                        row::menu_act(client, &self.favorites, act, APP);
                    }
                    self.stale = true;
                }
                return;
            }
            (Open::Entry(_), _) => return,
            (Open::Module(module), _) => *module,
        };
        let reply = self.with_module(shell, module, |drawn, cx, ui| drawn.event(cx, ui, event));
        self.reply(shell, parent, anchor, module, reply);
    }

    /// Runs `act` on `module` with the open popover's surface and widgets.
    fn with_module(
        &mut self,
        shell: &mut Shell,
        module: Module,
        act: impl FnOnce(&mut dyn modules::Module, &mut Cx, &mut Ui) -> Reply,
    ) -> Reply {
        let top = self.layout.panel() == PanelEdge::Top;
        let Some(popover) = &self.popover else {
            return Reply::Stay;
        };
        let (Some(out), Some(ui), Some((_, drawn))) = (
            self.outs.get(&popover.parent),
            shell.ui_mut(popover.popup),
            self.modules.iter_mut().find(|(m, _)| *m == module),
        ) else {
            return Reply::Stay;
        };
        let mut cx = Cx {
            icons: &mut self.icons,
            theme: &self.look.popover,
            secondary: self.look.secondary,
            rtl: self.rtl,
            top,
            scale: out.scale(),
            connector: out.connector.as_deref(),
        };
        act(drawn.as_mut(), &mut cx, ui)
    }

    fn reply(
        &mut self,
        shell: &mut Shell,
        parent: SurfaceId,
        anchor: WidgetId,
        module: Module,
        reply: Reply,
    ) {
        match reply {
            Reply::Stay => {}
            Reply::Close => self.close_popover(shell),
            Reply::Reopen => self.open_popover(shell, parent, anchor, module),
        }
    }

    /// Sets the open popover's widgets to their sources' state, after a change of them.
    fn refresh_popover(&mut self, shell: &mut Shell) {
        let Some((parent, anchor, module)) = self.popover.as_ref().and_then(|p| match p.open {
            Open::Module(module) => Some((p.parent, p.anchor, module)),
            Open::Entry(_) => None,
        }) else {
            return;
        };
        let reply = self.with_module(shell, module, |drawn, cx, ui| drawn.refresh(cx, ui));
        self.reply(shell, parent, anchor, module, reply);
    }

    /// Says `text` through the first surface, as the GTK bar through its first window.
    fn announce(&self, shell: &mut Shell, text: &str) {
        let first = self
            .outs
            .iter()
            .min_by(|a, b| a.1.info.name.cmp(&b.1.info.name))
            .map(|(id, _)| *id);
        if let Some(ui) = first.and_then(|id| shell.ui_mut(id)) {
            ui.announce(text, Priority::Polite);
        }
    }

    fn open_entry_menu(&mut self, shell: &mut Shell, surface: SurfaceId, button: WidgetId) {
        let Some(out) = self.outs.get(&surface) else {
            return;
        };
        let Some(state) = out.row.entry(button).cloned() else {
            return;
        };
        let scale = out.info.scale_factor.clamp(1, 16) as u16;
        let (mut ui, frame) = look::popover(
            &row::menu_title(&state),
            self.look.popover.clone(),
            self.rtl,
            self.layout.panel() == PanelEdge::Top,
        );
        let rows = row::menu(
            &mut ui,
            frame,
            &mut self.icons,
            scale,
            &state,
            &self.favorites,
            |pinned| {
                if pinned {
                    tr("Unpin from Bar")
                } else {
                    tr("Pin to Bar")
                }
            },
        );
        self.stale |= rows.stale;
        if let Some(popover) = self.popover.take() {
            self.closed(popover.open);
        }
        self.show_popover(shell, surface, button, ui, Open::Entry(rows));
    }

    fn press(&mut self, shell: &mut Shell, surface: SurfaceId, button: WidgetId) {
        let Some(out) = self.outs.get(&surface) else {
            return;
        };
        match out.act(button) {
            Some(Act::Open(opener)) => {
                if let Some(client) = &self.client {
                    row::open(client, opener);
                }
            }
            Some(Act::Popover(module)) => {
                // A second press closes the popover, as a GTK popover's button does.
                if self
                    .popover
                    .as_ref()
                    .is_some_and(|p| p.anchor == button && p.parent == surface)
                {
                    self.close_popover(shell);
                } else {
                    self.open_popover(shell, surface, button, module);
                }
            }
            Some(Act::ControlCenter) => toggle_control_center(),
            None => {
                let (Some(client), Some(state)) = (self.client.clone(), out.row.entry(button))
                else {
                    return;
                };
                if let Press::Menu = row::press(&client, state, APP) {
                    self.open_entry_menu(shell, surface, button);
                }
            }
        }
    }
}

impl Bar {
    fn hand_token(&mut self, module: Module, token: Option<String>) {
        if let Some((_, drawn)) = self.modules.iter_mut().find(|(m, _)| *m == module) {
            drawn.activation_token(token);
        }
    }
}

impl Application for Bar {
    /// Every output has a surface; `output_changed`, which follows at once, draws it.
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        let mut ui = Ui::new(&tr("Bar"), self.look.bar.clone(), self.rtl);
        ui.min_height = 1.0;
        Some((ui, layer(self.layout, 0)))
    }

    fn output_changed(&mut self, shell: &mut Shell, surface: SurfaceId, output: &OutputInfo) {
        let same = self
            .outs
            .get(&surface)
            .is_some_and(|out| out.info.scale_factor == output.scale_factor);
        if let Some(out) = self.outs.get_mut(&surface) {
            out.info = output.clone();
        } else {
            self.outs.insert(
                surface,
                Out {
                    info: output.clone(),
                    acts: Vec::new(),
                    row: Row::default(),
                    connector: output.name.clone(),
                },
            );
        }
        self.pick_first_layout(shell);
        if same {
            return;
        }
        if self.popover.as_ref().is_some_and(|p| p.parent == surface) {
            self.close_popover(shell);
        }
        let states = self.states();
        self.build(shell, surface, &states);
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if self.popover.as_ref().is_some_and(|p| p.popup == surface) {
            self.popover_event(shell, event);
            return;
        }
        if !self.outs.contains_key(&surface) {
            for (_, drawn) in &mut self.modules {
                drawn.surface_event(surface, &event);
            }
            return;
        }
        match event {
            Event::Clicked(widget)
            | Event::ContextMenu(widget)
            | Event::MiddleClick(widget)
            | Event::Wheel { widget, .. }
                if self.module_input(shell, surface, widget, &event) => {}
            Event::Clicked(button) => self.press(shell, surface, button),
            // Only a running entry has a menu: `open_entry_menu` does nothing for another.
            Event::ContextMenu(button) => self.open_entry_menu(shell, surface, button),
            _ => {}
        }
    }

    fn pointer(&mut self, _shell: &mut Shell, surface: SurfaceId, inside: bool) {
        for (_, drawn) in &mut self.modules {
            drawn.pointer(surface, inside);
        }
    }

    fn pressed(&mut self, shell: &mut Shell, surface: SurfaceId, at: (f32, f32)) {
        if self.outs.contains_key(&surface) {
            return;
        }
        let Some(ui) = shell.ui(surface) else {
            return;
        };
        match &self.popover {
            Some(popover) if popover.popup == surface => {
                let Open::Module(module) = popover.open else {
                    return;
                };
                if let Some((_, drawn)) = self.modules.iter_mut().find(|(m, _)| *m == module) {
                    drawn.pressed(ui, None, at);
                }
            }
            _ => {
                for (_, drawn) in &mut self.modules {
                    drawn.pressed(ui, Some(surface), at);
                }
            }
        }
    }

    fn activation_token(&mut self, _shell: &mut Shell, request: u64, token: String) {
        if let Some(at) = self.tokens.iter().position(|(asked, _)| *asked == request) {
            let (_, module) = self.tokens.remove(at);
            self.hand_token(module, Some(token));
        }
    }

    fn drawn(&mut self, shell: &mut Shell, _surface: SurfaceId) {
        self.notify_ready(shell);
        self.open_asked(shell);
    }

    fn turn(&mut self, shell: &mut Shell) {
        // The client reads its own connection: once it stops, the window list freezes and no
        // launch can go through the security context. End as the GTK bar does when its one
        // connection closes, so that systemd sees the failure and restarts it.
        if self
            .client
            .as_ref()
            .is_some_and(|client| client.is_closed())
        {
            shell.fail("the compositor client's connection closed".into());
            return;
        }
        self.outs.retain(|id, _| shell.ui(*id).is_some());
        // A popover closed by Escape, a press outside it or the compositor.
        if self
            .popover
            .as_ref()
            .is_some_and(|p| shell.popup_parent() != Some(p.parent))
        {
            if let Some(popover) = self.popover.take() {
                self.closed(popover.open);
            }
        }
        let changes = std::mem::take(&mut *self.queue.borrow_mut());
        for change in changes {
            match change {
                Change::Windows => self.stale = true,
                Change::Files => self
                    .debounce
                    .start(DEBOUNCE, &self.queue, || Change::Reload),
                Change::Reload => self.reload(shell),
                Change::Appearance(current) => {
                    self.look = look::look(&current, self.layout.preset());
                    self.appearance = current;
                    // After the look, whose ink tints the images anew.
                    self.icons.reset(self.look.bar.text);
                    // A module's popover stays open in the new look, as high contrast
                    // switched from the accessibility menu.
                    let open = self.popover.as_ref().and_then(|popover| match popover.open {
                        Open::Module(module) => Some((popover.parent, module)),
                        Open::Entry(_) => None,
                    });
                    self.rebuild_all(shell);
                    if let Some((surface, module)) = open {
                        if let Some(button) = self.anchor(shell, surface, module) {
                            self.open_popover(shell, surface, button, module);
                        }
                    }
                }
                Change::Apps => {
                    self.apps = None;
                    self.icons.reset(self.look.bar.text);
                    self.stale = true;
                }
                Change::Tick => {}
                Change::ControlCenter(present) => {
                    if present != self.control_center {
                        self.control_center = present;
                        self.rebuild_all(shell);
                    }
                }
                Change::Compositor(event) => {
                    let heard: Vec<String> = self
                        .modules
                        .iter_mut()
                        .filter_map(|(_, drawn)| drawn.heard(&event))
                        .collect();
                    for text in heard {
                        self.announce(shell, &text);
                    }
                    self.refresh_popover(shell);
                }
                Change::Module(Wakeup::Changed) => self.refresh_popover(shell),
                Change::Module(Wakeup::Activation(module)) => {
                    let input = shell.last_input();
                    match shell.request_activation_token(
                        input.map(|(surface, _)| surface),
                        input.map(|(_, serial)| serial),
                    ) {
                        Some(request) => self.tokens.push((request, module)),
                        None => self.hand_token(module, None),
                    }
                }
                Change::Module(Wakeup::Open(module)) => {
                    let open = self
                        .popover
                        .as_ref()
                        .filter(|p| matches!(p.open, Open::Module(m) if m == module))
                        .map(|p| (p.parent, p.anchor));
                    match open {
                        Some((surface, button)) => {
                            self.open_popover(shell, surface, button, module)
                        }
                        None => self.open_ready(shell, module),
                    }
                }
            }
        }
        // A source that asks for its popover, or whose open popover changed on its own, as the
        // pages of athanor-controls, which follow their models themselves.
        if self.ready {
            let asked = self
                .modules
                .iter_mut()
                .find_map(|(module, drawn)| drawn.wants_open().then_some(*module));
            if let Some(module) = asked {
                self.open_module(shell, module);
            }
        }
        let open = self.popover.as_ref().and_then(|popover| match popover.open {
            Open::Module(module) => Some(module),
            Open::Entry(_) => None,
        });
        if open.is_some_and(|open| {
            self.modules
                .iter()
                .any(|(module, drawn)| *module == open && drawn.changed())
        }) {
            self.refresh_popover(shell);
        }
        // The clock's minute, a keyboard aid's notice that expired, a layout or a workspace
        // that changed: each module says whether its buttons still show its source.
        self.update_buttons(shell);
        if self.modules_stale && self.popover.is_none() {
            self.rebuild_all(shell);
        }
        if self.stale && self.popover.is_none() {
            self.refresh(shell);
        }
        self.module_surfaces(shell);
        self.notify_ready(shell);
        self.open_asked(shell);
    }
}
