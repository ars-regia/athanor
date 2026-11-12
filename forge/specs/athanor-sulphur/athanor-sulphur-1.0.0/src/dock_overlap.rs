//! athanor-dock-overlap-v1 (doc_own_stack.md decision 35): whether a window covers the rectangle
//! the dock shows itself in, for its intellihide, and nothing else about windows (decision 32).
//! The description is Athanor's own,
//! `system/athanor-sulphur-client/protocols/athanor-dock-overlap-v1.xml`. The manager is
//! privileged (OS4) by a grant of its own on the dock's line.

use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_dock_overlap_manager_v1::{self, AthanorDockOverlapManagerV1};
use protocol::athanor_dock_overlap_v1::{self, AthanorDockOverlapV1};
use smithay::desktop::{layer_map_for_output, WindowSurfaceType};
use smithay::output::Output;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::compositor::get_role;
use smithay::wayland::shell::wlr_layer::LAYER_SURFACE_ROLE;
use smithay::wayland::{Dispatch2, GlobalDispatch2};

/// The server side of the description, laid out as `wayland-protocols` lays out its own.
#[allow(
    dead_code,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    clippy::all
)]
pub mod protocol {
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-sulphur-client/protocols/athanor-dock-overlap-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-sulphur-client/protocols/athanor-dock-overlap-v1.xml"
    );
}

/// The user data of the manager's global, of every manager and of every overlap object.
pub struct DockOverlaps;

/// A live overlap object, in `State::dock_overlaps`.
pub struct Overlap {
    object: AthanorDockOverlapV1,
    surface: WlSurface,
    /// The rectangle last set, in its output's logical coordinates; none before the first.
    rectangle: Option<Rectangle<i32, Logical>>,
    /// The output the surface was last seen on: a hidden dock that unmaps keeps its output.
    output: Option<Output>,
    /// The answer last sent; none before the first.
    sent: Option<bool>,
}

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorDockOverlapManagerV1, _>(1, DockOverlaps);
}

impl GlobalDispatch2<AthanorDockOverlapManagerV1, State> for DockOverlaps {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorDockOverlapManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, DockOverlaps);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::DockOverlap)
    }
}

impl Dispatch2<AthanorDockOverlapManagerV1, State> for DockOverlaps {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &AthanorDockOverlapManagerV1,
        request: athanor_dock_overlap_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_dock_overlap_manager_v1::Request;
        match request {
            Request::GetOverlap { id, surface } => {
                // The object is made all the same, as every new_id must be.
                let object = data_init.init(id, DockOverlaps);
                if get_role(&surface) != Some(LAYER_SURFACE_ROLE) {
                    manager.post_error(
                        athanor_dock_overlap_manager_v1::Error::NotLayerSurface,
                        "the surface is not a layer surface",
                    );
                    return;
                }
                state.dock_overlaps.push(Overlap {
                    object,
                    surface,
                    rectangle: None,
                    output: None,
                    sent: None,
                });
            }
            Request::Destroy => {}
        }
    }
}

impl Dispatch2<AthanorDockOverlapV1, State> for DockOverlaps {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        object: &AthanorDockOverlapV1,
        request: athanor_dock_overlap_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_dock_overlap_v1::Request;
        match request {
            Request::SetRectangle {
                x,
                y,
                width,
                height,
            } => {
                if let Some(overlap) = state
                    .dock_overlaps
                    .iter_mut()
                    .find(|overlap| overlap.object == *object)
                {
                    overlap.rectangle = Some(Rectangle::new((x, y).into(), (width, height).into()));
                }
                state.send_dock_overlaps();
            }
            Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorDockOverlapV1) {
        state
            .dock_overlaps
            .retain(|overlap| overlap.object != *object);
    }
}

impl State {
    /// Sends each overlap object its answer when it changed; nothing while the session is
    /// locked. Called after every frame, as `send_toplevel_states` is, and at each rectangle.
    pub(crate) fn send_dock_overlaps(&mut self) {
        self.answer_dock_overlaps(false);
    }

    /// At unlock: each overlap object is sent its current answer, changed or not.
    pub(crate) fn resend_dock_overlaps(&mut self) {
        self.answer_dock_overlaps(true);
    }

    /// The rectangle the dock last set on `output`, in its logical coordinates: the region
    /// VL13's probe blurs as the dock's.
    pub(crate) fn dock_rectangle(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        self.dock_overlaps
            .iter()
            .filter(|overlap| overlap.output.as_ref() == Some(output))
            .find_map(|overlap| overlap.rectangle)
    }

    fn answer_dock_overlaps(&mut self, again: bool) {
        if self.lock.is_some() {
            return;
        }
        let mut overlaps = std::mem::take(&mut self.dock_overlaps);
        for overlap in &mut overlaps {
            if let Some(output) = self.space.outputs().find(|output| {
                layer_map_for_output(output)
                    .layer_for_surface(&overlap.surface, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            }) {
                overlap.output = Some(output.clone());
            }
            let Some(rectangle) = overlap.rectangle else {
                continue;
            };
            let answer = overlap
                .output
                .as_ref()
                .is_some_and(|output| self.covered(output, rectangle));
            if again || overlap.sent != Some(answer) {
                overlap.object.overlapped(u32::from(answer));
                overlap.sent = Some(answer);
            }
        }
        // A request handled meanwhile cannot have pushed one: this runs between requests.
        self.dock_overlaps = overlaps;
    }

    /// Whether a mapped window of `output`'s current workspace intersects `rectangle`, in
    /// `output`'s logical coordinates; one without area intersects nothing. A minimised window
    /// and those of inactive workspaces are out of the space.
    fn covered(&self, output: &Output, rectangle: Rectangle<i32, Logical>) -> bool {
        let (Some(area), Some(current)) = (
            self.space.output_geometry(output),
            self.active_workspace(output),
        ) else {
            return false;
        };
        if rectangle.size.w <= 0 || rectangle.size.h <= 0 {
            return false;
        }
        // Saturating: the client's coordinates may be anything.
        let asked = Rectangle::new(
            (
                rectangle.loc.x.saturating_add(area.loc.x),
                rectangle.loc.y.saturating_add(area.loc.y),
            )
                .into(),
            rectangle.size,
        );
        self.space.elements().any(|window| {
            self.workspace_of(window) == Some(current)
                && self
                    .space
                    .element_geometry(window)
                    .is_some_and(|geometry| geometry.overlaps(asked))
        })
    }
}
