//! The compositor's state: the protocol globals, the space of windows and its outputs, the seat
//! and the backend that draws them.

use crate::handlers::{shown, HeldCommit};
use crate::headless::{FrameStats, Headless};
use crate::identity::{AllowList, Identity, Privilege, Proc, Process};
use crate::input_method::PlacedPopup;
use crate::a11y::Look;
use crate::keyboard_group::protocol::athanor_keyboard_group_v1::AthanorKeyboardGroupV1;
use crate::look::{FilterElement, Filters};
use crate::marks::{Marks, Stacked};
use crate::lock::Lock;
use crate::output_management::OutputManagement;
use crate::output_power::OutputPower;
use crate::toplevel_state::protocol::athanor_toplevel_state_v1::AthanorToplevelStateV1;
use crate::udev::Drm;
use crate::winit::Nested;
use crate::workspace::{Bound, Strip};
use crate::Error;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::input::InputTime;
use smithay::backend::renderer::damage::{
    Error as DamageError, OutputDamageTracker, RenderOutputResult,
};
use smithay::backend::renderer::element::surface::render_elements_from_surface_tree;
use smithay::backend::renderer::element::{
    default_primary_scanout_output_compare, Kind, RenderElement, RenderElementStates,
};
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::{ImportDma, Texture};
use smithay::desktop::utils::{
    send_frames_surface_tree, surface_presentation_feedback_flags_from_states,
    surface_primary_scanout_output, update_surface_primary_scanout_output,
    with_surfaces_surface_tree, OutputPresentationFeedback,
};
use smithay::desktop::{
    layer_map_for_output, LayerSurface, PopupManager, Space, Window, WindowSurfaceType,
};
use smithay::input::keyboard::Keycode;
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::channel::{self, Channel};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, LoopHandle, LoopSignal, PostAction};
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
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
use smithay::wayland::foreign_toplevel_list::{ForeignToplevelHandle, ForeignToplevelListState};
use smithay::wayland::fractional_scale::{with_fractional_scale, FractionalScaleManagerState};
use smithay::wayland::idle_inhibit::IdleInhibitManagerState;
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
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgShellState};
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::virtual_keyboard::{VirtualKeyboardDevice, VirtualKeyboardManagerState};
use smithay::wayland::virtual_pointer::{VirtualPointerDevice, VirtualPointerManagerState};
use smithay::wayland::xdg_activation::XdgActivationState;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
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
    /// The actions of the bound keys pressed, in order (`State::fire`); the fixture reads them.
    #[cfg(test)]
    pub fired: Vec<crate::bindings::Action>,
    /// The press of Super, alone, whose release shows the launcher unless another key, a
    /// button, an axis or a gesture comes first (plan D4).
    pub super_tap: Option<Keycode>,
    /// The toplevels exported by xdg-foreign (`foreign.rs`).
    pub foreign: crate::foreign::Foreign,
    /// The touchpad gesture in progress (`gestures.rs`).
    pub gesture: Option<crate::gestures::Kind>,
    /// The windows owed an answer to a decoration request (`decoration.rs`).
    pub decoration_owed: Vec<WlSurface>,
    /// The barrel buttons each tablet tool holds pressed, as the seat knows them (`tablet.rs`).
    pub tool_pressed: std::collections::HashMap<
        smithay::backend::input::TabletToolDescriptor,
        Vec<u32>,
    >,
    /// Counts the locks taken: a request for the session bus carries the epoch it was asked
    /// in, and the worker drops one asked before a lock (`session_calls`).
    pub lock_epoch: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// The session-bus worker of the key bindings; none headless.
    pub session_calls: Option<crate::session_calls::SessionCalls>,
    /// Orca's keyboard monitor: who watches and grabs keys (`keyboard_monitor.rs`).
    pub keyboard_monitor: crate::keyboard_monitor::Monitor,
    /// The keyboard aids of AX11, their settings and the keys they hold (`aids.rs`).
    pub aids: crate::aids::Aids,
    /// The modifiers each virtual keyboard last set, applied to its own keys only
    /// (`with_keymap_of`).
    pub virtual_modifiers: HashMap<VirtualKeyboardDevice, [u32; 4]>,
    /// The keys each virtual keyboard holds that went past the input method's grab, whose
    /// release goes past it too. Only evdev's key codes are kept, each once, so an entry holds
    /// at most 768.
    pub past_grab_keys: HashMap<VirtualKeyboardDevice, HashSet<Keycode>>,
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
    /// The cursor a client set last and that client, or the default and no client: shown only
    /// while that client holds the pointer's focus (`shown_cursor`).
    pub cursor_status: (CursorImageStatus, Option<ClientId>),
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
    /// Primary selection and data control (`clipboard`).
    pub clipboard: crate::clipboard::Clipboard,
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
    /// The surfaces that hold idle inhibitors, with how many each holds (`idle_inhibit`).
    pub inhibitors: HashMap<WlSurface, usize>,
    /// The keyboard-shortcuts inhibitors and the clients Ctrl+Alt+Tab suspended (`constraints`).
    pub input_hold: crate::constraints::Held,
    /// What the clients of ext-workspace-v1 were sent.
    pub workspace_clients: Vec<Bound>,
    /// The workspaces of each output (`workspace`).
    pub workspaces: Vec<Strip>,
    /// The number the last workspace made was given.
    pub next_workspace: u64,
    /// The windows of the inactive workspaces, out of the space, each with its location.
    pub parked: Vec<(Window, Point<i32, Logical>)>,
    /// The live state objects of athanor-toplevel-state-v1, an inert one excepted.
    pub toplevel_states: Vec<AthanorToplevelStateV1>,
    /// The live objects of athanor-keyboard-group-v1, an inert one excepted.
    pub keyboard_groups: Vec<AthanorKeyboardGroupV1>,
    /// The group the live objects were last sent (`State::send_keyboard_group`).
    pub sent_group: u32,
    /// The magnifier and the colour filters, and the managers of athanor-a11y-v1.
    pub a11y: crate::a11y::A11y,
    pub focus_theme: crate::focus_theme::FocusTheme,
    /// The keyboard's repeat, rate then delay as `repeat_info` takes them: `REPEAT_RATE` and
    /// `REPEAT_DELAY` until `xkb_config` sets it.
    pub repeat: (i32, i32),
    /// The minimised windows, out of the space, each with the location it is restored at.
    pub minimised: Vec<(Window, Point<i32, Logical>)>,
    /// The heads sent to the clients of wlr-output-management, and their configurations.
    pub output_management: OutputManagement,
    /// The outputs turned off, and the clients of wlr-output-power-management.
    pub output_power: OutputPower,

    /// Which client is granted which privileged global (OS4), read when the compositor starts.
    pub allow_list: AllowList,
    /// Where a connecting client's executable and cgroup are read.
    pub process: Box<dyn Process>,
    /// The X11 display xwayland-satellite serves, when the compositor holds one.
    pub x11: Option<crate::xwayland::X11>,
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
        // A confined client does not see it, since a surface of its own would stand unmarked
        // (`doc_own_stack.md` decision 31). Of the others, only a client granted `TopLayers`
        // keeps a surface on the top or overlay layer (`layer_commit`). The escape key takes the
        // keyboard back from the exclusive surface of a granted client (`escape_exclusive`).
        let layer_shell_state = WlrLayerShellState::new_with_filter::<Self, _>(&dh, unconfined);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        // The globals below keep no state the compositor reads back.
        OutputManagerState::new_with_xdg_output::<Self>(&dh);
        ViewporterState::new::<Self>(&dh);
        // cosmic-idle's fade to black (doc_own_stack.md OS17); Smithay draws such a buffer as
        // a solid colour on every renderer.
        SinglePixelBufferState::new::<Self>(&dh);
        IdleInhibitManagerState::new::<Self>(&dh);
        let input_hold = crate::constraints::Held::new(&dh);
        FractionalScaleManagerState::new::<Self>(&dh);
        PresentationState::new::<Self>(&dh, clock.id() as u32);
        CursorShapeManagerState::new::<Self>(&dh);
        let xdg_activation_state = XdgActivationState::new::<Self>(&dh);
        // GDK will not open a display without wl_data_device_manager.
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let clipboard = crate::clipboard::Clipboard::new(&dh);
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
        crate::toplevel_state::global(&dh);
        crate::keyboard_group::global(&dh);
        crate::a11y::global(&dh);
        crate::output_management::global(&dh);
        crate::output_power::global(&dh);
        // Synthetic input, which takes the hardware's path (`input.rs`).
        VirtualKeyboardManagerState::new::<Self, _>(&dh, |c| {
            granted(c, Privilege::VirtualKeyboard)
        });
        VirtualPointerManagerState::new::<Self, _>(&dh, |c| granted(c, Privilege::VirtualPointer));
        // Composed text (OS9 item 3): the input method by grant, text input outside a sandbox.
        crate::input_method::globals(&dh);
        crate::gestures::global(&dh);
        crate::tablet::global(&dh);
        crate::decoration::global(&dh);
        crate::foreign::global(&dh);
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
            #[cfg(test)]
            fired: Vec::new(),
            super_tap: None,
            foreign: Default::default(),
            gesture: None,
            decoration_owed: Vec::new(),
            tool_pressed: Default::default(),
            lock_epoch: std::sync::Arc::default(),
            session_calls: None,
            keyboard_monitor: crate::keyboard_monitor::Monitor::default(),
            aids: Default::default(),
            virtual_modifiers: HashMap::new(),
            past_grab_keys: HashMap::new(),
            virtual_keymap: None,
            composing: Default::default(),
            virtual_buttons: HashMap::new(),
            escaped: Vec::new(),
            lock_requests: 0,
            fatal: None,
            cursor_status: (CursorImageStatus::default_named(), None),
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
            clipboard,
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
            inhibitors: HashMap::new(),
            input_hold,
            workspace_clients: Vec::new(),
            workspaces: Vec::new(),
            next_workspace: 0,
            parked: Vec::new(),
            toplevel_states: Vec::new(),
            keyboard_groups: Vec::new(),
            sent_group: 0,
            a11y: crate::a11y::A11y::default(),
            focus_theme: crate::focus_theme::FocusTheme::default(),
            repeat: (REPEAT_RATE, REPEAT_DELAY),
            minimised: Vec::new(),
            output_management: OutputManagement::default(),
            output_power: OutputPower::default(),
            allow_list: AllowList::load(Path::new(crate::identity::ALLOW_LIST)),
            process: Box::new(Proc),
            x11: None,
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
            confined: context.is_some(),
            grants,
            identity,
        })
    }

    /// What the compositor keeps for a client of xwayland-satellite's socket: confined as a
    /// security context's client is, so granted nothing whatever its identity (OS4).
    pub fn satellite_state(&self) -> Arc<ClientState> {
        Arc::new(ClientState {
            compositor_state: CompositorClientState::default(),
            gone: self.client_gone.clone(),
            confined: true,
            grants: Vec::new(),
            identity: None,
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
        self.workspaces_changed();
    }

    /// An output that goes: its layer surfaces are closed, and its windows join the first
    /// output's active workspace, centred on that output when it does not show them.
    pub fn remove_output(&mut self, output: &Output) {
        self.forget_output_power(output);
        self.close_layers(output);
        self.space.unmap_output(output);
        self.fit_to_outputs();
        self.workspaces_changed();
    }

    /// After the outputs changed (one went, or took another size): the windows their home output
    /// (`State::home_output`) no longer shows are centred on it and the maximised ones fitted to
    /// it again, the pointer is brought back onto the outputs and the lock surfaces take the size
    /// of their output.
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
                    .is_some_and(|area| !self.shown_at_home(window, area))
            })
            .cloned()
            .collect();
        for window in stranded {
            if let Some(area) = self.home_area(&window) {
                let at = centred(area, window.geometry().size);
                self.space.relocate_element(&window, at);
            }
        }
        self.fit_maximised();
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

    /// What the layer surfaces' exclusive zones leave of the first output (`window_area_on`):
    /// where the fixture's windows are kept, maximised and restored.
    #[cfg(test)]
    pub fn window_area(&self) -> Option<Rectangle<i32, Logical>> {
        self.window_area_on(self.space.outputs().next()?)
    }

    /// The first output's `window_area` at the size the layer map lays the output out, its
    /// logical size rounded rather than the space's ceiling (853 and not 854 for 1280 px at
    /// 1.5): the non-exclusive zone cosmic-comp places a new window in (`placed`).
    pub fn placement_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        let area = self.space.output_geometry(output)?;
        let laid_out = laid_out(output)?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(inset(Rectangle::new(area.loc, laid_out), laid_out, zone))
    }

    /// What the zones leave of the output nearest to `point` (`output_nearest`): where a window
    /// being moved is kept.
    pub fn window_area_at(&self, point: Point<f64, Logical>) -> Option<Rectangle<i32, Logical>> {
        self.window_area_on(self.output_nearest(point)?)
    }

    /// The output under `point`, or the output nearest to it when `point` is on none, in a gap
    /// between outputs of different sizes.
    pub fn output_nearest(&self, point: Point<f64, Logical>) -> Option<&Output> {
        let distance = |output: &Output| {
            let shown = self.space.output_geometry(output)?.to_f64();
            let x = point.x.clamp(shown.loc.x, shown.loc.x + shown.size.w);
            let y = point.y.clamp(shown.loc.y, shown.loc.y + shown.size.h);
            Some((x - point.x).powi(2) + (y - point.y).powi(2))
        };
        self.space.output_under(point).next().or_else(|| {
            self.space
                .outputs()
                .filter_map(|output| Some((distance(output)?, output)))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, output)| output)
        })
    }

    /// What the layer surfaces' exclusive zones leave of `output`.
    pub fn window_area_on(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let area = self.space.output_geometry(output)?;
        // The layer map lays the output out at its logical size rounded, the space at its
        // ceiling (853 and 854 for 1280 px at 1.5): the zones are taken as insets, so that a
        // fractional scale with no zone places windows as the whole output does.
        let laid_out = laid_out(output)?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(inset(area, laid_out, zone))
    }

    /// Maximises `window` to `home_area`, or fits a maximised one to it again: a configure
    /// with the `maximized` state and the area's size, and the window moves into the area at
    /// the commit that answers it. The geometry it had before is kept for `restore`.
    pub fn maximise(&mut self, window: &Window) {
        let (Some(toplevel), Some(area)) = (window.toplevel(), self.home_area(window)) else {
            return;
        };
        let maximised = window.user_data().get_or_insert(Maximised::default);
        let before = match maximised.0.get() {
            Some(kept) => kept.before,
            None => self.settled_geometry(window),
        };
        maximised.0.set(Some(Kept { before, area }));
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Maximized);
            state.size = Some(area.size);
        });
        // Before its first commit the initial configure carries the state.
        let serial = toplevel
            .is_initial_configure_sent()
            .then(|| toplevel.send_configure());
        relocate(window, area.loc, serial);
    }

    /// Restores a maximised `window`: a configure without the `maximized` state and with the
    /// size it had, and the window moves back at the commit that answers it, centred in its
    /// `home_area` when its home output no longer shows that place. The geometry it had, or
    /// `None` when it is not maximised. A window maximised before it had a size picks its own
    /// and is placed as a new window is.
    pub fn restore(&mut self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        let toplevel = window.toplevel()?;
        let kept = window.user_data().get::<Maximised>()?.0.take()?.before;
        toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
        });
        self.back_to(window, kept);
        Some(kept)
    }

    /// A full-screen window no longer maximised leaves full-screen for the place it had before
    /// it was.
    pub fn forget_maximised(&self, window: &Window) {
        let data = window.user_data();
        let kept = data.get::<Maximised>().and_then(|kept| kept.0.take());
        if let (Some(Kept { before, .. }), Some(full)) = (kept, data.get::<FullScreen>()) {
            full.0.set(full.0.get().map(|kept| Kept { before, ..kept }));
        }
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
            });
        }
    }

    /// A configure with the size `kept` had, and the window moves back at the commit that
    /// answers it, centred in its `home_area` when its home output no longer shows that place.
    /// A window drawn with no earlier place is placed at that commit, as a new window is.
    fn back_to(&mut self, window: &Window, kept: Rectangle<i32, Logical>) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|state| {
            state.size = (!kept.is_empty()).then_some(kept.size);
        });
        let serial = toplevel
            .is_initial_configure_sent()
            .then(|| toplevel.send_configure());
        if !kept.is_empty() {
            relocate(window, self.kept_place(window, kept), serial);
        } else if crate::handlers::shown(window) {
            place_again(window, serial);
        } else if let Some(moving) = window.user_data().get::<Relocate>() {
            // Asked before the window was placed: nothing to go back to, and `place` places it.
            moving.0.set(None);
        }
    }

    /// Where a window goes back to `kept`: there, or the middle of its `home_area` when its home
    /// output no longer shows that place.
    fn kept_place(&self, window: &Window, kept: Rectangle<i32, Logical>) -> Point<i32, Logical> {
        match self
            .home_area(window)
            .filter(|_| !self.shown_at_home(window, kept))
        {
            Some(area) => centred(area, kept.size),
            None => kept.loc,
        }
    }

    /// A window that unmapped has asked for nothing (`xdg_toplevel`): it is neither full-screen
    /// nor maximised any more, and it draws again at the place it had.
    pub fn forget_requests(&self, window: &Window) {
        let data = window.user_data();
        let maximised = data.get::<Maximised>().and_then(|kept| kept.0.take());
        let full = data.get::<FullScreen>().and_then(|kept| kept.0.take());
        match maximised.or(full) {
            Some(Kept { before, .. }) if before.is_empty() => place_again(window, None),
            Some(Kept { before, .. }) => relocate(window, self.kept_place(window, before), None),
            None => {}
        }
    }

    /// Gives `window` the `output` the client named, or its home output, whole: a configure
    /// with the `fullscreen` state and the output's size, and the window moves there at the
    /// commit that answers it. The geometry it had before is kept for `leave_full_screen`. A
    /// maximised window stays so, and returns to that. A window on an inactive workspace or
    /// minimised stays there, whichever output it names: only the user brings it to the one they
    /// see.
    pub fn full_screen(&mut self, window: &Window, output: Option<Output>) {
        let visible = self.on_active_workspace(window) && !self.is_minimised(window);
        let named = output.filter(|_| visible);
        if let Some(id) = named.and_then(|output| self.active_workspace(&output)) {
            // Before its first commit a window has no workspace yet.
            self.place_on_workspace(window);
            self.move_window_to(window, id);
        }
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let Some(area) = self.home_output_area(window) else {
            if toplevel.is_initial_configure_sent() {
                toplevel.send_configure();
            }
            return;
        };
        let before = self.settled_geometry(window);
        let full = window.user_data().get_or_insert(FullScreen::default);
        let before = full.0.get().map_or(before, |kept| kept.before);
        full.0.set(Some(Kept { before, area }));
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
            state.size = Some(area.size);
        });
        // Before its first commit the initial configure carries the state.
        let serial = toplevel
            .is_initial_configure_sent()
            .then(|| toplevel.send_configure());
        relocate(window, area.loc, serial);
    }

    /// Where `window` stands, or will once a move on its way lands, at the size that move
    /// asked; out of the space, where it comes back; empty on its way to where `placed` puts it.
    fn settled_geometry(&self, window: &Window) -> Rectangle<i32, Logical> {
        let size = window.geometry().size;
        let moving = window
            .user_data()
            .get::<Relocate>()
            .and_then(|relocate| relocate.0.get());
        if let Some(Destination { to: None, .. }) = moving {
            return Rectangle::default();
        }
        if let Some(Destination { to: Some(to), .. }) = moving {
            let asked = window
                .toplevel()
                .and_then(|toplevel| toplevel.with_pending_state(|state| state.size));
            return Rectangle::new(to, asked.unwrap_or(size));
        }
        self.space.element_geometry(window).unwrap_or_else(|| {
            self.minimised
                .iter()
                .chain(self.parked.iter())
                .find(|(kept, _)| kept == window)
                .map(|(_, at)| Rectangle::new(*at, size))
                .unwrap_or_default()
        })
    }

    /// Ends `full_screen`: the window returns to where it was, or to its maximised size.
    pub fn leave_full_screen(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let Some(kept) = window
            .user_data()
            .get::<FullScreen>()
            .and_then(|full| full.0.take())
        else {
            return;
        };
        toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
        });
        let maximised = window
            .user_data()
            .get::<Maximised>()
            .is_some_and(|maximised| maximised.0.get().is_some());
        if maximised {
            self.maximise(window);
        } else {
            self.back_to(window, kept.before);
        }
    }

    /// Minimises `window`: it leaves the space, keeping its location, its popups are dismissed,
    /// and the keyboard and the pointer go to what is under it; it keeps committing until
    /// `unminimise`. A window the user cannot see, one that has not drawn yet included, is
    /// refused: false.
    pub fn minimise(&mut self, window: &Window) -> bool {
        let Some(at) = self
            .space
            .element_location(window)
            .filter(|_| shown(window))
        else {
            return false;
        };
        self.end_window_grab(window);
        self.dismiss_popups(window);
        window.set_activated(false);
        if let Some(toplevel) = window.toplevel() {
            toplevel.send_pending_configure();
        }
        self.space.unmap_elem(window);
        self.minimised.push((window.clone(), at));
        if let Some(toplevel) = window.toplevel() {
            self.refocus(toplevel.wl_surface());
        }
        // A button goes to the pointer's focus: a click without a motion must miss the window.
        self.refresh_pointer();
        self.request_redraw();
        true
    }

    /// Restores a minimised `window` where it was, or centred in its `home_area` when its output
    /// no longer shows that place, on top and with the keyboard, its workspace activated first.
    /// False when it is not minimised, or draws nothing (it committed a null buffer, as an app
    /// hiding into the tray does): it stays minimised.
    pub fn unminimise(&mut self, window: &Window) -> bool {
        if !self.is_minimised(window) || !shown(window) {
            return false;
        }
        if let Some(id) = self.workspace_of(window) {
            self.activate_workspace(id);
        }
        let Some(index) = self.minimised.iter().position(|(kept, _)| kept == window) else {
            return false;
        };
        let (window, at) = self.minimised.remove(index);
        let at = self.reachable(&window, at);
        self.space.map_element(window.clone(), at, true);
        // A move answered while it was minimised applies now, and a maximised window takes the
        // area of now.
        self.relocated(&window);
        self.fit_maximised();
        self.focus(&window);
        self.refresh_pointer();
        self.request_redraw();
        true
    }

    /// Fits the maximised windows to their `home_area` again, and the full-screen ones to their
    /// output, after the layer surfaces' exclusive zones or the outputs changed it.
    pub fn fit_maximised(&mut self) {
        let stale: Vec<Window> = self
            .space
            .elements()
            .filter(|window| {
                let full = window.user_data().get::<FullScreen>();
                if let Some(kept) = full.and_then(|full| full.0.get()) {
                    return self.home_output_area(window) != Some(kept.area);
                }
                window
                    .user_data()
                    .get::<Maximised>()
                    .and_then(|maximised| maximised.0.get())
                    .zip(self.home_area(window))
                    .is_some_and(|(kept, area)| kept.area != area)
            })
            .cloned()
            .collect();
        for window in &stale {
            if is_full_screen(window) {
                self.full_screen(window, None);
            } else {
                self.maximise(window);
            }
        }
    }

    /// Moves `window`'s geometry to `to`: now, or, while the window waits for a configure that
    /// changes its size, at the commit that answers it.
    pub fn move_window(&mut self, window: &Window, to: Point<i32, Logical>) {
        let serial = window
            .user_data()
            .get::<Relocate>()
            .and_then(|relocate| relocate.0.get())
            .and_then(|destination| destination.serial);
        relocate(window, to, serial);
        self.relocated(window);
    }

    /// Moves `window` where its `Relocate` asks, at a commit that answers the configure it
    /// waits for, so that the window moves with its new size, not before, and says whether it
    /// did. A full-screen window then keeps to the middle of its output, at every call.
    pub fn relocated(&mut self, window: &Window) -> bool {
        let mut landed = false;
        let relocate = window.user_data().get::<Relocate>();
        if let Some(Destination { to, serial }) = relocate.and_then(|relocate| relocate.0.get()) {
            if let (Some(serial), Some(toplevel)) = (serial, window.toplevel()) {
                if !answered(toplevel, serial) {
                    return false;
                }
            }
            let to = to.or_else(|| {
                let area = self.home_area(window)?;
                crate::handlers::shown(window).then(|| placed(area, window.geometry()))
            });
            if let (Some(relocate), Some(to)) = (relocate, to) {
                relocate.0.set(None);
                self.space.relocate_element(window, to);
                landed = true;
            }
        }
        // A client smaller than its output stands in the middle of it, as
        // `xdg_toplevel.set_fullscreen` says.
        if let Some(area) = full_screen_area(window) {
            let to = centred(area, window.geometry().size);
            landed |= self.space.element_location(window) != Some(to);
            self.space.relocate_element(window, to);
        }
        landed
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
        for surface in locked
            .iter()
            .chain(popups.iter().map(|(surface, _)| surface))
        {
            send_frames_surface_tree(
                surface,
                output,
                frame_target,
                throttle,
                surface_primary_scanout_output,
            );
        }
        // The cursor surface is not among the space's elements: its frames go to the output
        // the pointer is over.
        if let CursorImageStatus::Surface(surface) = self.shown_cursor() {
            let over = self.seat.get_pointer().is_some_and(|pointer| {
                self.space
                    .output_under(pointer.current_location())
                    .any(|under| under == output)
            });
            if over {
                send_frames_surface_tree(&surface, output, frame_target, throttle, |_, _| {
                    Some(output.clone())
                });
            }
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
        let shown = self.space.output_geometry(output)?;
        let origin = shown.loc;
        let map = layer_map_for_output(output);
        let focused = self.focused_window();
        let lifted = crate::marks::lifted(focused.as_ref(), &self.space, output);
        let above_top = lifted.and_then(|window| {
            let location = self.space.element_location(window)? - window.geometry().loc;
            window.surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)?;
            Some((Under::Window(window.clone()), location))
        });
        // The top surface of `layer` with input at `pos`: one whose input region leaves `pos`
        // out lets the click through to what is below.
        let on = |layer| {
            map.layers_on(layer).rev().find_map(|surface| {
                let location = map.layer_geometry(surface)?.loc + origin;
                surface.surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)?;
                Some((Under::Layer(surface.clone()), location))
            })
        };
        let front = on(Layer::Overlay).or(above_top);
        // The black around a smaller client hides what is below as well.
        if lifted.is_some() {
            return front;
        }
        let above = front.or_else(|| on(Layer::Top));
        if above.is_some() {
            return above;
        }
        let window = self.space.element_under(pos);
        let black = self
            .space
            .elements()
            .rev()
            .take_while(|above| window.as_ref().is_none_or(|(window, _)| above != window))
            .any(|above| crate::marks::whole_on(above, shown));
        if black {
            return None;
        }
        window
            .map(|(window, location)| (Under::Window(window.clone()), location))
            .or_else(|| on(Layer::Bottom))
            .or_else(|| on(Layer::Background))
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

    /// The window whose toplevel is `surface`, mapped or minimised.
    pub fn window_or_minimised(&self, surface: &WlSurface) -> Option<Window> {
        self.windows()
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
            .cloned()
    }

    /// The window the foreign toplevel list names by `handle`, mapped or minimised.
    pub fn window_by_handle(&self, handle: &ForeignToplevelHandle) -> Option<Window> {
        self.windows()
            .find(|w| {
                w.user_data()
                    .get::<ForeignToplevelHandle>()
                    .is_some_and(|own| own.matches(handle))
            })
            .cloned()
    }

    /// Whether `window` is minimised.
    pub fn is_minimised(&self, window: &Window) -> bool {
        self.minimised.iter().any(|(kept, _)| kept == window)
    }

    /// The mapped windows, then the minimised ones, then those of the inactive workspaces.
    pub(crate) fn windows(&self) -> impl Iterator<Item = &Window> {
        self.space
            .elements()
            .chain(self.minimised.iter().map(|(window, _)| window))
            .chain(self.parked.iter().map(|(window, _)| window))
    }
}

/// What an output shows: the space with the input method's popups on top, or while the session
/// is locked only the lock surface the locker gave it, if any, on the background.
pub enum View<'a> {
    Space(&'a Space<Window>, &'a [PlacedPopup], &'a Marks),
    Locked(Option<&'a WlSurface>),
}

impl<'a> View<'a> {
    pub fn of(
        space: &'a Space<Window>,
        lock: Option<&'a Lock>,
        popups: &'a [PlacedPopup],
        marks: &'a Marks,
        output: &Output,
    ) -> Self {
        match lock {
            Some(lock) => View::Locked(lock.surface_on(output)),
            None => View::Space(space, popups, marks),
        }
    }
}

/// Draws `view` on `output` into `framebuffer`, `age` frames after its previous contents, as
/// `look` says of the magnifier and the colour filters.
pub fn draw_view<'d, R>(
    renderer: &mut R,
    framebuffer: &mut R::Framebuffer<'_>,
    output: &Output,
    age: usize,
    view: View<'_>,
    damage_tracker: &'d mut OutputDamageTracker,
    look: Look,
) -> Result<RenderOutputResult<'d>, DamageError<R::Error>>
where
    R: Filters,
    R::TextureId: Send + Clone + Texture + 'static,
    FilterElement<R::TextureId>: RenderElement<R>,
{
    let scale = output.current_scale().fractional_scale();
    let surface = |renderer: &mut R, surface: &WlSurface, at: Point<i32, Physical>| {
        render_elements_from_surface_tree::<_, Stacked<R>>(
            renderer,
            surface,
            at,
            scale,
            1.0,
            Kind::Unspecified,
        )
    };
    // The popups and the lock surface are surfaces as the space's are, above them.
    let mut elements: Vec<Stacked<R>> = Vec::new();
    match view {
        View::Space(space, popups, marks) => {
            for (popup, at) in popups {
                let at = at.to_physical_precise_round(scale);
                elements.extend(surface(renderer, popup, at));
            }
            elements.extend(crate::marks::stack(renderer, space, output, marks));
        }
        View::Locked(Some(lock)) => elements.extend(surface(renderer, lock, Point::default())),
        View::Locked(None) => {}
    }
    crate::look::draw(
        renderer,
        framebuffer,
        output,
        age,
        elements,
        damage_tracker,
        look,
    )
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
    /// Whether it is confined: it connected through a security context, as a sandboxed
    /// application does, or through xwayland-satellite's socket.
    confined: bool,
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
        .is_some_and(|data| !data.confined)
}

impl State {
    /// What a frame's marks are drawn from (`doc_own_stack.md` decision 31).
    pub fn marks(&self) -> Marks {
        Marks {
            focused: self.focused_window(),
            outline: self.focus_theme.outline(),
        }
    }
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

/// Where a window's geometry goes (its top left, the space's location of the element: the
/// space draws the buffer at it less the geometry's offset, so that offset is never added or
/// taken off here), at the commit that answers the configure of the serial, or
/// at its next commit without one (`State::relocated`).
#[derive(Default)]
pub struct Relocate(Cell<Option<Destination>>);

#[derive(Clone, Copy)]
struct Destination {
    /// `None` is where `placed` puts the window at the size it then has.
    to: Option<Point<i32, Logical>>,
    serial: Option<Serial>,
}

/// Whether the state `toplevel` committed answers the configure of `serial`, or a later one.
pub fn answered(toplevel: &ToplevelSurface, serial: Serial) -> bool {
    toplevel
        .with_cached_state(|state| state.last_acked.as_ref().map(|configure| configure.serial))
        .is_some_and(|acked| acked >= serial)
}

/// Asks for `window` to move its geometry to `to` at the commit that answers `serial`.
pub fn relocate(window: &Window, to: Point<i32, Logical>, serial: Option<Serial>) {
    window
        .user_data()
        .get_or_insert(Relocate::default)
        .0
        .set(Some(Destination {
            to: Some(to),
            serial,
        }));
}

/// `relocate` to where `placed` puts the window at the size it then has.
fn place_again(window: &Window, serial: Option<Serial>) {
    window
        .user_data()
        .get_or_insert(Relocate::default)
        .0
        .set(Some(Destination { to: None, serial }));
}

/// A maximised window's geometry before it was maximised, and the area it was maximised to.
#[derive(Default)]
struct Maximised(Cell<Option<Kept>>);

/// As `Maximised`, for a full-screen window and its output.
#[derive(Default)]
struct FullScreen(Cell<Option<Kept>>);

/// The output `window` has whole, from the request to the request that ends it
/// (`State::full_screen`).
pub fn full_screen_area(window: &Window) -> Option<Rectangle<i32, Logical>> {
    let full = window.user_data().get::<FullScreen>()?;
    Some(full.0.get()?.area)
}

pub fn is_full_screen(window: &Window) -> bool {
    full_screen_area(window).is_some()
}

#[derive(Clone, Copy)]
struct Kept {
    before: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
}

/// Where the geometry's top left goes for a window of `size` to be centred in `area`: a space
/// location, to which the geometry's offset in the buffer does not belong.
pub fn centred(area: Rectangle<i32, Logical>, size: Size<i32, Logical>) -> Point<i32, Logical> {
    let x = area.loc.x + ((area.size.w - size.w) / 2).max(0);
    let y = area.loc.y + ((area.size.h - size.h) / 2).max(0);
    (x, y).into()
}

/// `output`'s logical size rounded, at which the layer map lays it out.
pub(crate) fn laid_out(output: &Output) -> Option<Size<i32, Logical>> {
    output.current_mode().map(|mode| {
        let size = mode
            .size
            .to_f64()
            .to_logical(output.current_scale().fractional_scale());
        output
            .current_transform()
            .transform_size(size.to_i32_round())
    })
}

/// Where a new window of `geometry` is mapped in `area`, `State::placement_area`, as cosmic-comp
/// places the first floating window it maps with no place of its own
/// (`src/shell/layout/floating/mod.rs`, `map_internal`): each half floored on its own, the
/// window's middle at the area's across, and its top half the area's height less half the
/// window's below the area's top, but no lower than an eighth of that height. Its top left is
/// kept inside, where cosmic-comp would shrink a window larger than the area, and every step
/// saturates, so a client's geometry, however large, cannot overflow. The point is the
/// geometry's top left: the space draws the buffer at it less the geometry's offset in the
/// buffer (smithay's `render_location`), so that offset, a client's shadow, is not taken off.
pub fn placed(
    area: Rectangle<i32, Logical>,
    geometry: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let half = |of: i32, window: i32| (of / 2).saturating_sub(window / 2).max(0);
    let across = half(area.size.w, geometry.size.w);
    let down = half(area.size.h, geometry.size.h).min(area.size.h / 8);
    (
        area.loc.x.saturating_add(across),
        area.loc.y.saturating_add(down),
    )
        .into()
}

/// `area`, the output's rectangle in the space, less the insets the layer map's `zone` leaves of
/// the output laid out at `laid_out`. Each inset is kept inside the output, so that a client's
/// exclusive zone, however large, cannot place windows outside it or overflow; zones that leave
/// no room at all are ignored, and windows take the whole output.
pub(crate) fn inset(
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
    use super::{inset, placed, GrabSerials};
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

    /// A new window is placed as cosmic-comp places it: centred across, and no lower than an
    /// eighth of the area's height below its top (#738).
    #[test]
    fn a_new_window_is_placed_as_cosmic_comp_places_it() {
        let output = rect(0, 0, 1280, 800);
        // Short: an eighth down, 100, where centring would put it at 350.
        assert_eq!(placed(output, rect(0, 0, 200, 100)), (540, 100).into());
        // Tall: centred, 50, above an eighth.
        assert_eq!(placed(output, rect(0, 0, 200, 700)), (540, 50).into());
        // Below a bar of 32; the geometry's offset in its buffer is not taken off.
        assert_eq!(
            placed(rect(0, 32, 1280, 768), rect(10, 20, 200, 100)),
            (540, 128).into()
        );
        // Larger than the area: its top left kept inside.
        assert_eq!(placed(output, rect(0, 0, 1400, 900)), (0, 0).into());
        // Each half floored on its own: 640 - 100 and 400 - 350, where halving the difference
        // gives 539 and 49.
        assert_eq!(placed(output, rect(0, 0, 201, 701)), (540, 50).into());
        // The layer map's 853 by 533 of 1280 by 800 px at 1.5: the chooser's 420 at 216, as on
        // cosmic-comp's golden chooser-light-1.5-en (324 px), where the space's 854 gives 217.
        assert_eq!(
            placed(rect(0, 0, 853, 533), rect(0, 0, 420, 100)),
            (216, 66).into()
        );
        // An area or a geometry at the ends of i32 does not overflow.
        assert_eq!(
            placed(rect(i32::MAX, i32::MAX, 1280, 800), rect(0, 0, 200, 100)),
            (i32::MAX, i32::MAX).into()
        );
        assert_eq!(
            placed(output, rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)),
            (0, 0).into()
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
