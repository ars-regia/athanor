//! The compositor's state: the protocol globals, the space of windows, the seat, the output and
//! the backend that draws it.

use crate::Error;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::ImportDma;
use smithay::backend::winit::WinitGraphicsBackend;
use smithay::desktop::{PopupManager, Space, Window, WindowSurfaceType};
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, LoopSignal, PostAction};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{Clock, Logical, Monotonic, Point, Serial, Transform};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufState};
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::xdg_activation::XdgActivationState;
use std::ffi::OsString;
use std::sync::Arc;

pub struct State {
    pub display_handle: DisplayHandle,
    pub socket_name: OsString,
    pub loop_signal: LoopSignal,
    pub clock: Clock<Monotonic>,

    pub space: Space<Window>,
    pub popups: PopupManager,
    pub seat: Seat<Self>,
    pub output: Output,
    /// The serial of the newest key or button event sent to a client.
    pub last_input_serial: Option<Serial>,
    pub backend: WinitGraphicsBackend<GlesRenderer>,
    pub damage_tracker: OutputDamageTracker,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub dmabuf_state: DmabufState,
    pub xdg_activation_state: XdgActivationState,
}

impl State {
    pub fn new(
        event_loop: &mut EventLoop<Self>,
        display: Display<Self>,
        mut backend: WinitGraphicsBackend<GlesRenderer>,
        scale: f64,
    ) -> Result<Self, Error> {
        let dh = display.handle();
        let clock = Clock::new();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        // The globals below keep no state the compositor reads back.
        OutputManagerState::new_with_xdg_output::<Self>(&dh);
        ViewporterState::new::<Self>(&dh);
        FractionalScaleManagerState::new::<Self>(&dh);
        PresentationState::new::<Self>(&dh, clock.id() as u32);
        CursorShapeManagerState::new::<Self>(&dh);
        let xdg_activation_state = XdgActivationState::new::<Self>(&dh);
        let (dmabuf_state, _) = dmabuf(&dh, &mut backend);

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        seat.add_keyboard(Default::default(), 200, 25)?;
        seat.add_pointer();

        // One output, the size of the window the parent compositor gives us.
        let output = Output::new(
            "athanor-0".into(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Athanor".into(),
                model: "Nested".into(),
                serial_number: String::new(),
            },
        );
        output.create_global::<Self>(&dh);
        let mode = Mode {
            size: backend.window_size(),
            refresh: 60_000,
        };
        // Flipped180: the winit backend's framebuffer is upside down.
        output.change_current_state(
            Some(mode),
            Some(Transform::Flipped180),
            Some(Scale::Fractional(scale)),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        let mut space = Space::default();
        space.map_output(&output, (0, 0));
        let damage_tracker = OutputDamageTracker::from_output(&output);

        let socket_name = listen(event_loop, display)?;

        Ok(Self {
            display_handle: dh,
            socket_name,
            loop_signal: event_loop.get_signal(),
            clock,
            space,
            popups: PopupManager::default(),
            seat,
            output,
            last_input_serial: None,
            backend,
            damage_tracker,
            compositor_state,
            xdg_shell_state,
            shm_state,
            seat_state,
            dmabuf_state,
            xdg_activation_state,
        })
    }

    /// The surface under `pos`, with its origin in the space.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space
            .element_under(pos)
            .and_then(|(window, location)| {
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
    }

    /// The mapped window whose toplevel is `surface`.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
            .cloned()
    }
}

/// linux-dmabuf version 3 with the renderer's formats.
// ponytail: no version 4 feedback; it names the render node, which Smithay finds only with
// backend_drm, and it arrives with the DRM backend.
fn dmabuf(
    dh: &DisplayHandle,
    backend: &mut WinitGraphicsBackend<GlesRenderer>,
) -> (DmabufState, DmabufGlobal) {
    let mut state = DmabufState::new();
    let global = state.create_global::<State>(dh, backend.renderer().dmabuf_formats());
    (state, global)
}

/// Listens on the next free `wayland-N` socket and dispatches the clients' requests.
fn listen(
    event_loop: &mut EventLoop<State>,
    mut display: Display<State>,
) -> Result<OsString, Error> {
    let fd = display.backend().poll_fd().try_clone_to_owned()?;
    let socket = ListeningSocketSource::new_auto()?;
    let name = socket.socket_name().to_os_string();
    let handle = event_loop.handle();
    handle
        .insert_source(socket, |stream, _, state| {
            if let Err(error) = state
                .display_handle
                .insert_client(stream, Arc::new(ClientState::default()))
            {
                tracing::warn!(%error, "a client could not connect");
            }
        })
        .map_err(|e| e.error)?;
    handle
        .insert_source(
            Generic::new(fd, Interest::READ, smithay::reexports::calloop::Mode::Level),
            // The callback owns the display: it dispatches without the unsafe access a source
            // holding it would need.
            move |_, _, state| {
                display.dispatch_clients(state)?;
                if let Err(error) = state.display_handle.flush_clients() {
                    tracing::warn!(%error, "the clients could not be flushed");
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| e.error)?;
    Ok(name)
}

/// What the compositor keeps per client.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
