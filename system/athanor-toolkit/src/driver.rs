//! The Wayland driver on smithay-client-toolkit: xdg-toplevel windows, wlr-layer-shell surfaces
//! and their xdg-popups (a menu, or a tooltip), several in one process, each drawn in shared memory by vello_cpu or through wgpu by
//! vello_gpu at the compositor's fractional scale, the keyboard and the pointer, the input method
//! through text-input-v3, and one AccessKit adapter on AT-SPI per surface.

use crate::a11y::tree_update;
#[cfg(feature = "gpu")]
use crate::gpu::Gpu;
use crate::popup::{focused_window, Dismiss, GrabSerial, OpenPopup, Tooltip, TOOLTIP_PADDING};
use crate::render::{layout, render, Error};
use crate::theme::Theme;
use crate::widget::{Event, Key, Kind, PointerButton, Rect, Ui, WidgetId, WindowButton};
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
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
            cursor_shape::CursorShapeManager, PointerEvent, PointerEventKind, PointerHandler,
            BTN_LEFT, BTN_RIGHT,
        },
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
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
use std::time::Instant;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

/// What a single window does with each event; it may change the widgets. See [`run_window`].
pub type Handler = Box<dyn FnMut(&mut Ui, Event)>;

/// The variable that picks the renderer: `cpu` (the default) or `gpu`, which only a build with
/// the `gpu` feature takes. Popups draw on the CPU either way.
pub const RENDERER: &str = "ATHANOR_TOOLKIT_RENDERER";

/// Whether [`RENDERER`]'s value asks for the GPU. Anything but `cpu`, `gpu` or unset is refused.
fn wants_gpu(value: Option<std::ffi::OsString>) -> Result<bool, Error> {
    match value.as_deref().map(|v| v.to_str()) {
        None | Some(Some("cpu")) => Ok(false),
        Some(Some("gpu")) if cfg!(feature = "gpu") => Ok(true),
        Some(Some("gpu")) => {
            Err(format!("{RENDERER} is gpu; this build has no gpu feature").into())
        }
        Some(Some(other)) => Err(format!("{RENDERER} is {other:?}; it takes cpu or gpu").into()),
        Some(None) => Err(format!("{RENDERER} is not valid UTF-8; it takes cpu or gpu").into()),
    }
}

/// A surface of the application, for as long as it is open. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(u64);

/// How a surface is shown: the same widgets, in a window or on a layer of the shell.
pub enum Role {
    /// An xdg-toplevel, which draws its own title bar.
    Window {
        /// The application id the compositor groups the window under, as the name of its
        /// `.desktop` file.
        app_id: String,
    },
    /// A wlr-layer-shell surface on the output the compositor picks.
    Layer(LayerOptions),
}

/// A `zwlr_layer_surface_v1`'s state. The surface takes the size of its widgets.
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

/// The side of its widget a popup opens on, as GTK's `GtkPositionType`; left and right are the
/// physical sides. The compositor flips the popup to the opposite side, and slides it along, to
/// keep it on the output, as GTK's popovers ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// output, closes it when the output leaves or the compositor closes it, and asks again for
    /// an output that comes back.
    fn output(&mut self, _output: &OutputInfo) -> Option<(Ui, LayerOptions)> {
        None
    }

    /// An output that has the application's `surface` changed: its scale, mode or position.
    /// The surface follows the scale by itself.
    fn output_changed(&mut self, _shell: &mut Shell, _surface: SurfaceId, _output: &OutputInfo) {}

    /// An event of the widgets of `surface`. The application may change the widgets of any
    /// surface through `shell`.
    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event);
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
    let gpu = wants_gpu(std::env::var_os(RENDERER))?;
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, App> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| e.error.to_string())?;
    #[cfg(not(feature = "gpu"))]
    debug_assert!(!gpu, "wants_gpu refuses gpu without the feature");

    // The adapters call their handlers on their own thread: actions travel to the event loop.
    let (actions, receiver) = channel::channel();
    event_loop
        .handle()
        .insert_source(receiver, |event, _, app: &mut App| {
            if let channel::Event::Msg((id, request)) = event {
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
        tooltip: Tooltip::default(),
        error: None,
    };
    for (ui, role) in app.application.start() {
        app.shell.open(ui, role, None)?;
    }
    while !app.shell.exit {
        event_loop.dispatch(None, &mut app)?;
        app.sync_window_focus();
        if let Some(error) = app.error.take() {
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
/// of #600). Generic, so that its tests feed it without a compositor.
struct PerOutput<O, S>(Vec<(O, S)>);

impl<O, S> Default for PerOutput<O, S> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<O: PartialEq, S: Copy + PartialEq> PerOutput<O, S> {
    /// An output announced or changed. When it has a surface, returns it, and opens none; when it
    /// has none, `open` may give it one.
    fn seen(
        &mut self,
        output: O,
        open: impl FnOnce(&O) -> Result<Option<S>, Error>,
    ) -> Result<Option<S>, Error> {
        if let Some((_, surface)) = self.0.iter().find(|(o, _)| *o == output) {
            return Ok(Some(*surface));
        }
        if let Some(surface) = open(&output)? {
            self.0.push((output, surface));
        }
        Ok(None)
    }

    /// An output left: its surface, for the driver to close.
    fn left(&mut self, output: &O) -> Option<S> {
        let at = self.0.iter().position(|(o, _)| o == output)?;
        Some(self.0.remove(at).1)
    }

    /// The compositor closed `surface`: its output has none until it changes or comes back.
    fn closed(&mut self, surface: S) {
        self.0.retain(|(_, s)| *s != surface);
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

    /// The widgets of `id`, which the driver draws again after the event.
    pub fn ui_mut(&mut self, id: SurfaceId) -> Option<&mut Ui> {
        let surface = self.get_mut(id)?;
        surface.dirty = true;
        Some(&mut surface.ui)
    }

    /// Sets the part of a layer surface that takes the pointer, in logical pixels; `None` is all
    /// of it. Takes effect with the next frame.
    pub fn set_input_region(&mut self, id: SurfaceId, rects: Option<&[Rect]>) -> Result<(), Error> {
        let region = match rects {
            Some(rects) => {
                let region = Region::new(&self.compositor)?;
                for r in rects {
                    // Whole pixels that cover the widget: the origin down, the far edge up.
                    let (x, y) = (r.x.floor(), r.y.floor());
                    let (w, h) = ((r.x + r.w).ceil() - x, (r.y + r.h).ceil() - y);
                    region.add(x as i32, y as i32, w as i32, h as i32);
                }
                Some(region)
            }
            None => None,
        };
        let surface = self
            .get_mut(id)
            .ok_or_else(|| format!("no open surface {id:?}"))?;
        surface
            .wl_surface()
            .set_input_region(region.as_ref().map(Region::wl_region));
        surface.dirty = true;
        Ok(())
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
        self.show_popup(parent, anchor, ui, side, true)
    }

    /// Closes the popup open, a menu or a tooltip, if any.
    pub fn close_popup(&mut self) {
        if let Some(open) = self.popup {
            self.close(open.surface);
        }
    }

    /// Opens a menu, or a tooltip, which takes neither the input nor the focus and keeps
    /// [`TOOLTIP_PADDING`] away from its widget.
    fn show_popup(
        &mut self,
        parent: SurfaceId,
        anchor: WidgetId,
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
        if anchor >= from.ui.len() {
            return Err(format!("no widget {anchor} on {parent:?}").into());
        }
        let r = from.ui.widget(anchor).rect;
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
        let along_x = ConstraintAdjustment::FlipY | ConstraintAdjustment::SlideX;
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
        match self.adopt(ui, Shown::Popup(popup), fractional, input) {
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
        let shown = match role {
            Role::Window { app_id } => {
                let xdg_shell = self
                    .xdg_shell
                    .as_ref()
                    .ok_or("the compositor does not offer xdg_wm_base")?;
                // The window draws its own title bar, as GTK does, so assistive technologies
                // find the title and the window's buttons in its tree; a compositor that insists
                // on drawing them says so in the first configure.
                let window = xdg_shell.create_window(wl, WindowDecorations::RequestClient, qh);
                window.set_title(ui.title.as_str());
                window.set_app_id(app_id);
                Shown::Window(window)
            }
            Role::Layer(options) => {
                let layer_shell = require_layer_shell(self.layer_shell.as_ref())?;
                let surface = layer_shell.create_layer_surface(
                    qh,
                    wl,
                    options.layer,
                    Some(options.namespace),
                    output,
                );
                surface.set_anchor(options.anchor);
                let [top, right, bottom, left] = options.margin;
                surface.set_margin(top, right, bottom, left);
                surface.set_exclusive_zone(options.exclusive_zone);
                surface.set_keyboard_interactivity(options.keyboard);
                input_region = options.input_region;
                Shown::Layer(surface)
            }
        };
        self.adopt(ui, shown, fractional, input_region)
    }

    /// Takes a surface just given its role: its renderer, its accessibility adapter, its size and
    /// input region, then its first commit.
    fn adopt(
        &mut self,
        mut ui: Ui,
        shown: Shown,
        fractional: Option<(WpFractionalScaleV1, WpViewport)>,
        input_region: Option<Vec<Rect>>,
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
            eprintln!("athanor-toolkit: vello_gpu on {}", gpu.adapter);
            Some(gpu)
        } else {
            None
        };
        // The first tree comes from the last update drawn.
        let initial = Arc::new(Mutex::new(tree_update(&ui, 1.0)));
        let a11y = accesskit_unix::Adapter::new(
            Activation(Arc::clone(&initial)),
            Actions(id, self.actions.clone()),
            Deactivation,
        );
        // A layer surface asks for its widgets' size before the first configure.
        let size = match &shown {
            Shown::Layer(surface) => {
                layout(&mut ui, 1.0)?;
                let size = logical_size(&ui);
                surface.set_size(size.0, size.1);
                size
            }
            Shown::Popup(_) => logical_size(&ui),
            Shown::Window(_) => (0, 0),
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
            scale: 1.0,
            size,
            configured: false,
            frame_pending: false,
            redraw: false,
            dirty: false,
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
    fn close(&mut self, id: SurfaceId) {
        if let Some(open) = self.popup.filter(|p| p.surface == id || p.parent == id) {
            self.popup = None;
            self.surfaces.retain(|s| s.id != open.surface);
            if let Some(from) = self.get_mut(open.parent) {
                open.closed(&mut from.ui);
                from.dirty = true;
            }
        }
        self.surfaces.retain(|s| s.id != id);
    }

    /// Draws the surfaces whose widgets changed.
    fn draw_dirty(&mut self) -> Result<(), Error> {
        for surface in &mut self.surfaces {
            if surface.dirty {
                surface.draw(&mut self.pool, &self.qh)?;
            }
        }
        Ok(())
    }
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
    a11y: accesskit_unix::Adapter,
    initial: Arc<Mutex<TreeUpdate>>,
    /// Physical pixels per logical pixel: the preferred fractional scale, else wl_surface's
    /// integer one.
    scale: f32,
    /// The logical size last asked of a layer surface.
    size: (u32, u32),
    configured: bool,
    /// A frame is committed and its callback has not come: the next one waits for it, so that
    /// drawing never runs ahead of the compositor, which throttles a covered surface.
    frame_pending: bool,
    /// The state changed while a frame was pending: the callback draws it.
    redraw: bool,
    /// The widgets changed since the last draw.
    dirty: bool,
}

impl Drop for Surface {
    fn drop(&mut self) {
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
        self.publish_tree();
        Ok(())
    }

    /// Sets, for the next commit, how the buffer drawn at `scale` maps onto the surface, and a
    /// layer surface's size when its widgets' changed.
    fn place(&mut self) {
        let size = logical_size(&self.ui);
        if let Shown::Layer(surface) = &self.shown {
            if size != self.size {
                surface.set_size(size.0, size.1);
                self.size = size;
            }
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

    /// Hands the tree of the surface just drawn to the accessibility adapter.
    fn publish_tree(&mut self) {
        let update = tree_update(&self.ui, f64::from(self.scale));
        if let Ok(mut initial) = self.initial.lock() {
            initial.clone_from(&update);
        }
        self.a11y.update_if_active(|| update);
    }
}

struct Activation(Arc<Mutex<TreeUpdate>>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().ok().map(|tree| tree.clone())
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
    /// We sent `enable` and no `disable` since.
    enabled: bool,
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

    /// Whether a focused field wants `enable` (`Some(true)`) or `disable` (`Some(false)`), or
    /// nothing changes; counts the `commit` that sends it.
    fn change(&mut self, field: bool) -> Option<bool> {
        if field == self.enabled {
            return None;
        }
        self.enabled = field;
        self.commits = self.commits.wrapping_add(1);
        Some(field)
    }
}

/// The events of text-input-v3 that take effect together at its next `done`.
#[derive(Default)]
struct Pending {
    /// The preedit and its cursor, a byte range in it.
    preedit: Option<(String, Option<(usize, usize)>)>,
    commit: Option<String>,
}

/// Text from the input method without control characters: the field holds one line.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// The preedit as [`printable`] keeps it, with its cursor's byte offsets moved to match. A
/// cursor that is not on a character boundary of the preedit is dropped.
fn printable_preedit(
    text: &str,
    cursor: Option<(usize, usize)>,
) -> (String, Option<(usize, usize)>) {
    let kept = |at: usize| {
        text.is_char_boundary(at).then(|| {
            let dropped: usize = text[..at]
                .chars()
                .filter(|c| c.is_control())
                .map(char::len_utf8)
                .sum();
            at - dropped
        })
    };
    let cursor = cursor.and_then(|(begin, end)| kept(begin).zip(kept(end)));
    (printable(text), cursor)
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
            Event::Leave { .. } => {
                app.ime.entered = None;
                app.sync_text_input();
            }
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
    /// accessibility trees and the input method.
    fn changed(&mut self, id: SurfaceId, event: Option<Event>) {
        self.update(id, event);
        if !self.shell.exit {
            self.sync_text_input();
        }
    }

    /// Hands an event of `id`'s widgets to the application, then redraws what changed and
    /// updates the accessibility trees.
    fn update(&mut self, id: SurfaceId, event: Option<Event>) {
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
            Some(Event::Window(WindowButton::Close)) => {
                self.shell.exit = true;
                return;
            }
            Some(event) => self.application.event(&mut self.shell, id, event),
            None => {}
        }
        if let Err(error) = self.shell.draw_dirty() {
            self.fail(error);
        }
    }

    /// An output announced or changed: the application's surface on it follows, or one opens.
    fn output_seen(&mut self, output: wl_output::WlOutput) {
        let Some(info) = self.output.info(&output) else {
            return;
        };
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
                if let Err(error) = self.shell.draw_dirty() {
                    self.fail(error);
                }
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
            .and_then(|id| self.shell.ui(id))
            .and_then(|ui| {
                let id = ui.focus().filter(|&id| ui.field_text(id).is_some())?;
                Some(ui.widget(id).rect)
            });
        if self.ime.change(field.is_some()).is_none() {
            return;
        }
        if let Some(r) = field {
            text_input.enable();
            text_input.set_content_type(ContentHint::None, ContentPurpose::Normal);
            text_input.set_cursor_rectangle(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
        } else {
            text_input.disable();
        }
        text_input.commit();
    }

    /// Closes a surface. Text input on it ends as at a `leave`, which the compositor may not
    /// send for a surface that is gone: the next `enter` then sends `enable` again.
    fn close(&mut self, id: SurfaceId) {
        self.shell.close(id);
        if self.ime.entered == Some(id) {
            self.ime.entered = None;
            self.sync_text_input();
        }
    }

    /// Moves the accessibility adapters' window focus to the surface that takes the keys, after
    /// every event: the open menu's while it holds them, though the compositor may keep the
    /// keyboard on its parent, then back to the parent once it closes. An adapter emits its
    /// focus events only while its window is focused.
    fn sync_window_focus(&mut self) {
        let focused = focused_window(self.shell.popup, self.keyboard_focus);
        if focused == self.window_focus {
            return;
        }
        for (id, is_focused) in [(self.window_focus, false), (focused, true)] {
            if let Some(surface) = id.and_then(|id| self.shell.get_mut(id)) {
                surface.a11y.update_window_focus_state(is_focused);
            }
        }
        self.window_focus = focused;
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
    /// tooltip shown hides, and the target's shows after GTK's delay.
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
        let text = (widget < ui.len() && ui.is_visible(widget))
            .then(|| ui.tooltip(widget))
            .flatten();
        let Some(text) = text else {
            return;
        };
        let tip = tooltip_ui(ui, text);
        // A tooltip that cannot be shown leaves the application running, and says why.
        if let Err(error) = self.shell.show_popup(id, widget, tip, Side::Bottom, false) {
            eprintln!("athanor-toolkit: no tooltip: {error}");
        }
    }

    /// The pointer moved on `id`: the hovered widget follows it, a drag may start, and the
    /// tooltip follows the hovered widget.
    fn pointer_moved(&mut self, id: SurfaceId, x: f32, y: f32) {
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        let before = surface.ui.hovered();
        let event = surface.ui.pointer_motion(x, y);
        let hovered = surface.ui.hovered();
        let target = hovered.filter(|&w| surface.ui.tooltip(w).is_some());
        if event.is_some() || hovered != before {
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
        self.changed(id, event);
    }

    /// Applies what the input method sent since the last `done`, in the protocol's order: the
    /// committed text replaces the preedit, then the new preedit is shown. A `done` that
    /// answers an older state sends no new state; the next one that answers the latest does.
    fn text_input_done(&mut self, serial: u32) {
        let (Pending { preedit, commit }, current) = self.ime.done(serial);
        let Some(id) = self.ime.entered else {
            return;
        };
        let Some(surface) = self.shell.get_mut(id) else {
            return;
        };
        let event = commit.and_then(|text| surface.ui.commit(&printable(&text)));
        let (preedit, cursor) = preedit.unwrap_or_default();
        let (preedit, cursor) = printable_preedit(&preedit, cursor);
        surface.ui.compose(&preedit, cursor);
        if current {
            self.changed(id, event);
        } else {
            self.update(id, event);
        }
    }

    /// A key held down: as pressed again, except Enter, and Space outside a field, which
    /// activate once.
    fn repeat(&mut self, event: KeyEvent) {
        let in_field = self
            .keyboard_focus
            .and_then(|id| self.shell.ui(id))
            .is_some_and(|ui| ui.focus().is_some_and(|id| ui.field_text(id).is_some()));
        let activates = match event.keysym {
            Keysym::Return | Keysym::KP_Enter => true,
            Keysym::space => !in_field,
            _ => false,
        };
        if !activates {
            self.key(event);
        }
    }

    fn key(&mut self, event: KeyEvent) {
        let key = match event.keysym {
            Keysym::Tab if self.modifiers.shift => Key::BackTab,
            Keysym::Tab => Key::Tab,
            Keysym::ISO_Left_Tab => Key::BackTab,
            Keysym::Left => Key::Left,
            Keysym::Right => Key::Right,
            Keysym::Up => Key::Up,
            Keysym::Down => Key::Down,
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
        // sends them to.
        let target = match self.shell.popup {
            Some(open) if open.menu => Some(open.surface),
            _ => self.keyboard_focus,
        };
        let Some(id) = target else {
            return;
        };
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

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, wl: &wl_surface::WlSurface, _: u32) {
        let Some(surface) = self.shell.find(wl) else {
            return;
        };
        surface.frame_pending = false;
        if surface.redraw {
            let id = surface.id;
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
        if let Some(id) = self.outputs.left(&output) {
            self.close(id);
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        let Some(surface) = self.shell.find(layer.wl_surface()) else {
            return;
        };
        let id = surface.id;
        self.outputs.closed(id);
        self.close(id);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(surface) = self.shell.find(layer.wl_surface()) else {
            return;
        };
        // ponytail: the surface keeps its widgets' size whatever the compositor suggests;
        // stretching to a size configured along both anchors of an edge comes with the dock.
        surface.configured = true;
        let id = surface.id;
        self.changed(id, None);
    }
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.shell.exit = true;
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
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.keyboard_focus = None;
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
            if let Some(surface) = self.shell.find(&event.surface) {
                let id = surface.id;
                let at = (event.position.0 as f32, event.position.1 as f32);
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        self.pointer_moved(id, at.0, at.1);
                    }
                    PointerEventKind::Leave { .. } => {
                        surface.ui.pointer_leave();
                        self.changed(id, None);
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
                    PointerEventKind::Axis { .. } => self.hide_tooltip(),
                    _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_renderer_variable_takes_cpu_or_gpu_only() {
        assert!(!wants_gpu(None).unwrap());
        assert!(!wants_gpu(Some("cpu".into())).unwrap());
        assert_eq!(
            wants_gpu(Some("gpu".into())).ok(),
            cfg!(feature = "gpu").then_some(true)
        );
        for refused in ["", "GPU", "vulkan"] {
            assert!(wants_gpu(Some(refused.into())).is_err(), "{refused:?}");
        }
        use std::os::unix::ffi::OsStringExt;
        let invalid = std::ffi::OsString::from_vec(vec![0x67, 0xff]);
        let error = wants_gpu(Some(invalid)).unwrap_err().to_string();
        assert!(error.contains("is not valid UTF-8"), "{error}");
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
        // The compositor closes a surface: its output gets one again at its next change.
        let closed = fake.open[0].0;
        fake.closed(closed);
        fake.one_each(&[b]);
        fake.seen(a);
        fake.one_each(&[a, b]);
    }

    #[test]
    fn an_output_the_application_leaves_bare_is_asked_again_at_its_change() {
        let mut outputs = PerOutput::<u32, u32>::default();
        assert_eq!(outputs.seen(1, |_| Ok(None)).ok(), Some(None));
        assert_eq!(outputs.seen(1, |_| Ok(Some(7))).ok(), Some(None));
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
        assert_eq!(ime.change(true), Some(true));
        // `a` closes without a `leave`: App::close ends its text input.
        ime.entered = None;
        assert_eq!(ime.change(false), Some(false));
        ime.entered = Some(b);
        assert_eq!(ime.change(true), Some(true), "no enable for the new field");
        assert_eq!(ime.commits, 3);
        assert_eq!(ime.change(true), None);
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
}
