//! The compositor's state: the protocol globals, the space of windows and its outputs, the seat
//! and the backend that draws them.

use crate::handlers::HeldCommit;
use crate::headless::{FrameStats, Headless};
use crate::winit::Nested;
use crate::Error;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::element::{
    default_primary_scanout_output_compare, RenderElementStates,
};
use smithay::backend::renderer::ImportDma;
use smithay::desktop::utils::{
    surface_presentation_feedback_flags_from_states, surface_primary_scanout_output,
    update_surface_primary_scanout_output, OutputPresentationFeedback,
};
use smithay::desktop::{
    layer_map_for_output, LayerSurface, PopupManager, Space, Window, WindowSurfaceType,
};
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::channel::{self, Channel};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, LoopHandle, LoopSignal, PostAction};
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{
    Clock, Logical, Monotonic, Physical, Point, Rectangle, Serial, Size, Time, Transform,
};
use smithay::wayland::compositor::{CompositorClientState, CompositorState, SurfaceData};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::drm_syncobj::DrmSyncobjState;
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::presentation::{PresentationState, Refresh};
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::wlr_layer::{Layer, WlrLayerShellState};
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::xdg_activation::XdgActivationState;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

/// What draws the outputs.
pub enum Backend {
    /// A window of a parent compositor.
    Nested(Box<Nested>),
    /// Virtual outputs drawn off screen.
    Headless(Box<Headless>),
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
    /// The clients' frame times, kept on the headless backend.
    pub frame_stats: Option<FrameStats>,
    /// The commits waiting on a fence, each with the event source that waits on it.
    pub held: Vec<HeldCommit>,
    /// The sequence number of the next held commit.
    pub next_held: u64,
    /// Given to each client's `ClientState`, which sends its id on disconnection.
    pub client_gone: channel::Sender<ClientId>,
}

/// The serials a popup grab may name, as mutter takes them: the last button press, sent to the
/// client under the pointer, and the last key press and release, sent to the focused client.
#[derive(Debug, Default)]
pub struct GrabSerials {
    pub button: Option<Serial>,
    pub key_press: Option<Serial>,
    pub key_release: Option<Serial>,
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
        // ponytail: every client may bind it; task 5 grants the top and overlay layers by identity (OS4).
        // Task 5 must also give the compositor an escape from exclusive keyboard focus: today a
        // 1x1 transparent exclusive overlay surface holds the keyboard for as long as it lives,
        // which is acceptable only while the compositor runs nested or headless.
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

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        seat.add_keyboard(Default::default(), 200, 25)?;
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
        })
    }

    /// What the compositor keeps for a client that connects now.
    pub fn client_state(&self) -> Arc<ClientState> {
        Arc::new(ClientState {
            compositor_state: CompositorClientState::default(),
            gone: self.client_gone.clone(),
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
        }
        Ok(())
    }

    /// After a frame of `output`: the output each surface is shown on, presentation feedback
    /// when the frame changed, and the frame callbacks.
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
        let scanout = |surface: &WlSurface, surface_states: &SurfaceData| {
            update_surface_primary_scanout_output(
                surface,
                output,
                surface_states,
                None,
                states,
                default_primary_scanout_output_compare,
            );
        };
        for window in self.space.elements() {
            window.with_surfaces(scanout);
        }
        for layer in &layers {
            layer.with_surfaces(scanout);
        }
        if damaged {
            let mut feedback = OutputPresentationFeedback::new(output);
            let flags = |surface: &WlSurface, _: &SurfaceData| {
                surface_presentation_feedback_flags_from_states(surface, None, states)
            };
            for window in self.space.elements() {
                window.take_presentation_feedback(
                    &mut feedback,
                    surface_primary_scanout_output,
                    flags,
                );
            }
            for layer in &layers {
                layer.take_presentation_feedback(
                    &mut feedback,
                    surface_primary_scanout_output,
                    flags,
                );
            }
            feedback.presented(
                frame_target,
                Refresh::fixed(period),
                0,
                wp_presentation_feedback::Kind::Vsync,
            );
        }
        // A surface on no output yet, or shown on another one, still gets a frame each second.
        let throttle = Some(Duration::from_secs(1));
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
    }

    /// What is under `pos`, with its origin in the space, in the order the outputs are drawn
    /// from the top: the overlay and top layers, the windows, the bottom and background layers.
    pub fn under(&self, pos: Point<f64, Logical>) -> Option<(Under, Point<i32, Logical>)> {
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
    output.create_global::<State>(dh);
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
            if let Err(error) = state
                .display_handle
                .insert_client(stream, state.client_state())
            {
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
    use super::inset;
    use smithay::utils::{Logical, Rectangle};

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
}
