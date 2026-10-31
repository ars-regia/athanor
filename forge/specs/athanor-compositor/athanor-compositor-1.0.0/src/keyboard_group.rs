//! athanor-keyboard-group-v1 (doc_own_stack.md OS5, decisions 19 and 27): the active layout
//! group of the seat's keyboard, and the shell's request that switches it. The description is
//! Athanor's own, `system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml`.
//! The manager is privileged (OS4): the bar's input-source switch is its one client.

use crate::identity::Privilege;
use crate::state::{granted, State};
use protocol::athanor_keyboard_group_manager_v1::{self, AthanorKeyboardGroupManagerV1};
use protocol::athanor_keyboard_group_v1::{self, AthanorKeyboardGroupV1};
use smithay::input::keyboard::{KeyboardHandle, Layout, Xkb};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::sync::{Mutex, PoisonError};

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
    dh.create_global::<State, AthanorKeyboardGroupManagerV1, _>(2, KeyboardGroups);
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
                    send_layouts(&object, &state.seat_layouts());
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
        state: &mut State,
        _client: &Client,
        object: &AthanorKeyboardGroupV1,
        request: athanor_keyboard_group_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_keyboard_group_v1::Request;
        match request {
            // An inert object switches nothing.
            Request::SetGroup { group } if state.keyboard_groups.contains(object) => {
                state.set_keyboard_group(group);
            }
            Request::SetGroup { .. } | Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorKeyboardGroupV1) {
        state.keyboard_groups.retain(|kept| kept != object);
    }
}

/// Sends `object` the names of the seat's layouts, when its version has the event.
fn send_layouts(object: &AthanorKeyboardGroupV1, names: &[String]) {
    if object.version() >= athanor_keyboard_group_v1::EVT_LAYOUTS_SINCE {
        object.layouts(lines_of(names));
    }
}

/// One line per name. Ended, not joined: a last name that is empty still counts as a group.
fn lines_of(names: &[String]) -> String {
    names
        .iter()
        .map(|name| name.replace(['\n', '\r'], " ") + "\n")
        .collect()
}

impl State {
    /// The seat's own layout group: the one saved while a virtual keyboard's keymap is current,
    /// whose modifiers never move it, else the keyboard's.
    pub fn seat_group(&self) -> u32 {
        if let Some(current) = &self.virtual_keymap {
            return current.seat_modifiers[3];
        }
        self.seat.get_keyboard().map_or(0, |keyboard| {
            keyboard.modifier_state().serialized.layout_effective
        })
    }

    /// The names of the seat's own layouts: the ones saved while a virtual keyboard's keymap is
    /// current, which names its own or none, else the keyboard's.
    fn seat_layouts(&mut self) -> Vec<String> {
        if let Some(current) = &self.virtual_keymap {
            return current.seat_layouts.clone();
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return Vec::new();
        };
        keyboard.with_xkb_state(self, |context| layout_names(context.xkb()))
    }

    /// Sends the names of the seat's layouts to every live object: where the seat's configuration
    /// changes, and never for a virtual keyboard's keymap.
    pub fn send_keyboard_layouts(&mut self) {
        let names = self.seat_layouts();
        for object in &self.keyboard_groups {
            send_layouts(object, &names);
        }
    }

    /// Sends the seat's group to every live object when it differs from the last one sent: where
    /// a change of the group is final, so a key that changes none sends nothing.
    pub fn send_keyboard_group(&mut self) {
        let group = self.seat_group();
        if group == self.sent_group {
            return;
        }
        self.sent_group = group;
        for object in &self.keyboard_groups {
            object.group(group);
        }
    }

    /// Makes `group` the seat's group, under the seat's own keymap, which a virtual keyboard's
    /// gives back first; a group at or beyond the keymap's layouts is ignored.
    fn set_keyboard_group(&mut self, group: u32) {
        self.restore_seat_keymap();
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        if usize::try_from(group).map_or(true, |group| group >= self.layouts()) {
            return;
        }
        keyboard.with_xkb_state(self, |mut context| context.set_layout(Layout(group)));
        self.input_method_modifiers();
        self.send_keyboard_group();
    }

    /// Super+Space (D14): the seat's next layout group, the first after the last, under the
    /// seat's own keymap; a keymap of one layout changes nothing.
    pub(crate) fn next_keyboard_group(&mut self) {
        self.restore_seat_keymap();
        let layouts = self.layouts();
        if layouts > 1 {
            let next = (self.seat_group() as usize + 1) % layouts;
            self.set_keyboard_group(next as u32);
        }
    }

    /// The layouts of the seat keyboard's current keymap.
    fn layouts(&mut self) -> usize {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return 0;
        };
        keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap_or_else(PoisonError::into_inner);
            xkb.layouts().count()
        })
    }
}

/// The names of the layouts of `xkb`'s keymap.
pub(crate) fn layout_names(xkb: &Mutex<Xkb>) -> Vec<String> {
    let xkb = xkb.lock().unwrap_or_else(PoisonError::into_inner);
    xkb.layouts()
        .map(|layout| xkb.layout_name(layout).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_cannot_add_a_line() {
        let names = ["A\r\nB".to_owned(), String::new(), "C\r".to_owned()];
        assert_eq!(lines_of(&names), "A  B\n\nC \n");
    }
}
