//! athanor-toplevel-state-v1 (doc_own_stack.md OS5, decision 19): the state of each window the
//! foreign toplevel list names, and the shell's actions on it. The description is Athanor's own,
//! `system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml`; the manager is a
//! privileged global (OS4).

use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_toplevel_manager_v1::{self, AthanorToplevelManagerV1};
use protocol::athanor_toplevel_state_v1::{self, AthanorToplevelStateV1};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New};
use smithay::wayland::foreign_toplevel_list::ForeignToplevelHandle;
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

/// The user data of a state object.
pub struct StateObject;

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
                let live = ForeignToplevelHandle::from_resource(&toplevel)
                    .is_some_and(|handle| !handle.is_closed());
                let object = data_init.init(id, StateObject);
                if live {
                    state.toplevel_states.push(object);
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
