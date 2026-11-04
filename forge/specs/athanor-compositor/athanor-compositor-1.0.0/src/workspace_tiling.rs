//! athanor-workspace-tiling-v1 (doc_own_stack.md OS5 and OS12, plan D12): whether a workspace of
//! ext-workspace-v1 is tiled, and the shell's request that tiles it or floats it. The
//! description is Athanor's own,
//! `system/athanor-compositor-client/protocols/athanor-workspace-tiling-v1.xml`. The manager is
//! privileged (OS4): the bar's tiling switch is its one client.

use crate::identity::Privilege;
use crate::state::{granted, State};
use crate::workspace::WorkspaceId;
use protocol::athanor_workspace_tiling_manager_v1::{self, AthanorWorkspaceTilingManagerV1};
use protocol::athanor_workspace_tiling_v1::{self, AthanorWorkspaceTilingV1};
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::ext_workspace_handle_v1::ExtWorkspaceHandleV1;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, WEnum};
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
    use smithay::reexports::wayland_protocols::ext::workspace::v1::server::*;
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_protocols::ext::workspace::v1::server::__interfaces::*;
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-compositor-client/protocols/athanor-workspace-tiling-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-compositor-client/protocols/athanor-workspace-tiling-v1.xml"
    );
}

/// The user data of the manager's global and of every manager.
pub struct WorkspaceTilings;

/// The user data of a tiling object: the workspace it was made for, none for a handle of no
/// workspace. A live object is in `State::workspace_tilings`; one whose workspace is gone is not.
pub struct TilingObject {
    workspace: Option<WorkspaceId>,
}

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorWorkspaceTilingManagerV1, _>(1, WorkspaceTilings);
}

impl GlobalDispatch2<AthanorWorkspaceTilingManagerV1, State> for WorkspaceTilings {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorWorkspaceTilingManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, WorkspaceTilings);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::WorkspaceTiling)
    }
}

impl Dispatch2<AthanorWorkspaceTilingManagerV1, State> for WorkspaceTilings {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &AthanorWorkspaceTilingManagerV1,
        request: athanor_workspace_tiling_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_workspace_tiling_manager_v1::Request;
        match request {
            Request::GetTiling { id, workspace } => {
                // A handle the compositor did not make cannot be: its user data is its own. The
                // object is made all the same, as every new_id must be, and ends at once.
                let workspace = workspace_of(&workspace);
                // One live object per workspace and client bounds what each change walks.
                let made = workspace.is_some() && state.workspace_tilings.iter().any(|kept| {
                    kept.id().same_client_as(&manager.id())
                        && kept
                            .data::<TilingObject>()
                            .is_some_and(|data| data.workspace == workspace)
                });
                let object = data_init.init(id, TilingObject { workspace });
                if made {
                    manager.post_error(
                        athanor_workspace_tiling_manager_v1::Error::AlreadyMade,
                        "a second tiling object of the same workspace",
                    );
                    return;
                }
                match workspace.filter(|id| state.output_of_workspace(*id).is_some()) {
                    Some(workspace) => {
                        object.state(shown_as(state.is_tiled(workspace)));
                        state.workspace_tilings.push(object);
                    }
                    None => object.removed(),
                }
            }
            Request::Destroy => {}
        }
    }
}

impl Dispatch2<AthanorWorkspaceTilingV1, State> for TilingObject {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _object: &AthanorWorkspaceTilingV1,
        request: athanor_workspace_tiling_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_workspace_tiling_v1::{Request, State as Tiling};
        match request {
            // An object whose workspace is gone changes nothing: `set_tiling` tiles no workspace
            // on no output, and `workspaces_changed` floated the workspace when it went.
            Request::SetState {
                state: WEnum::Value(asked),
            } => {
                if let Some(workspace) = self.workspace {
                    state.set_tiling(workspace, asked == Tiling::Tiled);
                }
            }
            Request::SetState { .. } | Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorWorkspaceTilingV1) {
        state.workspace_tilings.retain(|kept| kept != object);
    }
}

/// The workspace a handle of ext-workspace-v1 stands for.
fn workspace_of(handle: &ExtWorkspaceHandleV1) -> Option<WorkspaceId> {
    handle
        .data::<crate::workspace::Handle>()
        .map(crate::workspace::Handle::workspace)
}

fn shown_as(tiled: bool) -> athanor_workspace_tiling_v1::State {
    if tiled {
        athanor_workspace_tiling_v1::State::Tiled
    } else {
        athanor_workspace_tiling_v1::State::Floating
    }
}

impl State {
    /// Sends every live object of `id` whether it is tiled: at each change, whatever made it.
    pub(crate) fn send_tiling(&self, id: WorkspaceId) {
        let tiled = shown_as(self.is_tiled(id));
        for object in &self.workspace_tilings {
            if object
                .data::<TilingObject>()
                .is_some_and(|data| data.workspace == Some(id))
            {
                object.state(tiled);
            }
        }
    }

    /// Ends the live objects whose workspace is gone, after `workspaces_changed` dropped it.
    pub(crate) fn end_gone_tilings(&mut self) {
        let workspaces: Vec<WorkspaceId> = self
            .workspaces
            .iter()
            .flat_map(|strip| strip.ids().iter().copied())
            .collect();
        self.workspace_tilings.retain(|object| {
            let kept = object
                .data::<TilingObject>()
                .and_then(|data| data.workspace)
                .is_some_and(|id| workspaces.contains(&id));
            if !kept {
                object.removed();
            }
            kept
        });
    }
}
