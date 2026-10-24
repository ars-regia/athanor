//! athanor-toplevel-state-v1 (doc_own_stack.md OS5, decision 19): the state of each window the
//! foreign toplevel list names, and the shell's actions on it. The description is Athanor's own,
//! `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`; the manager is a
//! privileged global (OS4).

use crate::handlers::shown;
use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_toplevel_manager_v1::{self, AthanorToplevelManagerV1};
use protocol::athanor_toplevel_state_v1::{self, AthanorToplevelStateV1};
use smithay::desktop::Window;
use smithay::input::Seat;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::foreign_toplevel_list::v1::server::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::wayland::foreign_toplevel_list::{ForeignToplevelHandle, ForeignToplevelWeakHandle};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::sync::Mutex;

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
    use smithay::reexports::wayland_protocols::ext::foreign_toplevel_list::v1::server::*;
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_protocols::ext::foreign_toplevel_list::v1::server::__interfaces::*;
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml"
    );
}

/// The user data of the global and of every manager.
pub struct ToplevelStates;

/// The bits of the `state` event.
type Flags = athanor_toplevel_state_v1::State;

/// The user data of a state object: the handle of its window, none for an inert object, and
/// what it was last sent.
pub struct StateObject {
    handle: Option<ForeignToplevelWeakHandle>,
    sent: Mutex<Sent>,
}

/// What a state object was last sent: its state, none before the first `done`, and the outputs
/// its client was sent an `output_enter` for.
#[derive(Default)]
struct Sent {
    flags: Option<Flags>,
    outputs: Vec<Output>,
}

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorToplevelManagerV1, _>(1, ToplevelStates);
}

impl GlobalDispatch2<AthanorToplevelManagerV1, State> for ToplevelStates {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorToplevelManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ToplevelStates);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::ToplevelState)
    }
}

impl Dispatch2<AthanorToplevelManagerV1, State> for ToplevelStates {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &AthanorToplevelManagerV1,
        request: athanor_toplevel_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_toplevel_manager_v1::Request;
        match request {
            Request::GetToplevelState { id, toplevel } => {
                // On a closed handle the object is inert, and never sent anything.
                let handle = ForeignToplevelHandle::from_resource(&toplevel)
                    .filter(|handle| !handle.is_closed());
                // One object per window and client bounds what each frame walks.
                let made = handle.as_ref().is_some_and(|handle| {
                    state.toplevel_states.iter().any(|kept| {
                        kept.id().same_client_as(&manager.id())
                            && kept
                                .data::<StateObject>()
                                .and_then(|data| data.handle.as_ref())
                                .and_then(ForeignToplevelWeakHandle::upgrade)
                                .is_some_and(|own| own.matches(handle))
                    })
                });
                let data = StateObject {
                    handle: handle
                        .as_ref()
                        .filter(|_| !made)
                        .map(ForeignToplevelHandle::downgrade),
                    sent: Mutex::default(),
                };
                let object = data_init.init(id, data);
                if made {
                    manager.post_error(
                        athanor_toplevel_manager_v1::Error::AlreadyMade,
                        "a second state object of the same window",
                    );
                } else if handle.is_some() && state.send_toplevel_state(&object) {
                    state.toplevel_states.push(object);
                }
            }
            Request::Activate { toplevel, seat } => {
                let Some(window) = acted_on(state, &toplevel) else {
                    return;
                };
                if Seat::from_resource(&seat).as_ref() != Some(&state.seat) {
                    return;
                }
                if state.is_minimised(&window) {
                    state.unminimise(&window);
                } else {
                    state.focus(&window);
                }
            }
            Request::SetMinimized { toplevel } => {
                if let Some(window) = acted_on(state, &toplevel) {
                    state.minimise(&window);
                }
            }
            Request::UnsetMinimized { toplevel } => {
                if let Some(window) = acted_on(state, &toplevel) {
                    state.unminimise(&window);
                }
            }
            Request::Close { toplevel } => {
                if let Some(toplevel) = acted_on(state, &toplevel)
                    .as_ref()
                    .and_then(Window::toplevel)
                {
                    toplevel.send_close();
                }
            }
            Request::Destroy => {}
        }
    }
}

/// The window `toplevel` names, when the shell may act on it: the session is neither locked
/// (L14) nor paused on another virtual terminal, where the user cannot see what it would do, as
/// for virtual input; and the window is minimised or one the user can see. A closed handle names
/// none.
fn acted_on(state: &State, toplevel: &ExtForeignToplevelHandleV1) -> Option<Window> {
    if state.drops_virtual_input() {
        return None;
    }
    let handle = ForeignToplevelHandle::from_resource(toplevel).filter(|h| !h.is_closed())?;
    let window = state.window_by_handle(&handle)?;
    (state.is_minimised(&window) || shown(&window)).then_some(window)
}

impl Dispatch2<AthanorToplevelStateV1, State> for StateObject {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _object: &AthanorToplevelStateV1,
        request: athanor_toplevel_state_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            athanor_toplevel_state_v1::Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorToplevelStateV1) {
        state.toplevel_states.retain(|kept| kept != object);
    }
}

impl State {
    /// Sends each live state object what changed of its window since its last `done`: the
    /// state, the outputs it left and entered, then `done`. Run after each frame, which catches
    /// what moved, and when a client binds an output.
    /// Objects destroyed, or whose window is gone, are dropped.
    pub fn send_toplevel_states(&mut self) {
        let objects = std::mem::take(&mut self.toplevel_states);
        self.toplevel_states = objects
            .into_iter()
            .filter(|object| self.send_toplevel_state(object))
            .collect();
    }

    /// Sends `object` what changed of its window; false when the object or the window is gone.
    fn send_toplevel_state(&self, object: &AthanorToplevelStateV1) -> bool {
        let (Some(data), Some(client)) = (object.data::<StateObject>(), object.client()) else {
            return false;
        };
        let Some(handle) = data
            .handle
            .as_ref()
            .and_then(ForeignToplevelWeakHandle::upgrade)
            .filter(|handle| !handle.is_closed())
        else {
            return false;
        };
        let Some(window) = self.window_by_handle(&handle) else {
            return false;
        };
        let mut flags = Flags::empty();
        let activated = window.toplevel().is_some_and(|toplevel| {
            toplevel
                .with_pending_state(|state| state.states.contains(xdg_toplevel::State::Activated))
        });
        flags.set(Flags::Activated, activated);
        flags.set(Flags::Minimized, self.is_minimised(&window));
        // The outputs the window overlaps now, as `Space::refresh` finds them: its cache lags a
        // window mapped since the last frame. Only an output the client has bound counts as
        // sent, so that a later bind sends it.
        let bbox = self.space.element_bbox(&window);
        let outputs: Vec<Output> = self
            .space
            .outputs()
            .filter(|output| {
                let geometry = self.space.output_geometry(output);
                geometry
                    .zip(bbox)
                    .is_some_and(|(geometry, bbox)| geometry.intersection(bbox).is_some())
            })
            .filter(|output| output.client_outputs(&client).next().is_some())
            .cloned()
            .collect();
        let Ok(mut sent) = data.sent.lock() else {
            tracing::warn!("a toplevel state object's record is poisoned: dropped");
            return false;
        };
        let mut changed = false;
        if sent.flags != Some(flags) {
            object.state(flags);
            sent.flags = Some(flags);
            changed = true;
        }
        for output in sent
            .outputs
            .iter()
            .filter(|output| !outputs.contains(output))
        {
            let bound: Vec<_> = output.client_outputs(&client).collect();
            bound
                .iter()
                .for_each(|wl_output| object.output_leave(wl_output));
            changed = true;
        }
        for output in outputs
            .iter()
            .filter(|output| !sent.outputs.contains(output))
        {
            let bound: Vec<_> = output.client_outputs(&client).collect();
            bound
                .iter()
                .for_each(|wl_output| object.output_enter(wl_output));
            changed = true;
        }
        sent.outputs = outputs;
        if changed {
            object.done();
        }
        true
    }
}
