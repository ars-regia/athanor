//! idle-inhibit-unstable-v1 (doc_own_stack.md OS17, standard; doc_session.md SN13 step 6): a
//! surface the user sees keeps the idle notifications from firing, so a video holds off the
//! idle daemon's blank. As under cosmic-comp (doc_session_daemons.md section 1), an inhibitor
//! counts only while its surface is shown: a minimised or unmapped window, a dead client or a
//! locked session lets the timers run.

use crate::state::State;
use smithay::desktop::{layer_map_for_output, WindowSurfaceType};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::compositor::get_parent;
use smithay::wayland::idle_inhibit::IdleInhibitHandler;

// Neither request re-evaluates the inhibition: the event loop does once its dispatch is done, so
// a burst of requests costs one evaluation.
impl IdleInhibitHandler for State {
    fn inhibit(&mut self, surface: WlSurface) {
        *self.inhibitors.entry(surface).or_default() += 1;
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        // A surface with two inhibitors keeps the other.
        if let Some(count) = self.inhibitors.get_mut(&surface) {
            *count -= 1;
            if *count == 0 {
                self.inhibitors.remove(&surface);
            }
        }
    }
}

impl State {
    /// Inhibits the idle notifications while an inhibitor's surface is shown, and lets them run
    /// otherwise. The event loop calls it after every dispatch, since what is shown changes with
    /// commits, minimising, the lock and clients leaving; Smithay only reports the requests.
    pub fn refresh_idle_inhibit(&mut self) {
        // A client that leaves sends no `destroy`: its surfaces die with it.
        self.inhibitors.retain(|surface, _| surface.is_alive());
        // Each surface is looked at once, however many inhibitors it holds.
        let inhibited = self.lock.is_none()
            && self
                .inhibitors
                .keys()
                .any(|surface| self.surface_shown(surface));
        self.idle_notifier_state.set_is_inhibited(inhibited);
    }

    /// Whether `surface`, or the window or layer surface it is a subsurface of, is on an output
    /// and draws something: a layer surface is in the layer map from its creation, before any
    /// buffer.
    fn surface_shown(&self, surface: &WlSurface) -> bool {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if let Some(window) = self.window_for(&root) {
            return crate::handlers::shown(&window)
                && !self.space.outputs_for_element(&window).is_empty();
        }
        self.space.outputs().any(|output| {
            layer_map_for_output(output)
                .layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                .is_some()
        }) && crate::layer::mapped(&root)
    }
}
