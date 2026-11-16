//! wp_tearing_control_manager_v1 (doc_own_stack.md decision 33, wave 2b, `standard`): a surface's
//! presentation hint, kept in its cached state and applied with the commit after the request, as
//! the protocol makes it double-buffered.
//!
//! A frame of an output may honour a surface's `async` hint only when every condition of
//! decision 33 holds (`tears`): the surface is full-screen and alone on its output, no layer
//! surface drawn above it is mapped on that output, the session is not locked, and for a
//! confined client OS4's border is drawn in the same frame. Above it are the overlay layer, and
//! the top layer unless the window is lifted above it (`marks::lifted`: focused, with the output
//! whole and topmost); the background and bottom layers are always below. The DRM backend reads
//! the conditions from each frame it is about to draw (`conditions`), so a flip in flight when
//! the lock or such a layer arrives completes, and the next frame is presented with vsync
//! (amendment of 2026-11-16).
//!
//! Smithay's DRM backend has no asynchronous page flip at the pinned revision, so every frame
//! is presented with vsync, which the protocol allows: the compositor is free to ignore the
//! hint. The flip is to be proposed to Smithay first (decision 33); until it lands the DRM
//! backend logs, at debug level, each time the conditions start or stop holding.

use crate::input_method::PlacedPopup;
use crate::lock::Lock;
use crate::marks::Marks;
use crate::state::{unconfined, State};
use smithay::backend::renderer::element::Element;
use smithay::desktop::{layer_map_for_output, PopupManager, Space, Window};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::wp::tearing_control::v1::server::{
    wp_tearing_control_manager_v1::{self, WpTearingControlManagerV1},
    wp_tearing_control_v1::{self, PresentationHint, WpTearingControlV1},
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, DisplayHandle, New, Resource, WEnum, Weak,
};
use smithay::wayland::compositor::{with_states, Cacheable};
use smithay::wayland::shell::wlr_layer::Layer;
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::cell::Cell;

/// A surface's presentation hint, double-buffered: `true` when its last applied hint is `async`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Hint(pub bool);

impl Cacheable for Hint {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        *self
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// Whether a surface has a wp_tearing_control_v1 object.
#[derive(Default)]
struct Attached(Cell<bool>);

/// The data of the global and of every manager object.
pub struct Manager;

/// The data of a wp_tearing_control_v1 object: its surface, which can go before it.
pub struct Control(Weak<WlSurface>);

/// Offers the manager to every client.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, WpTearingControlManagerV1, _>(1, Manager);
}

impl GlobalDispatch2<WpTearingControlManagerV1, State> for Manager {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WpTearingControlManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, Manager);
    }
}

impl Dispatch2<WpTearingControlManagerV1, State> for Manager {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        manager: &WpTearingControlManagerV1,
        request: wp_tearing_control_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use wp_tearing_control_manager_v1::Request;
        match request {
            Request::GetTearingControl { id, surface } => {
                let taken = with_states(&surface, |states| {
                    let attached = states.data_map.get_or_insert(Attached::default);
                    attached.0.replace(true)
                });
                if taken {
                    manager.post_error(
                        wp_tearing_control_manager_v1::Error::TearingControlExists,
                        "the surface already has a wp_tearing_control_v1",
                    );
                }
                data_init.init(id, Control(surface.downgrade()));
            }
            Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch2<WpTearingControlV1, State> for Control {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _control: &WpTearingControlV1,
        request: wp_tearing_control_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use wp_tearing_control_v1::Request;
        // Once its surface is gone the object is inert.
        let Ok(surface) = self.0.upgrade() else {
            return;
        };
        match request {
            Request::SetPresentationHint { hint } => {
                // The description names no error for a value outside the enum: it changes
                // nothing.
                let WEnum::Value(hint) = hint else {
                    return;
                };
                with_states(&surface, |states| {
                    states.cached_state.get::<Hint>().pending().0 = hint == PresentationHint::Async;
                });
            }
            // The hint goes back to vsync with the surface's next commit.
            Request::Destroy => with_states(&surface, |states| {
                if let Some(attached) = states.data_map.get::<Attached>() {
                    attached.0.set(false);
                }
                states.cached_state.get::<Hint>().pending().0 = false;
            }),
            _ => {}
        }
    }
}

/// Whether `surface`'s applied hint is `async`.
pub fn asks(surface: &WlSurface) -> bool {
    with_states(surface, |states| {
        states.cached_state.get::<Hint>().current().0
    })
}

/// What decision 33 reads of a frame of an output before it may honour a hint.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Conditions {
    /// The surface's applied hint is `async`.
    pub asked: bool,
    /// The surface is full-screen and alone on its output.
    pub alone: bool,
    /// A layer surface drawn above the surface is mapped on that output.
    pub layered: bool,
    /// The session is locked.
    pub locked: bool,
    /// The surface's client is confined.
    pub confined: bool,
    /// OS4's border of the surface is drawn in the frame.
    pub bordered: bool,
}

/// Whether the frame may honour the hint: only when every condition of decision 33 holds.
pub fn tears(c: Conditions) -> bool {
    c.asked && c.alone && !c.layered && !c.locked && (!c.confined || c.bordered)
}

/// The conditions of the frame of `output` made of `elements`, drawn under `lock` if any, with
/// the input method's `popups` above the space and the `marks` the frame was stacked with: the
/// DRM backend passes what it draws the frame from, and the fixture what its frames are drawn
/// from, so that both read the same state. The surface is
/// the toplevel of the window that has the output whole (`marks::whole_on`), or else of the
/// topmost window on the output; it is alone when no other window meets the output and no popup
/// is shown. With no window, nothing asks.
pub fn conditions<E: Element>(
    space: &Space<Window>,
    output: &Output,
    lock: Option<&Lock>,
    popups: &[PlacedPopup],
    marks: &Marks,
    elements: &[E],
) -> Conditions {
    let locked = lock.is_some();
    let nothing = Conditions {
        layered: above(output, false),
        locked,
        ..Conditions::default()
    };
    let Some(area) = space.output_geometry(output) else {
        return nothing;
    };
    let on: Vec<&Window> = space
        .elements()
        .filter(|window| {
            space
                .element_bbox(window)
                .is_some_and(|bbox| area.overlaps(bbox))
        })
        .collect();
    let whole = on
        .iter()
        .rev()
        .find(|window| crate::marks::whole_on(window, area));
    let Some((window, surface)) = whole
        .or(on.last())
        .and_then(|window| Some((window, window.toplevel()?.wl_surface().clone())))
    else {
        return nothing;
    };
    let lifted = crate::marks::lifted(marks.focused.as_ref(), space, output) == Some(*window);
    let alone = on.len() == 1
        && crate::marks::whole_on(window, area)
        && popups.is_empty()
        && PopupManager::popups_for_surface(&surface).next().is_none();
    Conditions {
        asked: asks(&surface),
        alone,
        layered: above(output, lifted),
        locked,
        confined: surface.client().is_none_or(|client| !unconfined(&client)),
        bordered: crate::marks::bordered(&surface, elements, area, output),
    }
}

/// Whether a layer surface mapped on `output` is drawn above its windows: one on the overlay
/// layer, or on the top layer unless the window is `lifted` above it, as `marks::stack` draws.
fn above(output: &Output, lifted: bool) -> bool {
    layer_map_for_output(output)
        .layers()
        .any(|layer| match layer.layer() {
            Layer::Overlay => true,
            Layer::Top => !lifted,
            Layer::Background | Layer::Bottom => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full-screen confined surface alone on its output, asking for async, bordered.
    fn met() -> Conditions {
        Conditions {
            asked: true,
            alone: true,
            layered: false,
            locked: false,
            confined: true,
            bordered: true,
        }
    }

    #[test]
    fn every_condition_met_tears() {
        assert!(tears(met()));
        let unconfined = Conditions {
            confined: false,
            bordered: false,
            ..met()
        };
        assert!(tears(unconfined), "an unconfined client needs no border");
    }

    #[test]
    fn vsync_is_never_torn() {
        assert!(!tears(Conditions {
            asked: false,
            ..met()
        }));
    }

    #[test]
    fn a_surface_not_alone_and_full_screen_is_not_torn() {
        assert!(!tears(Conditions {
            alone: false,
            ..met()
        }));
    }

    #[test]
    fn a_mapped_layer_surface_refuses_the_tear() {
        assert!(!tears(Conditions {
            layered: true,
            ..met()
        }));
    }

    #[test]
    fn the_lock_refuses_the_tear() {
        assert!(!tears(Conditions {
            locked: true,
            ..met()
        }));
    }

    #[test]
    fn a_confined_client_without_its_border_is_not_torn() {
        assert!(!tears(Conditions {
            bordered: false,
            ..met()
        }));
    }
}
