//! xdg_toplevel_drag_manager_v1 (doc_own_stack.md decision 37, standard): a client attaches one
//! of its toplevels to a drag it started with its own `wl_data_source` (`dnd.rs`), and the
//! toplevel follows the pointer or the touch point that drag follows, less the offset the client
//! gives, as a move places it (`MoveGrab::follow`). While the drag goes on the attached window
//! takes no part in choosing the drop target (`State::under`). It is detached when it unmaps,
//! and stays where the drop or the cancel leaves it, a lock's cancel included.
//!
//! Smithay has no implementation at 62ec78d, and keeps which sources were set as the selection
//! to itself: the compositor answers the manager, and sees each `wl_data_device.set_selection`
//! before Smithay's handler does (`handlers.rs`, the blanket `Dispatch`).

use crate::state::State;
use smithay::backend::input::TouchSlot;
use smithay::desktop::Window;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::XdgToplevel;
use smithay::reexports::wayland_protocols::xdg::toplevel_drag::v1::server::{
    xdg_toplevel_drag_manager_v1::{self, XdgToplevelDragManagerV1},
    xdg_toplevel_drag_v1::{self, XdgToplevelDragV1},
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_data_device;
use smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, Weak};
use smithay::utils::{Logical, Point};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The user data of the manager's global, of every manager and of every drag object.
pub struct DragManagers;

/// The toplevel drags of every client, and the sources the protocol's `invalid_source` names.
#[derive(Default)]
pub struct ToplevelDrags {
    drags: Vec<ToplevelDrag>,
    /// Each source given to `get_xdg_toplevel_drag`, with the manager it was given to, for as
    /// long as the source lives: it serves one toplevel drag, the drag object's life aside.
    used: Vec<(Weak<WlDataSource>, XdgToplevelDragManagerV1)>,
    /// Each source given to `wl_data_device.set_selection`, for as long as it lives.
    selected: Vec<Weak<WlDataSource>>,
}

struct ToplevelDrag {
    object: XdgToplevelDragV1,
    source: Weak<WlDataSource>,
    /// The toplevel attached, and where the drag's point is on it, from its geometry's top left.
    attached: Option<(XdgToplevel, Point<i32, Logical>)>,
    /// Once the drag of `source` started: the touch point it follows, or the pointer when
    /// `None`, and its end (`dnd.rs`'s `AtDrop`).
    started: Option<(Option<TouchSlot>, Arc<AtomicBool>)>,
}

impl ToplevelDrag {
    /// Whether the drag started and has not ended.
    fn going(&self) -> bool {
        self.started
            .as_ref()
            .is_some_and(|(_, ended)| !ended.load(Ordering::SeqCst))
    }
}

impl ToplevelDrags {
    /// Forgets the sources that are gone.
    fn prune(&mut self) {
        self.used.retain(|(source, _)| source.is_alive());
        self.selected.retain(Weak::is_alive);
    }
}

/// Offers the manager to every client.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, XdgToplevelDragManagerV1, _>(1, DragManagers);
}

impl GlobalDispatch2<XdgToplevelDragManagerV1, State> for DragManagers {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<XdgToplevelDragManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, DragManagers);
    }
}

impl Dispatch2<XdgToplevelDragManagerV1, State> for DragManagers {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &XdgToplevelDragManagerV1,
        request: xdg_toplevel_drag_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use xdg_toplevel_drag_manager_v1::Request;
        match request {
            Request::GetXdgToplevelDrag { id, data_source } => {
                let drags = &mut state.toplevel_drags;
                drags.prune();
                let id_of = data_source.id();
                if drags.used.iter().any(|(used, _)| used.id() == id_of) {
                    manager.post_error(
                        xdg_toplevel_drag_manager_v1::Error::InvalidSource,
                        "the data source already serves a toplevel drag",
                    );
                } else if drags.selected.iter().any(|selected| selected.id() == id_of) {
                    manager.post_error(
                        xdg_toplevel_drag_manager_v1::Error::InvalidSource,
                        "the data source was set as the selection",
                    );
                }
                let object = data_init.init(id, DragManagers);
                drags.used.push((data_source.downgrade(), manager.clone()));
                drags.drags.push(ToplevelDrag {
                    object,
                    source: data_source.downgrade(),
                    attached: None,
                    started: None,
                });
            }
            Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch2<XdgToplevelDragV1, State> for DragManagers {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        object: &XdgToplevelDragV1,
        request: xdg_toplevel_drag_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use xdg_toplevel_drag_v1::Request;
        let Some(drag) = state
            .toplevel_drags
            .drags
            .iter_mut()
            .find(|drag| drag.object == *object)
        else {
            return;
        };
        match request {
            Request::Attach {
                toplevel,
                x_offset,
                y_offset,
            } => {
                // A toplevel destroyed keeps no role; one unmapped was detached at its unmap.
                if drag
                    .attached
                    .as_ref()
                    .is_some_and(|(attached, _)| attached.is_alive())
                {
                    object.post_error(
                        xdg_toplevel_drag_v1::Error::ToplevelAttached,
                        "a toplevel is attached already",
                    );
                    return;
                }
                drag.attached = Some((toplevel, Point::from((x_offset, y_offset))));
                state.follow_drags();
            }
            // A drag never started has not to end.
            Request::Destroy if drag.going() => object.post_error(
                xdg_toplevel_drag_v1::Error::OngoingDrag,
                "the drag has not ended",
            ),
            _ => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &XdgToplevelDragV1) {
        state
            .toplevel_drags
            .drags
            .retain(|drag| drag.object != *object);
    }
}

/// Whether `request` is a `wl_data_device.set_selection` of the source of a toplevel drag:
/// refused, its client ended with `invalid_source` on the manager that took the source. Any
/// other source set as the selection is kept, so that no toplevel drag takes it.
pub fn refused_selection(state: &mut State, request: &dyn Any) -> bool {
    let Some(wl_data_device::Request::SetSelection {
        source: Some(source),
        ..
    }) = request.downcast_ref()
    else {
        return false;
    };
    let drags = &mut state.toplevel_drags;
    drags.prune();
    if let Some((_, manager)) = drags.used.iter().find(|(used, _)| used.id() == source.id()) {
        manager.post_error(
            xdg_toplevel_drag_manager_v1::Error::InvalidSource,
            "the source of a toplevel drag set as the selection",
        );
        return true;
    }
    drags.selected.push(source.downgrade());
    false
}

impl State {
    /// The drag of `source` started, following the touch point `slot` or the pointer, until
    /// `ended`: its toplevel drag, if it has one, moves its toplevel from now on.
    pub(crate) fn toplevel_drag_started(
        &mut self,
        source: &WlDataSource,
        slot: Option<TouchSlot>,
        ended: &Arc<AtomicBool>,
    ) {
        for drag in &mut self.toplevel_drags.drags {
            if drag.started.is_none() && drag.source.id() == source.id() {
                drag.started = Some((slot, ended.clone()));
            }
        }
        self.follow_drags();
    }

    /// A drag ended: the toplevel each ended toplevel drag held is dropped there as a move's is
    /// (`workspace.rs`'s `settle`), and takes the pointer again once the grab that ends is gone,
    /// since this runs inside it.
    pub(crate) fn toplevel_drags_ended(&mut self) {
        let mut dropped = Vec::new();
        for drag in &mut self.toplevel_drags.drags {
            if drag.started.is_some() && !drag.going() {
                dropped.extend(drag.attached.take());
            }
        }
        if dropped.is_empty() {
            return;
        }
        for (toplevel, _) in dropped {
            if let Some(window) = self.window_of_toplevel(&toplevel) {
                self.settle(&window);
            }
        }
        self.loop_handle.insert_idle(State::repoint);
    }

    /// The window of `toplevel`, mapped, while it has its role.
    fn window_of_toplevel(&self, toplevel: &XdgToplevel) -> Option<Window> {
        let surface = self.xdg_shell_state.get_toplevel(toplevel)?;
        self.window_for(surface.wl_surface())
    }

    /// Whether `window` is attached to a drag that goes on: it takes no part in choosing the
    /// drop target, nor in what is under a point until the drag ends.
    pub(crate) fn carried(&self, window: &Window) -> bool {
        self.toplevel_drags
            .drags
            .iter()
            .filter(|drag| drag.going())
            .filter_map(|drag| self.window_of_toplevel(&drag.attached.as_ref()?.0))
            .any(|attached| attached == *window)
    }

    /// Moves each window attached to a drag that goes on to the point the drag follows, less
    /// its offset, kept reachable as a move keeps it. A window not shown yet goes there at the
    /// commit that places it; a maximised, full-screen or tiled one stays where its layout puts
    /// it.
    pub(crate) fn follow_drags(&mut self) {
        // The windows attached to a drag that goes on, with the point each follows and its offset.
        let carried: Vec<_> = self
            .toplevel_drags
            .drags
            .iter()
            .filter(|drag| drag.going())
            .filter_map(|drag| {
                let (toplevel, offset) = drag.attached.as_ref()?;
                let (slot, _) = drag.started.as_ref()?;
                let window = self.window_of_toplevel(toplevel)?;
                Some((window, self.drag_point(*slot)?, *offset))
            })
            .collect();
        let mut moved = false;
        for (window, at, offset) in carried {
            let laid_out = crate::state::is_maximised(&window)
                || crate::state::is_full_screen(&window)
                || self.is_tile(&window);
            if !crate::handlers::shown(&window) || laid_out {
                continue;
            }
            let Some(area) = self.window_area_at(at) else {
                continue;
            };
            let at = at.to_i32_round::<i32>();
            let to = Point::from((at.x.saturating_sub(offset.x), at.y.saturating_sub(offset.y)));
            crate::handlers::moved(&window);
            let to = crate::grabs::kept_reachable(area, to, window.geometry().size);
            self.move_window(&window, to);
            moved = true;
        }
        if moved {
            self.request_redraw();
        }
    }

    /// A window that unmapped is detached from every toplevel drag it was attached to.
    pub(crate) fn detach_unmapped(&mut self, window: &Window) {
        let Some(surface) = window
            .toplevel()
            .map(|toplevel| toplevel.wl_surface().clone())
        else {
            return;
        };
        let shell = &self.xdg_shell_state;
        for drag in &mut self.toplevel_drags.drags {
            let attached = drag
                .attached
                .as_ref()
                .and_then(|(toplevel, _)| shell.get_toplevel(toplevel));
            if attached.is_some_and(|attached| *attached.wl_surface() == surface) {
                drag.attached = None;
            }
        }
    }
}
