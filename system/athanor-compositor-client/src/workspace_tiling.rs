//! The tiling of workspaces under athanor-compositor, which offers it apart from cosmic-comp's
//! workspace extension to the bar alone (doc_own_stack.md OS5 and OS12, plan D12): one
//! `athanor_workspace_tiling_v1` per workspace of ext-workspace-v1. This crate binds the
//! manager at the first watch (`Client::watch_tiling`), so a client that never watches the
//! tiling never binds it. Every name of it in this crate is in this file.

use std::collections::HashMap;

use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::ExtWorkspaceHandleV1;

use crate::connection::{bind, Client, Error, State};
use crate::model::{Tiling, WorkspaceId};
use crate::protocols::workspace_tiling::client::{
    athanor_workspace_tiling_manager_v1::AthanorWorkspaceTilingManagerV1,
    athanor_workspace_tiling_v1::{self, AthanorWorkspaceTilingV1},
};

/// athanor-compositor's manager, bound at the first watch, and each workspace's object.
#[derive(Default)]
pub(crate) struct Globals {
    manager: Option<AthanorWorkspaceTilingManagerV1>,
    objects: HashMap<WorkspaceId, AthanorWorkspaceTilingV1>,
}

impl Client {
    /// Follows each workspace's tiling (`Workspace::tiling`) through athanor-compositor's
    /// manager. Nothing to do under cosmic-comp, whose workspace extension reports it, nor where
    /// the manager is already bound. Where the compositor does not grant the manager, the tiling
    /// stays `None` and `set_tiling` reports it unavailable.
    pub fn watch_tiling(&self) -> Result<(), Error> {
        self.live()?;
        {
            let mut state = self.inner.state.borrow_mut();
            let state = &mut *state;
            if !state.own_compositor() || state.globals.workspace_tiling.manager.is_some() {
                return Ok(());
            }
            state.globals.workspace_tiling.manager =
                bind(&self.inner.registry, &self.inner.qh, 1..=1);
            for (id, workspace) in state.ext_workspaces() {
                watch(state, id, &workspace, &self.inner.qh);
            }
        }
        self.flush()
    }
}

/// Asks for the tiling object of `workspace` when the manager is bound: at the first watch, or
/// when the workspace comes after it.
pub(crate) fn watch(
    state: &mut State,
    id: WorkspaceId,
    workspace: &ExtWorkspaceHandleV1,
    qh: &QueueHandle<State>,
) {
    let globals = &mut state.globals.workspace_tiling;
    if let Some(manager) = &globals.manager {
        // One object per workspace: a second is the manager's already_made error.
        globals
            .objects
            .entry(id)
            .or_insert_with(|| manager.get_tiling(workspace, qh, id));
    }
}

/// Destroys the workspace's object, when its workspace is removed.
pub(crate) fn forget(state: &mut State, id: &WorkspaceId) {
    if let Some(object) = state.globals.workspace_tiling.objects.remove(id) {
        object.destroy();
    }
}

/// The request of `Client::set_tiling` under athanor-compositor, applied at once.
pub(crate) fn set_state(state: &State, id: WorkspaceId, tiling: Tiling) -> Result<(), Error> {
    let globals = &state.globals.workspace_tiling;
    if globals.manager.is_none() {
        return Err(Error::Unavailable("athanor_workspace_tiling_manager_v1"));
    }
    globals
        .objects
        .get(&id)
        .ok_or(Error::NoWorkspace)?
        .set_state(match tiling {
            Tiling::Floating => athanor_workspace_tiling_v1::State::Floating,
            Tiling::Tiled => athanor_workspace_tiling_v1::State::Tiled,
        });
    Ok(())
}

/// The tiling a `state` event names; `None` for a value this crate does not know.
fn tiling_of(state: WEnum<athanor_workspace_tiling_v1::State>) -> Option<Tiling> {
    match state {
        WEnum::Value(athanor_workspace_tiling_v1::State::Floating) => Some(Tiling::Floating),
        WEnum::Value(athanor_workspace_tiling_v1::State::Tiled) => Some(Tiling::Tiled),
        WEnum::Unknown(_) => None,
    }
}

impl Dispatch<AthanorWorkspaceTilingV1, WorkspaceId> for State {
    fn event(
        state: &mut Self,
        _: &AthanorWorkspaceTilingV1,
        event: athanor_workspace_tiling_v1::Event,
        id: &WorkspaceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            athanor_workspace_tiling_v1::Event::State { state: tiling } => {
                state.set_workspace_tiling(*id, tiling_of(tiling));
            }
            // The workspace's own `removed` follows; the object sends nothing more.
            athanor_workspace_tiling_v1::Event::Removed => forget(state, id),
        }
    }
}

delegate_noop!(State: AthanorWorkspaceTilingManagerV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_is_read_as_the_protocol_numbers_it() {
        use athanor_workspace_tiling_v1::State as Wire;
        assert_eq!(
            tiling_of(WEnum::Value(Wire::Floating)),
            Some(Tiling::Floating)
        );
        assert_eq!(tiling_of(WEnum::Value(Wire::Tiled)), Some(Tiling::Tiled));
        assert_eq!(tiling_of(WEnum::Unknown(2)), None);
    }
}
