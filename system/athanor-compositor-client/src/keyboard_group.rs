//! The keyboard's layout group under athanor-compositor, which offers it apart from cosmic-comp's
//! keyboard layout manager to the bar alone (doc_own_stack.md OS5, decision 19): this crate binds
//! it at the first watch (`Client::watch_keyboard_group`), so a client that never watches the
//! group never binds it. Every name of it in this crate is in this file.

use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};

use crate::connection::{bind, Client, Error, State};
use crate::model::Event;
use crate::protocols::keyboard_group::client::{
    athanor_keyboard_group_manager_v1::AthanorKeyboardGroupManagerV1,
    athanor_keyboard_group_v1::{self, AthanorKeyboardGroupV1},
};

/// athanor-compositor's manager, bound at the first watch, and the seat keyboard's object.
#[derive(Default)]
pub(crate) struct Globals {
    manager: Option<AthanorKeyboardGroupManagerV1>,
    group: Option<AthanorKeyboardGroupV1>,
}

impl Client {
    /// Follows the keyboard's group (`Event::KeyboardGroup`) through athanor-compositor's
    /// manager. Nothing to do under cosmic-comp, whose manager is bound at connect, nor where the
    /// manager is already bound.
    pub fn watch_keyboard_group(&self) -> Result<(), Error> {
        self.live()?;
        {
            let mut state = self.inner.state.borrow_mut();
            let state = &mut *state;
            if state.globals.keyboard_layouts.is_some()
                || state.globals.keyboard_group.manager.is_some()
            {
                return Ok(());
            }
            state.globals.keyboard_group.manager =
                bind(&self.inner.registry, &self.inner.qh, 1..=1);
            if let Some(keyboard) = state.keyboard.clone() {
                watch(state, &keyboard, &self.inner.qh);
            }
        }
        self.flush()
    }
}

/// Asks for `keyboard`'s group object, when the manager is bound and there is none yet; at the
/// first watch, or at the seat's keyboard when it comes after it.
pub(crate) fn watch(state: &mut State, keyboard: &WlKeyboard, qh: &QueueHandle<State>) {
    let globals = &mut state.globals.keyboard_group;
    if let (Some(manager), None) = (&globals.manager, &globals.group) {
        globals.group = Some(manager.get_keyboard_group(keyboard, qh, ()));
    }
}

/// The request of `Client::set_keyboard_group` where cosmic-comp's manager is not there.
pub(crate) fn set_group(state: &State, group: u32) -> Result<(), Error> {
    state
        .globals
        .keyboard_group
        .group
        .as_ref()
        .ok_or(Error::Unavailable("athanor_keyboard_group_manager_v1"))?
        .set_group(group);
    Ok(())
}

impl Dispatch<AthanorKeyboardGroupV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &AthanorKeyboardGroupV1,
        event: athanor_keyboard_group_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let athanor_keyboard_group_v1::Event::Group { group } = event;
        state.set_group(group);
    }
}

delegate_noop!(State: AthanorKeyboardGroupManagerV1);

impl State {
    /// The group either manager sent, as an event when it changed.
    pub(crate) fn set_group(&mut self, group: u32) {
        if group != self.keyboard_group {
            self.keyboard_group = group;
            self.events.push(Event::KeyboardGroup(group));
        }
    }
}
