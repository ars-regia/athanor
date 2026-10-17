//! wlr-layer-shell (doc_own_stack.md, C1 task 3): the shell's surfaces on the four layers of an
//! output. Each output's layer map places them and keeps their exclusive zones; windows are
//! placed in what the zones leave (`State::window_area`). The keyboard follows each surface's
//! `keyboard_interactivity`, as register row 1 of CO3 asks: an exclusive surface on the top or
//! overlay layer holds the keyboard while it is mapped, an on-demand one takes it when clicked,
//! and one with none never has it.

use crate::state::State;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::{layer_map_for_output, LayerSurface, WindowSurfaceType};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface};
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::shell::wlr_layer::{
    KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, WlrLayerShellHandler,
    WlrLayerShellState,
};
use smithay::wayland::shell::xdg::PopupSurface;

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    /// Maps the surface on the output the client names, or on the first one.
    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());
        let Some(output) = output else {
            // Nothing to show it on: the protocol's answer is `closed`.
            surface.send_close();
            return;
        };
        let mut map = layer_map_for_output(&output);
        if let Err(error) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            tracing::warn!(%error, "a layer surface could not be mapped");
        }
    }

    /// An xdg_popup given a layer surface as its parent: kept inside the output from now, when
    /// its parent is known.
    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let wl_surface = surface.wl_surface();
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            if let Some(layer) = map
                .layer_for_surface(wl_surface, WindowSurfaceType::TOPLEVEL)
                .cloned()
            {
                map.unmap_layer(&layer);
            }
        }
        self.refocus(wl_surface);
    }
}

impl State {
    /// A commit of a layer surface: its output's layers are arranged again, a configure goes
    /// out when its size changed or it has had none, and the keyboard follows its
    /// interactivity.
    pub fn layer_commit(&mut self, surface: &WlSurface) {
        let Some(layer) = self.space.outputs().find_map(|output| {
            let mut map = layer_map_for_output(output);
            let layer = map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .cloned()?;
            map.arrange();
            Some(layer)
        }) else {
            return;
        };
        layer.layer_surface().send_pending_configure();

        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let focused = keyboard.current_focus().as_ref() == Some(surface);
        if holds_exclusively(&layer) {
            if !focused {
                keyboard.set_focus(self, Some(surface.clone()), SERIAL_COUNTER.next_serial());
            }
        } else if focused && !(layer.can_receive_keyboard_focus() && mapped(surface)) {
            self.refocus(surface);
        }
    }

    /// The surface that holds the keyboard exclusively: the top one of the mapped exclusive
    /// surfaces of the overlay layer, else of the top layer.
    // ponytail: the outputs in order, not the one the user is on; per output with several.
    pub fn exclusive_layer(&self) -> Option<WlSurface> {
        [Layer::Overlay, Layer::Top].into_iter().find_map(|layer| {
            self.space.outputs().find_map(|output| {
                layer_map_for_output(output)
                    .layers_on(layer)
                    .rev()
                    .find(|surface| holds_exclusively(surface))
                    .map(|surface| surface.wl_surface().clone())
            })
        })
    }

    /// A click on `layer`: it takes the keyboard if it asks for it on demand and no surface
    /// holds the keyboard exclusively.
    pub fn focus_layer(&mut self, layer: &LayerSurface) {
        if !layer.can_receive_keyboard_focus() || self.exclusive_layer().is_some() {
            return;
        }
        if let Some(keyboard) = self.seat.get_keyboard() {
            let surface = layer.wl_surface().clone();
            keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
        }
    }
}

/// Whether `layer` holds the keyboard while it is mapped: exclusive interactivity, on the top
/// or overlay layer. On the bottom and background layers it takes the keyboard on demand.
fn holds_exclusively(layer: &LayerSurface) -> bool {
    layer.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive
        && matches!(layer.layer(), Layer::Top | Layer::Overlay)
        && mapped(layer.wl_surface())
}

/// Whether `surface` shows a buffer.
fn mapped(surface: &WlSurface) -> bool {
    with_renderer_surface_state(surface, |state| state.buffer().is_some()).unwrap_or(false)
}
