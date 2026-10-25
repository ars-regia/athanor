//! The shell's actions on windows: activate, minimise, restore and close. athanor-compositor
//! offers them apart from the window state, to the clients that act on windows (plan D2), and
//! this crate binds them at the first action (`Client::window_actions`): a client that never
//! acts never binds them. Every name of them in this crate is in this file.

use wayland_client::delegate_noop;

use crate::connection::{bind, Client, Error, State};
use crate::model::WindowId;
use crate::protocols::toplevel_state::client::athanor_toplevel_actions_v1::AthanorToplevelActionsV1;

/// athanor-compositor's actions, bound at the first action. Where the compositor does not
/// offer them, they are looked for again at the next action.
#[derive(Default)]
pub(crate) struct Globals {
    actions: Option<AthanorToplevelActionsV1>,
}

/// The actions on the windows of a client (`Client::window_actions`).
pub struct WindowActions<'a> {
    client: &'a Client,
}

#[derive(Clone, Copy)]
enum Action {
    Activate,
    Minimize,
    Unminimize,
    Close,
}

impl Client {
    /// The actions on windows; only an action binds what it needs.
    pub fn window_actions(&self) -> WindowActions<'_> {
        WindowActions { client: self }
    }
}

impl WindowActions<'_> {
    pub fn activate(&self, window: WindowId) -> Result<(), Error> {
        self.act(window, Action::Activate)
    }

    pub fn minimize(&self, window: WindowId) -> Result<(), Error> {
        self.act(window, Action::Minimize)
    }

    pub fn unminimize(&self, window: WindowId) -> Result<(), Error> {
        self.act(window, Action::Unminimize)
    }

    pub fn close(&self, window: WindowId) -> Result<(), Error> {
        self.act(window, Action::Close)
    }

    /// Through cosmic-comp's manager, else athanor-compositor's actions: no compositor offers
    /// both, so the order never chooses, and the COSMIC path stays as it was (plan D3).
    fn act(&self, window: WindowId, action: Action) -> Result<(), Error> {
        let client = self.client;
        client.live()?;
        {
            let mut state = client.inner.state.borrow_mut();
            let state = &mut *state;
            if let Some(manager) = &state.globals.toplevel_manager {
                let toplevel = state.cosmic_toplevel(window)?;
                match action {
                    Action::Activate => manager.activate(toplevel, state.seat()?),
                    Action::Minimize => manager.set_minimized(toplevel),
                    Action::Unminimize => manager.unset_minimized(toplevel),
                    Action::Close => manager.close(toplevel),
                }
            } else {
                if state.globals.actions.actions.is_none() {
                    state.globals.actions.actions =
                        bind(&client.inner.registry, &client.inner.qh, 1..=1);
                }
                let actions = state
                    .globals
                    .actions
                    .actions
                    .as_ref()
                    .ok_or(Error::Unavailable("athanor_toplevel_actions_v1"))?;
                let toplevel = &state.toplevels.get(&window).ok_or(Error::NoWindow)?.ext;
                match action {
                    Action::Activate => actions.activate(toplevel, state.seat()?),
                    Action::Minimize => actions.set_minimized(toplevel),
                    Action::Unminimize => actions.unset_minimized(toplevel),
                    Action::Close => actions.close(toplevel),
                }
            }
        }
        client.flush()
    }
}

delegate_noop!(State: AthanorToplevelActionsV1);
