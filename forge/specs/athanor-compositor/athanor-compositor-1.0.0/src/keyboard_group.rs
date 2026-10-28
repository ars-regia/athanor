//! athanor-keyboard-group-v1 (doc_own_stack.md OS5, decisions 19 and 27): the active layout
//! group of the seat's keyboard, and the shell's request that switches it. The description is
//! Athanor's own, `system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`.
//! The manager is privileged (OS4): the bar's input-source switch is its one client.

use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_keyboard_group_manager_v1::{self, AthanorKeyboardGroupManagerV1};
use protocol::athanor_keyboard_group_v1::{self, AthanorKeyboardGroupV1};
use smithay::input::keyboard::KeyboardHandle;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
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
            "../../../../system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml"
    );
}

/// The user data of the manager's global and of every manager.
pub struct KeyboardGroups;

/// The user data of a keyboard group object: the `wl_keyboard` it was made for. A live object is
/// in `State::keyboard_groups`; an inert one is not.
pub struct GroupObject {
    keyboard: smithay::reexports::wayland_server::protocol::wl_keyboard::WlKeyboard,
}

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorKeyboardGroupManagerV1, _>(1, KeyboardGroups);
}

impl GlobalDispatch2<AthanorKeyboardGroupManagerV1, State> for KeyboardGroups {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorKeyboardGroupManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, KeyboardGroups);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::KeyboardGroup)
    }
}

impl Dispatch2<AthanorKeyboardGroupManagerV1, State> for KeyboardGroups {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &AthanorKeyboardGroupManagerV1,
        request: athanor_keyboard_group_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_keyboard_group_manager_v1::Request;
        match request {
            Request::GetKeyboardGroup { id, keyboard } => {
                let live = KeyboardHandle::<State>::from_resource(&keyboard)
                    .is_some_and(|handle| state.seat.get_keyboard().as_ref() == Some(&handle));
                // One object per keyboard and client bounds what each change of group walks.
                let made = state.keyboard_groups.iter().any(|kept| {
                    kept.data::<GroupObject>()
                        .is_some_and(|data| data.keyboard == keyboard)
                });
                let object = data_init.init(id, GroupObject { keyboard });
                if made {
                    manager.post_error(
                        athanor_keyboard_group_manager_v1::Error::AlreadyMade,
                        "a second keyboard group object of the same keyboard",
                    );
                } else if live {
                    object.group(state.seat_group());
                    state.keyboard_groups.push(object);
                }
            }
            Request::Destroy => {}
        }
    }
}

impl Dispatch2<AthanorKeyboardGroupV1, State> for GroupObject {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _object: &AthanorKeyboardGroupV1,
        request: athanor_keyboard_group_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_keyboard_group_v1::Request;
        match request {
            // Switched by the next task of the keyboard group plan.
            Request::SetGroup { .. } => {}
            Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorKeyboardGroupV1) {
        state.keyboard_groups.retain(|kept| kept != object);
    }
}

impl State {
    /// The seat's own layout group.
    pub fn seat_group(&self) -> u32 {
        self.seat.get_keyboard().map_or(0, |keyboard| {
            keyboard.modifier_state().serialized.layout_effective
        })
    }
}
