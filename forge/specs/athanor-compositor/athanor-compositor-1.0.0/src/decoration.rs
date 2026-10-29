//! zxdg_decoration_manager_v1 (doc_own_stack.md OS17 and decision 30, standard): every window
//! is decorated by its client. The compositor draws no title bar, so whatever mode a client
//! asks for, or none, it is configured with client-side decoration, and a client that asks for
//! a title bar draws its own, as under GNOME.

use crate::state::State;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::wayland::shell::xdg::decoration::{XdgDecorationHandler, XdgDecorationState};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData};

/// Offers the global to every client.
pub fn global(dh: &DisplayHandle) {
    XdgDecorationState::new::<State>(dh);
}

/// How many configures of any kind (resizes and state changes the compositor sends, and the
/// answers to decoration requests) a client may leave unacknowledged before a request is no
/// longer answered at once. It is the bound on the queue Smithay keeps per window, which a
/// client that sends requests and never acknowledges would otherwise grow without limit.
const UNACKED_CONFIGURES: usize = 8;

/// Configures `toplevel` with client-side decoration, and returns whether it did. Before the
/// first configure the mode travels with it. After, a configure is sent now whatever the state
/// was, which is the answer to a request: Smithay sends the decoration's `configure` only when
/// the mode differs from the one of the last configure sent, so a client that repeats a request,
/// or makes its decoration object again, would wait for it. The pending mode is therefore set
/// to whichever of "client-side" and "none" differs from the last sent; Smithay sends
/// client-side for both. With `UNACKED_CONFIGURES` unacknowledged the answer is not sent, and
/// the caller owes it (`State::decoration_acked`).
fn client_side(toplevel: &ToplevelSurface) -> bool {
    if !toplevel.is_initial_configure_sent() {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(Mode::ClientSide));
        return true;
    }
    let (sent, unacked) = with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok())
            .map(|data| {
                (
                    data.current_server_state().decoration_mode,
                    data.pending_configures().len(),
                )
            })
            .unwrap_or((None, 0))
    });
    if unacked >= UNACKED_CONFIGURES {
        return false;
    }
    toplevel.with_pending_state(|state| {
        state.decoration_mode = match sent {
            Some(Mode::ClientSide) => None,
            _ => Some(Mode::ClientSide),
        }
    });
    toplevel.send_configure();
    true
}

impl State {
    /// Answers a decoration request now, or remembers that it is owed.
    fn answer_decoration(&mut self, toplevel: &ToplevelSurface) {
        let surface = toplevel.wl_surface();
        if client_side(toplevel) {
            self.decoration_owed.retain(|owed| owed != surface);
        } else if !self.decoration_owed.contains(surface) {
            self.decoration_owed.retain(WlSurface::is_alive);
            self.decoration_owed.push(surface.clone());
        }
    }

    /// `surface` acknowledged a configure: the answer it was owed, if any, is sent now.
    pub(crate) fn decoration_acked(&mut self, surface: &WlSurface) {
        if !self.decoration_owed.contains(surface) {
            return;
        }
        let toplevel = self
            .xdg_shell_state
            .toplevel_surfaces()
            .iter()
            .find(|toplevel| toplevel.wl_surface() == surface)
            .cloned();
        match toplevel {
            Some(toplevel) => self.answer_decoration(&toplevel),
            None => self.decoration_owed.retain(|owed| owed != surface),
        }
    }
}

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        self.answer_decoration(&toplevel);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: Mode) {
        self.answer_decoration(&toplevel);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.answer_decoration(&toplevel);
    }
}
