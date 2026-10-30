//! The keyboard's layouts and group under athanor-compositor, which offers them apart from
//! cosmic-comp's keyboard layout manager to the bar alone (doc_own_stack.md OS5, decision 19). A
//! manager of version 2 names the seat's layouts, in place of `wl_keyboard.keymap`, which also
//! brings every virtual keyboard's keymap to an unfocused client. This crate binds
//! it at the first watch (`Client::watch_keyboard_group`), so a client that never watches the
//! group never binds it. Every name of it in this crate is in this file.

use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};

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
    /// The manager's `layouts` event names the seat's layouts, so a keymap no longer does.
    names_layouts: bool,
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
            let manager: Option<AthanorKeyboardGroupManagerV1> =
                bind(&self.inner.registry, &self.inner.qh, 1..=2);
            state.globals.keyboard_group.names_layouts = manager.as_ref().is_some_and(|manager| {
                manager.version() >= athanor_keyboard_group_v1::EVT_LAYOUTS_SINCE
            });
            state.globals.keyboard_group.manager = manager;
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
        match event {
            athanor_keyboard_group_v1::Event::Group { group } => state.set_group(group),
            athanor_keyboard_group_v1::Event::Layouts { names } => {
                state.set_layouts(names_of(&names));
            }
        }
    }
}

delegate_noop!(State: AthanorKeyboardGroupManagerV1);

/// The names of a `layouts` event, one per group: every line is one, an empty one included.
fn names_of(lines: &str) -> Vec<String> {
    lines.lines().map(str::to_owned).collect()
}

impl State {
    /// The group either manager sent, as an event when it changed.
    pub(crate) fn set_group(&mut self, group: u32) {
        if group != self.keyboard_group {
            self.keyboard_group = group;
            self.events.push(Event::KeyboardGroup(group));
        }
    }

    /// The seat's layouts, as an event when they changed.
    pub(crate) fn set_layouts(&mut self, names: Vec<String>) {
        if names != self.keyboard_layouts {
            self.keyboard_layouts = names.clone();
            self.events.push(Event::KeyboardLayouts(names));
        }
    }

    /// The names of a keymap, which are the seat's where no `layouts` event names them.
    pub(crate) fn keymap_layouts(&mut self, names: Vec<String>) {
        if !self.globals.keyboard_group.names_layouts {
            self.set_layouts(names);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_of_the_event_is_a_group() {
        assert_eq!(names_of(""), Vec::<String>::new());
        assert_eq!(names_of("English (US)\n\n"), ["English (US)", ""]);
        assert_eq!(names_of("\n\n"), ["", ""]);
    }

    /// wtype's keymap names no layout: under version 2 the seat's stay, under cosmic-comp and
    /// version 1 a keymap is all there is.
    #[test]
    fn a_keymap_names_the_layouts_only_where_no_event_does() {
        let seat = vec!["English (US)".to_owned(), "Italian".to_owned()];
        let mut state = State::default();
        state.keymap_layouts(seat.clone());
        state.globals.keyboard_group.names_layouts = true;
        state.keymap_layouts(Vec::new());
        assert_eq!(state.keyboard_layouts, seat);
        state.globals.keyboard_group.names_layouts = false;
        state.keymap_layouts(Vec::new());
        assert_eq!(state.keyboard_layouts, Vec::<String>::new());
    }
}
