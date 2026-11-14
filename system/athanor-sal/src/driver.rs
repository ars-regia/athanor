//! The Wayland driver on smithay-client-toolkit: xdg-toplevel windows, wlr-layer-shell surfaces
//! and their xdg-popups (a menu, or a tooltip), several in one process, each drawn in shared memory by vello_cpu or through wgpu by
//! vello_gpu at the compositor's fractional scale, the keyboard and the pointer, the input method
//! through text-input-v3, the clipboard and the primary selection, and one AccessKit adapter on
//! AT-SPI per surface.

mod clipboard;
mod dialog;
mod modifiers;

pub use modifiers::KeyModifiers;

use crate::a11y::{leave, rejoining, tree_update, tree_updates};
use crate::motion::{Motion, Pose, Running};
#[cfg(feature = "gpu")]
use crate::gpu::Gpu;
use crate::popup::{focused_window, Dismiss, GrabSerial, OpenPopup, Tooltip, TOOLTIP_PADDING};
use crate::render::{layout, render, Error};
use crate::theme::Theme;
use crate::widget::{Event, Key, Kind, PointerButton, Rect, Ui, WidgetId, WindowButton};
use dialog::CloseRequest;
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, EventSource, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use smithay_client_toolkit::reexports::protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::{self, ZwpTextInputManagerV3},
    zwp_text_input_v3::{self, ContentHint, ContentPurpose, ZwpTextInputV3},
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_positioner::{
    Anchor as Edge, ConstraintAdjustment, Gravity,
};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};
pub use smithay_client_toolkit::{
    output::OutputInfo,
    shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{
            cursor_shape::CursorShapeManager, AxisScroll, PointerEvent, PointerEventKind,
            PointerHandler, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT,
        },
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure, SurfaceKind,
        },
        xdg::{
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgPositioner, XdgShell, XdgSurface,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Proxy, QueueHandle,
};

/// What a single window does with each event; it may change the widgets. See [`run_window`].
pub type Handler = Box<dyn FnMut(&mut Ui, Event)>;

/// The variable that picks the renderer: `cpu` (the default) or `gpu`, which only a build with
/// the `gpu` feature takes. Popups draw on the CPU either way.
pub const RENDERER: &str = "ATHANOR_SAL_RENDERER";

/// The name [`RENDERER`] had before the toolkit became Sal (ADR-0134), still read, with a
/// warning, until the 1.0 cut.
const OLD_RENDERER: &str = "ATHANOR_TOOLKIT_RENDERER";

/// What [`RENDERER`] holds, falling back to [`OLD_RENDERER`]: the value, a warning to print and
/// the name of the variable the value came from. The new name wins when both are set.
fn renderer_value(
    new: Option<std::ffi::OsString>,
    old: Option<std::ffi::OsString>,
) -> (Option<std::ffi::OsString>, Option<String>, &'static str) {
    match (new, old) {
        (None, Some(old)) => (
            Some(old),
            Some(format!("{OLD_RENDERER} is deprecated; set {RENDERER}")),
            OLD_RENDERER,
        ),
        (new, Some(_)) => (
            new,
            Some(format!("{OLD_RENDERER} is ignored because {RENDERER} is set")),
            RENDERER,
        ),
        (new, None) => (new, None, RENDERER),
    }
}

/// Whether the value of the variable `name` asks for the GPU. Anything but `cpu`, `gpu` or unset
/// is refused.
fn wants_gpu(value: Option<std::ffi::OsString>, name: &str) -> Result<bool, Error> {
    match value.as_deref().map(|v| v.to_str()) {
        None | Some(Some("cpu")) => Ok(false),
        Some(Some("gpu")) if cfg!(feature = "gpu") => Ok(true),
        Some(Some("gpu")) => {
            Err(format!("{name} is gpu; this build has no gpu feature").into())
        }
        Some(Some(other)) => Err(format!("{name} is {other:?}; it takes cpu or gpu").into()),
        Some(None) => Err(format!("{name} is not valid UTF-8; it takes cpu or gpu").into()),
    }
}

/// A surface of the application, for as long as it is open. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(u64);

/// How a surface is shown: the same widgets, in a window or on a layer of the shell. New roles
/// are added over time: match with a `_` arm.
#[non_exhaustive]
pub enum Role {
    /// An xdg-toplevel, which draws its own title bar.
    Window {
        /// The application id the compositor groups the window under, as the name of its
        /// `.desktop` file.
        app_id: String,
    },
    /// A wlr-layer-shell surface on the output the compositor picks; [`Shell::open_layer`]
    /// opens one on a chosen output.
    Layer(LayerOptions),
}

/// A `zwlr_layer_surface_v1`'s state. The surface takes the size of its widgets, and along an
/// axis anchored on both edges at least the size the compositor configures: a surface anchored
/// on all four whose widgets are a [`crate::Kind::Picture`] takes exactly the configured size.
/// New fields are added over time: start from [`LayerOptions::new`] and set the fields that
/// differ.
#[non_exhaustive]
pub struct LayerOptions {
    /// The layer of the shell the surface sits on.
    pub layer: Layer,
    /// What the surface is for, as the compositor's configuration names it.
    pub namespace: String,
    /// The edges of the output the surface is attached to.
    pub anchor: Anchor,
    /// Top, right, bottom, left, in logical pixels.
    pub margin: [i32; 4],
    /// How far the surface pushes other surfaces away from its edge; `-1` ignores their zones.
    pub exclusive_zone: i32,
    /// Whether and when the surface takes the keyboard.
    pub keyboard: KeyboardInteractivity,
    /// The part of the surface that takes the pointer, in logical pixels; `None` is all of it.
    pub input_region: Option<Vec<Rect>>,
}

impl LayerOptions {
    /// A surface on `layer`, with the protocol's defaults where it has them: no anchor, no
    /// margin, an exclusive zone of 0, no keyboard and the pointer on the whole surface. The
    /// namespace is empty. The layer has no default: the compositor grants the top and overlay
    /// layers only to the clients of its allow-list, so each client names the one it asks for.
    pub fn new(layer: Layer) -> Self {
        Self {
            layer,
            namespace: String::new(),
            anchor: Anchor::empty(),
            margin: [0; 4],
            exclusive_zone: 0,
            keyboard: KeyboardInteractivity::None,
            input_region: None,
        }
    }
}

/// The side of its widget a popup opens on, as GTK's `GtkPositionType`; left and right are the
/// physical sides. The compositor flips the popup to the opposite side, and slides it along, to
/// keep it on the output, as GTK's popovers ask. New sides may be added: match with a `_` arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Side {
    /// Above the widget: from a dock along the bottom edge.
    Top,
    /// Below the widget: from a bar along the top edge.
    Bottom,
    /// To the left of the widget.
    Left,
    /// To the right of the widget.
    Right,
}

/// What runs on the driver: the surfaces it opens, and what it does with their events.
pub trait Application {
    /// The surfaces to open at start, before any output is known: a window, say.
    fn start(&mut self) -> Vec<(Ui, Role)> {
        Vec::new()
    }

    /// The layer surface for `output`, asked when the output is announced, and when it changes
    /// while it has none; `None` leaves it without one. The driver keeps at most one on each
    /// output and closes it when the output leaves, and asks again for an output that comes
    /// back. One the compositor or [`Shell::close`] closed is not asked for again on that output
    /// until the output leaves and comes back: a compositor that refused it would refuse it again.
    /// More surfaces on an output open through [`Shell::open_layer`].
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        None
    }

    /// The output of the application's `surface`, right after the surface opened on it and each
    /// time it changes: its scale, mode or position. The surface follows the scale by itself.
    fn output_changed(&mut self, _shell: &mut Shell, _surface: SurfaceId, _output: &OutputInfo) {}

    /// An event of the widgets of `surface`. The application may change the widgets of any
    /// surface through `shell`.
    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event);

    /// The pointer entered (`inside`) or left `surface`, a popup included.
    fn pointer(&mut self, _shell: &mut Shell, _surface: SurfaceId, _inside: bool) {}

    /// The keyboard entered (`focused`) or left `surface`, a popup included.
    fn keyboard(&mut self, _shell: &mut Shell, _surface: SurfaceId, _focused: bool) {}

    /// A key pressed, or repeated, on `surface`, before its widgets see it, a focused field
    /// included: `true` takes it, and the widgets never see it. Where an application gives keys
    /// a meaning of its own while the focus stays in a field: the arrows that move a list of
    /// results, say, or Ctrl+Enter, which [`Key::Enter`] with `modifiers.ctrl` tells from Enter.
    /// The keys of an open menu come to the menu's surface. Escape on a popup that Escape
    /// dismisses closes it first, without this call. The default takes no key.
    ///
    /// Only the keys a [`Key`] names come here: Tab and Shift+Tab, the arrows, Home, End,
    /// Page Up, Page Down, Enter, Backspace, Delete, Escape, Menu and Shift+F10, Ctrl+A, Ctrl+C,
    /// Ctrl+X and Ctrl+V, and a key that types text, which comes as [`Key::Text`] whatever the
    /// modifiers held, Alt or Super included. Shift with Left or Right comes as
    /// [`Key::SelectLeft`] or [`Key::SelectRight`]. The other keys reach neither this call nor
    /// the widgets: the function keys but Shift+F10, Ctrl with any other letter, and the other
    /// keys that type nothing, such as Insert.
    fn key(
        &mut self,
        _shell: &mut Shell,
        _surface: SurfaceId,
        _key: &Key,
        _modifiers: KeyModifiers,
    ) -> bool {
        false
    }

    /// Runs after each turn of the event loop, once its events are handled: where the
    /// application applies what its own source (see [`run_with`]) changed since.
    fn turn(&mut self, _shell: &mut Shell) {}

    /// The first frame of `surface` was committed: called in the turn of the event loop that
    /// drew it, after [`Application::turn`], so that nothing waits on a frame callback the
    /// compositor may withhold from a surface it does not show. Where a `Type=notify`
    /// service tells systemd it is ready. Not called for a surface closed by then.
    fn drawn(&mut self, _shell: &mut Shell, _surface: SurfaceId) {}

    /// The motion of `surface` ([`Shell::animate`]) reached its end pose, which the surface keeps:
    /// called in the turn of the event loop that drew it, after [`Application::turn`]. Not
    /// called for a motion a later one replaced, nor for a surface closed by then.
    fn settled(&mut self, _shell: &mut Shell, _surface: SurfaceId) {}

    /// The layer surface `surface` closed without the application asking: the compositor closed
    /// it (`zwlr_layer_surface_v1.closed`), or the output it was opened on left. It is no longer
    /// open, and the driver opens nothing in its place.
    fn closed(&mut self, _shell: &mut Shell, _surface: SurfaceId) {}
}

/// Opens `ui` in a window and runs until the user closes it.
pub fn run_window(ui: Ui, app_id: &str, on_event: Handler) -> Result<(), Error> {
    struct Single {
        window: Option<(Ui, Role)>,
        on_event: Handler,
    }
    impl Application for Single {
        fn start(&mut self) -> Vec<(Ui, Role)> {
            self.window.take().into_iter().collect()
        }
        fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
            if let Some(ui) = shell.ui_mut(surface) {
                (self.on_event)(ui, event);
            }
        }
    }
    let role = Role::Window {
        app_id: app_id.to_owned(),
    };
    run(Single {
        window: Some((ui, role)),
        on_event,
    })
}

/// Runs `application` until its window closes, or until an error.
pub fn run(application: impl Application + 'static) -> Result<(), Error> {
    run_on(application, |_| Ok(()))
}

/// Runs `application` as [`run`] does, with `source` in the same event loop: GLib's main
/// context, say, for an application that awaits GIO. [`Application::turn`] follows each turn.
pub fn run_with<S>(application: impl Application + 'static, source: S) -> Result<(), Error>
where
    S: EventSource<Event = (), Metadata = (), Ret = ()> + 'static,
{
    run_on(application, |handle| {
        handle
            .insert_source(source, |_, _, _| {})
            .map(drop)
            .map_err(|e| e.error.to_string().into())
    })
}

fn run_on(
    application: impl Application + 'static,
    insert: impl FnOnce(&LoopHandle<'static, App>) -> Result<(), Error>,
) -> Result<(), Error> {
    let (value, warning, name) =
        renderer_value(std::env::var_os(RENDERER), std::env::var_os(OLD_RENDERER));
    if let Some(warning) = warning {
        eprintln!("athanor-sal: {warning}");
    }
    let gpu = wants_gpu(value, name)?;
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| e.error.to_string())?;
    insert(&event_loop.handle())?;
    #[cfg(not(feature = "gpu"))]
    debug_assert!(!gpu, "wants_gpu refuses gpu without the feature");

    // The adapters call their handlers on their own thread: actions travel to the event loop.
    let (actions, receiver) = channel::channel();
    event_loop
        .handle()
        .insert_source(receiver, |event, _, app: &mut App| {
            if let channel::Event::Msg((id, request)) = event {
                if app.shell.dialogs.blocked(id) {
                    return;
                }
                let event = app
                    .shell
                    .get_mut(id)
                    .and_then(|surface| surface.ui.do_action(&request));
                app.changed(id, event);
            }
        })
        .map_err(|e| e.error.to_string())?;

    let shm = Shm::bind(&globals, &qh)?;
    let shell = Shell {
        #[cfg(feature = "gpu")]
        conn: conn.clone(),
        #[cfg(feature = "gpu")]
        gpu,
        qh: qh.clone(),
        pool: SlotPool::new(4096, &shm)?,
        compositor: CompositorState::bind(&globals, &qh)?,
        // Each is required only by the surfaces of its role, and refused when one is opened.
        xdg_shell: XdgShell::bind(&globals, &qh).ok(),
        layer_shell: LayerShell::bind(&globals, &qh).ok(),
        // Optional: without them, the integer scale of wl_surface.
        fractional_scale: globals.bind(&qh, 1..=1, Scaling).ok(),
        viewporter: globals.bind(&qh, 1..=1, Scaling).ok(),
        actions,
        next: 0,
        surfaces: Vec::new(),
        popup: None,
        seat: None,
        grab: GrabSerial::default(),
        exit: false,
        failure: None,
        first_frames: Vec::new(),
        outputs: Vec::new(),
        closing: Vec::new(),
        reduced_motion: false,
        settled: Vec::new(),
        dialogs: dialog::Dialogs::bind(&globals, &qh),
        blocking: Vec::new(),
    };
    let mut app = App {
        application: Box::new(application),
        shell,
        outputs: PerOutput::default(),
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        shm,
        keyboard: None,
        keyboard_focus: None,
        window_focus: None,
        pointer: None,
        // Optional: without it, the pointer keeps the compositor's cursor.
        cursor_shape: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor_shape_device: None,
        pointer_seat: None,
        // Optional: without it, text comes from the keyboard alone.
        text_input_manager: globals.bind(&qh, 1..=1, TextInput).ok(),
        text_input: None,
        ime: Ime::default(),
        loop_handle: event_loop.handle(),
        modifiers: Modifiers::default(),
        // Optional: without them, a field copies and pastes nothing.
        clipboards: clipboard::Clipboards::bind(&globals, &qh),
        own_menu: None,
        pressed_at: None,
        tooltip: Tooltip::default(),
        error: None,
    };
    for (ui, role) in app.application.start() {
        app.shell.open(ui, role, None)?;
    }
    while !app.shell.exit {
        dispatch(&mut event_loop, &conn, wait(!app.shell.settled.is_empty()), &mut app)?;
        app.application.turn(&mut app.shell);
        app.draw_dirty();
        // A surface closed since its first frame, by the compositor, with its output or by an
        // earlier `drawn`, is not reported drawn.
        let first_frames = std::mem::take(&mut app.shell.first_frames);
        each_open(
            &mut app,
            first_frames,
            |app, surface| app.shell.ui(surface).is_some(),
            |app, surface| app.application.drawn(&mut app.shell, surface),
        );
        // A surface that an earlier `settled` of this batch moved again is not reported settled
        // here: a running motion settles later, and an instantaneous one is queued anew for the
        // next turn.
        let settled = std::mem::take(&mut app.shell.settled);
        settle_each(
            &mut app,
            settled,
            |app| &app.shell.settled,
            |app, surface| {
                app.shell
                    .surfaces
                    .iter()
                    .any(|s| s.id == surface && s.motion.is_none())
            },
            |app, surface| app.application.settled(&mut app.shell, surface),
        );
        // What the application asked of the clipboards outside an input handler is served now,
        // a copy refused, and never at a later key or click.
        app.clips_outside_input();
        app.closed_by_application();
        app.blocked_by_dialogs();
        app.sync_window_focus();
        if let Some(error) = app.error.take().or_else(|| app.shell.failure.take()) {
            return Err(error);
        }
    }
    Ok(())
}

/// Why a popup cannot be had without xdg-shell.
const NO_XDG_SHELL: &str = "no popup: the compositor does not offer xdg_wm_base";

/// The layer shell, or why a layer surface cannot be had: the check doc_shell.md SH4 asks of
/// every layer surface. The driver speaks the protocol itself, so a surface it opens on this
/// global is a layer surface; without the global there is none, and nothing degrades to a window.
fn require_layer_shell(layer_shell: Option<&LayerShell>) -> Result<&LayerShell, Error> {
    layer_shell.ok_or_else(|| {
        "not a layer surface: the compositor does not offer zwlr_layer_shell_v1".into()
    })
}

/// Which surface the application has on which output: at most one on each (the class of defect
/// of #600), or none since its surface closed. Generic, so that its tests feed it without a
/// compositor.
struct PerOutput<O, S>(Vec<(O, Option<S>)>);

impl<O, S> Default for PerOutput<O, S> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<O: PartialEq, S: Copy + PartialEq> PerOutput<O, S> {
    /// An output announced or changed: its surface, the one it had or the one `open` gave it,
    /// if any. `open` is asked only when it never had one, or `open` gave it none.
    fn seen(
        &mut self,
        output: O,
        open: impl FnOnce(&O) -> Result<Option<S>, Error>,
    ) -> Result<Option<S>, Error> {
        if let Some((_, surface)) = self.0.iter().find(|(o, _)| *o == output) {
            return Ok(*surface);
        }
        let opened = open(&output)?;
        if let Some(surface) = opened {
            self.0.push((output, Some(surface)));
        }
        Ok(opened)
    }

    /// An output left: its surface, if it still has one, for the driver to close. An output that
    /// comes back is asked again.
    fn left(&mut self, output: &O) -> Option<S> {
        let at = self.0.iter().position(|(o, _)| o == output)?;
        self.0.remove(at).1
    }

    /// `surface` closed, by the compositor or the application: its output has none, and is not
    /// asked again, until it leaves.
    fn closed(&mut self, surface: S) {
        for (_, s) in &mut self.0 {
            if *s == Some(surface) {
                *s = None;
            }
        }
    }
}

/// The surfaces open, and what opening one takes. The application reaches the widgets of each
/// through it.
pub struct Shell {
    #[cfg(feature = "gpu")]
    conn: Connection,
    /// [`RENDERER`] asks for vello_gpu.
    #[cfg(feature = "gpu")]
    gpu: bool,
    qh: QueueHandle<App>,
    pool: SlotPool,
    compositor: CompositorState,
    xdg_shell: Option<XdgShell>,
    layer_shell: Option<LayerShell>,
    fractional_scale: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    actions: Sender<(SurfaceId, ActionRequest)>,
    next: u64,
    surfaces: Vec<Surface>,
    /// The popup open, at most one.
    popup: Option<OpenPopup<SurfaceId>>,
    /// The seat, and the serial of the input under way, which a menu's grab names.
    seat: Option<wl_seat::WlSeat>,
    grab: GrabSerial,
    /// A window was closed: [`run`] returns.
    exit: bool,
    /// Why the application stopped the run: [`run`] returns it.
    failure: Option<Error>,
    /// The surfaces whose first frame was committed since [`Application::drawn`] last ran.
    first_frames: Vec<SurfaceId>,
    /// The outputs announced, as they last changed.
    outputs: Vec<(wl_output::WlOutput, OutputInfo)>,
    /// The surfaces [`Shell::close`] closed since the driver last ended what they held.
    closing: Vec<SurfaceId>,
    /// Whether motions only fade ([`Shell::set_reduced_motion`]).
    reduced_motion: bool,
    /// The surfaces whose motion ended since [`Application::settled`] last ran.
    settled: Vec<SurfaceId>,
    /// The modal dialogs open, each on its window.
    dialogs: dialog::Dialogs,
    /// The windows a dialog opened on since the driver last ended the pointer's press and hover
    /// on them.
    blocking: Vec<SurfaceId>,
}

impl Shell {
    /// The surfaces open, in the order they were opened.
    pub fn surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.surfaces.iter().map(|s| s.id)
    }

    /// The widgets of an open surface, or `None` once it is closed.
    pub fn ui(&self, id: SurfaceId) -> Option<&Ui> {
        self.surfaces.iter().find(|s| s.id == id).map(|s| &s.ui)
    }

    /// Whether `id` committed a frame since its first configure: what GTK's `map` signal
    /// tells, and a dock's Type=notify waits for. `false` once it is closed.
    pub fn drawn(&self, id: SurfaceId) -> bool {
        self.surfaces.iter().any(|s| s.id == id && s.drawn)
    }

    /// The widgets of `id`, which the driver draws again after the event.
    pub fn ui_mut(&mut self, id: SurfaceId) -> Option<&mut Ui> {
        let surface = self.get_mut(id)?;
        surface.dirty = true;
        Some(&mut surface.ui)
    }

    /// Sets the part of a layer surface that takes the pointer, in logical pixels; `None` is all
    /// of it. Takes effect with the next frame.
    pub fn set_input_region(&mut self, id: SurfaceId, rects: Option<&[Rect]>) -> Result<(), Error> {
        let region = Self::region(&self.compositor, rects)?;
        let surface = self
            .get_mut(id)
            .ok_or_else(|| format!("no open surface {id:?}"))?;
        surface
            .wl_surface()
            .set_input_region(region.as_ref().map(Region::wl_region));
        surface.dirty = true;
        Ok(())
    }

    /// A `wl_region` of `rects`, in logical pixels; `None` for all of the surface.
    fn region(
        compositor: &CompositorState,
        rects: Option<&[Rect]>,
    ) -> Result<Option<Region>, Error> {
        let Some(rects) = rects else {
            return Ok(None);
        };
        let region = Region::new(compositor)?;
        for r in rects {
            // Whole pixels that cover the widget: the origin down, the far edge up.
            let (x, y) = (r.x.floor(), r.y.floor());
            let (w, h) = ((r.x + r.w).ceil() - x, (r.y + r.h).ceil() - y);
            region.add(x as i32, y as i32, w as i32, h as i32);
        }
        Ok(Some(region))
    }

    /// Changes a layer surface's layer, anchor, margin, exclusive zone, keyboard and input
    /// region to `options`'. The layer changes in place, through
    /// `zwlr_layer_surface_v1.set_layer`: the surface is not created again. Refused, with nothing
    /// changed and no request sent, for another namespace, which the protocol fixes when the
    /// surface is created, for another layer on a layer shell older than version 2, which has no
    /// `set_layer`: close the surface and open another; and for widgets that cannot be laid out
    /// at the surface's scale. Takes effect at once: the driver commits the new state, with
    /// the size of the widgets, without waiting for a frame, which the compositor may hold back
    /// from a covered surface. The widgets are drawn again at the next frame.
    pub fn set_layer(&mut self, id: SurfaceId, options: &LayerOptions) -> Result<(), Error> {
        // What can refuse comes before any request, the region's included: a request sent stays
        // pending, and the next commit, of this call or not, would apply it.
        let compositor = &self.compositor;
        let surface = self
            .surfaces
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("no open surface {id:?}"))?;
        let (Shown::Layer(layer), Some(placed)) = (&surface.shown, &mut surface.layer) else {
            return Err(format!("{id:?} is not a layer surface").into());
        };
        let version = match layer.kind() {
            SurfaceKind::Wlr(wlr) => wlr.version(),
            _ => 0,
        };
        let new = layer_change(placed, options, version)?;
        let stretch = stretched(options.anchor);
        let was = (surface.ui.min_width, surface.ui.min_height);
        // An axis no longer stretched takes its widgets' size again.
        let shrink = (surface.stretch.0 && !stretch.0, surface.stretch.1 && !stretch.1);
        let min = (if shrink.0 { 0.0 } else { was.0 }, if shrink.1 { 0.0 } else { was.1 });
        layout_at_min(&mut surface.ui, surface.scale, min)?;
        let region = match Self::region(compositor, options.input_region.as_deref()) {
            Ok(region) => region,
            Err(error) => {
                put_back(&mut surface.ui, was);
                return Err(error);
            }
        };
        if let Some(new) = new {
            layer.set_layer(new);
            placed.0 = new;
        }
        layer.set_anchor(options.anchor);
        let [top, right, bottom, left] = options.margin;
        layer.set_margin(top, right, bottom, left);
        layer.set_exclusive_zone(options.exclusive_zone);
        layer.set_keyboard_interactivity(options.keyboard);
        surface.stretch = stretch;
        surface.request_size();
        surface
            .wl_surface()
            .set_input_region(region.as_ref().map(Region::wl_region));
        surface.dirty = true;
        // A commit with no new buffer applies the layer surface's state and keeps the buffer
        // attached; the frame callback of a draw under way is untouched.
        surface.wl_surface().commit();
        Ok(())
    }

    /// Whether the surface `id` is in the accessibility tree, as it is when it opens. A surface
    /// kept mapped while it shows nothing (`doc_launcher.md` LA8) leaves the tree, as a hidden
    /// GTK window does: its window leaves the application's children on AT-SPI, and comes back
    /// with its widgets as they are, the next frame publishing them again. Out of the tree, the
    /// surface announces nothing ([`Ui::announce`]). Refused for a surface that is not open.
    pub fn set_accessible(&mut self, id: SurfaceId, accessible: bool) -> Result<(), Error> {
        let actions = self.actions.clone();
        let surface = self
            .get_mut(id)
            .ok_or_else(|| format!("no open surface {id:?}"))?;
        match (surface.a11y.is_some(), accessible) {
            (true, false) => surface.withdraw(),
            (false, true) => {
                let tree = rejoining(&mut surface.ui, f64::from(surface.scale));
                if let Ok(mut initial) = surface.initial.lock() {
                    *initial = Some(tree);
                }
                let mut a11y = adapter(id, &surface.initial, &actions);
                a11y.update_window_focus_state(surface.window_focused);
                surface.a11y = Some(a11y);
                surface.dirty = true;
            }
            _ => {}
        }
        Ok(())
    }

    /// Moves the surface `id` from `motion.from` to `motion.to`, its widgets with it, one step per
    /// frame callback of the surface: no timer, so a surface the compositor does not show does
    /// not move until it shows it. The first pose is drawn at once, and the motion's time starts
    /// at the surface's next frame callback; at the end the surface keeps `to` and the
    /// application is told through [`Application::settled`]. A motion asked while another is
    /// under way replaces it, from wherever `from` says: [`Shell::pose`] gives the current pose,
    /// so that an interrupted motion does not jump. With reduced motion only the opacity moves,
    /// over `motion.reduced`. The offset moves what is drawn inside the surface, which keeps its
    /// size: leave it room. Refused for a surface that is not open.
    pub fn animate(&mut self, id: SurfaceId, motion: Motion) -> Result<(), Error> {
        let reduced = self.reduced_motion;
        let surface = self
            .get_mut(id)
            .ok_or_else(|| format!("no open surface {id:?}"))?;
        surface.ui.pose = motion.at(Duration::ZERO, reduced);
        surface.dirty = true;
        let ended = motion.ended(Duration::ZERO, reduced);
        surface.motion = (!ended).then(|| Running::new(motion));
        requeue(&mut self.settled, id, ended);
        Ok(())
    }

    /// How the surface `id` is drawn now: [`Pose::REST`] unless a motion moved it.
    pub fn pose(&self, id: SurfaceId) -> Option<Pose> {
        self.ui(id).map(|ui| ui.pose)
    }

    /// Reduced motion, as the application reads it from the session (GNOME's
    /// `enable-animations` off, doc_visual_language.md VL9): every motion, those under way
    /// included from their next frame, then moves only its opacity, over its `reduced` time.
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
    }

    /// Whether reduced motion is on: off until [`Shell::set_reduced_motion`] turns it on.
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// The surface the open menu was opened from, if a menu is open.
    pub fn popup_parent(&self) -> Option<SurfaceId> {
        self.popup.filter(|p| p.menu).map(|p| p.parent)
    }

    /// The outputs the compositor announced, in that order, as they last changed.
    pub fn outputs(&self) -> impl Iterator<Item = &OutputInfo> + '_ {
        self.outputs.iter().map(|(_, info)| info)
    }

    /// Opens `ui` as a layer surface on the output named `output`, as [`OutputInfo::name`]
    /// gives it (`DP-1`), or, for `None`, on the output the compositor picks: cosmic-comp's
    /// active one, athanor-sulphur's first. The application may keep several on one output; it is
    /// not told of their output's changes, which their scale follows by itself. It is drawn at
    /// its first configure. An output not announced, or announced without a name, is refused.
    pub fn open_layer(
        &mut self,
        ui: Ui,
        options: LayerOptions,
        output: Option<&str>,
    ) -> Result<SurfaceId, Error> {
        let output = match output {
            Some(name) => Some(
                named(self.outputs.iter().map(|(o, i)| (o, i.name.as_deref())), name)
                    .ok_or_else(|| format!("no output named {name:?}"))?
                    .clone(),
            ),
            None => None,
        };
        self.open(ui, Role::Layer(options), output.as_ref())
    }

    /// Closes the surface `id`, and the popup open from it first; the application is not told
    /// through [`Application::closed`]. A surface of [`Application::output`] closed so is not
    /// asked for again on that output until the output leaves and comes back. Closing a window
    /// does not end the run.
    pub fn close(&mut self, id: SurfaceId) {
        if self.surfaces.iter().any(|s| s.id == id) {
            self.remove(id);
            self.closing.push(id);
        }
    }

    /// Opens `ui` as a modal dialog of the window `parent`, as GTK's modal windows: a window of
    /// the parent's application id, its child, modal through `xdg_wm_dialog_v1` when the
    /// compositor offers it. Its root reads as a modal dialog (an alert dialog keeps its role),
    /// its title bar has only Close, and while it is open the parent takes no input: neither the
    /// pointer, nor the keyboard, nor the input method, nor an assistive technology's action.
    /// Escape, its Close button and the compositor's request to close it reach the application
    /// as [`Event::Window`] with [`WindowButton::Close`], which closes the run for any other
    /// window: the application answers it, at least with [`Shell::close`]. It closes with its
    /// parent. It is drawn at its first configure; a parent that is not an open window is
    /// refused.
    pub fn open_dialog(&mut self, parent: SurfaceId, mut ui: Ui) -> Result<SurfaceId, Error> {
        let Some(Surface {
            shown: Shown::Window(parent_window),
            app_id,
            ..
        }) = self.surfaces.iter().find(|s| s.id == parent)
        else {
            return Err(format!("no window {parent:?} to open a dialog on").into());
        };
        let (parent_window, app_id) = (parent_window.clone(), app_id.clone());
        ui.set_dialog();
        let (wl, fractional) = self.new_surface();
        let window = self.window(wl, &ui, &app_id)?;
        window.set_parent(Some(&parent_window));
        let id = self.adopt(ui, Shown::Window(window), fractional, None, (false, false))?;
        // Made once the surface is taken, so that no refusal leaves it behind its toplevel:
        // the surface destroys it before the toplevel when it closes.
        if let Some(surface) = self.surfaces.iter_mut().find(|s| s.id == id) {
            surface.app_id = app_id;
            if let Shown::Window(window) = &surface.shown {
                surface.modal = self.dialogs.set_modal(window.xdg_toplevel(), &self.qh);
            }
        }
        self.dialogs.opened(id, parent);
        // The parent's menu or tooltip closes, since the parent takes no input, and the
        // pointer's press and hover on it end at the end of the turn.
        if self.popup.is_some_and(|p| p.parent == parent) {
            self.close_popup();
        }
        self.blocking.push(parent);
        Ok(id)
    }

    /// An xdg-toplevel for `ui`, which draws its own title bar, as GTK does, so that assistive
    /// technologies find the title and the window's buttons in its tree; a compositor that
    /// insists on drawing them says so in the first configure.
    fn window(&self, wl: wl_surface::WlSurface, ui: &Ui, app_id: &str) -> Result<Window, Error> {
        let xdg_shell = self
            .xdg_shell
            .as_ref()
            .ok_or("the compositor does not offer xdg_wm_base")?;
        let window = xdg_shell.create_window(wl, WindowDecorations::RequestClient, &self.qh);
        window.set_title(ui.title.as_str());
        window.set_app_id(app_id);
        Ok(window)
    }

    fn get_mut(&mut self, id: SurfaceId) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }

    fn find(&mut self, wl: &wl_surface::WlSurface) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|s| s.wl_surface() == wl)
    }

    /// Opens `ui` as a menu in a popup of the surface `parent`, on `side` of its widget `anchor`,
    /// and closes the popup open before: one is open at a time, as GTK's popovers, so `parent`
    /// is a window or a layer surface, never a popup. A refused request leaves the open popup
    /// open. The menu takes the pointer and the keyboard, and its first item the focus; while it
    /// is open, `anchor` reads as expanded if it has a popup ([`Ui::set_has_popup`]). It closes
    /// at Escape, at a press outside it, when the compositor dismisses it or at
    /// [`Shell::close_popup`], and the focus then returns to `anchor`. It is drawn at its first configure.
    pub fn open_popup(
        &mut self,
        parent: SurfaceId,
        anchor: WidgetId,
        ui: Ui,
        side: Side,
    ) -> Result<SurfaceId, Error> {
        self.show_popup(parent, anchor, None, ui, side, true)
    }

    /// Ends the run with `error`, which [`run`] returns once the turn is over.
    pub fn fail(&mut self, error: Error) {
        self.failure.get_or_insert(error);
        self.exit = true;
    }

    /// Closes the popup open, a menu or a tooltip, if any.
    pub fn close_popup(&mut self) {
        if let Some(open) = self.popup {
            self.remove(open.surface);
        }
    }

    /// Opens a menu, or a tooltip, which takes neither the input nor the focus and keeps
    /// [`TOOLTIP_PADDING`] away from its widget. A menu opened `at` a point of the parent is
    /// placed against that point, not its widget's bounds.
    fn show_popup(
        &mut self,
        parent: SurfaceId,
        anchor: WidgetId,
        at: Option<(f32, f32)>,
        mut ui: Ui,
        side: Side,
        menu: bool,
    ) -> Result<SurfaceId, Error> {
        // Everything that can refuse the request is checked before the open popup closes: a
        // refused request leaves it open.
        let xdg_shell = self.xdg_shell.as_ref().ok_or(NO_XDG_SHELL)?;
        let from = self
            .surfaces
            .iter()
            .find(|s| s.id == parent)
            .ok_or_else(|| format!("no open surface {parent:?}"))?;
        if matches!(from.shown, Shown::Popup(_)) {
            return Err(format!("no popup of the popup {parent:?}: one is open at a time").into());
        }
        if self.dialogs.blocked(parent) {
            return Err(format!("no popup of {parent:?}: a dialog blocks it").into());
        }
        if !from.ui.contains(anchor) {
            return Err(format!("no widget {anchor:?} on {parent:?}").into());
        }
        let r = at.map_or(from.ui.widget(anchor).rect, |(x, y)| Rect {
            x,
            y,
            w: 0.0,
            h: 0.0,
        });
        layout(&mut ui, 1.0)?;
        let (w, h) = logical_size(&ui);
        let positioner = XdgPositioner::new(xdg_shell)?;
        self.close_popup();
        let xdg_shell = self.xdg_shell.as_ref().ok_or(NO_XDG_SHELL)?;
        let from = self
            .surfaces
            .iter()
            .find(|s| s.id == parent)
            .ok_or_else(|| format!("no open surface {parent:?}"))?;
        positioner.set_size(w as i32, h as i32);
        // The widget's bounds in whole pixels of its surface, grown for a tooltip.
        let pad = if menu { 0.0 } else { TOOLTIP_PADDING };
        let (x, y) = ((r.x - pad).floor(), (r.y - pad).floor());
        let (right, bottom) = ((r.x + r.w + pad).ceil(), (r.y + r.h + pad).ceil());
        positioner.set_anchor_rect(
            x as i32,
            y as i32,
            ((right - x) as i32).max(1),
            ((bottom - y) as i32).max(1),
        );
        // A menu also slides along its axis, so that one as high as the output stays on it.
        let along_x = ConstraintAdjustment::FlipY
            | ConstraintAdjustment::SlideX
            | if menu {
                ConstraintAdjustment::SlideY
            } else {
                ConstraintAdjustment::empty()
            };
        let along_y = ConstraintAdjustment::FlipX | ConstraintAdjustment::SlideY;
        let (edge, gravity, adjust) = match side {
            Side::Top => (Edge::Top, Gravity::Top, along_x),
            Side::Bottom => (Edge::Bottom, Gravity::Bottom, along_x),
            Side::Left => (Edge::Left, Gravity::Left, along_y),
            Side::Right => (Edge::Right, Gravity::Right, along_y),
        };
        positioner.set_anchor(edge);
        positioner.set_gravity(gravity);
        positioner.set_constraint_adjustment(adjust);
        let (wl, fractional) = self.new_surface();
        let of = match &from.shown {
            Shown::Window(window) => Some(window.xdg_surface()),
            _ => None,
        };
        let popup = Popup::from_surface(of, &positioner, &self.qh, wl, xdg_shell)?;
        if let Shown::Layer(layer) = &from.shown {
            layer.get_popup(popup.xdg_popup());
        }
        // A menu grabs the input with the press or the key that opened it, before its first
        // commit: the compositor then dismisses it at a press in another client. A menu opened
        // with no input under way, as by an assistive technology, grabs nothing.
        if menu {
            if let (Some(seat), Some(serial)) = (&self.seat, self.grab.serial()) {
                popup.xdg_popup().grab(seat, serial);
            }
        }
        let id = SurfaceId(self.next);
        let from = self
            .get_mut(parent)
            .ok_or_else(|| format!("no open surface {parent:?}"))?;
        let open = OpenPopup::opened(parent, anchor, id, &mut from.ui, &mut ui, menu);
        from.dirty = true;
        // A tooltip takes no pointer: it never stands between the pointer and its widget.
        let input = (!menu).then(Vec::new);
        match self.adopt(ui, Shown::Popup(popup), fractional, input, (false, false)) {
            Ok(id) => {
                self.popup = Some(open);
                Ok(id)
            }
            Err(error) => {
                if let Some(from) = self.get_mut(parent) {
                    open.closed(&mut from.ui);
                }
                Err(error)
            }
        }
    }

    /// A new wl_surface, with its fractional scale and viewport when the compositor offers both.
    fn new_surface(
        &self,
    ) -> (
        wl_surface::WlSurface,
        Option<(WpFractionalScaleV1, WpViewport)>,
    ) {
        let qh = &self.qh;
        let wl = self.compositor.create_surface(qh);
        // Fractional scale needs the viewport, which maps the buffer onto the logical size.
        let fractional = match (&self.fractional_scale, &self.viewporter) {
            (Some(scale), Some(viewporter)) => Some((
                scale.get_fractional_scale(&wl, qh, Scaling),
                viewporter.get_viewport(&wl, qh, Scaling),
            )),
            _ => None,
        };
        (wl, fractional)
    }

    /// Opens `ui` as `role`, on `output` for a layer surface. It is drawn at its first configure.
    fn open(
        &mut self,
        ui: Ui,
        role: Role,
        output: Option<&wl_output::WlOutput>,
    ) -> Result<SurfaceId, Error> {
        let (wl, fractional) = self.new_surface();
        let qh = &self.qh;
        let mut input_region = None;
        let mut stretch = (false, false);
        let mut placed = None;
        let mut window_app_id = String::new();
        let shown = match role {
            Role::Window { app_id } => {
                let window = self.window(wl, &ui, &app_id)?;
                window_app_id = app_id;
                Shown::Window(window)
            }
            Role::Layer(options) => {
                let layer_shell = require_layer_shell(self.layer_shell.as_ref())?;
                let surface = layer_shell.create_layer_surface(
                    qh,
                    wl,
                    options.layer,
                    Some(options.namespace.clone()),
                    output,
                );
                placed = Some((options.layer, options.namespace));
                surface.set_anchor(options.anchor);
                let [top, right, bottom, left] = options.margin;
                surface.set_margin(top, right, bottom, left);
                surface.set_exclusive_zone(options.exclusive_zone);
                surface.set_keyboard_interactivity(options.keyboard);
                input_region = options.input_region;
                stretch = stretched(options.anchor);
                Shown::Layer(surface)
            }
        };
        let id = self.adopt(ui, shown, fractional, input_region, stretch)?;
        if let Some(surface) = self.get_mut(id) {
            surface.layer = placed;
            surface.output = output.cloned();
            surface.app_id = window_app_id;
        }
        Ok(id)
    }

    /// Takes a surface just given its role: its renderer, its accessibility adapter, its size and
    /// input region, then its first commit.
    fn adopt(
        &mut self,
        mut ui: Ui,
        shown: Shown,
        fractional: Option<(WpFractionalScaleV1, WpViewport)>,
        input_region: Option<Vec<Rect>>,
        stretch: (bool, bool),
    ) -> Result<SurfaceId, Error> {
        let id = SurfaceId(self.next);
        // ponytail: a popup draws on vello_cpu: a wgpu device of its own would cost every tooltip
        // and menu a new instance, adapter, device and renderer in the event loop. Sharing the
        // window's needs `Gpu` split into one device and a surface per wl_surface.
        #[cfg(feature = "gpu")]
        let gpu = if self.gpu && !matches!(shown, Shown::Popup(_)) {
            // SAFETY: the surface owns the wgpu surface and drops it before its wl_surface, by
            // the order of Surface's fields.
            #[allow(unsafe_code)]
            let gpu = unsafe { Gpu::new(&self.conn, shown.wl_surface()) }
                .map_err(|e| format!("{RENDERER}=gpu: {e}"))?;
            eprintln!("athanor-sal: vello_gpu on {}", gpu.adapter);
            Some(gpu)
        } else {
            None
        };
        // The first tree comes from the last update drawn.
        let initial = Arc::new(Mutex::new(Some(tree_update(&ui, 1.0))));
        let a11y = Some(adapter(id, &initial, &self.actions));
        // A layer surface asks for its widgets' size before the first configure.
        let size = match &shown {
            Shown::Layer(surface) => {
                layout(&mut ui, 1.0)?;
                let size = requested(logical_size(&ui), stretch);
                surface.set_size(size.0, size.1);
                size
            }
            Shown::Popup(_) => logical_size(&ui),
            // A window gives its widgets' size as its minimum before the first configure, as
            // GTK does: a compositor that suggests a smaller first size, as cosmic-comp does on
            // a small output, would otherwise place the window for a width it never takes.
            Shown::Window(window) => {
                layout(&mut ui, 1.0)?;
                let size = logical_size(&ui);
                window.set_min_size(Some(size));
                window.set_max_size(max_size(&ui, size));
                size
            }
        };
        self.next += 1;
        self.surfaces.push(Surface {
            #[cfg(feature = "gpu")]
            gpu,
            id,
            fractional,
            shown,
            ui,
            a11y,
            initial,
            window_focused: false,
            scale: 1.0,
            size,
            stretch,
            layer: None,
            output: None,
            configured: false,
            drawn: false,
            frame_pending: false,
            redraw: false,
            dirty: false,
            motion: None,
            app_id: String::new(),
            modal: None,
        });
        if input_region.is_some() {
            self.set_input_region(id, input_region.as_deref())?;
        }
        if let Some(surface) = self.get_mut(id) {
            surface.wl_surface().commit();
        }
        Ok(id)
    }

    /// Closes the surface `id`, and the popup open from it first. Closing a menu returns the focus
    /// to its widget.
    fn remove(&mut self, id: SurfaceId) {
        // The dialogs open on it close with it, first, and the application ends what they held.
        let open: Vec<SurfaceId> = self.surfaces.iter().map(|s| s.id).collect();
        for surface in self.dialogs.close(id, &open) {
            if let Some(popup) = self
                .popup
                .filter(|p| p.surface == surface || p.parent == surface)
            {
                self.popup = None;
                self.surfaces.retain(|s| s.id != popup.surface);
                if let Some(from) = self.get_mut(popup.parent) {
                    popup.closed(&mut from.ui);
                    from.dirty = true;
                }
            }
            self.surfaces.retain(|s| s.id != surface);
            if surface != id {
                self.closing.push(surface);
            }
        }
    }

    /// Draws the surfaces whose widgets changed.
    fn draw_dirty(&mut self) -> Result<(), Error> {
        for surface in &mut self.surfaces {
            if surface.dirty {
                let first = !surface.drawn;
                surface.draw(&mut self.pool, &self.qh)?;
                if first && surface.drawn {
                    self.first_frames.push(surface.id);
                }
            }
        }
        Ok(())
    }
}

/// The height a drop-down's menu may take, in logical pixels: that of `output`, the output of
/// the drop-down's surface; when it is not known, as for a window or a layer surface the
/// compositor placed, that of the shortest of `outputs`, which the menu fits on whichever it
/// opens on; and without any, the surface's own height `own`. Heights of 0 are not known ones.
fn menu_max_height(output: Option<f32>, outputs: &[f32], own: u32) -> Option<f32> {
    let known = |h: &f32| *h > 0.0;
    output
        .filter(known)
        .or_else(|| outputs.iter().copied().filter(known).reduce(f32::min))
        .or_else(|| (own > 0).then_some(own as f32))
}

/// Calls `lost` on the surface `next` finds in `state` until it finds none, so that one `lost`
/// closed along the way is not lost after: `lost` closes the surface it is given.
fn lose_each<T, S>(state: &mut T, next: impl Fn(&T) -> Option<S>, mut lost: impl FnMut(&mut T, S)) {
    while let Some(surface) = next(state) {
        lost(state, surface);
    }
}

/// Calls `call` on each of `surfaces` that `open` finds open in `state` when its turn comes, so
/// that one closed before, or by an earlier `call`, is skipped.
fn each_open<T, S: Copy>(
    state: &mut T,
    surfaces: Vec<S>,
    open: impl Fn(&T, S) -> bool,
    mut call: impl FnMut(&mut T, S),
) {
    for surface in surfaces {
        if open(state, surface) {
            call(state, surface);
        }
    }
}

/// One turn of `event_loop`, then an error if the compositor ended `conn`, so that the process
/// exits and its unit's `Restart=` applies (#1129). calloop-wayland-source returns an end of file,
/// but only logs a protocol error, and the dead socket then wakes the loop at every turn with
/// nothing to dispatch. The error names the protocol error's code and object, and its message
/// where the backend keeps one: libwayland's keeps none. Every caller of [`run`] logs it before
/// it exits.
fn dispatch<D>(
    event_loop: &mut EventLoop<'static, D>,
    conn: &Connection,
    timeout: Option<Duration>,
    data: &mut D,
) -> Result<(), Error> {
    let turn = event_loop.dispatch(timeout, data);
    if let Some(error) = conn.backend().last_error() {
        return Err(format!("the compositor ended the Wayland connection: {error}").into());
    }
    Ok(turn?)
}

/// How long the loop waits for an event: not at all while a surface is queued `pending` for the
/// next turn, which no event is coming to wake, and until one arrives otherwise.
fn wait(pending: bool) -> Option<std::time::Duration> {
    pending.then_some(std::time::Duration::ZERO)
}

/// Calls `call` on each of the `settled` surfaces that is `at_rest` and that `queued` does not
/// find queued anew in `state` when its turn comes: one that an earlier `call` of this batch
/// moved again is reported on the turn it is queued for, not twice.
fn settle_each<T>(
    state: &mut T,
    settled: Vec<SurfaceId>,
    queued: impl Fn(&T) -> &[SurfaceId],
    at_rest: impl Fn(&T, SurfaceId) -> bool,
    call: impl FnMut(&mut T, SurfaceId),
) {
    each_open(
        state,
        settled,
        |state, surface| !queued(state).contains(&surface) && at_rest(state, surface),
        call,
    );
}

/// The output named `name` among `outputs` and their names.
fn named<'a, O: 'a>(
    outputs: impl IntoIterator<Item = (&'a O, Option<&'a str>)>,
    name: &str,
) -> Option<&'a O> {
    outputs
        .into_iter()
        .find(|(_, n)| *n == Some(name))
        .map(|(output, _)| output)
}

/// The layer `set_layer` sends to move a layer surface `placed` on a layer, with a namespace, to
/// `options`'s, if the layer changes, on a layer shell of `version`. Refused for another
/// namespace, which `get_layer_surface` fixes, and for another layer before version 2, which
/// added `set_layer`.
fn layer_change(
    placed: &(Layer, String),
    options: &LayerOptions,
    version: u32,
) -> Result<Option<Layer>, Error> {
    let (layer, namespace) = placed;
    if options.namespace != *namespace {
        return Err(format!(
            "a layer surface keeps the namespace it opened with, {namespace:?}, not {:?}: \
             close it and open another",
            options.namespace
        )
        .into());
    }
    if options.layer == *layer {
        return Ok(None);
    }
    if version < 2 {
        return Err(format!(
            "zwlr_layer_shell_v1 version {version} cannot move a layer surface to {:?}: close it \
             and open another",
            options.layer
        )
        .into());
    }
    Ok(Some(options.layer))
}

/// Lays `ui` out at `scale` with the minimum size `min`. Refused when it cannot be laid out,
/// with the minimum size `ui` had and the geometry it lays out to.
fn layout_at_min(ui: &mut Ui, scale: f32, min: (f32, f32)) -> Result<(), Error> {
    let was = (ui.min_width, ui.min_height);
    (ui.min_width, ui.min_height) = min;
    match layout(ui, scale) {
        Ok(_) => Ok(()),
        Err(error) => {
            put_back(ui, was);
            Err(error)
        }
    }
}

/// Gives `ui` the minimum size `min` again, and the geometry it lays out to: none of a refused
/// layout's stays.
fn put_back(ui: &mut Ui, min: (f32, f32)) {
    (ui.min_width, ui.min_height) = min;
    ui.layout();
}

/// Whether a layer surface anchored at `anchor` spans its output's width, and its height.
fn stretched(anchor: Anchor) -> (bool, bool) {
    (
        anchor.contains(Anchor::LEFT | Anchor::RIGHT),
        anchor.contains(Anchor::TOP | Anchor::BOTTOM),
    )
}

/// The size a layer surface asks for: its widgets' `size`, and 0 along a `stretch`ed axis,
/// which the compositor then sizes.
fn requested(size: (u32, u32), stretch: (bool, bool)) -> (u32, u32) {
    (
        if stretch.0 { 0 } else { size.0 },
        if stretch.1 { 0 } else { size.1 },
    )
}

/// The maximum size of a window whose widgets take `size`: that size when the window is not
/// resizable, as GTK sets a fixed window's on Wayland, and none otherwise.
fn max_size(ui: &Ui, size: (u32, u32)) -> Option<(u32, u32)> {
    (!ui.resizable).then_some(size)
}

/// The widgets' size in whole logical pixels, never empty.
fn logical_size(ui: &Ui) -> (u32, u32) {
    let (w, h) = ui.size();
    ((w.ceil() as u32).max(1), (h.ceil() as u32).max(1))
}

enum Shown {
    Window(Window),
    Layer(LayerSurface),
    Popup(Popup),
}

impl Shown {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Shown::Window(window) => window.wl_surface(),
            Shown::Layer(surface) => surface.wl_surface(),
            Shown::Popup(popup) => popup.wl_surface(),
        }
    }
}

/// A tooltip's widgets: its text in `parent`'s text and background colours swapped, as
/// libadwaita's tooltips, with libadwaita's 6 pixels of padding.
fn tooltip_ui(parent: &Ui, text: &str) -> Ui {
    let t = &parent.theme;
    let theme = Theme {
        background: t.text,
        text: t.background,
        margin: 6.0,
        ..t.clone()
    };
    let mut ui = Ui::new(text, theme, parent.rtl);
    ui.add(Ui::ROOT, Kind::Tooltip(text.to_owned()));
    ui
}

struct Surface {
    /// Drawing through wgpu when [`RENDERER`] asks for it; first, so that it drops before the
    /// wl_surface it draws on.
    #[cfg(feature = "gpu")]
    gpu: Option<Gpu>,
    id: SurfaceId,
    /// wp_fractional_scale_v1 and the viewport it draws through, when the compositor offers both.
    fractional: Option<(WpFractionalScaleV1, WpViewport)>,
    shown: Shown,
    ui: Ui,
    /// The surface's window on the accessibility bus; `None` while [`Shell::set_accessible`]
    /// keeps it out of the tree.
    a11y: Option<accesskit_unix::Adapter>,
    /// The tree the adapter activates with; `None` once the surface left the tree.
    initial: Arc<Mutex<Option<TreeUpdate>>>,
    /// The adapter's window is the focused one, which a new adapter takes over.
    window_focused: bool,
    /// Physical pixels per logical pixel: the preferred fractional scale, else wl_surface's
    /// integer one.
    scale: f32,
    /// The logical size last asked of a layer surface, or given as a window's minimum.
    size: (u32, u32),
    /// Whether a layer surface is anchored to both ends of its width, and of its height: it then
    /// asks 0 along that axis and takes the size the compositor configures.
    stretch: (bool, bool),
    /// A layer surface's layer and namespace.
    layer: Option<(Layer, String)>,
    /// The output a layer surface was opened on, when one was named: always for the surfaces of
    /// [`Application::output`]. The driver closes the surface when the output leaves.
    output: Option<wl_output::WlOutput>,
    configured: bool,
    /// A frame was committed since the first configure.
    drawn: bool,
    /// A frame is committed and its callback has not come: the next one waits for it, so that
    /// drawing never runs ahead of the compositor, which throttles a covered surface.
    frame_pending: bool,
    /// The state changed while a frame was pending: the callback draws it.
    redraw: bool,
    /// The widgets changed since the last draw.
    dirty: bool,
    /// The motion under way, which each frame callback advances.
    motion: Option<Running>,
    /// A window's application id, which its dialogs take.
    app_id: String,
    /// A dialog's `xdg_dialog_v1`, when the compositor offers the global.
    modal: Option<dialog::XdgDialogV1>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // Every close drops the surface: at the application's asking, the compositor's, or with
        // its output.
        self.withdraw();
        // Before the toplevel, which the fields drop after this.
        if let Some(modal) = self.modal.take() {
            modal.destroy();
        }
        // Before the wl_surface, which the fields drop after this.
        if let Some((scale, viewport)) = self.fractional.take() {
            scale.destroy();
            viewport.destroy();
        }
    }
}

impl Surface {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        self.shown.wl_surface()
    }

    /// Takes the surface out of the accessibility tree, if it is in it. Dropping the adapter
    /// takes its window off the application's children, and leaves its nodes on the bus: the
    /// window is deactivated and its nodes withdrawn first ([`leave`]).
    fn withdraw(&mut self) {
        let Some(mut a11y) = self.a11y.take() else {
            return;
        };
        // The dropped adapter's alone: a later adapter of the surface starts from a new one.
        let initial = std::mem::take(&mut self.initial);
        let scale = f64::from(self.scale);
        leave(&mut a11y, &initial, &mut self.ui, scale, self.window_focused);
    }

    fn draw(&mut self, pool: &mut SlotPool, qh: &QueueHandle<App>) -> Result<(), Error> {
        self.dirty = false;
        if !self.configured {
            return Ok(());
        }
        if self.frame_pending {
            // Lay out now, so that the accessibility tree follows the state at once; the
            // pixels follow at the frame callback.
            layout(&mut self.ui, self.scale)?;
            self.publish_tree();
            self.redraw = true;
            return Ok(());
        }
        self.redraw = false;
        self.frame_pending = true;
        let surface = self.wl_surface().clone();
        // Takes effect with the next commit: vello_gpu's present, or the driver's below.
        surface.frame(qh, FrameCallbackData(surface.clone()));
        #[cfg(feature = "gpu")]
        if self.gpu.is_some() {
            layout(&mut self.ui, self.scale)?;
            self.place();
            // Presenting commits the surface, with the state placed here.
            if let Some(gpu) = &mut self.gpu {
                gpu.draw(&mut self.ui, self.scale)?;
            }
            // A present wgpu dropped as Outdated or Lost commits nothing and reports nothing:
            // without this commit the frame callback would never come, and the surface would
            // never draw again. After a present that did commit, this one is empty.
            surface.commit();
            self.drawn = true;
            self.publish_tree();
            return Ok(());
        }
        let pixmap = render(&mut self.ui, self.scale)?;
        let (w, h) = (i32::from(pixmap.width()), i32::from(pixmap.height()));
        let (buffer, canvas) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)?;
        // vello_cpu writes premultiplied RGBA bytes; ARGB8888 is BGRA in memory.
        for (dst, src) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixmap.data_as_u8_slice().as_chunks::<4>().0)
        {
            *dst = [src[2], src[1], src[0], src[3]];
        }
        self.place();
        surface.damage_buffer(0, 0, w, h);
        buffer.attach_to(&surface)?;
        surface.commit();
        self.drawn = true;
        self.publish_tree();
        Ok(())
    }

    /// Sets, for the next commit, how the buffer drawn at `scale` maps onto the surface, and a
    /// layer surface's size, or a window's minimum size, when its widgets' changed.
    fn place(&mut self) {
        let size = logical_size(&self.ui);
        match &self.shown {
            Shown::Layer(surface) => {
                let asked = requested(size, self.stretch);
                if asked != self.size {
                    surface.set_size(asked.0, asked.1);
                    self.size = asked;
                }
            }
            Shown::Window(window) if size != self.size => {
                window.set_min_size(Some(size));
                window.set_max_size(max_size(&self.ui, size));
                self.size = size;
            }
            _ => {}
        }
        match &self.fractional {
            Some((_, viewport)) => {
                self.wl_surface().set_buffer_scale(1);
                viewport.set_destination(size.0 as i32, size.1 as i32);
            }
            // The integer scale divides the buffer: `layout` draws whole logical pixels.
            None => self.wl_surface().set_buffer_scale(self.scale as i32),
        }
    }

    /// Asks the compositor for a layer surface's size now: its widgets' size as last laid out, and
    /// 0 along a stretched axis. Sent with the next commit.
    fn request_size(&mut self) {
        if let Shown::Layer(surface) = &self.shown {
            let asked = requested(logical_size(&self.ui), self.stretch);
            surface.set_size(asked.0, asked.1);
            self.size = asked;
        }
    }

    /// Hands the tree of the surface just drawn to the accessibility adapter.
    fn publish_tree(&mut self) {
        for update in tree_updates(&mut self.ui, f64::from(self.scale)) {
            if let Ok(mut initial) = self.initial.lock() {
                *initial = Some(update.clone());
            }
            if let Some(a11y) = &mut self.a11y {
                a11y.update_if_active(|| update);
            }
        }
    }
}

/// The accessibility adapter of surface `id`, whose first tree is `initial`'s and whose actions
/// travel on `actions` to the event loop.
fn adapter(
    id: SurfaceId,
    initial: &Arc<Mutex<Option<TreeUpdate>>>,
    actions: &Sender<(SurfaceId, ActionRequest)>,
) -> accesskit_unix::Adapter {
    accesskit_unix::Adapter::new(
        Activation(Arc::clone(initial)),
        Actions(id, actions.clone()),
        Deactivation,
    )
}

struct Activation(Arc<Mutex<Option<TreeUpdate>>>);

impl ActivationHandler for Activation {
    /// `None` once the surface left the tree: the adapter then stays pending, registering
    /// nothing, until it is dropped.
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().ok().and_then(|tree| tree.clone())
    }
}

struct Actions(SurfaceId, Sender<(SurfaceId, ActionRequest)>);

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // The receiver lives as long as the event loop; once it is gone the driver is ending
        // and there is nothing left to act on.
        let _ = self.1.send((self.0, request));
    }
}

struct Deactivation;

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

/// The user data of wp_fractional_scale_v1 and wp_viewporter's objects.
struct Scaling;

impl Dispatch2<WpFractionalScaleManagerV1, App> for Scaling {
    fn event(
        &self,
        _: &mut App,
        _: &WpFractionalScaleManagerV1,
        _: wp_fractional_scale_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

impl Dispatch2<WpFractionalScaleV1, App> for Scaling {
    fn event(
        &self,
        app: &mut App,
        proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let Some(surface) = app
                .shell
                .surfaces
                .iter_mut()
                .find(|s| s.fractional.as_ref().is_some_and(|(f, _)| f == proxy))
            else {
                return;
            };
            // The protocol's unit: 120ths.
            surface.scale = scale as f32 / 120.0;
            surface.dirty = true;
            let id = surface.id;
            app.changed(id, None);
        }
    }
}

impl Dispatch2<WpViewporter, App> for Scaling {
    fn event(
        &self,
        _: &mut App,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

impl Dispatch2<WpViewport, App> for Scaling {
    fn event(
        &self,
        _: &mut App,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

/// The text-input-v3 state of the seat.
#[derive(Default)]
struct Ime {
    /// The surface the compositor sent `enter` for, and no `leave` since.
    entered: Option<SurfaceId>,
    /// The field we sent `enable` for, and no `disable` since.
    enabled: Option<ImeField>,
    /// The `commit` requests sent so far, which a `done` that answers the latest state names.
    commits: u32,
    pending: Pending,
}

impl Ime {
    /// Takes what a `done` applies, and whether its serial answers the latest state we sent:
    /// text-input-v3 says a `done` with another serial is applied, but must not change the
    /// object's state.
    fn done(&mut self, serial: u32) -> (Pending, bool) {
        (std::mem::take(&mut self.pending), serial == self.commits)
    }

    /// The compositor's `leave`, or the close of the surface it entered: what the input method
    /// sent and no `done` applied goes too. Returns the surface it had entered.
    fn leave(&mut self) -> Option<SurfaceId> {
        self.pending = Pending::default();
        self.entered.take()
    }

    /// Whether the text input is to be enabled for another focused field, or disabled for none:
    /// text-input-v3 asks for `enable` at every change of the focused field, on the same surface
    /// too. Counts the `commit` that sends it.
    fn change(&mut self, field: Option<ImeField>) -> bool {
        if field == self.enabled {
            return false;
        }
        self.enabled = field;
        self.commits = self.commits.wrapping_add(1);
        true
    }
}

/// A focused field the input method writes to: its surface, its widget, and its content hint.
type ImeField = (SurfaceId, WidgetId, ContentHint);

/// The focused field of `ui` the input method writes to: its widget, its rectangle, and its
/// content hint, multi-line for a text area.
fn ime_field(ui: &Ui) -> Option<(WidgetId, Rect, ContentHint)> {
    let id = ui.focus().filter(|&id| ui.field_text(id).is_some())?;
    let widget = ui.widget(id);
    let hint = match widget.kind {
        Kind::TextArea(_) => ContentHint::Multiline,
        _ => ContentHint::None,
    };
    Some((id, widget.rect, hint))
}

/// The events of text-input-v3 that take effect together at its next `done`.
#[derive(Default)]
struct Pending {
    /// The preedit and its cursor, a byte range in it.
    preedit: Option<(String, Option<(usize, usize)>)>,
    commit: Option<String>,
}

/// The preedit without what a single-line field drops from it, the newline included, with its
/// cursor's byte offsets moved to match: a preedit is shown on one line. A cursor that is not
/// on a character boundary of the preedit is dropped.
fn printable_preedit(
    text: &str,
    cursor: Option<(usize, usize)>,
) -> (String, Option<(usize, usize)>) {
    let kept = |at: usize| {
        text.is_char_boundary(at).then(|| {
            let dropped: usize = text[..at]
                .chars()
                .filter(|&c| !crate::text::keeps(c, false))
                .map(char::len_utf8)
                .sum();
            at - dropped
        })
    };
    let cursor = cursor.and_then(|(begin, end)| kept(begin).zip(kept(end)));
    (crate::text::printable(text, false), cursor)
}

/// The user data of the text-input-v3 objects.
struct TextInput;

impl Dispatch2<ZwpTextInputManagerV3, App> for TextInput {
    fn event(
        &self,
        _: &mut App,
        _: &ZwpTextInputManagerV3,
        _: zwp_text_input_manager_v3::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
    }
}

impl Dispatch2<ZwpTextInputV3, App> for TextInput {
    fn event(
        &self,
        app: &mut App,
        _: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        _: &Connection,
        _: &QueueHandle<App>,
    ) {
        use zwp_text_input_v3::Event;
        match event {
            Event::Enter { surface } => {
                app.ime.entered = app.shell.find(&surface).map(|s| s.id);
                app.sync_text_input();
            }
            Event::Leave { .. } => app.leave_text_input(),
            Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                let cursor = usize::try_from(cursor_begin)
                    .ok()
                    .zip(usize::try_from(cursor_end).ok());
                app.ime.pending.preedit = Some((text.unwrap_or_default(), cursor));
            }
            Event::CommitString { text } => app.ime.pending.commit = text,
            // ponytail: the field sends no surrounding text, so an input method has none to
            // delete; send it with set_surrounding_text when an engine needs it.
            Event::DeleteSurroundingText { .. } => {}
            Event::Done { serial } => app.text_input_done(serial),
            _ => {}
        }
    }
}

/// Where an event of a surface's widgets goes, other than a window button's.
#[derive(Debug, PartialEq)]
enum Route {
    /// A field opens its own context menu, whose items the driver does.
    FieldMenu(WidgetId),
    /// A drop-down opens its own items, as a menu the driver does too.
    DropDown(WidgetId),
    /// An item of the driver's own menu was activated: a menu item, a check or a radio.
    Chosen(WidgetId),
    /// The driver keeps it: the application never opened its menus, and gets none of their
    /// events, such as a context-menu event of an item at a press of the Menu key, nor the click
    /// of a drop-down with nothing to offer.
    Kept,
    /// The application's.
    Application(Event),
}

/// Where `event` of `ui` goes; `own` when `ui` is a menu the driver opened.
fn route(ui: &Ui, event: Event, own: bool) -> Route {
    let drop_down = |widget: WidgetId| {
        ui.widgets
            .get(widget)
            .is_some_and(|w| matches!(w.kind, Kind::DropDown(_)))
    };
    match event {
        Event::Clicked(item) | Event::Toggled(item) | Event::Selected(item) if own => {
            Route::Chosen(item)
        }
        _ if own => Route::Kept,
        Event::ContextMenu(widget) if ui.field_text(widget).is_some() => Route::FieldMenu(widget),
        Event::Clicked(widget) if drop_down(widget) => match ui.chosen(widget) {
            Some(_) => Route::DropDown(widget),
            None => Route::Kept,
        },
        event => Route::Application(event),
    }
}

/// What an item of a menu the driver opened does.
enum Choice {
    /// A field's context menu: the edit of this key.
    Edit(Key),
    /// A drop-down's items: choose the one at this index.
    Item(usize),
}

struct App {
    application: Box<dyn Application>,
    shell: Shell,
    /// The layer surface the application has on each output.
    outputs: PerOutput<wl_output::WlOutput, SurfaceId>,
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    shm: Shm,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// The surface that has the keyboard.
    keyboard_focus: Option<SurfaceId>,
    /// The surface whose accessibility adapter is the focused window.
    window_focus: Option<SurfaceId>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shape: Option<CursorShapeManager>,
    cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    /// The seat of the pointer, which moving a window by its title bar names.
    pointer_seat: Option<wl_seat::WlSeat>,
    text_input_manager: Option<ZwpTextInputManagerV3>,
    text_input: Option<ZwpTextInputV3>,
    ime: Ime,
    /// For the keyboard's repeat timer. The keyboard's data keeps a clone, and the loop owns the
    /// connection that owns that data: the cycle lives until the process ends, as `run` is
    /// called once per process.
    loop_handle: LoopHandle<'static, App>,
    modifiers: Modifiers,
    clipboards: clipboard::Clipboards,
    /// The menu the driver opened, a field's context menu or a drop-down's items: its popup,
    /// and what each of its items does.
    own_menu: Option<(SurfaceId, Vec<(WidgetId, Choice)>)>,
    /// Where a button is being pressed, while its press is handled: a field's context menu
    /// opens there.
    pressed_at: Option<(f32, f32)>,
    /// When the tooltip of the widget under the pointer, or focused, shows.
    tooltip: Tooltip<(SurfaceId, WidgetId)>,
    /// An error raised inside a callback, returned by [`run`].
    error: Option<Error>,
}

impl App {
    fn fail(&mut self, error: Error) {
        self.error = Some(error);
        self.shell.exit = true;
    }

    /// Hands an event of `id`'s widgets to the application, then redraws and updates the
    /// accessibility trees, the input method and the clipboards.
    fn changed(&mut self, id: SurfaceId, event: Option<Event>) {
        self.update(id, event);
        if !self.shell.exit {
            self.sync_text_input();
            self.clips(id);
        }
    }

    /// Hands an event of `id`'s widgets to the application, then redraws what changed and
    /// updates the accessibility trees.
    fn update(&mut self, id: SurfaceId, event: Option<Event>) {
        let dialog = self.shell.dialogs.is_dialog(id);
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        // The event came from these widgets, which it changed.
        surface.dirty = true;
        match event {
            Some(Event::Window(WindowButton::Minimize)) => {
                if let Shown::Window(window) = &surface.shown {
                    window.set_minimized();
                }
            }
            // A dialog's Close is the application's to answer.
            Some(Event::Window(WindowButton::Close)) if !dialog => {
                self.shell.exit = true;
                return;
            }
            Some(event) => {
                let own = self.own_menu.as_ref().is_some_and(|m| m.0 == id);
                match route(&surface.ui, event, own) {
                    Route::FieldMenu(widget) => self.open_field_menu(id, widget),
                    Route::DropDown(widget) => self.open_drop_down(id, widget),
                    Route::Chosen(item) => self.menu_chosen(item),
                    Route::Kept => {}
                    Route::Application(event) => self.application.event(&mut self.shell, id, event),
                }
            }
            None => {}
        }
        self.draw_dirty();
    }

    /// Opens the edit menu of field `widget` of `id`, as GTK's entry does: under the pointer for
    /// a press, under the field for a key.
    fn open_field_menu(&mut self, id: SurfaceId, widget: WidgetId) {
        let can_paste = self.can_paste();
        let Some((menu, items)) = self
            .shell
            .ui(id)
            .and_then(|ui| ui.field_menu(widget, can_paste))
        else {
            return;
        };
        // ponytail: a key opens the menu under the whole field, not under its caret; place it
        // at the caret's rectangle if a tall text area needs it.
        match self
            .shell
            .show_popup(id, widget, self.pressed_at, menu, Side::Bottom, true)
        {
            Ok(surface) => {
                let items = items.into_iter().map(|(item, key)| (item, Choice::Edit(key)));
                self.own_menu = Some((surface, items.collect()));
            }
            Err(error) => eprintln!("athanor-sal: no context menu for the field: {error}"),
        }
    }

    /// Opens the items of drop-down `widget` of `id` in a menu under it, with the focus on the
    /// chosen one, as GTK's DropDown does.
    fn open_drop_down(&mut self, id: SurfaceId, widget: WidgetId) {
        let max_height = self.shell.surfaces.iter().find(|s| s.id == id).and_then(|from| {
            let height = |info: &OutputInfo| info.logical_size.map(|(_, h)| h as f32);
            let output = from.output.as_ref().and_then(|output| {
                let (_, info) = self.shell.outputs.iter().find(|(o, _)| o == output)?;
                height(info)
            });
            let outputs: Vec<f32> = self.shell.outputs.iter().filter_map(|(_, i)| height(i)).collect();
            menu_max_height(output, &outputs, from.size.1)
        });
        let Some((menu, items)) = self
            .shell
            .ui(id)
            .and_then(|ui| ui.drop_down_menu(widget, max_height))
        else {
            return;
        };
        let chosen = items.iter().find(|&&(item, _)| menu.is_active(item)).map(|&(item, _)| item);
        match self.shell.show_popup(id, widget, None, menu, Side::Bottom, true) {
            Ok(surface) => {
                if let Some(ui) = self.shell.ui_mut(surface) {
                    ui.set_focus(chosen);
                }
                let items = items.into_iter().map(|(item, index)| (item, Choice::Item(index)));
                self.own_menu = Some((surface, items.collect()));
            }
            Err(error) => eprintln!("athanor-sal: no menu for the drop-down: {error}"),
        }
    }

    /// Does what `item` of the driver's open menu says, once the menu has closed and the focus
    /// returned to its widget: edits the field, or chooses in the drop-down.
    fn menu_chosen(&mut self, item: WidgetId) {
        let (Some((surface, items)), Some(open)) = (self.own_menu.take(), self.shell.popup)
        else {
            return;
        };
        let Some((_, choice)) = items.into_iter().find(|&(i, _)| i == item) else {
            return;
        };
        if open.surface != surface {
            return;
        }
        self.close_popup();
        let event = self
            .shell
            .get_mut(open.parent)
            .and_then(|parent| {
                let anchor = open.anchor_in(&parent.ui)?;
                match choice {
                    Choice::Edit(key) => parent.ui.field_edit(anchor, key),
                    Choice::Item(index) => parent.ui.choose(anchor, index),
                }
            });
        self.changed(open.parent, event);
    }

    /// An output announced or changed: one opens for the application if it has none, and the
    /// application learns of the output of its surface.
    fn output_seen(&mut self, output: wl_output::WlOutput) {
        let Some(info) = self.output.info(&output) else {
            return;
        };
        match self.shell.outputs.iter_mut().find(|(o, _)| *o == output) {
            Some((_, known)) => *known = info.clone(),
            None => self.shell.outputs.push((output.clone(), info.clone())),
        }
        // A surface the application closed since is not asked again, nor told of the change.
        self.closed_by_application();
        let App {
            application,
            shell,
            outputs,
            ..
        } = self;
        let seen = outputs.seen(output, |output| {
            application
                .output(&info)
                .map(|(ui, options)| shell.open(ui, Role::Layer(options), Some(output)))
                .transpose()
        });
        match seen {
            Ok(Some(id)) => {
                self.application.output_changed(&mut self.shell, id, &info);
                self.draw_dirty();
            }
            Ok(None) => {}
            Err(error) => self.fail(error),
        }
    }

    /// Enables the input method while a surface has it and a field the focus, and disables it
    /// otherwise.
    fn sync_text_input(&mut self) {
        let Some(text_input) = &self.text_input else {
            return;
        };
        let field = self
            .ime
            .entered
            .and_then(|surface| Some((surface, ime_field(self.shell.ui(surface)?)?)));
        if !self.ime.change(field.map(|(surface, (id, _, hint))| (surface, id, hint))) {
            return;
        }
        if let Some((_, (_, r, hint))) = field {
            text_input.enable();
            text_input.set_content_type(hint, ContentPurpose::Normal);
            text_input.set_cursor_rectangle(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
        } else {
            text_input.disable();
        }
        text_input.commit();
    }

    /// Text input leaves the surface it entered: the focused field there ends its composition,
    /// as the input method sends no more for it, and the text input is disabled.
    fn leave_text_input(&mut self) {
        if let Some(id) = self.ime.leave() {
            let ended = self
                .shell
                .get_mut(id)
                .is_some_and(|surface| surface.ui.end_composition());
            if ended {
                self.update(id, None);
            }
        }
        self.sync_text_input();
    }

    /// Closes a surface. Text input on it ends as at a `leave`, which the compositor may not
    /// send for a surface that is gone: the next `enter` then sends `enable` again. The primary
    /// selection its fields offered ends with it.
    fn close(&mut self, id: SurfaceId) {
        self.shell.remove(id);
        self.forget(id);
    }

    /// Ends the text input and the primary selection of the closed surface `id`; a dialog that
    /// held the keyboard hands it to the surface it was opened on, while that one is open.
    fn forget(&mut self, id: SurfaceId) {
        let parent = self.shell.dialogs.take_parent(id);
        if self.keyboard_focus == Some(id) {
            self.keyboard_focus = parent.filter(|&parent| self.shell.ui(parent).is_some());
        }
        self.unselect(id);
        if self.ime.entered == Some(id) {
            self.leave_text_input();
        }
    }

    /// Ends what the surfaces [`Shell::close`] closed held: their place on their output, their
    /// text input and their primary selection.
    fn closed_by_application(&mut self) {
        for id in std::mem::take(&mut self.shell.closing) {
            self.outputs.closed(id);
            self.forget(id);
        }
    }

    /// Ends the pointer's press and hover on each window a dialog opened on in this turn and
    /// still blocks, as if the pointer left it: the release it waits for is the dialog's, a drag
    /// under way ends over no widget, and its tooltip, shown or waiting, ends. A paste still
    /// being read into it is dropped: its text would reach a window that takes no input.
    fn blocked_by_dialogs(&mut self) {
        for parent in std::mem::take(&mut self.shell.blocking) {
            if !self.shell.dialogs.blocked(parent) {
                continue;
            }
            if self.tooltip.target().is_some_and(|&(id, _)| id == parent) {
                self.point_tooltip(None);
            }
            self.drop_paste_into(parent);
            let Some(surface) = self.shell.get_mut(parent) else {
                continue;
            };
            let event = surface.ui.pointer_leave();
            self.changed(parent, event);
        }
    }

    /// Closes `id`, which the application did not ask to close, and tells it. One the
    /// application closes from Application::closed is closed at its asking, and it is not told
    /// of it. Only layer surfaces are lost today, and dialogs open on windows only: the dialogs
    /// of `id`, told first, each before the surface it was opened on, are for a window lost.
    fn lost(&mut self, id: SurfaceId) {
        let order = self.shell.dialogs.closing_order(id);
        each_open(
            self,
            order,
            |app, surface| app.shell.surfaces.iter().any(|s| s.id == surface),
            |app, surface| {
                app.outputs.closed(surface);
                app.close(surface);
                app.application.closed(&mut app.shell, surface);
            },
        );
    }

    /// Moves the accessibility adapters' window focus to the surface that takes the keys, after
    /// every event: the open menu's while it holds them, though the compositor may keep the
    /// keyboard on its parent, then back to the parent once it closes, and a blocked window's
    /// topmost dialog. An adapter emits its focus events only while its window is focused.
    fn sync_window_focus(&mut self) {
        let focused = focused_window(self.shell.popup, self.key_target());
        if focused == self.window_focus {
            return;
        }
        for (id, is_focused) in [(self.window_focus, false), (focused, true)] {
            if let Some(surface) = id.and_then(|id| self.shell.get_mut(id)) {
                surface.window_focused = is_focused;
                if let Some(a11y) = &mut surface.a11y {
                    a11y.update_window_focus_state(is_focused);
                }
            }
        }
        self.window_focus = focused;
    }

    /// Closes a drop-down's menu whose items the application replaced, as Escape does, then
    /// draws the surfaces whose widgets changed.
    fn draw_dirty(&mut self) {
        if let Some(open) = self.shell.popup {
            let stale = self
                .shell
                .get_mut(open.parent)
                .is_some_and(|from| std::mem::take(&mut from.ui.stale_popup));
            if stale {
                self.own_menu = None;
                self.close_popup();
            }
        }
        if let Err(error) = self.shell.draw_dirty() {
            self.fail(error);
        }
    }

    /// Closes the popup open, and redraws the surface it was opened from.
    fn close_popup(&mut self) {
        if let Some(open) = self.shell.popup {
            self.close(open.surface);
            self.changed(open.parent, None);
        }
    }

    /// Closes the tooltip shown, if the popup open is one.
    fn close_tooltip(&mut self) {
        if let Some(open) = self.shell.popup.filter(|p| !p.menu) {
            self.close(open.surface);
        }
    }

    /// A press, a key or a scroll hides the tooltip.
    fn hide_tooltip(&mut self) {
        if self.tooltip.hide(Instant::now()) {
            self.close_tooltip();
        }
    }

    /// The widget with a tooltip under the pointer, or newly focused, is now `target`: the
    /// tooltip shown hides, and the target's shows after GTK's delay, if the widget was not
    /// removed since.
    fn point_tooltip(&mut self, target: Option<(SurfaceId, WidgetId)>) {
        let (hide, wait) = self.tooltip.point(target, Instant::now());
        if hide {
            self.close_tooltip();
        }
        let Some((wait, generation)) = wait else {
            return;
        };
        let timer = Timer::from_duration(wait);
        let inserted = self
            .loop_handle
            .insert_source(timer, move |_, _, app: &mut App| {
                app.tooltip_due(generation);
                TimeoutAction::Drop
            });
        if let Err(error) = inserted {
            self.fail(error.error.to_string().into());
        }
    }

    /// The wait of `generation` ran out: the tooltip shows, unless a menu is open, as none shows
    /// over GTK's open popover.
    fn tooltip_due(&mut self, generation: u64) {
        if self.shell.popup.is_some_and(|p| p.menu) {
            return;
        }
        let Some((id, widget)) = self.tooltip.due(generation) else {
            return;
        };
        let Some(ui) = self.shell.ui(id) else {
            return;
        };
        let text = (ui.contains(widget) && ui.is_visible(widget))
            .then(|| ui.tooltip(widget))
            .flatten();
        let Some(text) = text else {
            return;
        };
        let tip = tooltip_ui(ui, text);
        // A tooltip that cannot be shown leaves the application running, and says why.
        if let Err(error) = self
            .shell
            .show_popup(id, widget, None, tip, Side::Bottom, false)
        {
            eprintln!("athanor-sal: no tooltip: {error}");
        }
    }

    /// The pointer moved on `id`: the hovered widget follows it, a drag may start or a field's
    /// selection follow it, and the tooltip follows the hovered widget.
    fn pointer_moved(&mut self, id: SurfaceId, x: f32, y: f32) {
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        let (before, selecting) = (surface.ui.hovered(), surface.ui.selecting());
        let event = surface.ui.pointer_motion(x, y);
        let hovered = surface.ui.hovered();
        let target = hovered.filter(|&w| surface.ui.tooltip(w).is_some());
        if event.is_some() || hovered != before || selecting {
            self.changed(id, event);
        }
        self.point_tooltip(target.map(|w| (id, w)));
    }

    /// A button pressed on `id`. A press outside the open menu closes it and goes no further,
    /// as with GTK's popovers; a press on a window's title bar outside its buttons moves it.
    fn pointer_pressed(&mut self, id: SurfaceId, (x, y): (f32, f32), button: u32, serial: u32) {
        self.shell.grab.pressed(serial);
        self.hide_tooltip();
        if self
            .shell
            .popup
            .is_some_and(|p| p.dismissed_by(Dismiss::Press(id)))
        {
            self.close_popup();
            return;
        }
        let button = match button {
            BTN_LEFT => PointerButton::Primary,
            BTN_RIGHT => PointerButton::Secondary,
            BTN_MIDDLE => PointerButton::Middle,
            _ => return,
        };
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        if let (Shown::Window(window), PointerButton::Primary) = (&surface.shown, button) {
            if surface.ui.hit(x, y).is_none() && surface.ui.in_title_bar(x, y) {
                if let Some(seat) = &self.pointer_seat {
                    window.move_(seat, serial);
                }
                return;
            }
        }
        let event = surface.ui.pointer_press(x, y, button);
        self.pressed_at = Some((x, y));
        self.changed(id, event);
        self.pressed_at = None;
    }

    /// Applies what the input method sent since the last `done`, in the protocol's order: the
    /// committed text replaces the preedit, then the new preedit is shown. A `done` that
    /// answers an older state sends no new state; the next one that answers the latest does.
    fn text_input_done(&mut self, serial: u32) {
        let (Pending { preedit, commit }, current) = self.ime.done(serial);
        let Some(id) = self.ime.entered.filter(|&id| !self.shell.dialogs.blocked(id)) else {
            return;
        };
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        // The field keeps what it takes of the text: a text area its newlines.
        let event = commit.and_then(|text| surface.ui.commit(&text));
        let (preedit, cursor) = preedit.unwrap_or_default();
        let (preedit, cursor) = printable_preedit(&preedit, cursor);
        surface.ui.compose(&preedit, cursor);
        if current {
            self.changed(id, event);
        } else {
            self.update(id, event);
        }
    }

    /// Tells the application the keyboard entered or left `wl`, one of its surfaces.
    fn keyboard_moved(&mut self, wl: &wl_surface::WlSurface, focused: bool) {
        let Some(id) = self.shell.find(wl).map(|surface| surface.id) else {
            return;
        };
        self.application.keyboard(&mut self.shell, id, focused);
        self.draw_dirty();
    }

    /// The surface that takes the keys the compositor sends: the one it focused, or the topmost
    /// dialog that blocks it, which the compositor may never focus.
    fn key_target(&self) -> Option<SurfaceId> {
        self.keyboard_focus.map(|id| self.shell.dialogs.topmost(id))
    }

    /// A key held down: as pressed again, with the serial of its press, which the repeat does
    /// not carry, except the keys that act once per press.
    fn repeat(&mut self, event: KeyEvent) {
        let focused = self
            .key_target()
            .and_then(|id| self.shell.ui(id))
            .and_then(|ui| Some((ui, ui.focus()?)));
        let in_field = focused.is_some_and(|(ui, id)| ui.field_text(id).is_some());
        let in_area =
            focused.is_some_and(|(ui, id)| matches!(ui.widget(id).kind, Kind::TextArea(_)));
        if !once_per_press(event.keysym, self.modifiers.ctrl, in_field, in_area) {
            self.shell.grab.repeat();
            self.key(event);
            self.shell.grab.key(None);
        }
    }

    fn key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Tab if self.modifiers.shift => Key::BackTab,
            Keysym::Tab => Key::Tab,
            Keysym::ISO_Left_Tab => Key::BackTab,
            Keysym::Left if self.modifiers.shift => Key::SelectLeft,
            Keysym::Right if self.modifiers.shift => Key::SelectRight,
            Keysym::Left => Key::Left,
            Keysym::Right => Key::Right,
            Keysym::a | Keysym::A if self.modifiers.ctrl => Key::SelectAll,
            Keysym::c | Keysym::C if self.modifiers.ctrl => Key::Copy,
            Keysym::x | Keysym::X if self.modifiers.ctrl => Key::Cut,
            Keysym::v | Keysym::V if self.modifiers.ctrl => Key::Paste,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
            Keysym::Home => Key::Home,
            Keysym::End => Key::End,
            Keysym::Page_Up => Key::PageUp,
            Keysym::Page_Down => Key::PageDown,
            Keysym::Return | Keysym::KP_Enter => Key::Enter,
            Keysym::BackSpace => Key::Backspace,
            Keysym::Delete => Key::Delete,
            Keysym::Escape => Key::Escape,
            Keysym::F10 if self.modifiers.shift => Key::Menu,
            Keysym::Menu => Key::Menu,
            _ => match event.utf8 {
                Some(text) if !text.chars().any(char::is_control) => Key::Text(text),
                _ => return,
            },
        };
        self.hide_tooltip();
        if key == Key::Escape
            && self
                .shell
                .popup
                .is_some_and(|p| p.dismissed_by(Dismiss::Escape))
        {
            self.close_popup();
            return;
        }
        // An open menu takes the keys, whichever of the application's surfaces the compositor
        // sends them to, and a blocked window's topmost dialog takes them from it.
        let target = match self.shell.popup {
            Some(open) if open.menu => Some(open.surface),
            _ => self.key_target(),
        };
        let Some(id) = target.filter(|&id| !self.shell.dialogs.blocked(id)) else {
            return;
        };
        let modifiers = KeyModifiers::from(self.modifiers);
        if self.shell.get_mut(id).is_some()
            && self.application.key(&mut self.shell, id, &key, modifiers)
        {
            self.changed(id, None);
            return;
        }
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        let before = surface.ui.focus();
        let event = surface.ui.key(key);
        let after = surface.ui.focus();
        let tip = after.filter(|&w| surface.ui.tooltip(w).is_some());
        self.changed(id, event);
        // The keyboard focus shows its widget's tooltip as the pointer does.
        if after != before {
            self.point_tooltip(tip.map(|w| (id, w)));
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        wl: &wl_surface::WlSurface,
        factor: i32,
    ) {
        let Some(surface) = self.shell.find(wl) else {
            return;
        };
        // The fractional scale, when there is one, is the scale.
        if surface.fractional.is_none() {
            surface.scale = factor.max(1) as f32;
            let id = surface.id;
            self.changed(id, None);
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        wl: &wl_surface::WlSurface,
        time: u32,
    ) {
        let reduced = self.shell.reduced_motion;
        let Some(surface) = self.shell.find(wl) else {
            return;
        };
        surface.frame_pending = false;
        let id = surface.id;
        // The motion under way takes its next step, which this callback's time places.
        let ended = surface.motion.as_mut().is_some_and(|running| {
            let (pose, ended) = running.frame(time, reduced);
            surface.ui.pose = pose;
            surface.redraw = true;
            ended
        });
        if ended {
            surface.motion = None;
        }
        let redraw = surface.redraw;
        if ended {
            self.shell.settled.push(id);
        }
        if redraw {
            self.update(id, None);
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.output_seen(output);
    }

    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.output_seen(output);
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.shell.outputs.retain(|(o, _)| *o != output);
        self.outputs.left(&output);
        // One at a time: a surface of the output the application closes from
        // Application::closed is closed at its asking, and it is not told of it.
        lose_each(
            self,
            |app| {
                let mut on = app.shell.surfaces.iter();
                on.find(|s| s.output.as_ref() == Some(&output)).map(|s| s.id)
            },
            App::lost,
        );
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        let Some(surface) = self.shell.find(layer.wl_surface()) else {
            return;
        };
        let id = surface.id;
        self.lost(id);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(surface) = self.shell.find(layer.wl_surface()) else {
            return;
        };
        // Along a stretched axis the surface takes the size configured; along any other it
        // keeps its widgets' size whatever the compositor suggests.
        let (width, height) = configure.new_size;
        if surface.stretch.0 && width > 0 {
            surface.ui.min_width = width as f32;
        }
        if surface.stretch.1 && height > 0 {
            surface.ui.min_height = height as f32;
        }
        surface.configured = true;
        let id = surface.id;
        self.changed(id, None);
    }
}

impl WindowHandler for App {
    /// Ends the run, but for a dialog, whose application is asked as at its Close button, and
    /// for a window a dialog blocks, which stays open with the dialog unanswered, as GTK's.
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, window: &Window) {
        let Some(id) = self.shell.find(window.wl_surface()).map(|s| s.id) else {
            self.shell.exit = true;
            return;
        };
        match self.shell.dialogs.close_request(id) {
            CloseRequest::Dialog => self.update(id, Some(Event::Window(WindowButton::Close))),
            CloseRequest::Ignored => {}
            CloseRequest::Exit => self.shell.exit = true,
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let Some(surface) = self.shell.find(window.wl_surface()) else {
            return;
        };
        // ponytail: once drawn, the title bar stays if the compositor later takes the
        // decorations over; no compositor of Athanor's switches modes on a mapped window.
        if configure.decoration_mode == DecorationMode::Client {
            surface.ui.add_title_bar();
        }
        // ponytail: the window keeps the widgets' natural size whatever the compositor
        // suggests; stretching to the configured size comes with the layout milestone.
        surface.configured = true;
        let id = surface.id;
        self.changed(id, None);
    }
}

impl PopupHandler for App {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        _: PopupConfigure,
    ) {
        // ponytail: the popup keeps its widgets' size and the place the compositor gave it; a
        // menu whose items change while it is open would need a reposition.
        let Some(surface) = self.shell.find(popup.wl_surface()) else {
            return;
        };
        surface.configured = true;
        let id = surface.id;
        self.changed(id, None);
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        let Some(surface) = self.shell.find(popup.wl_surface()) else {
            return;
        };
        let id = surface.id;
        if self.shell.popup.is_some_and(|p| p.surface == id) {
            self.close_popup();
        } else {
            self.close(id);
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.shell.seat = Some(seat.clone());
        self.clipboards.seat(qh, &seat);
        let result: Result<(), Error> = match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                if let (Some(manager), None) = (&self.text_input_manager, &self.text_input) {
                    self.text_input = Some(manager.get_text_input(&seat, qh, TextInput));
                }
                self.seat
                    .get_keyboard_with_repeat(
                        qh,
                        &seat,
                        None,
                        self.loop_handle.clone(),
                        Box::new(|app: &mut App, _, event| app.repeat(event)),
                    )
                    .map(|k| self.keyboard = Some(k))
                    .map_err(Into::into)
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer_seat = Some(seat.clone());
                self.seat
                    .get_pointer(qh, &seat)
                    .map(|p| {
                        self.cursor_shape_device = self
                            .cursor_shape
                            .as_ref()
                            .map(|manager| manager.get_shape_device(&p, qh));
                        self.pointer = Some(p);
                    })
                    .map_err(Into::into)
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(error);
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboard.take() {
                    keyboard.release();
                }
            }
            Capability::Pointer => {
                if let Some(device) = self.cursor_shape_device.take() {
                    device.destroy();
                }
                if let Some(pointer) = self.pointer.take() {
                    pointer.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        wl: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_focus = self.shell.find(wl).map(|surface| surface.id);
        self.keyboard_moved(wl, true);
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        wl: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.keyboard_focus = None;
        self.keyboard_moved(wl, false);
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.shell.grab.key(Some(serial));
        self.key(event);
        self.shell.grab.key(None);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.repeat(event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(device) = &self.cursor_shape_device {
                        device.set_shape(serial, Shape::Default);
                    }
                }
                PointerEventKind::Leave { .. } => self.point_tooltip(None),
                _ => {}
            }
            // A parent a dialog blocks takes nothing but the pointer leaving it.
            let leaving = matches!(event.kind, PointerEventKind::Leave { .. });
            let open = self.shell.find(&event.surface).map(|s| s.id);
            let blocked = !leaving && open.is_some_and(|id| self.shell.dialogs.blocked(id));
            if let Some(surface) = self.shell.find(&event.surface).filter(|_| !blocked) {
                let id = surface.id;
                let at = (event.position.0 as f32, event.position.1 as f32);
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        self.pointer_moved(id, at.0, at.1);
                    }
                    PointerEventKind::Leave { .. } => {
                        let event = surface.ui.pointer_leave();
                        self.changed(id, event);
                    }
                    PointerEventKind::Press { button, serial, .. } => {
                        self.pointer_pressed(id, at, button, serial);
                    }
                    PointerEventKind::Release {
                        button: BTN_LEFT, ..
                    } => {
                        let event = surface
                            .ui
                            .pointer_release(at.0, at.1, PointerButton::Primary);
                        self.changed(id, event);
                    }
                    PointerEventKind::Axis {
                        horizontal,
                        vertical,
                        ..
                    } => {
                        let (changed, event) = axis(&mut surface.ui, at, horizontal, vertical);
                        self.hide_tooltip();
                        if changed {
                            self.changed(id, event);
                        }
                    }
                    _ => {}
                }
            }
            if let PointerEventKind::Enter { .. } | PointerEventKind::Leave { .. } = event.kind {
                if let Some(id) = self.shell.find(&event.surface).map(|s| s.id) {
                    let inside = matches!(event.kind, PointerEventKind::Enter { .. });
                    self.application.pointer(&mut self.shell, id, inside);
                    self.draw_dirty();
                }
            }
            // The press grabs no more once its button is up or the pointer has left, on any
            // surface, one closed since included.
            if matches!(
                event.kind,
                PointerEventKind::Release { .. } | PointerEventKind::Leave { .. }
            ) {
                self.shell.grab.released();
            }
        }
    }
}

/// The wheel or the touchpad turned at `at` on `ui`: the innermost scroll or text area under
/// the pointer takes the vertical axis, whether or not it moves, as GTK's scrolled window does,
/// and the widget under it gets what no scroll took, each axis on its own, as whole steps
/// ([`Ui::pointer_wheel`]). Whether the widgets changed, and the event.
fn axis(
    ui: &mut Ui,
    at: (f32, f32),
    horizontal: AxisScroll,
    vertical: AxisScroll,
) -> (bool, Option<Event>) {
    // wl_pointer's axis values are in surface coordinates: logical pixels.
    let taken = ui.scroll_under(at.0, at.1).is_some();
    let moved = ui.pointer_axis(at.0, at.1, vertical.absolute as f32);
    let steps = crate::widget::wheel::steps;
    let dx = steps(horizontal.value120, horizontal.discrete);
    let dy = if taken {
        0
    } else {
        steps(vertical.value120, vertical.discrete)
    };
    let event = ui.pointer_wheel(at.0, at.1, dx, dy);
    (moved || event.is_some(), event)
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);

/// Whether a key held down acts only at its press, not as it repeats: Enter outside a text
/// area and Space outside a field, which activate, Ctrl+C, Ctrl+X and Ctrl+V, which copy, cut
/// and paste once per press, the Menu key and F10, which open a context menu, and Escape, which
/// closes it.
fn once_per_press(keysym: Keysym, ctrl: bool, in_field: bool, in_area: bool) -> bool {
    match keysym {
        Keysym::Return | Keysym::KP_Enter => !in_area,
        Keysym::space => !in_field,
        Keysym::c | Keysym::C | Keysym::x | Keysym::X | Keysym::v | Keysym::V => ctrl,
        // The field's menu takes the keys once open: a repeat would reach it as a context-menu
        // event of the item in focus.
        Keysym::Menu | Keysym::F10 => true,
        // A held Escape closes the menu once: a repeat would reach the surface under it as
        // `Event::Escape`, which closes a dialog the user only meant to leave the menu of.
        Keysym::Escape => true,
        _ => false,
    }
}

/// Forgets the settling of a motion of `id` that a new one replaces, so that the application
/// is not told of it during the new one, and queues `id` once if the new motion `ended` at once.
fn requeue(settled: &mut Vec<SurfaceId>, id: SurfaceId, ended: bool) {
    settled.retain(|s| *s != id);
    if ended {
        settled.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use wayland_client::protocol::wl_registry;

    #[test]
    fn a_turn_no_scroll_takes_reaches_the_widget_as_whole_steps_and_each_axis_apart() {
        let mut ui = Ui::new("Tray", Theme::default(), false);
        let scroll = ui.scroll(Ui::ROOT, 40.0);
        for n in 0..20 {
            ui.add(scroll, Kind::Button(format!("Line {n}")));
        }
        let item = ui.add(Ui::ROOT, Kind::Button("Item".into()));
        ui.layout();
        let r = ui.widget(item).rect;
        let at = (r.x + 1.0, r.y + 1.0);
        let turn = |value120, absolute| AxisScroll {
            absolute,
            value120,
            ..AxisScroll::default()
        };
        let none = AxisScroll::default();
        // A high resolution wheel sends a quarter of a notch per event.
        for _ in 0..3 {
            assert_eq!(axis(&mut ui, at, none, turn(30, 3.75)), (false, None));
        }
        let step = Event::Wheel { widget: item, dx: 0, dy: 1 };
        assert_eq!(axis(&mut ui, at, none, turn(30, 3.75)), (true, Some(step)));
        // The deprecated discrete count, from an older compositor.
        let old = AxisScroll {
            discrete: -1,
            ..turn(0, -15.0)
        };
        let up = Event::Wheel { widget: item, dx: 0, dy: -1 };
        assert_eq!(axis(&mut ui, at, none, old), (true, Some(up)));
        // Over the scroll, the scroll takes the vertical axis, and the horizontal one is kept.
        let s = ui.widget(scroll).rect;
        let over = (s.x + 1.0, s.y + 1.0);
        let (moved, event) = axis(&mut ui, over, turn(120, 15.0), turn(120, 15.0));
        assert!(moved, "the scroll follows");
        assert!(
            matches!(event, Some(Event::Wheel { dx: 1, dy: 0, .. })),
            "the vertical turn the scroll took reaches no widget: {event:?}"
        );
    }

    /// A turn of `value120` 120ths of a notch, `absolute` logical pixels.
    fn turn(value120: i32, absolute: f64) -> AxisScroll {
        AxisScroll {
            absolute,
            value120,
            ..AxisScroll::default()
        }
    }

    #[test]
    fn a_scroll_at_its_end_takes_the_vertical_turn() {
        let none = AxisScroll::default();
        // At offset 0, a turn up over a button inside moves nothing and reaches no widget.
        let mut ui = Ui::new("Tray", Theme::default(), false);
        let scroll = ui.scroll(Ui::ROOT, 40.0);
        let first = ui.add(scroll, Kind::Button("Line 0".into()));
        for n in 1..20 {
            ui.add(scroll, Kind::Button(format!("Line {n}")));
        }
        ui.layout();
        let r = ui.widget(first).rect;
        assert_eq!(axis(&mut ui, (r.x + 1.0, r.y + 1.0), none, turn(-120, -15.0)), (false, None));
    }

    #[test]
    fn a_scroll_whose_content_fits_takes_the_vertical_turn() {
        let none = AxisScroll::default();
        let mut ui = Ui::new("Tray", Theme::default(), false);
        let scroll = ui.scroll(Ui::ROOT, 400.0);
        let only = ui.add(scroll, Kind::Button("Only".into()));
        ui.layout();
        let r = ui.widget(only).rect;
        let at = (r.x + 1.0, r.y + 1.0);
        assert_eq!(axis(&mut ui, at, none, turn(120, 15.0)), (false, None));
        assert_eq!(axis(&mut ui, at, none, turn(-120, -15.0)), (false, None));
    }

    #[test]
    fn a_window_that_is_not_resizable_takes_its_minimum_as_its_maximum() {
        let mut ui = Ui::new("Fixed", crate::Theme::default(), false);
        assert_eq!(max_size(&ui, (420, 300)), None, "a window is resizable by default");
        ui.resizable = false;
        assert_eq!(max_size(&ui, (420, 300)), Some((420, 300)));
    }

    #[test]
    fn a_drop_down_menu_is_bounded_by_its_output_else_the_shortest_one() {
        assert_eq!(menu_max_height(Some(1080.0), &[720.0, 1080.0], 40), Some(1080.0), "its own");
        assert_eq!(
            menu_max_height(None, &[1440.0, 720.0, 1080.0], 40),
            Some(720.0),
            "a window, or a layer surface the compositor placed"
        );
        assert_eq!(menu_max_height(Some(0.0), &[0.0, 900.0], 40), Some(900.0), "0 is unknown");
        assert_eq!(menu_max_height(None, &[], 40), Some(40.0), "no output: the surface");
        assert_eq!(menu_max_height(None, &[0.0], 0), None, "nothing known");
    }

    /// The path of a choice in a drop-down's menu, as the driver runs it: the item activated in
    /// the menu, its event routed, then the item's index chosen in the drop-down.
    #[test]
    fn an_item_activated_in_a_drop_down_menu_chooses_it_and_a_cancel_chooses_nothing() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let players = ui.drop_down(Ui::ROOT, "Media player", &["Firefox", "Spotify"], 0);
        let empty = ui.drop_down(Ui::ROOT, "Nothing", &[], 0);
        let opened = ui.activate(players).map(|event| route(&ui, event, false));
        assert_eq!(opened, Some(Route::DropDown(players)));
        let kept = ui.activate(empty).map(|event| route(&ui, event, false));
        assert_eq!(kept, Some(Route::Kept), "an empty drop-down opens nothing");
        let Some((mut menu, items)) = ui.drop_down_menu(players, None) else {
            unreachable!("a drop-down with items opens a menu");
        };
        let open = OpenPopup::opened(0, players, 1, &mut ui, &mut menu, true);
        let (spotify, index) = items[1];
        let routed = menu.activate(spotify).map(|event| route(&menu, event, true));
        assert_eq!(routed, Some(Route::Chosen(spotify)));
        open.closed(&mut ui);
        assert_eq!(ui.choose(players, index), Some(Event::Changed(players)));
        assert_eq!(ui.chosen(players), Some(1));
        // Escape, or a press outside, closes the menu and chooses nothing.
        let Some((mut menu, _)) = ui.drop_down_menu(players, None) else {
            unreachable!("a drop-down with items opens a menu");
        };
        let open = OpenPopup::opened(0, players, 1, &mut ui, &mut menu, true);
        assert!(open.dismissed_by(Dismiss::Escape) && open.dismissed_by(Dismiss::Press(0)));
        assert!(!open.dismissed_by(Dismiss::Press(1)), "a press in the menu");
        open.closed(&mut ui);
        assert_eq!(ui.chosen(players), Some(1));
        assert_eq!(route(&menu, Event::Escape, true), Route::Kept);
        assert_eq!(
            route(&ui, Event::Escape, false),
            Route::Application(Event::Escape)
        );
    }

    #[test]
    fn a_new_motion_forgets_the_settling_of_the_one_it_replaces() {
        let (a, b) = (SurfaceId(1), SurfaceId(2));
        // A motion of `a` ended this turn; a new one under way replaces it.
        let mut settled = vec![a, b];
        requeue(&mut settled, a, false);
        assert_eq!(settled, [b]);
        // Two motions that end at once: `a` settles once.
        requeue(&mut settled, a, true);
        requeue(&mut settled, a, true);
        assert_eq!(settled, [b, a]);
    }

    #[test]
    fn a_surface_moved_again_within_a_batch_is_reported_once_on_the_next_turn() {
        let (a, b) = (SurfaceId(1), SurfaceId(2));
        // Reporting `a` replaces `b`, settled in the same batch, by an instantaneous motion.
        let mut state = (Vec::new(), Vec::new());
        let turn = |state: &mut (Vec<SurfaceId>, Vec<SurfaceId>), batch| {
            settle_each(
                state,
                batch,
                |s| &s.0,
                |_, _| true,
                |s, id| {
                    s.1.push(id);
                    if id == a {
                        requeue(&mut s.0, b, true);
                    }
                },
            );
        };
        turn(&mut state, vec![a, b]);
        assert_eq!(state.1, [a], "b is not reported with the batch that moved it");
        let next = std::mem::take(&mut state.0);
        turn(&mut state, next);
        assert_eq!(state.1, [a, b], "b is reported once, on the next turn");
        assert!(state.0.is_empty());
    }

    #[test]
    fn the_loop_does_not_block_while_a_surface_is_queued() {
        assert_eq!(wait(true), Some(std::time::Duration::ZERO));
        assert_eq!(wait(false), None);
    }

    #[test]
    fn a_held_key_acts_again_but_the_clipboard_keys_and_the_activating_ones() {
        let once = |keysym, ctrl| once_per_press(keysym, ctrl, true, false);
        assert!(!once(Keysym::Right, false), "an arrow repeats");
        assert!(!once(Keysym::Right, true));
        assert!(!once(Keysym::c, false), "a letter repeats");
        assert!(!once(Keysym::a, true), "Ctrl+A selecting all again changes nothing");
        for keysym in [Keysym::c, Keysym::C, Keysym::x, Keysym::X, Keysym::v, Keysym::V] {
            assert!(once(keysym, true), "{keysym:?}");
        }
        // Enter breaks lines in a text area and activates once elsewhere; Space activates once
        // outside a field.
        assert!(once_per_press(Keysym::Return, false, true, false));
        assert!(!once_per_press(Keysym::Return, false, true, true));
        assert!(once_per_press(Keysym::space, false, false, false));
        assert!(!once_per_press(Keysym::space, false, true, false));
        // A held Menu key or Shift+F10 opens the field's menu once, and does not repeat into it.
        assert!(once(Keysym::Menu, false));
        assert!(once(Keysym::F10, false));
        // A held Escape closes the menu, and does not repeat into the surface's escape.
        assert!(once(Keysym::Escape, false));
        assert!(once_per_press(Keysym::Escape, false, false, false));
    }

    #[test]
    fn the_renderer_variable_falls_back_to_its_old_name() {
        let gpu = || Some(std::ffi::OsString::from("gpu"));
        let cpu = || Some(std::ffi::OsString::from("cpu"));
        assert_eq!(renderer_value(None, None), (None, None, RENDERER));
        let (value, warning, name) = renderer_value(None, gpu());
        assert_eq!((value, name), (gpu(), OLD_RENDERER));
        assert!(warning.is_some_and(|w| w.contains("set ATHANOR_SAL_RENDERER")));
        let (value, warning, name) = renderer_value(cpu(), gpu());
        assert_eq!((value, name), (cpu(), RENDERER));
        assert!(warning.is_some_and(|w| w.contains("ignored")));
        assert_eq!(renderer_value(gpu(), None), (gpu(), None, RENDERER));
        let (value, _, name) = renderer_value(None, Some("foo".into()));
        let error = wants_gpu(value, name).unwrap_err().to_string();
        assert!(error.starts_with("ATHANOR_TOOLKIT_RENDERER is"), "{error}");
    }

    #[test]
    fn the_renderer_variable_takes_cpu_or_gpu_only() {
        assert!(!wants_gpu(None, RENDERER).unwrap());
        assert!(!wants_gpu(Some("cpu".into()), RENDERER).unwrap());
        assert_eq!(
            wants_gpu(Some("gpu".into()), RENDERER).ok(),
            cfg!(feature = "gpu").then_some(true)
        );
        for refused in ["", "GPU", "vulkan"] {
            assert!(wants_gpu(Some(refused.into()), RENDERER).is_err(), "{refused:?}");
        }
        use std::os::unix::ffi::OsStringExt;
        let invalid = std::ffi::OsString::from_vec(vec![0x67, 0xff]);
        let error = wants_gpu(Some(invalid), RENDERER).unwrap_err().to_string();
        assert!(error.contains("is not valid UTF-8"), "{error}");
    }

    #[test]
    fn a_layer_surface_on_both_ends_of_an_axis_asks_the_compositor_for_its_length() {
        let bottom = Anchor::BOTTOM;
        let spanning = Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT;
        let side = Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM;
        assert_eq!(stretched(bottom), (false, false));
        assert_eq!(stretched(spanning), (true, false));
        assert_eq!(stretched(side), (false, true));
        assert_eq!(requested((300, 60), stretched(bottom)), (300, 60));
        assert_eq!(requested((300, 60), stretched(spanning)), (0, 60));
        assert_eq!(requested((60, 300), stretched(side)), (60, 0));
    }

    #[test]
    fn a_picture_on_all_four_edges_takes_the_size_configured() -> Result<(), Error> {
        let all = stretched(Anchor::all());
        assert_eq!(all, (true, true));
        let theme = Theme {
            margin: 0.0,
            ..Theme::default()
        };
        let mut ui = Ui::new("Wallpaper", theme, false);
        ui.picture(Ui::ROOT, crate::Image::from_rgba8(1, 1, vec![0, 0, 0, 255])?, "");
        // Before the first configure: 0 on both axes, which the compositor sizes.
        layout(&mut ui, 1.0)?;
        assert_eq!(requested(logical_size(&ui), all), (0, 0));
        // As `configure` sets it, at an output's size and again at another.
        for configured in [(1280.0, 800.0), (853.0, 533.0)] {
            (ui.min_width, ui.min_height) = configured;
            layout(&mut ui, 1.5)?;
            assert_eq!(ui.size(), configured);
            assert_eq!(requested(logical_size(&ui), all), (0, 0));
        }
        Ok(())
    }

    #[test]
    fn without_the_layer_shell_there_is_no_layer_surface() {
        let error = require_layer_shell(None).err().map(|e| e.to_string());
        assert_eq!(
            error.as_deref(),
            Some("not a layer surface: the compositor does not offer zwlr_layer_shell_v1")
        );
    }

    /// An output as the compositor announces it: its global name and its scale.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Output {
        name: u32,
        scale: f32,
    }

    /// The outputs and surfaces of a fake driver: what [`PerOutput`] opened and closed.
    #[derive(Default)]
    struct Fake {
        outputs: PerOutput<u32, u32>,
        /// Open surfaces: (surface, output, the scale it was opened or last changed at).
        open: Vec<(u32, u32, f32)>,
        next: u32,
    }

    impl Fake {
        fn seen(&mut self, output: Output) {
            let Fake {
                outputs,
                open,
                next,
            } = self;
            let existing = outputs
                .seen(output.name, |&name| {
                    *next += 1;
                    open.push((*next, name, output.scale));
                    Ok(Some(*next))
                })
                .unwrap();
            if let Some(surface) = existing {
                for s in open.iter_mut().filter(|s| s.0 == surface) {
                    s.2 = output.scale;
                }
            }
        }

        fn left(&mut self, name: u32) {
            if let Some(surface) = self.outputs.left(&name) {
                self.open.retain(|s| s.0 != surface);
            }
        }

        fn closed(&mut self, surface: u32) {
            self.outputs.closed(surface);
            self.open.retain(|s| s.0 != surface);
        }

        /// Exactly one surface on each of `outputs`, at its scale, and none elsewhere.
        fn one_each(&self, outputs: &[Output]) {
            for output in outputs {
                let on: Vec<_> = self.open.iter().filter(|s| s.1 == output.name).collect();
                assert_eq!(on.len(), 1, "output {}: {on:?}", output.name);
                assert_eq!(on[0].2, output.scale, "output {}", output.name);
            }
            assert_eq!(self.open.len(), outputs.len(), "{:?}", self.open);
        }
    }

    #[test]
    fn every_output_has_one_surface_as_outputs_come_go_and_change_scale() {
        let mut fake = Fake::default();
        let a = Output {
            name: 1,
            scale: 1.0,
        };
        let b = Output {
            name: 2,
            scale: 1.0,
        };
        fake.seen(a);
        fake.seen(b);
        fake.one_each(&[a, b]);
        // wl_output's done after the scale's change: the same surface, at 1.5.
        let first = fake.open[1].0;
        let b = Output { scale: 1.5, ..b };
        fake.seen(b);
        fake.one_each(&[a, b]);
        assert_eq!(
            fake.open[1].0, first,
            "a scale change opened a second surface"
        );
        // Removed, as cosmic-comp does when an output is disabled (#600).
        fake.left(b.name);
        fake.one_each(&[a]);
        // Added again: a new global, and a new surface.
        let b = Output { name: 3, ..b };
        fake.seen(b);
        fake.one_each(&[a, b]);
        fake.seen(b);
        fake.one_each(&[a, b]);
        // An output that leaves twice, or one never seen, closes nothing more.
        fake.left(2);
        fake.left(9);
        fake.one_each(&[a, b]);
        // The compositor closes a surface: its output gets none at its next change, which would
        // be refused again, until it leaves and comes back.
        let closed = fake.open[0].0;
        fake.closed(closed);
        fake.one_each(&[b]);
        fake.seen(a);
        fake.one_each(&[b]);
        fake.left(a.name);
        fake.seen(a);
        fake.one_each(&[a, b]);
    }

    #[test]
    fn an_output_whose_surface_closed_is_not_asked_again_until_it_leaves() {
        let mut outputs = PerOutput::<u32, u32>::default();
        assert_eq!(outputs.seen(1, |_| Ok(Some(7))).ok(), Some(Some(7)));
        outputs.closed(7);
        let asked = std::cell::Cell::new(false);
        let again = outputs.seen(1, |_| {
            asked.set(true);
            Ok(Some(8))
        });
        assert_eq!(again.ok(), Some(None));
        assert!(!asked.get(), "the application was asked again for a closed surface");
        // Leaving, the output has no surface to close; back, it is asked again.
        assert_eq!(outputs.left(&1), None);
        assert_eq!(outputs.seen(1, |_| Ok(Some(9))).ok(), Some(Some(9)));
        assert_eq!(outputs.left(&1), Some(9));
    }

    #[test]
    fn a_surface_the_application_closes_while_its_output_leaves_is_not_reported_lost() {
        // Surfaces 1 and 2 on the leaving output 9, 3 on another; told of 1, the application
        // closes 2 itself.
        let mut state = (vec![(1, 9), (2, 9), (3, 8)], Vec::new());
        lose_each(
            &mut state,
            |(open, _)| open.iter().find(|(_, o)| *o == 9).map(|(s, _)| *s),
            |(open, told), surface| {
                open.retain(|(s, _)| *s != surface);
                told.push(surface);
                if surface == 1 {
                    open.retain(|(s, _)| *s != 2);
                }
            },
        );
        assert_eq!(state.1, [1], "a surface closed at the application's asking was lost");
        assert_eq!(state.0, [(3, 8)]);
    }

    #[test]
    fn a_surface_closed_before_its_turn_is_not_reported_drawn() {
        // Surfaces 1, 2 and 3 drew their first frame; 2 closed since, and drawing 1 closes 3.
        let mut state = (vec![1, 3], Vec::new());
        each_open(
            &mut state,
            vec![1, 2, 3],
            |(open, _), surface| open.contains(&surface),
            |(open, told), surface| {
                told.push(surface);
                if surface == 1 {
                    open.retain(|s| *s != 3);
                }
            },
        );
        assert_eq!(state.1, [1], "a closed surface was reported drawn");
    }

    #[test]
    fn a_refused_layout_keeps_the_minimum_size_and_the_geometry_it_had() {
        let mut ui = Ui::new("t", Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Button("OK".into()));
        assert!(layout_at_min(&mut ui, 1.0, (300.0, 0.0)).is_ok());
        assert!(layout_at_min(&mut ui, 1.0, (0.0, 0.0)).is_ok());
        let size = ui.size();
        assert!(size.0 < 300.0, "{size:?}");
        // A width no buffer holds: wl_shm sizes are 16-bit.
        assert!(layout_at_min(&mut ui, 1.0, (100_000.0, 0.0)).is_err());
        assert_eq!((ui.min_width, ui.min_height), (0.0, 0.0));
        assert_eq!(ui.size(), size, "the refused layout's geometry stayed");
    }

    #[test]
    fn a_layer_surface_is_opened_on_the_output_of_its_name() {
        let outputs = [(1, Some("DP-1")), (2, None), (3, Some("HDMI-A-1"))];
        let named = |name| named(outputs.iter().map(|(o, n)| (o, *n)), name).copied();
        assert_eq!(named("HDMI-A-1"), Some(3));
        assert_eq!(named("DP-1"), Some(1));
        assert_eq!(named("DP-2"), None);
        assert_eq!(named(""), None, "an output without a name was chosen");
    }

    #[test]
    fn set_layer_moves_a_layer_surface_to_another_layer_and_keeps_its_namespace() {
        let placed = (Layer::Background, "athanor-osd".to_owned());
        let mut options = LayerOptions {
            namespace: "athanor-osd".into(),
            ..LayerOptions::new(Layer::Background)
        };
        assert_eq!(layer_change(&placed, &options, 4).ok(), Some(None));
        options.layer = Layer::Overlay;
        assert_eq!(layer_change(&placed, &options, 2).ok(), Some(Some(Layer::Overlay)));
        assert_eq!(layer_change(&placed, &options, 4).ok(), Some(Some(Layer::Overlay)));
        // Version 1 has no set_layer: refused, not dropped.
        let error = layer_change(&placed, &options, 1).err().map(|e| e.to_string());
        assert!(error.is_some_and(|e| e.contains("version 1")));
        // Version 1 keeps a surface that stays on its layer.
        options.layer = Layer::Background;
        assert_eq!(layer_change(&placed, &options, 1).ok(), Some(None));
        options.namespace = "athanor-other".into();
        let error = layer_change(&placed, &options, 4).err().map(|e| e.to_string());
        assert!(error.is_some_and(|e| e.contains("\"athanor-osd\"")));
    }

    #[test]
    fn an_output_the_application_leaves_bare_is_asked_again_at_its_change() {
        let mut outputs = PerOutput::<u32, u32>::default();
        assert_eq!(outputs.seen(1, |_| Ok(None)).ok(), Some(None));
        // The surface opened is returned, so the application learns which one it is.
        assert_eq!(outputs.seen(1, |_| Ok(Some(7))).ok(), Some(Some(7)));
        assert_eq!(outputs.seen(1, |_| Ok(Some(8))).ok(), Some(Some(7)));
        // A surface that fails to open leaves the output bare.
        assert!(outputs.seen(2, |_| Err("refused".into())).is_err());
        assert_eq!(outputs.left(&2), None);
    }

    #[test]
    fn a_done_answers_the_latest_state_only_with_the_count_of_commits() {
        let mut ime = Ime {
            commits: 2,
            ..Ime::default()
        };
        ime.pending.commit = Some("a".into());
        let (pending, current) = ime.done(1);
        assert_eq!((pending.commit.as_deref(), current), (Some("a"), false));
        assert!(
            ime.pending.commit.is_none(),
            "a stale done still applies its text"
        );
        assert!(ime.done(2).1);
    }

    #[test]
    fn a_field_on_a_new_surface_is_enabled_after_the_old_one_closes() {
        let (a, b) = (SurfaceId(1), SurfaceId(2));
        let mut ime = Ime {
            entered: Some(a),
            ..Ime::default()
        };
        assert!(ime.change(Some((a, WidgetId(1, 0), ContentHint::None))));
        // `a` closes without a `leave`: App::close ends its text input.
        ime.entered = None;
        assert!(ime.change(None));
        ime.entered = Some(b);
        let field = Some((b, WidgetId(1, 0), ContentHint::None));
        assert!(ime.change(field), "no enable for the new field");
        assert_eq!(ime.commits, 3);
        assert!(!ime.change(field));
    }

    #[test]
    fn every_change_of_the_focused_field_enables_the_text_input_again() {
        let a = SurfaceId(1);
        let mut ime = Ime::default();
        assert!(ime.change(Some((a, WidgetId(1, 0), ContentHint::None))));
        // Another field of the same hint, on the same surface.
        assert!(ime.change(Some((a, WidgetId(2, 0), ContentHint::None))));
        assert!(!ime.change(Some((a, WidgetId(2, 0), ContentHint::None))));
    }

    #[test]
    fn a_field_focused_in_a_removed_fields_slot_enables_the_text_input_again() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let column = ui.add(Ui::ROOT, Kind::Column);
        let old = ui.field(column, "Old", "");
        ui.layout();
        ui.set_focus(Some(old));
        let a = SurfaceId(1);
        let field = |ui: &Ui| ime_field(ui).map(|(id, _, hint)| (a, id, hint));
        let mut ime = Ime::default();
        assert!(ime.change(field(&ui)));
        ui.remove_children(column);
        let new = ui.field(column, "New", "");
        ui.layout();
        ui.set_focus(Some(new));
        assert_eq!(new.index(), old.index(), "the slot is reused");
        assert!(ime.change(field(&ui)), "the removed field's cursor rectangle stays");
        assert!(!ime.change(field(&ui)));
    }

    #[test]
    fn a_leave_drops_what_no_done_applied() {
        let a = SurfaceId(1);
        let mut ime = Ime {
            entered: Some(a),
            ..Ime::default()
        };
        ime.pending.preedit = Some(("ni".into(), None));
        assert_eq!(ime.leave(), Some(a));
        assert_eq!(ime.entered, None);
        // The first done after the next enter applies nothing from before the leave.
        ime.entered = Some(a);
        assert!(ime.done(ime.commits).0.preedit.is_none());
    }

    #[test]
    fn a_leave_ends_the_focused_fields_composition() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let name = ui.field(Ui::ROOT, "Name", "ab");
        ui.layout();
        ui.set_focus(Some(name));
        ui.compose("ni", Some((2, 2)));
        assert!(ui.end_composition());
        let Kind::Field(field) = &ui.widget(name).kind else {
            panic!("not a field");
        };
        assert!(!field.is_composing());
        assert_eq!(field.shown(), "ab");
        assert!(!ui.end_composition(), "a second end changed the field");
    }

    #[test]
    fn a_text_area_asks_for_multiline_text() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let name = ui.field(Ui::ROOT, "Name", "");
        let notes = ui.text_area(Ui::ROOT, "Notes", "", 3);
        ui.layout();
        let hint = |ui: &Ui| ime_field(ui).map(|(id, _, hint)| (id, hint));
        ui.set_focus(Some(name));
        assert_eq!(hint(&ui), Some((name, ContentHint::None)));
        ui.set_focus(Some(notes));
        assert_eq!(hint(&ui), Some((notes, ContentHint::Multiline)));
    }

    #[test]
    fn the_preedit_cursor_follows_the_text_printable_keeps() {
        // "a\tbç": the tab goes, so the cursor after "b" (byte 3) moves to byte 2.
        assert_eq!(
            printable_preedit("a\tb\u{e7}", Some((3, 5))),
            ("ab\u{e7}".to_owned(), Some((2, 4)))
        );
        assert_eq!(printable_preedit("ab", None), ("ab".to_owned(), None));
        // Inside "ç", or past the end: no cursor.
        assert_eq!(printable_preedit("\u{e7}", Some((1, 1))).1, None);
        assert_eq!(printable_preedit("ab", Some((0, 3))).1, None);
    }

    /// A client that dispatches nothing: the registry it asks for gives the compositor an object
    /// to raise a protocol error on.
    struct Client;

    impl wayland_client::Dispatch<wl_registry::WlRegistry, ()> for Client {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    /// An event loop on the client end of a socket pair, as the driver runs one on the
    /// compositor's socket, its registry as object 2, and the server end, which has read the
    /// request for it.
    fn on_socket_pair(
    ) -> Result<(EventLoop<'static, Client>, Connection, wl_registry::WlRegistry, UnixStream), Error>
    {
        let (client, mut server) = UnixStream::pair()?;
        let conn = Connection::from_socket(client)?;
        let queue = conn.new_event_queue::<Client>();
        let registry = conn.display().get_registry(&queue.handle(), ());
        conn.flush()?;
        // Read, so that closing the server end ends the stream instead of resetting it.
        server.read_exact(&mut [0; 12])?;
        let event_loop = EventLoop::try_new()?;
        WaylandSource::new(conn.clone(), queue)
            .insert(event_loop.handle())
            .map_err(|e| e.error.to_string())?;
        Ok((event_loop, conn, registry, server))
    }

    /// The error of the first of a hundred turns of the loop that fails, if one does.
    fn first_error(event_loop: &mut EventLoop<'static, Client>, conn: &Connection) -> Option<String> {
        (0..100).find_map(|_| {
            let turn = dispatch(event_loop, conn, Some(Duration::from_millis(10)), &mut Client);
            turn.err().map(|error| error.to_string())
        })
    }

    #[test]
    fn the_loop_ends_with_the_protocol_error_that_ended_the_connection() -> Result<(), Error> {
        let (mut event_loop, conn, registry, mut server) = on_socket_pair()?;
        assert_eq!(registry.id().protocol_id(), 2);
        // wl_display.error(wl_registry 2, code 3, "invalid size"), then the compositor closes.
        // Not on wl_display itself: libwayland reports its codes as an errno, ENOMEM for 2.
        let text = b"invalid size\0\0\0\0";
        let mut message = Vec::new();
        for word in [1, (8 + 12 + text.len() as u32) << 16, 2, 3, 13] {
            message.extend_from_slice(&u32::to_ne_bytes(word));
        }
        message.extend_from_slice(text);
        server.write_all(&message)?;
        drop(server);
        let error = first_error(&mut event_loop, &conn);
        let ended = "the compositor ended the Wayland connection: \
                     Protocol error 3 on object wl_registry@2";
        // The Rust backend keeps the message; libwayland's does not. Which one runs is not Sal's
        // to choose: any crate built with it may turn on wayland-backend's client_system, as Sal's
        // `gpu` feature does, and Cargo unifies features across a build, so both forms are right.
        let message = format!("{ended}: invalid size");
        assert!(
            error.as_deref() == Some(&message) || error.as_deref() == Some(ended),
            "{error:?}"
        );
        Ok(())
    }

    #[test]
    fn the_loop_ends_when_the_compositor_closes_the_connection() -> Result<(), Error> {
        let (mut event_loop, conn, _registry, server) = on_socket_pair()?;
        drop(server);
        let error = first_error(&mut event_loop, &conn);
        assert!(
            error.as_deref().is_some_and(|e| e.starts_with("the compositor ended the Wayland")),
            "{error:?}"
        );
        Ok(())
    }
}
