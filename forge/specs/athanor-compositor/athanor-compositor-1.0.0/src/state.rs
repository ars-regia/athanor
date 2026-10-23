//! The compositor's state: the protocol globals, the space of windows and its outputs, the seat
//! and the backend that draws them.

use crate::handlers::HeldCommit;
use crate::headless::{FrameStats, Headless};
use crate::identity::{AllowList, Identity, Privilege, Proc, Process};
use crate::input_method::PlacedPopup;
use crate::lock::Lock;
use crate::output_management::OutputManagement;
use crate::udev::Drm;
use crate::winit::Nested;
use crate::workspace::Group;
use crate::Error;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::input::InputTime;
use smithay::backend::renderer::damage::{
    Error as DamageError, OutputDamageTracker, RenderOutputResult,
};
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{
    default_primary_scanout_output_compare, Kind, RenderElementStates,
};
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::{ImportAll, ImportDma, Renderer, Texture};
use smithay::desktop::space::render_output;
use smithay::desktop::utils::{
    send_frames_surface_tree, surface_presentation_feedback_flags_from_states,
    surface_primary_scanout_output, update_surface_primary_scanout_output,
    with_surfaces_surface_tree, OutputPresentationFeedback,
};
use smithay::desktop::{
    layer_map_for_output, LayerSurface, PopupManager, Space, Window, WindowSurfaceType,
};
use smithay::input::keyboard::Keycode;
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::channel::{self, Channel};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, LoopHandle, LoopSignal, PostAction};
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, GlobalId,
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle};
use smithay::utils::{
    Clock, Logical, Monotonic, Physical, Point, Rectangle, Serial, Size, Time, Transform,
};
use smithay::wayland::compositor::{
    send_surface_state, CompositorClientState, CompositorState, SurfaceAttributes, SurfaceData,
};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::drm_syncobj::DrmSyncobjState;
use smithay::wayland::foreign_toplevel_list::ForeignToplevelListState;
use smithay::wayland::fractional_scale::{with_fractional_scale, FractionalScaleManagerState};
use smithay::wayland::idle_notify::IdleNotifierState;
use smithay::wayland::image_capture_source::{
    OutputCaptureSourceState, ToplevelCaptureSourceState,
};
use smithay::wayland::image_copy_capture::{Frame, ImageCopyCaptureState, Session, SessionRef};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::presentation::{PresentationState, Refresh};
use smithay::wayland::security_context::{SecurityContext, SecurityContextState};
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::session_lock::SessionLockManagerState;
use smithay::wayland::shell::wlr_layer::{Layer, WlrLayerShellState};
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::virtual_keyboard::{VirtualKeyboardDevice, VirtualKeyboardManagerState};
use smithay::wayland::virtual_pointer::{VirtualPointerDevice, VirtualPointerManagerState};
use smithay::wayland::xdg_activation::XdgActivationState;
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// What the outputs show where no surface is: Calmo's `bg1` in the dark variant, #101323, the
/// colour the desktop's hearth wallpaper starts from (system/athanor-style/calmo/tokens.toml).
// ponytail: dark always; the session's light or dark theme when the compositor reads appearance.
/// A surface on no output, or shown on another one, still gets a frame callback this often.
pub const FRAME_THROTTLE: Duration = Duration::from_secs(1);

pub const BACKGROUND: [f32; 4] = [16.0 / 255.0, 19.0 / 255.0, 35.0 / 255.0, 1.0];

/// The keyboard's repeat: the delay before a held key repeats, in milliseconds, and the
/// repeats per second.
pub const REPEAT_DELAY: i32 = 200;
pub const REPEAT_RATE: i32 = 25;

/// What draws the outputs.
pub enum Backend {
    /// A window of a parent compositor.
    Nested(Box<Nested>),
    /// Virtual outputs drawn off screen.
    Headless(Box<Headless>),
    /// The monitors of a GPU, through DRM and KMS, in a session of the seat (C1 task 8).
    Drm(Box<Drm>),
}

pub struct State {
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, State>,
    pub loop_signal: LoopSignal,
    pub clock: Clock<Monotonic>,

    pub space: Space<Window>,
    pub popups: PopupManager,
    pub seat: Seat<Self>,
    /// The serial of the newest key or button event sent to a client.
    pub last_input_serial: Option<Serial>,
    /// The keys whose press the compositor kept for itself: their release is kept as well.
    pub kept_keys: Vec<Keycode>,
    /// The modifiers each virtual keyboard last set, applied to its own keys only
    /// (`with_keymap_of`).
    pub virtual_modifiers: HashMap<VirtualKeyboardDevice, [u32; 4]>,
    /// The virtual keyboard whose keymap the seat has, the keyboard that sent the last key;
    /// none while the seat has its own (`with_keymap_of`).
    pub virtual_keymap: Option<crate::input::VirtualKeymap>,
    /// The text inputs and the input method (`input_method.rs`).
    pub composing: crate::input_method::Composing,
    /// The buttons each virtual pointer holds, whose release goes on while the session is
    /// locked; a pointer's entry goes with it. Only evdev's button codes are counted, so an entry
    /// holds at most 512.
    pub virtual_buttons: HashMap<VirtualPointerDevice, Vec<u32>>,
    /// The identities (executable and unit, as the grants go) whose exclusive layer surface
    /// held the keyboard when the user pressed Super+Escape: their exclusive layer surfaces,
    /// on any connection, hold it again only after a click on one of their surfaces
    /// (`escape_exclusive`).
    pub escaped: Vec<Identity>,
    /// The locks the user asked for with Super+Escape (`request_lock`).
    pub lock_requests: u64,
    /// Why the compositor must end with an error: set by `fail`, read by `main` once the loop
    /// has stopped.
    pub fatal: Option<String>,
    pub grab_serials: GrabSerials,
    pub backend: Backend,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub dmabuf_state: DmabufState,
    pub xdg_activation_state: XdgActivationState,
    pub data_device_state: DataDeviceState,
    /// linux-drm-syncobj-v1, offered only on a render node that can wait on a syncobj.
    pub syncobj_state: Option<DrmSyncobjState>,
    /// The clients' frame times and the outputs' draw times.
    pub frame_stats: Option<FrameStats>,
    /// The commits waiting on a fence, each with the event source that waits on it.
    pub held: Vec<HeldCommit>,
    /// The sequence number of the next held commit.
    pub next_held: u64,
    /// Given to each client's `ClientState`, which sends its id on disconnection.
    pub client_gone: channel::Sender<ClientId>,

    pub foreign_toplevel_state: ForeignToplevelListState,
    pub toplevel_capture_state: ToplevelCaptureSourceState,
    pub output_capture_state: OutputCaptureSourceState,
    pub image_copy_capture_state: ImageCopyCaptureState,
    /// The capture sessions in progress; dropping one stops it.
    pub capture_sessions: Vec<Session>,
    /// The frames asked for, answered at the next frame of the outputs (`serve_captures`).
    pub captures: Vec<(SessionRef, Frame)>,
    pub lock_state: SessionLockManagerState,
    /// The session's lock, while it is locked.
    pub lock: Option<Lock>,
    pub idle_notifier_state: IdleNotifierState<Self>,
    /// The workspace groups sent to the clients of ext-workspace-v1.
    pub workspace_groups: Vec<Group>,
    /// The heads sent to the clients of wlr-output-management, and their configurations.
    pub output_management: OutputManagement,

    /// Which client is granted which privileged global (OS4), read when the compositor starts.
    pub allow_list: AllowList,
    /// Where a connecting client's executable and cgroup are read.
    pub process: Box<dyn Process>,
}

/// The serials a popup grab may name, as mutter takes them, each with the client it was sent
/// to: the last button press, sent to the client under the pointer, and the last key press and
/// release, sent to the focused client. Serials come from one counter and can be guessed; the
/// client is what keeps one client from naming another's.
#[derive(Debug, Default)]
pub struct GrabSerials {
    pub button: Option<(Serial, ClientId)>,
    pub key_press: Option<(Serial, ClientId)>,
    pub key_release: Option<(Serial, ClientId)>,
}

impl GrabSerials {
    /// Whether `serial` is the last button press, and was sent to `client`.
    pub fn clicked(&self, serial: Serial, client: &ClientId) -> bool {
        sent(self.button.as_ref(), serial, client)
    }

    /// Whether `serial` is the last key press or release, and was sent to `client`.
    pub fn typed(&self, serial: Serial, client: &ClientId) -> bool {
        sent(self.key_press.as_ref(), serial, client)
            || sent(self.key_release.as_ref(), serial, client)
    }
}

fn sent(event: Option<&(Serial, ClientId)>, serial: Serial, client: &ClientId) -> bool {
    event.is_some_and(|(sent, to)| *sent == serial && to == client)
}

impl State {
    /// The globals that do not depend on the backend, the seat and the dispatch of the clients'
    /// requests. The backend adds its outputs, linux-dmabuf and linux-drm-syncobj.
    pub fn new(
        event_loop: &mut EventLoop<'static, Self>,
        display: Display<Self>,
        backend: Backend,
    ) -> Result<Self, Error> {
        let dh = display.handle();
        let clock = Clock::new();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        // Every client may bind it; only a client granted `TopLayers` keeps a surface on the top
        // or overlay layer (`layer_commit`). The escape key takes the keyboard back from the
        // exclusive surface of a granted client (`escape_exclusive`).
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        // The globals below keep no state the compositor reads back.
        OutputManagerState::new_with_xdg_output::<Self>(&dh);
        ViewporterState::new::<Self>(&dh);
        FractionalScaleManagerState::new::<Self>(&dh);
        PresentationState::new::<Self>(&dh, clock.id() as u32);
        CursorShapeManagerState::new::<Self>(&dh);
        let xdg_activation_state = XdgActivationState::new::<Self>(&dh);
        // GDK will not open a display without wl_data_device_manager.
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        // The privileged globals of OS4, each to the clients the allow-list grants it.
        let foreign_toplevel_state = ForeignToplevelListState::new_with_filter::<Self>(&dh, |c| {
            granted(c, Privilege::ToplevelList)
        });
        let toplevel_capture_state =
            ToplevelCaptureSourceState::new_with_filter::<Self, _>(&dh, |c| {
                granted(c, Privilege::ToplevelCaptureSource)
            });
        let output_capture_state = OutputCaptureSourceState::new_with_filter::<Self, _>(&dh, |c| {
            granted(c, Privilege::OutputCaptureSource)
        });
        let image_copy_capture_state =
            ImageCopyCaptureState::new_with_filter::<Self, _>(&dh, |c| {
                granted(c, Privilege::ImageCopyCapture)
            });
        let lock_state =
            SessionLockManagerState::new::<Self, _>(&dh, |c| granted(c, Privilege::SessionLock));
        crate::workspace::global(&dh);
        crate::output_management::global(&dh);
        // Synthetic input, which takes the hardware's path (`input.rs`).
        VirtualKeyboardManagerState::new::<Self, _>(&dh, |c| {
            granted(c, Privilege::VirtualKeyboard)
        });
        VirtualPointerManagerState::new::<Self, _>(&dh, |c| granted(c, Privilege::VirtualPointer));
        // Composed text (OS9 item 3): the input method by grant, text input outside a sandbox.
        crate::input_method::globals(&dh);
        // A client inside a security context cannot make another (spike Q4), as Smithay asks.
        SecurityContextState::new::<Self, _>(&dh, unconfined);
        let idle_notifier_state = IdleNotifierState::new(&dh, event_loop.handle());

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        seat.add_keyboard(Default::default(), REPEAT_DELAY, REPEAT_RATE)?;
        seat.add_pointer();

        dispatch_clients(event_loop, display)?;
        let (client_gone, gone) = channel::channel();
        release_held_on_disconnection(event_loop, gone)?;

        Ok(Self {
            display_handle: dh,
            loop_handle: event_loop.handle(),
            loop_signal: event_loop.get_signal(),
            clock,
            space: Space::default(),
            popups: PopupManager::default(),
            seat,
            last_input_serial: None,
            kept_keys: Vec::new(),
            virtual_modifiers: HashMap::new(),
            virtual_keymap: None,
            composing: Default::default(),
            virtual_buttons: HashMap::new(),
            escaped: Vec::new(),
            lock_requests: 0,
            fatal: None,
            grab_serials: GrabSerials::default(),
            backend,
            compositor_state,
            xdg_shell_state,
            layer_shell_state,
            shm_state,
            seat_state,
            dmabuf_state: DmabufState::new(),
            xdg_activation_state,
            data_device_state,
            syncobj_state: None,
            frame_stats: None,
            held: Vec::new(),
            next_held: 0,
            client_gone,
            foreign_toplevel_state,
            toplevel_capture_state,
            output_capture_state,
            image_copy_capture_state,
            capture_sessions: Vec::new(),
            captures: Vec::new(),
            lock_state,
            lock: None,
            idle_notifier_state,
            workspace_groups: Vec::new(),
            output_management: OutputManagement::default(),
            allow_list: AllowList::load(Path::new(crate::identity::ALLOW_LIST)),
            process: Box::new(Proc),
        })
    }

    /// Input from the user: the idle timers start again.
    pub fn activity(&mut self) {
        let seat = self.seat.clone();
        self.idle_notifier_state.notify_activity(&seat);
    }

    /// What the compositor keeps for a client that connects now on `stream`, through
    /// `context`'s listener or through the compositor's own socket without one. The privileged
    /// globals it is granted are decided here, once: inside a security context none, outside
    /// one what the allow-list grants its identity, and none when its identity cannot be read.
    pub fn client_state(
        &self,
        stream: &UnixStream,
        context: Option<SecurityContext>,
    ) -> Arc<ClientState> {
        let (identity, grants) = match &context {
            Some(_) => (None, Vec::new()),
            None => match crate::identity::identify(
                stream.as_fd(),
                self.process.as_ref(),
                &self.allow_list,
            ) {
                Ok((identity, grants)) => {
                    tracing::debug!(?identity, ?grants, "a client connected");
                    (Some(identity), grants)
                }
                Err(error) => {
                    tracing::warn!(%error, "a client's identity cannot be read: no privileged global");
                    (None, Vec::new())
                }
            },
        };
        Arc::new(ClientState {
            compositor_state: CompositorClientState::default(),
            gone: self.client_gone.clone(),
            security_context: context,
            grants,
            identity,
        })
    }

    /// Maps `output` to the right of the outputs already in the space.
    pub fn map_output(&mut self, output: &Output) {
        let x = self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .map(|area| area.loc.x + area.size.w)
            .max()
            .unwrap_or(0);
        self.space.map_output(output, (x, 0));
        self.output_heads_changed();
    }

    /// An output that goes: its layer surfaces are closed, and the windows no output left shows
    /// are centred on the first one, where the user can reach them.
    pub fn remove_output(&mut self, output: &Output) {
        self.close_layers(output);
        self.space.unmap_output(output);
        self.fit_to_outputs();
    }

    /// After the outputs changed (one went, or took another size): the windows no output shows
    /// are centred on the first one, the pointer is brought back onto the outputs and the lock
    /// surfaces take the size of their output.
    pub fn fit_to_outputs(&mut self) {
        let outputs: Vec<Rectangle<i32, Logical>> = self
            .space
            .outputs()
            .filter_map(|output| self.space.output_geometry(output))
            .collect();
        let stranded: Vec<Window> = self
            .space
            .elements()
            .filter(|window| {
                self.space
                    .element_geometry(window)
                    .is_some_and(|area| !outputs.iter().any(|output| output.overlaps(area)))
            })
            .cloned()
            .collect();
        if let Some(area) = self.window_area() {
            for window in stranded {
                let at = centred(area, window.geometry());
                self.space.relocate_element(&window, at);
            }
        }
        self.space.refresh();
        if let Some(pointer) = self.seat.get_pointer() {
            let at = pointer.current_location();
            if let Some(to) = crate::input::confine(&outputs, at, at).filter(|to| *to != at) {
                self.pointer_to(to, InputTime::now());
            }
        }
        if let Some(lock) = &self.lock {
            lock.resize(&self.space);
        }
        self.output_heads_changed();
    }

    /// Whether a mapped surface still waits for a frame callback: one shown on no output, whose
    /// callbacks are sent at the throttle of `frame_done`.
    pub fn frame_callbacks_waiting(&self) -> bool {
        let mut waiting = false;
        let mut check = |_: &WlSurface, states: &SurfaceData| {
            let mapped = states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .is_some_and(|data| data.lock().is_ok_and(|data| data.buffer().is_some()));
            let mut attributes = states.cached_state.get::<SurfaceAttributes>();
            waiting |= mapped && !attributes.current().frame_callbacks.is_empty();
        };
        for window in self.space.elements() {
            window.with_surfaces(&mut check);
        }
        for output in self.space.outputs() {
            for layer in layer_map_for_output(output).layers() {
                layer.with_surfaces(&mut check);
            }
        }
        if let Some(lock) = &self.lock {
            for output in self.space.outputs() {
                if let Some(surface) = lock.surface_on(output) {
                    with_surfaces_surface_tree(surface, &mut check);
                }
            }
        }
        waiting
    }

    /// The area of the first output: where the pointer moves.
    pub fn output_area(&self) -> Option<Rectangle<i32, Logical>> {
        self.space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
    }

    /// What the layer surfaces' exclusive zones leave of the first output: where windows are
    /// placed.
    pub fn window_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        let area = self.space.output_geometry(output)?;
        // The layer map lays the output out at its logical size rounded, the space at its
        // ceiling (853 and 854 for 1280 px at 1.5): the zones are taken as insets, so that a
        // fractional scale with no zone places windows as the whole output does.
        let laid_out = output.current_mode().map(|mode| {
            let size = mode
                .size
                .to_f64()
                .to_logical(output.current_scale().fractional_scale());
            output
                .current_transform()
                .transform_size(size.to_i32_round())
        })?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(inset(area, laid_out, zone))
    }

    /// Imports a client's dmabuf into the renderer, which refuses one it cannot sample.
    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> Result<(), Error> {
        match &mut self.backend {
            Backend::Nested(nested) => {
                nested.backend.renderer().import_dmabuf(dmabuf, None)?;
            }
            Backend::Headless(headless) => headless.import_dmabuf(dmabuf)?,
            Backend::Drm(drm) => drm.import_dmabuf(dmabuf)?,
        }
        Ok(())
    }

    /// After a frame of `output`: the output each surface is shown on, and the scale of that
    /// output as the surface's preferred one, presentation feedback when the frame changed, and
    /// the frame callbacks. The scale is sent here, whatever changed it: a surface hears of a new
    /// scale at the first frame of its output that shows it.
    pub fn frame_done(
        &mut self,
        output: &Output,
        states: &RenderElementStates,
        damaged: bool,
        frame_target: Time<Monotonic>,
        period: Duration,
    ) {
        // Collected: the output's layer map is not held while the surfaces are visited.
        let layers: Vec<LayerSurface> = layer_map_for_output(output).layers().cloned().collect();
        let scale = output.current_scale();
        let transform = output.current_transform();
        let scanout = |surface: &WlSurface, surface_states: &SurfaceData| {
            let primary = update_surface_primary_scanout_output(
                surface,
                output,
                surface_states,
                None,
                states,
                default_primary_scanout_output_compare,
            );
            // Both send only what changed since they last sent it.
            if primary.as_ref() == Some(output) {
                with_fractional_scale(surface_states, |fractional| {
                    fractional.set_preferred_scale(scale.fractional_scale())
                });
                send_surface_state(surface, surface_states, scale.integer_scale(), transform);
            }
        };
        for window in self.space.elements() {
            window.with_surfaces(scanout);
        }
        for layer in &layers {
            layer.with_surfaces(scanout);
        }
        let locked = self
            .lock
            .as_ref()
            .and_then(|lock| lock.surface_on(output))
            .cloned();
        if let Some(surface) = &locked {
            with_surfaces_surface_tree(surface, scanout);
        }
        let popups = self.composing.popups_on(&self.space, output);
        for (surface, _) in &popups {
            with_surfaces_surface_tree(surface, scanout);
        }
        if damaged {
            self.presentation_feedback(output, states).presented(
                frame_target,
                Refresh::fixed(period),
                0,
                wp_presentation_feedback::Kind::Vsync,
            );
        }
        let throttle = Some(FRAME_THROTTLE);
        for window in self.space.elements() {
            window.send_frame(
                output,
                frame_target,
                throttle,
                surface_primary_scanout_output,
            );
        }
        for layer in &layers {
            layer.send_frame(
                output,
                frame_target,
                throttle,
                surface_primary_scanout_output,
            );
        }
        // ponytail: no presentation feedback for the input method's popups, which ask for none
        // yet; take theirs in `presentation_feedback` when one does.
        for surface in locked.iter().chain(popups.iter().map(|(surface, _)| surface)) {
            send_frames_surface_tree(
                surface,
                output,
                frame_target,
                throttle,
                surface_primary_scanout_output,
            );
        }
    }

    /// The presentation feedback the surfaces shown on `output` asked for, to be answered when
    /// the frame drawn with `states` is presented; after `frame_done`, which says where each
    /// surface is shown.
    pub fn presentation_feedback(
        &self,
        output: &Output,
        states: &RenderElementStates,
    ) -> OutputPresentationFeedback {
        let mut feedback = OutputPresentationFeedback::new(output);
        let flags = |surface: &WlSurface, _: &SurfaceData| {
            surface_presentation_feedback_flags_from_states(surface, None, states)
        };
        for window in self.space.elements() {
            window.take_presentation_feedback(&mut feedback, surface_primary_scanout_output, flags);
        }
        // Collected, as in `frame_done`.
        let layers: Vec<LayerSurface> = layer_map_for_output(output).layers().cloned().collect();
        for layer in &layers {
            layer.take_presentation_feedback(&mut feedback, surface_primary_scanout_output, flags);
        }
        feedback
    }

    /// What is under `pos`, with its origin in the space, in the order the outputs are drawn
    /// from the top: the overlay and top layers, the windows, the bottom and background layers.
    /// Nothing while the session is locked.
    pub fn under(&self, pos: Point<f64, Logical>) -> Option<(Under, Point<i32, Logical>)> {
        if self.lock.is_some() {
            return None;
        }
        let output = self.space.output_under(pos).next()?;
        let origin = self.space.output_geometry(output)?.loc;
        let map = layer_map_for_output(output);
        // The top surface of `layer` with input at `pos`: one whose input region leaves `pos`
        // out lets the click through to what is below.
        let on = |layer| {
            map.layers_on(layer).rev().find_map(|surface| {
                let location = map.layer_geometry(surface)?.loc + origin;
                surface.surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)?;
                Some((Under::Layer(surface.clone()), location))
            })
        };
        let found = on(Layer::Overlay)
            .or_else(|| on(Layer::Top))
            .or_else(|| {
                self.space
                    .element_under(pos)
                    .map(|(window, location)| (Under::Window(window.clone()), location))
            })
            .or_else(|| on(Layer::Bottom))
            .or_else(|| on(Layer::Background));
        found
    }

    /// The surface under `pos`, with its origin in the space.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        if let Some(lock) = &self.lock {
            // While locked, only the lock surface of the output under `pos`, which fills it.
            let output = self.space.output_under(pos).next()?;
            let origin = self.space.output_geometry(output)?.loc.to_f64();
            return lock
                .surface_on(output)
                .map(|surface| (surface.clone(), origin));
        }
        let (under, origin) = self.under(pos)?;
        let local = pos - origin.to_f64();
        let found = match &under {
            Under::Layer(layer) => layer.surface_under(local, WindowSurfaceType::ALL),
            Under::Window(window) => window.surface_under(local, WindowSurfaceType::ALL),
        };
        found.map(|(surface, at)| (surface, (at + origin).to_f64()))
    }

    /// The mapped window whose toplevel is `surface`.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
            .cloned()
    }
}

/// What an output shows: the space with the input method's popups on top, or while the session
/// is locked only the lock surface the locker gave it, if any, on the background.
pub enum View<'a> {
    Space(&'a Space<Window>, &'a [PlacedPopup]),
    Locked(Option<&'a WlSurface>),
}

impl<'a> View<'a> {
    pub fn of(
        space: &'a Space<Window>,
        lock: Option<&'a Lock>,
        popups: &'a [PlacedPopup],
        output: &Output,
    ) -> Self {
        match lock {
            Some(lock) => View::Locked(lock.surface_on(output)),
            None => View::Space(space, popups),
        }
    }
}

/// Draws `view` on `output` into `framebuffer`, `age` frames after its previous contents.
pub fn draw_view<'d, R>(
    renderer: &mut R,
    framebuffer: &mut R::Framebuffer<'_>,
    output: &Output,
    age: usize,
    view: View<'_>,
    damage_tracker: &'d mut OutputDamageTracker,
) -> Result<RenderOutputResult<'d>, DamageError<R::Error>>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + Texture + 'static,
{
    match view {
        View::Space(space, popups) => {
            let scale = output.current_scale().fractional_scale();
            let popups: Vec<WaylandSurfaceRenderElement<R>> = popups
                .iter()
                .flat_map(|(surface, at)| {
                    render_elements_from_surface_tree(
                        renderer,
                        surface,
                        at.to_physical_precise_round(scale),
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                })
                .collect();
            render_output::<_, WaylandSurfaceRenderElement<R>, _, _>(
                output,
                renderer,
                framebuffer,
                1.0,
                age,
                [space],
                &popups,
                damage_tracker,
                BACKGROUND,
            )
        }
        View::Locked(surface) => {
            let scale = output.current_scale().fractional_scale();
            let elements: Vec<WaylandSurfaceRenderElement<R>> = surface
                .map(|surface| {
                    render_elements_from_surface_tree(
                        renderer,
                        surface,
                        (0, 0),
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                })
                .unwrap_or_default();
            damage_tracker.render_output(renderer, framebuffer, age, &elements, BACKGROUND)
        }
    }
}

/// What the pointer can find under it.
pub enum Under {
    Layer(LayerSurface),
    Window(Window),
}

/// A new output with its global: `size` in pixels, refreshed at `refresh` mHz.
pub fn new_output(
    dh: &DisplayHandle,
    name: &str,
    model: &str,
    size: Size<i32, Physical>,
    refresh: i32,
    transform: Transform,
    scale: f64,
) -> Output {
    let output = build_output(name, model, size, refresh, transform, scale);
    publish_output(dh, &output);
    output
}

/// An output with no global yet (`publish_output`): for a backend that learns whether the
/// monitor works only after it has the output.
pub fn build_output(
    name: &str,
    model: &str,
    size: Size<i32, Physical>,
    refresh: i32,
    transform: Transform,
    scale: f64,
) -> Output {
    let output = Output::new(
        name.into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Athanor".into(),
            model: model.into(),
            serial_number: String::new(),
        },
    );
    let mode = Mode { size, refresh };
    output.change_current_state(
        Some(mode),
        Some(transform),
        Some(Scale::Fractional(scale)),
        None,
    );
    output.set_preferred(mode);
    output
}

/// Offers `output` to the clients; the global is kept in its user data.
pub fn publish_output(dh: &DisplayHandle, output: &Output) {
    let global = output.create_global::<State>(dh);
    output
        .user_data()
        .insert_if_missing(|| OutputGlobal(global));
}

/// The global of an output, kept in its user data: removed with the output.
pub struct OutputGlobal(pub GlobalId);

/// Dispatches the clients' requests. The callback owns the display: it dispatches without the
/// unsafe access a source holding it would need.
fn dispatch_clients(
    event_loop: &mut EventLoop<'static, State>,
    mut display: Display<State>,
) -> Result<(), Error> {
    let fd = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop
        .handle()
        .insert_source(
            Generic::new(fd, Interest::READ, smithay::reexports::calloop::Mode::Level),
            move |_, _, state| {
                display.dispatch_clients(state)?;
                // A request may change what is on screen: a surface destroyed, a lock taken.
                state.request_redraw();
                if let Err(error) = state.display_handle.flush_clients() {
                    tracing::warn!(%error, "the clients could not be flushed");
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| e.error)?;
    Ok(())
}

/// Drops the held commits of each client that disconnects, with their event sources: a client
/// gone with a fence that never signals would otherwise keep them, and its buffers, for ever.
fn release_held_on_disconnection(
    event_loop: &mut EventLoop<'static, State>,
    gone: Channel<ClientId>,
) -> Result<(), Error> {
    event_loop
        .handle()
        .insert_source(gone, |event, _, state| {
            if let channel::Event::Msg(client) = event {
                state.release_held(&client);
            }
        })
        .map_err(|e| e.error)?;
    Ok(())
}

/// Listens on the next free `wayland-N` socket and returns its name.
pub fn listen(handle: &LoopHandle<'static, State>) -> Result<OsString, Error> {
    let socket = ListeningSocketSource::new_auto()?;
    let name = socket.socket_name().to_os_string();
    handle
        .insert_source(socket, |stream, _, state| {
            let data = state.client_state(&stream, None);
            if let Err(error) = state.display_handle.insert_client(stream, data) {
                tracing::warn!(%error, "a client could not connect");
            }
        })
        .map_err(|e| e.error)?;
    Ok(name)
}

/// What the compositor keeps per client.
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    gone: channel::Sender<ClientId>,
    /// The security context the client connected through, if any (a sandboxed application).
    pub security_context: Option<SecurityContext>,
    /// The privileged globals granted when it connected.
    grants: Vec<Privilege>,
    /// What the grants were decided on, when the process could be read.
    identity: Option<Identity>,
}

/// The identity `client` connected with, when it could be read.
pub fn identity_of(client: &Client) -> Option<Identity> {
    client
        .get_data::<ClientState>()
        .and_then(|data| data.identity.clone())
}

/// Whether `client` was granted `privilege` when it connected.
pub fn granted(client: &Client, privilege: Privilege) -> bool {
    client
        .get_data::<ClientState>()
        .is_some_and(|data| data.grants.contains(&privilege))
}

/// Whether `client` connected outside a security context: only such a client sees the security
/// context manager.
pub fn unconfined(client: &Client) -> bool {
    client
        .get_data::<ClientState>()
        .is_some_and(|data| data.security_context.is_none())
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        if let DisconnectReason::ProtocolError(error) = &reason {
            tracing::info!(?client_id, %error, "a client was disconnected");
        }
        // The receiver lives as long as the event loop: an error means the compositor is ending.
        if let Err(error) = self.gone.send(client_id) {
            tracing::warn!(%error, "a client's disconnection reached no event loop");
        }
    }
}

/// Where a window of `geometry` is mapped to be centred in `area`, its top left kept inside.
pub fn centred(
    area: Rectangle<i32, Logical>,
    geometry: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let x = area.loc.x + ((area.size.w - geometry.size.w) / 2).max(0) - geometry.loc.x;
    let y = area.loc.y + ((area.size.h - geometry.size.h) / 2).max(0) - geometry.loc.y;
    (x, y).into()
}

/// `area`, the output's rectangle in the space, less the insets the layer map's `zone` leaves of
/// the output laid out at `laid_out`. Each inset is kept inside the output, so that a client's
/// exclusive zone, however large, cannot place windows outside it or overflow; zones that leave
/// no room at all are ignored, and windows take the whole output.
fn inset(
    area: Rectangle<i32, Logical>,
    laid_out: Size<i32, Logical>,
    zone: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let near = |at: i32, max: i32| at.clamp(0, max);
    let far = |at: i32, size: i32, laid: i32, max: i32| {
        laid.saturating_sub(at.saturating_add(size)).clamp(0, max)
    };
    let (left, top) = (near(zone.loc.x, area.size.w), near(zone.loc.y, area.size.h));
    let right = far(zone.loc.x, zone.size.w, laid_out.w, area.size.w - left);
    let bottom = far(zone.loc.y, zone.size.h, laid_out.h, area.size.h - top);
    let size = Size::from((area.size.w - left - right, area.size.h - top - bottom));
    if size.w <= 0 || size.h <= 0 {
        return area;
    }
    Rectangle::new(area.loc + Point::from((left, top)), size)
}

#[cfg(test)]
mod tests {
    use super::{inset, GrabSerials};
    use smithay::reexports::wayland_server::backend::{ClientData, ClientId};
    use smithay::reexports::wayland_server::Display;
    use smithay::utils::{Logical, Rectangle, Serial};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn exclusive_zones_are_kept_inside_the_output() {
        let output = rect(1280, 0, 854, 534);
        let laid_out = (853, 533).into();
        // No zone: the whole output, also where the layer map rounds the size down.
        assert_eq!(inset(output, laid_out, rect(0, 0, 853, 533)), output);
        // A bar of 32 at the top and a dock of 64 at the bottom.
        assert_eq!(
            inset(output, laid_out, rect(0, 32, 853, 437)),
            rect(1280, 32, 854, 438)
        );
        // The layer map saturates and keeps the zone's size at zero or more; its location can
        // still be anywhere. A top zone of 100000 leaves no room: ignored, rather than windows
        // off the output.
        assert_eq!(inset(output, laid_out, rect(0, 100_000, 853, 0)), output);
        // A bottom zone of 100000 leaves only the row the layer map's rounding drops: windows
        // are placed from the output's top edge, still on it.
        assert_eq!(
            inset(output, laid_out, rect(0, 0, 853, 0)),
            rect(1280, 0, 854, 1)
        );
        // Values at the ends of i32 do not overflow.
        assert_eq!(inset(output, laid_out, rect(0, i32::MAX, 853, 0)), output);
        assert_eq!(
            inset(
                output,
                laid_out,
                rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)
            ),
            output
        );
    }

    struct Data;
    impl ClientData for Data {}

    /// A client grabbing a popup with the serial of a press or key sent to another client is
    /// refused, however well it guesses it.
    #[test]
    fn a_serial_is_good_only_for_the_client_it_was_sent_to() {
        let display = Display::<()>::new().expect("display");
        let mut handle = display.handle();
        let mut client = || -> ClientId {
            let (stream, _peer) = UnixStream::pair().expect("socket pair");
            handle
                .insert_client(stream, Arc::new(Data))
                .expect("client")
                .id()
        };
        let (b, m) = (client(), client());
        let s = Serial::from;
        let serials = GrabSerials {
            button: Some((s(7), b.clone())),
            key_press: Some((s(8), b.clone())),
            key_release: Some((s(9), m.clone())),
        };
        assert!(serials.clicked(s(7), &b));
        assert!(!serials.clicked(s(7), &m), "B's press named by M");
        assert!(!serials.clicked(s(6), &b), "an older press");
        assert!(!serials.clicked(s(8), &b), "a key is not a click");
        assert!(serials.typed(s(8), &b) && serials.typed(s(9), &m));
        assert!(!serials.typed(s(8), &m), "B's key named by M");
        assert!(!serials.typed(s(7), &b), "a click is not a key");
    }
}
