//! The workspaces of ext-workspace-v1. The client binds the manager when it connects
//! (`Client::connect`, `Client::connect_wayland`) and never when it connects without them
//! (`Client::connect_without_workspaces`), so a client that has no use for the workspaces never
//! binds it. Every name of the manager in this crate is in this file (`verify.py allow-list`).

use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
    ext_workspace_handle_v1::ExtWorkspaceHandleV1,
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};

use crate::connection::{fresh, Error, State, WorkspaceHandles};
use crate::model::{Workspace, WorkspaceId};

pub(crate) type Manager = ExtWorkspaceManagerV1;

/// The manager, which commits a change of a workspace's state.
pub(crate) fn manager(state: &State) -> Result<&Manager, Error> {
    state
        .globals
        .workspace_manager
        .as_ref()
        .ok_or(Error::Unavailable("ext_workspace_manager_v1"))
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.groups.insert(workspace_group.id(), Vec::new());
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                let Some(&id) = workspace.data::<WorkspaceId>() else {
                    return;
                };
                let cosmic = state
                    .globals
                    .cosmic_workspaces
                    .as_ref()
                    .map(|manager| manager.get_cosmic_workspace(&workspace, qh, id));
                crate::workspace_tiling::watch(state, id, &workspace, qh);
                state.workspaces.insert(
                    id,
                    Workspace {
                        id,
                        name: String::new(),
                        active: false,
                        tiling: None,
                        output: None,
                    },
                );
                state.workspace_handles.insert(
                    id,
                    WorkspaceHandles {
                        ext: workspace,
                        cosmic,
                    },
                );
            }
            ext_workspace_manager_v1::Event::Done => state.commit_workspaces(),
            _ => {}
        }
    }

    event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, WorkspaceId(fresh()))
    ]);
}

#[cfg(test)]
mod tests {
    use crate::connection::Client;
    use crate::windows::tests::connect_with;

    const MANAGER: &str = "ext_workspace_manager_v1";

    /// Least privilege: a client connected without the workspaces never binds the manager,
    /// even where the compositor offers it, and its workspaces stay empty; one connected as
    /// the shell clients are binds it at connect.
    #[test]
    fn connecting_without_the_workspaces_binds_no_manager() {
        let _context = crate::MAIN_CONTEXT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_connection, client, bound) =
            connect_with(&[MANAGER], Client::connect_without_workspaces);
        let bound = bound.lock().expect("lock").clone();
        assert!(!bound.iter().any(|i| i == MANAGER), "{bound:?}");
        assert!(client.workspaces().is_empty());

        let (_connection, _client, bound) = connect_with(&[MANAGER], Client::connect_wayland);
        let bound = bound.lock().expect("lock").clone();
        assert_eq!(
            bound.iter().filter(|i| *i == MANAGER).count(),
            1,
            "{bound:?}"
        );
    }
}
