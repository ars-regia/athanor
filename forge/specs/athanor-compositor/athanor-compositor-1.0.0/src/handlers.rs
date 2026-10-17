//! Smithay's handler traits: what the compositor does when a client commits, maps a window,
//! opens a popup, imports a dmabuf or asks for activation.

use crate::state::{ClientState, State};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::backend::renderer::ImportDma;
use smithay::desktop::{find_popup_root_surface, get_popup_toplevel_coords, PopupKind, Window};
use smithay::input::tablet::TabletSeatHandler;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::Client;
use smithay::utils::{Serial, SERIAL_COUNTER};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    get_parent, is_sync_subsurface, with_states, CompositorClientState, CompositorHandler,
    CompositorState,
};
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::fractional_scale::{with_fractional_scale, FractionalScaleHandler};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::pointer_constraints::PointerConstraintsHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};
use std::sync::LazyLock;
use std::time::Duration;

/// How long an activation token stays good.
const ACTIVATION_TOKEN_LIFETIME: Duration = Duration::from_secs(10);

/// Marks a window the compositor has placed: placement happens once, at its first sized commit.
struct Placed;

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // Every client connects through `listen`, which gives it a ClientState. A client
        // without one cannot exist; were it to, it would share this state rather than end the
        // compositor.
        static UNKNOWN: LazyLock<CompositorClientState> = LazyLock::new(Default::default);
        match client.get_data::<ClientState>() {
            Some(data) => &data.compositor_state,
            None => &UNKNOWN,
        }
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for(&root) {
                window.on_commit();
                self.place(&window);
            }
        }

        // The first commit of a toplevel or a popup gets its initial configure.
        if let Some(toplevel) = self.window_for(surface).as_ref().and_then(Window::toplevel) {
            if !toplevel.is_initial_configure_sent() {
                toplevel.send_configure();
            }
        }
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) {
            if !popup.is_initial_configure_sent() {
                if let Err(error) = popup.send_configure() {
                    tracing::warn!(%error, "a popup's initial configure failed");
                }
            }
        }
    }
}

impl State {
    /// Centres a window on the output at its first commit with a size, activates it and gives
    /// it the keyboard.
    fn place(&mut self, window: &Window) {
        let geometry = window.geometry();
        if geometry.size.w <= 0 || geometry.size.h <= 0 {
            return;
        }
        let Some(area) = self.space.output_geometry(&self.output) else {
            return;
        };
        if !window.user_data().insert_if_missing(|| Placed) {
            return;
        }
        let x = area.loc.x + ((area.size.w - geometry.size.w) / 2).max(0) - geometry.loc.x;
        let y = area.loc.y + ((area.size.h - geometry.size.h) / 2).max(0) - geometry.loc.y;
        self.space.map_element(window.clone(), (x, y), true);
        self.focus(window);
    }

    /// Raises `window`, activates it and gives it the keyboard; the others are deactivated.
    pub fn focus(&mut self, window: &Window) {
        self.space.raise_element(window, true);
        for other in self.space.elements() {
            other.set_activated(other == window);
            if let Some(toplevel) = other.toplevel() {
                toplevel.send_pending_configure();
            }
        }
        if let (Some(keyboard), Some(toplevel)) = (self.seat.get_keyboard(), window.toplevel()) {
            let surface = toplevel.wl_surface().clone();
            keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
        }
    }

    /// Keeps a popup inside the output.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for(&root) else {
            return;
        };
        let (Some(output), Some(window)) = (
            self.space.output_geometry(&self.output),
            self.space.element_geometry(&window),
        ) else {
            return;
        };
        // Relative to the popup's parent.
        let mut target = output;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // Mapped now, placed at its first sized commit.
        self.space
            .map_element(Window::new_wayland_window(surface), (0, 0), false);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(error) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!(%error, "a popup could not be tracked");
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    // ponytail: no popup grab; menus that need one arrive with the shell's popovers (task 3).
    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
}

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

// cursor-shape-v1 serves tablet tools too.
impl TabletSeatHandler for State {
    type ToolFocus = WlSurface;
}

impl OutputHandler for State {}

// A pointer target needs it; no client gets a lock or a confinement yet.
impl PointerConstraintsHandler for State {}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        match self.backend.renderer().import_dmabuf(&dmabuf, None) {
            Ok(_) => {
                if let Err(error) = notifier.successful::<Self>() {
                    tracing::warn!(%error, "an imported dmabuf has no buffer");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "a dmabuf could not be imported");
                notifier.failed();
            }
        }
    }
}

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self.output.current_scale().fractional_scale();
        with_states(&surface, |states| {
            with_fractional_scale(states, |fractional| fractional.set_preferred_scale(scale));
        });
    }
}

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    /// A token is good only when it carries a serial of this seat's keyboard no older than its
    /// last focus: a client cannot take the focus without the user's input.
    fn token_created(&mut self, _token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        let Some((serial, seat)) = data.serial else {
            return false;
        };
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        Seat::from_resource(&seat).as_ref() == Some(&self.seat)
            && keyboard
                .last_enter()
                .is_some_and(|last| serial.is_no_older_than(&last))
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if data.timestamp.elapsed() < ACTIVATION_TOKEN_LIFETIME {
            if let Some(window) = self.window_for(&surface) {
                self.focus(&window);
            }
        }
    }
}

smithay::delegate_dispatch2!(State);
