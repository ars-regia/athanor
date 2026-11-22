//! xdg_toplevel_drag_manager_v1 (doc_own_stack.md decision 37, standard): a client attaches one
//! of its toplevels to a drag it started with its own `wl_data_source` (`dnd.rs`), and the
//! toplevel follows the pointer or the touch point that drag follows, less the offset the client
//! gives, as a move places it (`MoveGrab::follow`). While the drag goes on the attached window
//! takes no part in choosing the drop target (`State::under`). It is detached when it unmaps,
//! and stays where the drop or the cancel leaves it, a lock's cancel included.
//!
//! Smithay has no implementation at 62ec78d, and keeps which sources were set as the selection
//! to itself: the compositor answers the manager, and sees each `wl_data_device.set_selection`
//! before Smithay's handler does (`handlers.rs`, the blanket `Dispatch`). Every request costs
//! the same however many sources, drag objects and toplevels a client holds: each is found by
//! its id, and only the drags in progress, one for each pointer or touch point at most, are
//! gone through on a motion.

use crate::state::State;
use smithay::backend::input::TouchSlot;
use smithay::desktop::Window;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::XdgToplevel;
use smithay::reexports::wayland_protocols::xdg::toplevel_drag::v1::server::{
    xdg_toplevel_drag_manager_v1::{self, XdgToplevelDragManagerV1},
    xdg_toplevel_drag_v1::{self, XdgToplevelDragV1},
};
use smithay::reexports::wayland_server::backend::{ClientId, ObjectId};
use smithay::reexports::wayland_server::protocol::wl_data_device;
use smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, Weak};
use smithay::utils::{Logical, Point};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The user data of the manager's global, of every manager and of every drag object.
pub struct DragManagers;

/// The toplevel drags of every client, and the sources the protocol's `invalid_source` names.
#[derive(Default)]
pub struct ToplevelDrags {
    /// Every drag object, by its id.
    drags: HashMap<ObjectId, ToplevelDrag>,
    /// Each attached toplevel, by its id, with the drag object it is attached to: a toplevel
    /// follows one drag at a time.
    attached: HashMap<ObjectId, ObjectId>,
    /// The drag objects whose drag started, until its end is seen.
    started: Vec<ObjectId>,
    /// Each source given to `get_xdg_toplevel_drag`, with the manager it was given to and the
    /// drag object made of it: it serves one toplevel drag, the drag object's life aside.
    used: Marks<(XdgToplevelDragManagerV1, ObjectId)>,
    /// Each source given to `wl_data_device.set_selection`.
    selected: Marks<()>,
}

/// Sources marked, by their id, for as long as each lives. An id is never given again to
/// another object (its serial differs), so a mark left by a source gone never names a live one;
/// those marks are dropped once the map has doubled since it was last pruned, so that marking
/// costs the same, amortised, and the map holds at most twice the sources that live.
struct Marks<T> {
    by_source: HashMap<ObjectId, (Weak<WlDataSource>, T)>,
    /// How many marks were left at the last prune.
    kept: usize,
    /// How many prunes there were.
    #[cfg(test)]
    prunes: usize,
}

impl<T> Default for Marks<T> {
    fn default() -> Self {
        Self {
            by_source: HashMap::new(),
            kept: 0,
            #[cfg(test)]
            prunes: 0,
        }
    }
}

impl<T> Marks<T> {
    /// The mark of `source`.
    fn get(&self, source: &WlDataSource) -> Option<&T> {
        self.by_source.get(&source.id()).map(|(_, mark)| mark)
    }

    /// Marks `source`, in place of its mark if it has one.
    fn mark(&mut self, source: &WlDataSource, mark: T) {
        self.by_source
            .insert(source.id(), (source.downgrade(), mark));
        if self.by_source.len() >= 2 * self.kept.max(32) {
            self.by_source.retain(|_, (source, _)| source.is_alive());
            self.kept = self.by_source.len();
            #[cfg(test)]
            {
                self.prunes += 1;
            }
        }
    }
}

struct ToplevelDrag {
    /// The toplevel attached, and where the drag's point is on it, from its geometry's top left.
    attached: Option<(XdgToplevel, Point<i32, Logical>)>,
    /// Once the drag of its source started: the touch point it follows, or the pointer when
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
    /// Detaches the toplevel of the drag object `id`, if it has one.
    fn detach(&mut self, id: &ObjectId) -> Option<(XdgToplevel, Point<i32, Logical>)> {
        let (toplevel, offset) = self.drags.get_mut(id)?.attached.take()?;
        if self.attached.get(&toplevel.id()) == Some(id) {
            self.attached.remove(&toplevel.id());
        }
        Some((toplevel, offset))
    }

    /// How many drag objects, attached toplevels, drags in progress, sources used by a toplevel
    /// drag and sources set as the selection are kept, in that order, and how many times the
    /// marks of the selection were pruned.
    #[cfg(test)]
    pub(crate) fn sizes(&self) -> [usize; 6] {
        [
            self.drags.len(),
            self.attached.len(),
            self.started.len(),
            self.used.by_source.len(),
            self.selected.by_source.len(),
            self.selected.prunes,
        ]
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
        if let Request::GetXdgToplevelDrag { id, data_source } = request {
            let drags = &mut state.toplevel_drags;
            if drags.used.get(&data_source).is_some() {
                manager.post_error(
                    xdg_toplevel_drag_manager_v1::Error::InvalidSource,
                    "the data source already serves a toplevel drag",
                );
            } else if drags.selected.get(&data_source).is_some() {
                manager.post_error(
                    xdg_toplevel_drag_manager_v1::Error::InvalidSource,
                    "the data source was set as the selection",
                );
            }
            let id = data_init.init(id, DragManagers).id();
            drags.used.mark(&data_source, (manager.clone(), id.clone()));
            drags.drags.insert(
                id,
                ToplevelDrag {
                    attached: None,
                    started: None,
                },
            );
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
        let drags = &mut state.toplevel_drags;
        let id = object.id();
        let Some(drag) = drags.drags.get(&id) else {
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
                drags.detach(&id);
                // Taken from the drag it followed, if any.
                if let Some(other) = drags.attached.insert(toplevel.id(), id.clone()) {
                    drags.detach(&other);
                }
                if let Some(drag) = drags.drags.get_mut(&id) {
                    drag.attached = Some((toplevel, Point::from((x_offset, y_offset))));
                }
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
        let drags = &mut state.toplevel_drags;
        let id = object.id();
        drags.detach(&id);
        drags.drags.remove(&id);
        drags.started.retain(|started| *started != id);
    }
}

/// Whether `request` is a `wl_data_device.set_selection` of the source of a toplevel drag:
/// refused, its client ended with `invalid_source` on the manager that took the source. Any
/// other source set as the selection is marked, so that no toplevel drag takes it.
pub fn refused_selection(state: &mut State, request: &dyn Any) -> bool {
    let Some(wl_data_device::Request::SetSelection {
        source: Some(source),
        ..
    }) = request.downcast_ref()
    else {
        return false;
    };
    let drags = &mut state.toplevel_drags;
    if let Some((manager, _)) = drags.used.get(source) {
        manager.post_error(
            xdg_toplevel_drag_manager_v1::Error::InvalidSource,
            "the source of a toplevel drag set as the selection",
        );
        return true;
    }
    drags.selected.mark(source, ());
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
        let drags = &mut self.toplevel_drags;
        let Some((_, id)) = drags.used.get(source) else {
            return;
        };
        let id = id.clone();
        let Some(drag) = drags.drags.get_mut(&id) else {
            return;
        };
        if drag.started.is_some() {
            return;
        }
        drag.started = Some((slot, ended.clone()));
        drags.started.push(id);
        self.follow_drags();
    }

    /// A drag ended: the toplevel each ended toplevel drag held is dropped there as a move's is
    /// (`workspace.rs`'s `settle`), and takes the pointer again once the grab that ends is gone,
    /// since this runs inside it.
    pub(crate) fn toplevel_drags_ended(&mut self) {
        let drags = &mut self.toplevel_drags;
        let (ended, going): (Vec<_>, Vec<_>) = std::mem::take(&mut drags.started)
            .into_iter()
            .partition(|id| drags.drags.get(id).is_none_or(|drag| !drag.going()));
        drags.started = going;
        let dropped: Vec<_> = ended.iter().filter_map(|id| drags.detach(id)).collect();
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

    /// The drags that go on, each with its toplevel attached, its offset and the touch point
    /// it follows, or the pointer when `None`.
    fn carrying(
        &self,
    ) -> impl Iterator<Item = (&XdgToplevel, Point<i32, Logical>, Option<TouchSlot>)> {
        let drags = &self.toplevel_drags;
        drags
            .started
            .iter()
            .filter_map(|id| drags.drags.get(id))
            .filter(|drag| drag.going())
            .filter_map(|drag| {
                let (toplevel, offset) = drag.attached.as_ref()?;
                let (slot, _) = drag.started.as_ref()?;
                Some((toplevel, *offset, *slot))
            })
    }

    /// The windows attached to a drag that goes on, one for each drag at most: they take no
    /// part in choosing the drop target, nor in what is under a point until the drag ends.
    pub(crate) fn carried_windows(&self) -> Vec<Window> {
        self.carrying()
            .filter_map(|(toplevel, _, _)| self.window_of_toplevel(toplevel))
            .collect()
    }

    /// Moves each window attached to a drag that goes on to the point the drag follows, less
    /// its offset, kept reachable as a move keeps it. A window not shown yet goes there at the
    /// commit that places it; a maximised, full-screen or tiled one stays where its layout puts
    /// it.
    pub(crate) fn follow_drags(&mut self) {
        let carried: Vec<_> = self
            .carrying()
            .filter_map(|(toplevel, offset, slot)| {
                Some((
                    self.window_of_toplevel(toplevel)?,
                    self.drag_point(slot)?,
                    offset,
                ))
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

    /// A window that unmapped is detached from the toplevel drag it was attached to.
    pub(crate) fn detach_unmapped(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let drags = &mut self.toplevel_drags;
        if let Some(id) = drags.attached.get(&toplevel.xdg_toplevel().id()).cloned() {
            drags.detach(&id);
        }
    }
}
