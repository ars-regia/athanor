//! The toolkit bar's surfaces (doc_bar.md, BR3, BR6, BR7), as `ui/` draws the GTK bar's: one
//! layer surface per output along the layout's panel edge, the preset's three groups in a
//! centre box in the order of `order::groups`, which the toolkit mirrors under right-to-left
//! text as `order::visual` does, and one popover open at a time. Its inputs are the GTK
//! bar's: the layout's layers and the favourites file, watched live, the appearance, the
//! installed applications and the compositor's windows. GLib's callbacks only queue a
//! change, which `turn` applies with the shell in hand.
//!
//! The modules drawn so far: the launcher, the application library and the workspaces
//! openers, the running applications with their menus, the clock and its calendar, the input
//! sources, the tiling switch, the accessibility menu with the keyboard aids, the power menu
//! and the control center's button. The others are not built yet, and a preset's group keeps
//! its place without them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use athanor_apps::entries::{self, Apps, State};
use athanor_apps::favorites::Store;
use athanor_apps::sal::{self as row, Icons, Press, Row};
use athanor_bar::clock::Clock;
use athanor_bar::control_center::{NAME, PATH};
use athanor_bar::i18n::{self, tr};
use athanor_bar::order::{self, Module};
use athanor_bar::power::Action;
use athanor_bar::{bus, logind, magnifier, screen_reader, start, tiling};
use athanor_layout::loader::{Paths, Source};
use athanor_layout::placement::Output;
use athanor_layout::preset::{Layout, PanelEdge, Preset};
use athanor_sal::driver::{
    Anchor as Edges, Application, KeyboardInteractivity, Layer, LayerOptions, OutputInfo, Shell,
    Side, SurfaceId,
};
use athanor_sal::{Event, Priority, TextStyle, Ui, WidgetId};
use athanor_style::appearance::{self, Appearance};
use athanor_sulphur_client::{theme, Client, Event as Compositor, Opener, Tiling, WorkspaceId};
use gio::glib;
use gio::prelude::*;

use crate::aids;
use crate::look::{self, Look};
use crate::modules::{self, AccessState, AccessUi};
use crate::power::{self, Act as PowerAct, Page};

/// The program's name: of its failed-start notifications, as the GTK bar's (`Host::APP`).
const APP: &str = "athanor-bar";
/// The layer shell namespace cosmic-comp and the rig know the bar by.
const NAMESPACE: &str = "athanor-bar";
/// Editors write a file in several steps: the bar acts this long after the last event.
const DEBOUNCE: Duration = Duration::from_millis(250);
/// The icon theme the GTK bar draws with.
const ICON_THEME: &str = "Adwaita";
/// The size of a module's symbolic icon, as GTK's default image size in a button.
const ICON: u16 = 16;

/// What GLib's callbacks tell the bar, applied at the next turn.
enum Change {
    Windows,
    /// A file in a watched directory changed: a reload follows the debounce.
    Files,
    Reload,
    Appearance(Appearance),
    Apps,
    /// Once a second: the clock.
    Tick,
    /// logind's answer for an action of the power menu.
    Offered(Action, bool),
    /// Whether the control center can be started on the session bus (CC1).
    ControlCenter(bool),
    /// The compositor's keyboard layouts, workspaces or accessibility state changed: the
    /// modules that show them.
    Modules,
    /// What the keyboard aids say of a change (doc_accessibility.md AX11).
    Announce(String),
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
    /// The clock's button, when its preset shows it.
    clock: Option<WidgetId>,
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
    Power(power::Menu),
    Calendar,
    /// Each layout's radio and its group.
    Input(Vec<(WidgetId, u32)>),
    /// The switch and the workspace it changes, with the mode it had when drawn.
    Tiling(WidgetId, (WorkspaceId, Tiling)),
    Accessibility(AccessUi),
}

impl Open {
    /// The module whose popover this is; `None` for a running entry's menu.
    fn module(&self) -> Option<Module> {
        match self {
            Open::Entry(_) => None,
            Open::Power(_) => Some(Module::Power),
            Open::Calendar => Some(Module::Clock),
            Open::Input(_) => Some(Module::InputSource),
            Open::Tiling(..) => Some(Module::Tiling),
            Open::Accessibility(_) => Some(Module::Accessibility),
        }
    }
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
    clock: Clock,
    /// The clock's text and the name screen readers say, as last drawn.
    clock_texts: Option<(String, String)>,
    popover: Option<Popover>,
    /// logind's last answers for the power menu, in the order of `Action::ALL`.
    offered: [bool; 5],
    control_center: bool,
    /// The windows or the favourites changed while a popover held the rows.
    stale: bool,
    /// A module's source changed while a popover was open: the surfaces are drawn again once
    /// it closes.
    modules_stale: bool,
    /// One keyboard aids' service for the bar: a notice is announced once.
    aids: Rc<aids::Service>,
    /// What the accessibility button showed beside its icon at the last drawing.
    aids_shown: String,
    /// The screen reader's key; without it, no row.
    reader_key: Option<gio::Settings>,
    /// The magnifier's key, followed for the compositor (doc_accessibility.md AX10).
    magnifier_key: Option<gio::Settings>,
    ready: bool,
    /// The popover the captures of BR9 ask for (`ATHANOR_BAR_OPEN`), opened once ready.
    open_on_start: Option<Module>,
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

/// Asks logind whether each action it decides is offered, for the power menu.
fn ask_logind(queue: &Queue) {
    for action in Action::ALL {
        if action.can_method().is_none() {
            continue;
        }
        let queue = queue.clone();
        glib::spawn_future_local(async move {
            let offered = logind::offered(action).await;
            queue.borrow_mut().push(Change::Offered(action, offered));
        });
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

/// Asks the compositor for what the modules show: the keyboard group, the workspaces' tiling
/// and the accessibility state. A source that cannot be watched leaves its module hidden.
fn watch_modules(client: &Client) {
    if let Err(err) = client.watch_keyboard_group() {
        tracing::warn!(error = %err, "the keyboard group is not followed");
    }
    if let Err(err) = client.watch_tiling() {
        tracing::warn!(error = %err, "the tiling of the workspaces is not followed");
    }
    // athanor-sulphur's manager is bound at the first watch, before the magnifier's key is
    // sent.
    if let Err(err) = client.watch_accessibility() {
        tracing::warn!(error = %err, "cannot watch the compositor's accessibility state");
    }
}

/// The magnifier's key (doc_accessibility.md AX10): true at the start turns the magnifier on,
/// and each change is sent to the compositor. `None` without the schema.
fn follow_magnifier(client: &Rc<Client>) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(magnifier::SCHEMA, true);
    if !schema.is_some_and(|schema| schema.has_key(magnifier::KEY)) {
        tracing::warn!(
            "{} has no {}; the magnifier row switches the compositor alone",
            magnifier::SCHEMA,
            magnifier::KEY
        );
        return None;
    }
    let settings = gio::Settings::new(magnifier::SCHEMA);
    let weak = Rc::downgrade(client);
    magnifier::follow(&settings, move |enabled| {
        if let Some(Err(err)) = weak.upgrade().map(|client| client.set_magnifier(enabled)) {
            tracing::info!(error = %err, enabled, "the magnifier's key cannot reach the compositor");
        }
    });
    Some(settings)
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
        let aids = Rc::new(aids::Service::default());
        let mut magnifier_key = None;
        if let Some(client) = &client {
            let (queue, heard) = (queue.clone(), aids.clone());
            client.connect_events(move |_, event| {
                let change = match event {
                    Compositor::WindowAdded(_)
                    | Compositor::WindowChanged(_)
                    | Compositor::WindowRemoved(_) => Change::Windows,
                    _ => Change::Modules,
                };
                let mut queue = queue.borrow_mut();
                if let Some(text) = heard.heard(event) {
                    queue.push(Change::Announce(text));
                }
                queue.push(change);
            });
            watch_modules(client);
            aids.start(client);
            magnifier_key = follow_magnifier(client);
        }
        let reader_key =
            screen_reader::settings(gio::SettingsSchemaSource::default().as_ref(), None);
        if reader_key.is_none() {
            tracing::warn!(
                "{} has no {}; the screen reader row is hidden",
                screen_reader::SCHEMA,
                screen_reader::KEY
            );
        }
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
        ask_logind(&queue);
        ask_control_center(&queue);
        let current = Appearance::read();
        let look = look::look(&current, layout.preset());
        let clock = Clock::new();
        let clock_texts = clock.texts();
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
            clock,
            clock_texts,
            popover: None,
            offered: [false; 5],
            control_center: false,
            stale: false,
            modules_stale: false,
            aids,
            aids_shown: String::new(),
            reader_key,
            magnifier_key,
            ready: false,
            open_on_start: std::env::var("ATHANOR_BAR_OPEN")
                .ok()
                .and_then(|id| Module::from_id(&id)),
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
    /// not yet built by the toolkit bar.
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
            Module::Clock => {
                let (short, long) = self.clock_texts.clone().unwrap_or_default();
                let button = ui.labelled_button(group, &long, None, 0.0, &short);
                ui.set_tooltip(button, Some(&long));
                ui.set_has_popup(button, true);
                out.acts.push((button, Act::Popover(Module::Clock)));
                // `.bar-clock`: semibold figures of one width.
                ui.set_text_style(
                    button,
                    TextStyle {
                        weight: 600,
                        features: vec![*b"tnum"],
                        ..TextStyle::default()
                    },
                );
                out.clock = Some(button);
                true
            }
            Module::Power => {
                let button = self.icons.button(
                    ui,
                    group,
                    "system-shutdown-symbolic",
                    ICON,
                    scale,
                    &tr("Power"),
                    None,
                );
                ui.set_has_popup(button, true);
                out.acts.push((button, Act::Popover(Module::Power)));
                true
            }
            Module::InputSource => {
                let Some(name) = self.client.as_deref().and_then(modules::input_name) else {
                    return false;
                };
                let icon = "input-keyboard-symbolic";
                let button = self.icons.button(ui, group, icon, ICON, scale, &name, None);
                ui.set_has_popup(button, true);
                out.acts.push((button, Act::Popover(module)));
                true
            }
            Module::Tiling => {
                let connector = out.connector.as_deref();
                let shown = self
                    .client
                    .as_deref()
                    .and_then(|client| modules::tiling_target(client, connector))
                    .is_some();
                if !shown {
                    return false;
                }
                let icon = "view-grid-symbolic";
                let button = self.icons.button(ui, group, icon, ICON, scale, &tr("Tiling"), None);
                ui.set_has_popup(button, true);
                out.acts.push((button, Act::Popover(module)));
                true
            }
            Module::Accessibility => {
                // The keyboard aids' state beside the icon (AX11).
                let shown = self.aids.shown();
                let name = tr("Accessibility");
                let icon = "preferences-desktop-accessibility-symbolic";
                let button = match self.icons.image(icon, ICON, scale) {
                    Some(image) if !shown.is_empty() => {
                        let button =
                            ui.labelled_button(group, &name, Some(image), f32::from(ICON), &shown);
                        ui.set_tooltip(button, Some(&name));
                        button
                    }
                    _ => self.icons.button(ui, group, icon, ICON, scale, &name, None),
                };
                ui.set_has_popup(button, true);
                out.acts.push((button, Act::Popover(module)));
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
            _ => false,
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
            clock: None,
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
        self.aids_shown = self.aids.shown();
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

    /// The clock's text, once a second: each surface's button changes only when it does.
    fn tick(&mut self, shell: &mut Shell) {
        let Some(texts) = self.clock.texts() else {
            return;
        };
        if self.clock_texts.as_ref() == Some(&texts) {
            return;
        }
        let (short, long) = &texts;
        for (id, out) in &self.outs {
            let (Some(button), Some(ui)) = (out.clock, shell.ui_mut(*id)) else {
                continue;
            };
            ui.set_shown_text(button, short);
            ui.set_text(button, long);
            ui.set_tooltip(button, Some(long));
            ui.layout();
        }
        self.clock_texts = Some(texts);
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
    /// window's `map`; then the popover the captures ask for. The first turns run before the
    /// outputs are announced, so an empty output list is not a reason to be ready.
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
        if let Some(module) = self.open_on_start.take() {
            self.open_module(shell, module);
        }
    }

    /// Opens `module`'s popover on the surface of the first output by name, for the captures
    /// of BR9.
    fn open_module(&mut self, shell: &mut Shell, module: Module) {
        let target = Some(Act::Popover(module));
        let first = self
            .outs
            .iter()
            .min_by(|a, b| a.1.info.name.cmp(&b.1.info.name))
            .map(|(id, _)| *id);
        let found = target.zip(first).and_then(|(act, surface)| {
            let button = self.outs.get(&surface)?.button(act)?;
            Some((surface, button))
        });
        match found {
            Some((surface, button)) => self.open_popover(shell, surface, button, module),
            None => tracing::warn!(
                module = module.id(),
                "ATHANOR_BAR_OPEN names a module the toolkit bar does not show"
            ),
        }
    }

    fn close_popover(&mut self, shell: &mut Shell) {
        if self.popover.take().is_some() {
            shell.close_popup();
        }
    }

    fn open_power(&mut self, shell: &mut Shell, surface: SurfaceId, button: WidgetId, page: Page) {
        if page == Page::Actions {
            ask_logind(&self.queue);
        }
        let top = self.layout.panel() == PanelEdge::Top;
        let (ui, menu) = power::build(self.look.popover.clone(), self.rtl, top, page, &self.offered);
        self.show_popover(shell, surface, button, ui, Open::Power(menu));
    }

    /// Opens `module`'s popover from its `button` on `surface`.
    fn open_popover(
        &mut self,
        shell: &mut Shell,
        surface: SurfaceId,
        button: WidgetId,
        module: Module,
    ) {
        let (theme, rtl) = (self.look.popover.clone(), self.rtl);
        let top = self.layout.panel() == PanelEdge::Top;
        let Some(out) = self.outs.get(&surface) else {
            return;
        };
        let scale = out.info.scale_factor.clamp(1, 16) as u16;
        let client = self.client.clone();
        let (ui, open) = match (module, client) {
            (Module::Power, _) => return self.open_power(shell, surface, button, Page::Actions),
            (Module::Clock, _) => {
                let Some(ui) = modules::calendar(theme, rtl, top, &mut self.icons, scale) else {
                    return;
                };
                (ui, Open::Calendar)
            }
            (Module::InputSource, Some(client)) => {
                let (ui, radios) = modules::input(theme, rtl, top, &client);
                (ui, Open::Input(radios))
            }
            (Module::Tiling, Some(client)) => {
                let Some(target) = modules::tiling_target(&client, out.connector.as_deref())
                else {
                    return;
                };
                let (ui, switch) = modules::tiling(theme, rtl, top, target.1);
                (ui, Open::Tiling(switch, target))
            }
            (Module::Accessibility, client) => {
                let state = AccessState {
                    reader: self.reader_key.as_ref(),
                    compositor: client.and_then(|client| client.accessibility()),
                    offer: self.aids.offer(),
                };
                let (ui, widgets) = modules::accessibility(theme, rtl, top, &state);
                (ui, Open::Accessibility(widgets))
            }
            _ => return,
        };
        self.show_popover(shell, surface, button, ui, open);
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
            Err(err) => tracing::error!(error = %err, "cannot open the popover"),
        }
    }

    /// Acts on `event` of the open popover's widgets.
    fn popover_event(&mut self, shell: &mut Shell, event: Event) {
        let Some(popover) = &self.popover else {
            return;
        };
        let popup = popover.popup;
        match (&popover.open, event) {
            (Open::Entry(rows), Event::Clicked(widget)) => {
                if let Some(act) = rows.act(widget) {
                    self.close_popover(shell);
                    if let Some(client) = &self.client {
                        row::menu_act(client, &self.favorites, act, APP);
                    }
                    self.stale = true;
                }
            }
            (Open::Power(menu), Event::Clicked(widget)) => {
                if let Some(act) = menu.act(widget) {
                    self.power_act(shell, act);
                }
            }
            (Open::Input(radios), Event::Selected(widget)) => {
                let group = radios.iter().find(|(radio, _)| *radio == widget);
                if let (Some((_, group)), Some(client)) = (group, &self.client) {
                    if let Err(err) = client.set_keyboard_group(*group) {
                        tracing::error!(error = %err, "cannot change the input source");
                    }
                }
            }
            (Open::Tiling(switch, (id, mode)), Event::Toggled(widget)) if *switch == widget => {
                let (switch, id, mode) = (*switch, *id, *mode);
                let sent = self
                    .client
                    .as_ref()
                    .map(|client| client.set_tiling(id, tiling::toggled(mode)));
                if let Some(Err(err)) = sent {
                    tracing::error!(error = %err, "cannot change the tiling mode");
                    // No workspace event follows a request that was not sent: show the mode
                    // the workspace still has.
                    if let Some(ui) = shell.ui_mut(popup) {
                        ui.set_active(switch, mode == Tiling::Tiled);
                    }
                }
            }
            (
                Open::Accessibility(_),
                Event::Toggled(widget) | Event::Selected(widget) | Event::Clicked(widget),
            ) => self.access_event(shell, popup, widget),
            _ => {}
        }
    }

    /// Acts on the accessibility menu's `widget`, as the GTK menu's rows do.
    fn access_event(&mut self, shell: &mut Shell, popup: SurfaceId, widget: WidgetId) {
        let Some(Popover {
            open: Open::Accessibility(widgets),
            parent,
            anchor,
            ..
        }) = &self.popover
        else {
            return;
        };
        let (parent, anchor) = (*parent, *anchor);
        let Some(ui) = shell.ui_mut(popup) else {
            return;
        };
        let on = ui.is_active(widget);
        let compositor = self.client.as_ref().and_then(|client| client.accessibility());
        if widget == widgets.high_contrast {
            if let Err(err) =
                appearance::apply(|choice| choice.high_contrast = on, theme::write_cosmic)
            {
                tracing::error!(error = %err, "cannot switch high contrast");
                ui.set_active(widget, Appearance::read().high_contrast);
            }
        } else if Some(widget) == widgets.reader {
            if let Some(key) = &self.reader_key {
                if let Err(err) = key.set_boolean(screen_reader::KEY, on) {
                    tracing::error!(error = %err, "cannot switch the screen reader");
                    ui.set_active(widget, key.boolean(screen_reader::KEY));
                }
            }
        } else if Some(widget) == widgets.magnifier {
            let sent = magnifier::switch(self.magnifier_key.as_ref(), on, |enabled| {
                self.client
                    .as_ref()
                    .map_or(Ok(()), |client| client.set_magnifier(enabled))
                    .map_err(|err| err.to_string())
            });
            if let Err(err) = sent {
                tracing::error!(error = %err, "cannot switch the magnifier");
                // Show the compositor's state again.
                if let Some(state) = compositor {
                    widgets.show(ui, state);
                }
            }
        } else if Some(widget) == widgets.accept {
            self.aids.accept();
            // The menu again, without the offer, and the button without it.
            self.modules_stale = true;
            self.open_popover(shell, parent, anchor, Module::Accessibility);
        } else if Some(widget) == widgets.inverted
            || widgets.filters.iter().any(|(radio, _)| *radio == widget)
        {
            // Inversion and the filter are one compositor request: each sends the other's
            // state.
            if let (Some((inverted, filter)), Some(client)) =
                (widgets.screen_filter(ui), &self.client)
            {
                if let Err(err) = client.set_screen_filter(inverted, filter) {
                    tracing::error!(error = %err, "cannot change the screen filter");
                    if let Some(state) = compositor {
                        widgets.show(ui, state);
                    }
                }
            }
        }
    }

    /// Sets the open popover's widgets to their sources' state, after a change of them.
    fn refresh_popover(&mut self, shell: &mut Shell) {
        let (Some(popover), Some(client)) = (self.popover.as_mut(), &self.client) else {
            return;
        };
        let Some(ui) = shell.ui_mut(popover.popup) else {
            return;
        };
        let connector = self
            .outs
            .get(&popover.parent)
            .and_then(|out| out.connector.as_deref());
        match &mut popover.open {
            Open::Input(radios) => {
                let group = client.keyboard_group();
                if let Some((radio, _)) = radios.iter().find(|(_, g)| *g == group) {
                    ui.set_active(*radio, true);
                }
            }
            Open::Tiling(switch, drawn) => match modules::tiling_target(client, connector) {
                Some(target) if target.0 == drawn.0 => {
                    ui.set_active(*switch, target.1 == Tiling::Tiled);
                    *drawn = target;
                }
                // Another workspace is active, or none reports its mode.
                _ => self.close_popover(shell),
            },
            Open::Accessibility(widgets) => {
                if let Some(state) = client.accessibility() {
                    widgets.show(ui, state);
                }
            }
            _ => {}
        }
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

    fn power_act(&mut self, shell: &mut Shell, act: PowerAct) {
        let Some((parent, anchor)) = self.popover.as_ref().map(|p| (p.parent, p.anchor)) else {
            return;
        };
        match act {
            PowerAct::Ask(action) => self.open_power(shell, parent, anchor, Page::Confirm(action)),
            PowerAct::Cancel => self.open_power(shell, parent, anchor, Page::Actions),
            PowerAct::Run(action) => {
                self.close_popover(shell);
                glib::spawn_future_local(async move {
                    if let Err(err) = logind::run(action).await {
                        tracing::error!(error = %err, action = action.id(), "the power action failed");
                    }
                });
            }
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
        match shell.open_popup(surface, button, ui, inside(self.layout)) {
            Ok(popup) => {
                self.popover = Some(Popover {
                    parent: surface,
                    anchor: button,
                    popup,
                    open: Open::Entry(rows),
                });
            }
            Err(err) => tracing::error!(error = %err, "cannot open the entry's menu"),
        }
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
                    clock: None,
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
        match event {
            Event::Clicked(button) => self.press(shell, surface, button),
            // Only a running entry has a menu: `open_entry_menu` does nothing for another.
            Event::ContextMenu(button) => self.open_entry_menu(shell, surface, button),
            _ => {}
        }
    }

    fn drawn(&mut self, shell: &mut Shell, _surface: SurfaceId) {
        self.notify_ready(shell);
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
            self.popover = None;
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
                    let open = self.popover.as_ref().and_then(|popover| {
                        Some((popover.parent, popover.open.module()?))
                    });
                    self.rebuild_all(shell);
                    if let Some((surface, module)) = open {
                        let button = self
                            .outs
                            .get(&surface)
                            .and_then(|out| out.button(Act::Popover(module)));
                        if let Some(button) = button {
                            self.open_popover(shell, surface, button, module);
                        }
                    }
                }
                Change::Apps => {
                    self.apps = None;
                    self.icons.reset(self.look.bar.text);
                    self.stale = true;
                }
                Change::Tick => self.tick(shell),
                Change::Offered(action, offered) => {
                    if let Some(at) = Action::ALL.iter().position(|a| *a == action) {
                        self.offered[at] = offered;
                    }
                }
                Change::ControlCenter(present) => {
                    if present != self.control_center {
                        self.control_center = present;
                        self.rebuild_all(shell);
                    }
                }
                Change::Modules => {
                    self.modules_stale = true;
                    self.refresh_popover(shell);
                }
                Change::Announce(text) => self.announce(shell, &text),
            }
        }
        // A rejected key or an offer expires with time, not with an event.
        if self.aids.shown() != self.aids_shown {
            self.modules_stale = true;
        }
        if self.modules_stale && self.popover.is_none() {
            self.modules_stale = false;
            self.rebuild_all(shell);
        }
        if self.stale && self.popover.is_none() {
            self.refresh(shell);
        }
        self.notify_ready(shell);
    }
}
