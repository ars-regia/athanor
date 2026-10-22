//! wlr-layer-shell (doc_own_stack.md, C1 task 3): the shell's surfaces on the four layers of an
//! output. Each output's layer map places them and keeps their exclusive zones; windows are
//! placed in what the zones leave (`State::window_area`). The keyboard follows each surface's
//! `keyboard_interactivity`, as register row 1 of CO3 asks: an exclusive surface on the top or
//! overlay layer holds the keyboard while it is mapped, an on-demand one takes it when clicked,
//! and one with none never has it. A lock takes the keyboard from an exclusive
//! surface (`escape_exclusive_holder`), and the layers of an output that goes are closed
//! (`close_layers`).

use crate::identity::Privilege;
use crate::state::{granted, State};
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::{
    find_popup_root_surface, layer_map_for_output, LayerSurface, WindowSurfaceType,
};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface};
use smithay::reexports::wayland_server::Resource;
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
        if self.refuse_top_layers(surface) {
            return;
        }
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
        let focus = keyboard.current_focus();
        let focused = focus.as_ref() == Some(surface);
        if holds_exclusively(&layer) {
            // The surface holds the keyboard itself or through its own popup, whose grab
            // `grab` let it take: a commit leaves that grab alone.
            let held = focused
                || focus
                    .and_then(|focus| self.popups.find_popup(&focus))
                    .and_then(|popup| find_popup_root_surface(&popup).ok())
                    .is_some_and(|root| &root == surface);
            // Only the surface `exclusive_layer` picks: a top-layer client committing each frame
            // does not take the keyboard from an overlay one.
            if !held && self.exclusive_layer().as_ref() == Some(surface) {
                self.focus_exclusive(surface.clone());
            }
        } else if focused && !(layer.can_receive_keyboard_focus() && mapped(surface)) {
            self.refocus(surface);
        }
    }

    /// A surface on the top or overlay layer, whether asked for at its creation or by
    /// `set_layer` since, of a client not granted them (OS4): unmapped and closed. A layer takes
    /// effect at a commit, so a refused surface is never drawn there.
    fn refuse_top_layers(&mut self, surface: &WlSurface) -> bool {
        if surface
            .client()
            .is_some_and(|client| granted(&client, Privilege::TopLayers))
        {
            return false;
        }
        let refused = self.space.outputs().find_map(|output| {
            let mut map = layer_map_for_output(output);
            let layer = map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .cloned()?;
            if !matches!(layer.layer(), Layer::Top | Layer::Overlay) {
                return None;
            }
            map.unmap_layer(&layer);
            Some(layer)
        });
        let Some(layer) = refused else {
            return false;
        };
        tracing::info!(
            namespace = layer.namespace(),
            "a surface on the top layers of a client not granted them was closed"
        );
        layer.layer_surface().send_close();
        self.refocus(surface);
        true
    }

    /// Gives the keyboard to `surface`, the exclusive layer. A popup grab that holds the
    /// keyboard ends first: while it holds it, Smithay lets no other surface have the focus,
    /// and at its end hands the focus to the popup's root.
    pub fn focus_exclusive(&mut self, surface: WlSurface) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        if keyboard.is_grabbed() {
            keyboard.unset_grab(self);
        }
        self.set_keyboard_focus(Some(surface));
    }

    /// The surface that holds the keyboard exclusively: the top one of the mapped exclusive
    /// surfaces of the overlay layer, else of the top layer, of a client the escape key has not
    /// taken the keyboard from.
    // ponytail: the outputs in order, not the one the user is on; per output with several.
    pub fn exclusive_layer(&self) -> Option<WlSurface> {
        [Layer::Overlay, Layer::Top].into_iter().find_map(|layer| {
            self.space.outputs().find_map(|output| {
                layer_map_for_output(output)
                    .layers_on(layer)
                    .rev()
                    .find(|surface| {
                        holds_exclusively(surface) && !self.escaped(surface.wl_surface())
                    })
                    .map(|surface| surface.wl_surface().clone())
            })
        })
    }

    /// The session locks: the client whose exclusive top or overlay surface holds the keyboard is
    /// marked as escaped, since the lock takes the keyboard from it. The escape holds for the
    /// client, not the surface: no exclusive surface of it, remapped or new, takes the keyboard
    /// after the unlock until the user clicks on one of the client's layer surfaces that takes
    /// the keyboard (`click_focus`) or the client disconnects. A transparent exclusive surface
    /// that never ends cannot keep the keyboard from the user for longer than the lock.
    pub fn escape_exclusive_holder(&mut self) {
        let Some(client) = self
            .exclusive_layer()
            .and_then(|surface| surface.client())
            .map(|client| client.id())
        else {
            return;
        };
        if !self.escaped.contains(&client) {
            tracing::info!("the lock took the keyboard from an exclusive surface");
            self.escaped.push(client);
        }
    }

    /// An output that goes: its layer surfaces are closed, as the protocol asks, and the
    /// keyboard leaves them.
    // ponytail: closed, not moved to another output; a shell maps them again where it wants.
    pub fn close_layers(&mut self, output: &Output) {
        let layers: Vec<LayerSurface> = layer_map_for_output(output).layers().cloned().collect();
        for layer in &layers {
            layer_map_for_output(output).unmap_layer(layer);
            layer.layer_surface().send_close();
        }
        for layer in &layers {
            self.refocus(layer.wl_surface());
        }
    }

    /// Whether the lock took the keyboard from the client of `surface`.
    pub fn escaped(&self, surface: &WlSurface) -> bool {
        surface
            .client()
            .is_some_and(|client| self.escaped.contains(&client.id()))
    }

    /// Whether `surface` can hold the keyboard: a window can, and a layer surface unless its
    /// interactivity is `none`.
    pub fn takes_keyboard(&self, surface: &WlSurface) -> bool {
        self.space
            .outputs()
            .find_map(|output| {
                layer_map_for_output(output)
                    .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                    .map(|layer| layer.can_receive_keyboard_focus())
            })
            .unwrap_or(true)
    }

    /// A click on `layer`: it takes the keyboard if it asks for it on demand and no surface
    /// holds the keyboard exclusively. A click on a surface of the client whose surface holds
    /// it gives that surface the keyboard, which the escape key may have taken.
    pub fn focus_layer(&mut self, layer: &LayerSurface) {
        if !layer.can_receive_keyboard_focus() {
            return;
        }
        let client = |surface: &WlSurface| surface.client().map(|client| client.id());
        match self.exclusive_layer() {
            Some(exclusive) if client(&exclusive) == client(layer.wl_surface()) => {
                self.focus_exclusive(exclusive)
            }
            Some(_) => {}
            None => self.set_keyboard_focus(Some(layer.wl_surface().clone())),
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
