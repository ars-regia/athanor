//! athanor-toplevel-state-v1 (doc_own_stack.md OS5, decision 19): the state of each window the
//! foreign toplevel list names, and the shell's actions on it. The description is Athanor's own,
//! `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`; the manager is a
//! privileged global (OS4).

use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_toplevel_manager_v1::{self, AthanorToplevelManagerV1};
use protocol::athanor_toplevel_state_v1::{self, AthanorToplevelStateV1};
use smithay::output::Output;
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
        _manager: &AthanorToplevelManagerV1,
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
                let data = StateObject {
                    handle: handle.as_ref().map(ForeignToplevelHandle::downgrade),
                    sent: Mutex::default(),
                };
                let object = data_init.init(id, data);
                if handle.is_some() {
                    state.toplevel_states.push(object);
                    // Its first state, outputs and done go after the next frame's refresh of
                    // the space, which knows the outputs of a window mapped since the last.
                    state.request_redraw();
                }
            }
            // ponytail: the actions arrive with the minimise and the manager's actions (plan
            // tasks 2 and 3); until then they change nothing.
            Request::Activate { .. }
            | Request::SetMinimized { .. }
            | Request::UnsetMinimized { .. }
            | Request::Close { .. }
            | Request::Destroy => {}
        }
    }
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
    /// state, the outputs it left and entered, then `done`. Run after each frame's refresh of
    /// the space, which places the windows on the outputs, and when a client binds an output.
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
            toplevel.with_pending_state(|state| {
                state.states.contains(xdg_toplevel::State::Activated)
            })
        });
        flags.set(Flags::Activated, activated);
        flags.set(Flags::Minimized, self.is_minimised(&window));
        // Only an output the client has bound counts as sent, so that a later bind sends it.
        let outputs: Vec<Output> = self
            .space
            .outputs_for_element(&window)
            .into_iter()
            .filter(|output| output.client_outputs(&client).next().is_some())
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
        for output in sent.outputs.iter().filter(|output| !outputs.contains(output)) {
            let bound: Vec<_> = output.client_outputs(&client).collect();
            bound.iter().for_each(|wl_output| object.output_leave(wl_output));
            changed = true;
        }
        for output in outputs.iter().filter(|output| !sent.outputs.contains(output)) {
            let bound: Vec<_> = output.client_outputs(&client).collect();
            bound.iter().for_each(|wl_output| object.output_enter(wl_output));
            changed = true;
        }
        sent.outputs = outputs;
        if changed {
            object.done();
        }
        true
    }
}
