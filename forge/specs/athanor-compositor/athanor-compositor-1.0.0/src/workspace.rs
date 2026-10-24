//! ext-workspace-v1 (doc_own_stack.md, C1 task 4): each output is one workspace group with one
//! workspace, active. The compositor offers no capability, so the clients' requests change
//! nothing.
// ponytail: one fixed workspace per output; groups and workspaces follow the outputs and the
// user's workspaces once the compositor has them (task 8 hot-plug, then the shell's switcher).

use crate::identity::Privilege;
use crate::state::{granted, State};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1, GroupCapabilities},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1, WorkspaceCapabilities},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::wayland::{Dispatch2, GlobalDispatch2};

/// The user data of the global and of every object of the protocol.
pub struct Workspaces;

/// A workspace group sent to a client: its manager and the output it stands for.
pub struct Group {
    manager: ExtWorkspaceManagerV1,
    group: ExtWorkspaceGroupHandleV1,
    output: Output,
}

/// Offers the global to the clients the allow-list grants it: a client may switch workspaces
/// through it (open question 7 of the OS9 plan).
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, ExtWorkspaceManagerV1, _>(1, Workspaces);
}

impl GlobalDispatch2<ExtWorkspaceManagerV1, State> for Workspaces {
    fn bind(
        &self,
        state: &mut State,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        let manager = data_init.init(resource, Workspaces);
        let outputs: Vec<Output> = state.space.outputs().cloned().collect();
        for output in outputs {
            match announce(dh, client, &manager, &output) {
                Ok(group) => state.workspace_groups.push(Group {
                    manager: manager.clone(),
                    group,
                    output,
                }),
                Err(error) => tracing::warn!(%error, "a workspace group could not be sent"),
            }
        }
        manager.done();
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::Workspaces)
    }
}

/// Sends `output`'s group and its one workspace, active, to `manager`'s client.
fn announce(
    dh: &DisplayHandle,
    client: &Client,
    manager: &ExtWorkspaceManagerV1,
    output: &Output,
) -> Result<ExtWorkspaceGroupHandleV1, smithay::reexports::wayland_server::backend::InvalidId> {
    let version = manager.version();
    let group =
        client.create_resource::<ExtWorkspaceGroupHandleV1, _, State>(dh, version, Workspaces)?;
    manager.workspace_group(&group);
    group.capabilities(GroupCapabilities::empty());
    for wl_output in output.client_outputs(client) {
        group.output_enter(&wl_output);
    }
    let workspace =
        client.create_resource::<ExtWorkspaceHandleV1, _, State>(dh, version, Workspaces)?;
    manager.workspace(&workspace);
    workspace.id(format!("{}-1", output.name()));
    workspace.name("1".into());
    workspace.state(ext_workspace_handle_v1::State::Active);
    workspace.capabilities(WorkspaceCapabilities::empty());
    group.workspace_enter(&workspace);
    Ok(group)
}

impl State {
    /// A client bound `wl_output` after the workspace manager: its groups of that output enter it.
    pub fn workspace_output_bound(&mut self, output: &Output, wl_output: &WlOutput) {
        let client = wl_output.client();
        self.workspace_groups.retain(|group| group.group.is_alive());
        for group in &self.workspace_groups {
            if &group.output == output && group.group.client() == client {
                group.group.output_enter(wl_output);
                group.manager.done();
            }
        }
    }
}

impl Dispatch2<ExtWorkspaceManagerV1, State> for Workspaces {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &ExtWorkspaceManagerV1,
        request: ext_workspace_manager_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        // `commit` applies nothing: no request changes a workspace.
        if let ext_workspace_manager_v1::Request::Stop = request {
            state
                .workspace_groups
                .retain(|group| &group.manager != manager);
            manager.finished();
        }
    }

    fn destroyed(
        &self,
        state: &mut State,
        _client: smithay::reexports::wayland_server::backend::ClientId,
        manager: &ExtWorkspaceManagerV1,
    ) {
        state
            .workspace_groups
            .retain(|group| &group.manager != manager);
    }
}

impl Dispatch2<ExtWorkspaceGroupHandleV1, State> for Workspaces {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        group: &ExtWorkspaceGroupHandleV1,
        request: ext_workspace_group_handle_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        // `create_workspace` is not among the group's capabilities.
        if let ext_workspace_group_handle_v1::Request::Destroy = request {
            state.workspace_groups.retain(|kept| &kept.group != group);
        }
    }
}

impl Dispatch2<ExtWorkspaceHandleV1, State> for Workspaces {
    /// None of `activate`, `deactivate`, `assign` and `remove` is among the capabilities.
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _workspace: &ExtWorkspaceHandleV1,
        _request: ext_workspace_handle_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }
}
