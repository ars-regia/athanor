//! athanor-workspace-v1 (doc_own_stack.md OS5, decision 32): the overview's control of
//! workspaces and of their windows, and the capture source of a workspace. The description is
//! Athanor's own, `system/athanor-sulphur-client/protocols/athanor-workspace-v1.xml`. The
//! manager is privileged (OS4): the overview is its one client. A workspace object is sent its
//! pin and capabilities at each change; a window object is sent what changed of its workspace
//! and its places after each frame, as athanor-toplevel-state-v1's objects are.

use crate::identity::Privilege;
use crate::state::{granted, State};
use crate::workspace::WorkspaceId;
use protocol::athanor_workspace_manager_v1::{self, AthanorWorkspaceManagerV1};
use protocol::athanor_workspace_v1::{self, AthanorWorkspaceV1, Capabilities, State as Flags};
use protocol::athanor_workspace_window_v1::{self, AthanorWorkspaceWindowV1};
use smithay::desktop::Window;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::ext_workspace_handle_v1::ExtWorkspaceHandleV1;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::foreign_toplevel_list::{ForeignToplevelHandle, ForeignToplevelWeakHandle};
use smithay::wayland::image_capture_source::ImageCaptureSourceData;
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::sync::Mutex;

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
    use smithay::reexports::wayland_protocols::ext::image_capture_source::v1::server::*;
    use smithay::reexports::wayland_protocols::ext::workspace::v1::server::*;
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_protocols::ext::foreign_toplevel_list::v1::server::__interfaces::*;
        use smithay::reexports::wayland_protocols::ext::image_capture_source::v1::server::__interfaces::*;
        use smithay::reexports::wayland_protocols::ext::workspace::v1::server::__interfaces::*;
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "../../../../system/athanor-sulphur-client/protocols/athanor-workspace-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!(
        "../../../../system/athanor-sulphur-client/protocols/athanor-workspace-v1.xml"
    );
}

/// A workspace object.
pub type Workspace = AthanorWorkspaceV1;

/// A window object.
pub type WorkspaceWindow = AthanorWorkspaceWindowV1;

/// The user data of the manager's global and of every manager.
pub struct WorkspaceControls;

/// The user data of a workspace object: the workspace it was made for, none for a handle of no
/// workspace, and what it was last sent. A live object is in `State::workspace_objects`; one
/// whose workspace is gone is not.
pub struct WorkspaceObject {
    workspace: Option<WorkspaceId>,
    sent: Mutex<Option<(Flags, Capabilities)>>,
}

/// The user data of a window object: the handle of its window, none for an inert object, and
/// what it was last sent.
pub struct WindowObject {
    handle: Option<ForeignToplevelWeakHandle>,
    sent: Mutex<WindowSent>,
}

/// What a window object was last sent: whether its first `done` was, the handle of the
/// workspace it entered, and its place on each output its client has bound.
#[derive(Default)]
struct WindowSent {
    done: bool,
    workspace: Option<ExtWorkspaceHandleV1>,
    places: Vec<(Output, Rectangle<i32, Logical>)>,
}

/// Offers the manager to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, AthanorWorkspaceManagerV1, _>(1, WorkspaceControls);
}

impl GlobalDispatch2<AthanorWorkspaceManagerV1, State> for WorkspaceControls {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<AthanorWorkspaceManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, WorkspaceControls);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::WorkspaceControls)
    }
}

impl Dispatch2<AthanorWorkspaceManagerV1, State> for WorkspaceControls {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        manager: &AthanorWorkspaceManagerV1,
        request: athanor_workspace_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_workspace_manager_v1::{Error, Request};
        match request {
            Request::GetWorkspace { id, workspace } => {
                // A handle the compositor did not make cannot be: its user data is its own. The
                // object is made all the same, as every new_id must be, and ends at once.
                let workspace = workspace_of(&workspace);
                // One live object per workspace and client bounds what each change walks.
                let made = workspace.is_some()
                    && state.workspace_objects.iter().any(|kept| {
                        kept.id().same_client_as(&manager.id())
                            && kept
                                .data::<WorkspaceObject>()
                                .is_some_and(|data| data.workspace == workspace)
                    });
                let object = data_init.init(
                    id,
                    WorkspaceObject {
                        workspace,
                        sent: Mutex::default(),
                    },
                );
                if made {
                    manager.post_error(Error::AlreadyMade, "a second object of the same workspace");
                    return;
                }
                match workspace.filter(|id| state.output_of_workspace(*id).is_some()) {
                    Some(workspace) => {
                        state.send_workspace_object(&object, workspace);
                        state.workspace_objects.push(object);
                    }
                    None => object.removed(),
                }
            }
            Request::GetWorkspaceWindow { id, toplevel } => {
                // On a closed handle the object is inert, and never sent anything.
                let handle = ForeignToplevelHandle::from_resource(&toplevel)
                    .filter(|handle| !handle.is_closed());
                // One object per window and client bounds what each frame walks.
                let made = handle.as_ref().is_some_and(|handle| {
                    state.workspace_windows.iter().any(|kept| {
                        kept.id().same_client_as(&manager.id())
                            && kept
                                .data::<WindowObject>()
                                .and_then(|data| data.handle.as_ref())
                                .and_then(ForeignToplevelWeakHandle::upgrade)
                                .is_some_and(|own| own.matches(handle))
                    })
                });
                let data = WindowObject {
                    handle: handle
                        .as_ref()
                        .filter(|_| !made)
                        .map(ForeignToplevelHandle::downgrade),
                    sent: Mutex::default(),
                };
                let object = data_init.init(id, data);
                if made {
                    manager.post_error(Error::AlreadyMade, "a second object of the same window");
                } else if handle.is_some() && state.send_workspace_window(&object) {
                    state.workspace_windows.push(object);
                }
            }
            Request::GetCaptureSource { source, workspace } => {
                // A handle of no workspace makes a source whose every session is refused.
                let captured = crate::capture::workspace_source(workspace_of(&workspace));
                let resource = data_init.init(
                    source,
                    ImageCaptureSourceData {
                        source: captured.clone(),
                    },
                );
                captured.add_instance(&resource);
            }
            Request::Destroy => {}
        }
    }
}

impl Dispatch2<AthanorWorkspaceV1, State> for WorkspaceObject {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _object: &AthanorWorkspaceV1,
        request: athanor_workspace_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_workspace_v1::Request;
        // Nothing while the session is locked or paused, as for the toplevel actions, and
        // nothing for a workspace gone.
        let Some(id) = self
            .workspace
            .filter(|id| !state.drops_virtual_input() && state.output_of_workspace(*id).is_some())
        else {
            return;
        };
        match request {
            Request::Pin => {
                if !state.pinned.contains(&id) {
                    state.pinned.push(id);
                }
                state.send_workspace_objects();
            }
            Request::Unpin => {
                state.pinned.retain(|kept| *kept != id);
                // An empty workspace unpinned goes, as any emptied workspace does.
                state.workspaces_changed();
            }
            Request::MoveBefore { other } => {
                if let Some(other) = workspace_of(&other) {
                    state.move_workspace(id, other, false);
                }
            }
            Request::MoveAfter { other } => {
                if let Some(other) = workspace_of(&other) {
                    state.move_workspace(id, other, true);
                }
            }
            Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorWorkspaceV1) {
        state.workspace_objects.retain(|kept| kept != object);
    }
}

impl Dispatch2<AthanorWorkspaceWindowV1, State> for WindowObject {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _object: &AthanorWorkspaceWindowV1,
        request: athanor_workspace_window_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        use athanor_workspace_window_v1::Request;
        match request {
            Request::MoveToWorkspace { workspace, output } => {
                let Some(window) = self
                    .handle
                    .as_ref()
                    .and_then(ForeignToplevelWeakHandle::upgrade)
                    .and_then(|handle| crate::toplevel_state::acted_on_handle(state, &handle))
                else {
                    return;
                };
                let Some(id) = workspace_of(&workspace) else {
                    return;
                };
                // The output names the workspace's own; any other is refused.
                if Output::from_resource(&output)
                    .is_some_and(|output| state.output_of_workspace(id).as_ref() == Some(&output))
                {
                    state.move_window_to(&window, id);
                }
            }
            Request::Destroy => {}
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &AthanorWorkspaceWindowV1) {
        state.workspace_windows.retain(|kept| kept != object);
    }
}

/// The workspace a handle of ext-workspace-v1 stands for.
fn workspace_of(handle: &ExtWorkspaceHandleV1) -> Option<WorkspaceId> {
    handle
        .data::<crate::workspace::Handle>()
        .map(crate::workspace::Handle::workspace)
}

impl State {
    /// Sends `object` what changed of the workspace `id`'s pin and capabilities: every
    /// workspace can be pinned, and every one but the empty one kept last can move.
    fn send_workspace_object(&self, object: &AthanorWorkspaceV1, id: WorkspaceId) {
        let Some(data) = object.data::<WorkspaceObject>() else {
            return;
        };
        let mut flags = Flags::empty();
        flags.set(Flags::Pinned, self.pinned.contains(&id));
        let mut capabilities = Capabilities::Pin;
        capabilities.set(Capabilities::Move, !self.kept_last(id));
        let Ok(mut sent) = data.sent.lock() else {
            tracing::warn!("a workspace object's record is poisoned: not sent");
            return;
        };
        let (was_flags, was_capabilities) = match *sent {
            Some((flags, capabilities)) => (Some(flags), Some(capabilities)),
            None => (None, None),
        };
        if was_flags != Some(flags) {
            object.state(flags);
        }
        if was_capabilities != Some(capabilities) {
            object.capabilities(capabilities);
        }
        *sent = Some((flags, capabilities));
    }

    /// Sends each live workspace object what changed, at each change of the workspaces; an
    /// object whose workspace is gone is sent `removed` and dropped.
    pub(crate) fn send_workspace_objects(&mut self) {
        let objects = std::mem::take(&mut self.workspace_objects);
        self.workspace_objects = objects
            .into_iter()
            .filter(|object| {
                let live = object
                    .data::<WorkspaceObject>()
                    .and_then(|data| data.workspace)
                    .filter(|id| self.output_of_workspace(*id).is_some());
                match live {
                    Some(id) => self.send_workspace_object(object, id),
                    None => object.removed(),
                }
                live.is_some()
            })
            .collect();
    }

    /// Sends each live window object what changed of its window since its last `done`. Run
    /// with `send_toplevel_states`: after each frame, which catches what moved, and when a
    /// client binds an output. Objects destroyed, or whose window is gone, are dropped.
    pub(crate) fn send_workspace_windows(&mut self) {
        let objects = std::mem::take(&mut self.workspace_windows);
        self.workspace_windows = objects
            .into_iter()
            .filter(|object| self.send_workspace_window(object))
            .collect();
    }

    /// The places of `window`, each relative to its output's top left: a window in the space
    /// on each output it overlaps, a parked or minimised one where it comes back, on its
    /// workspace's output.
    fn places(&self, window: &Window) -> Vec<(Output, Rectangle<i32, Logical>)> {
        if let Some(geometry) = self.space.element_geometry(window) {
            return self
                .space
                .outputs()
                .filter_map(|output| {
                    let area = self.space.output_geometry(output)?;
                    area.overlaps(geometry).then(|| {
                        (
                            output.clone(),
                            Rectangle::new(geometry.loc - area.loc, geometry.size),
                        )
                    })
                })
                .collect();
        }
        let kept = self.parked_at(window).and_then(|at| {
            let output = self.output_of_workspace(self.workspace_of(window)?)?;
            let area = self.space.output_geometry(&output)?;
            Some((
                output,
                Rectangle::new(at - area.loc, window.geometry().size),
            ))
        });
        kept.into_iter().collect()
    }

    /// Sends `object` what changed of its window: the workspace it left and entered, the
    /// outputs it was withdrawn from, its new places, then `done`; false when the object or the
    /// window is gone.
    fn send_workspace_window(&self, object: &AthanorWorkspaceWindowV1) -> bool {
        let (Some(data), Some(client)) = (object.data::<WindowObject>(), object.client()) else {
            return false;
        };
        let Some(handle) = data
            .handle
            .as_ref()
            .and_then(ForeignToplevelWeakHandle::upgrade)
            .filter(|handle| !handle.is_closed())
        else {
            return false;
        };
        let Some(window) = self.window_by_handle(&handle) else {
            return false;
        };
        // Only a handle the client holds is sent, so that a later bind of ext-workspace-v1 sends
        // it; and only an output it has bound counts as sent, so that a later bind sends it.
        let workspace = self
            .workspace_of(&window)
            .and_then(|id| self.workspace_handle_for(&client.id(), id));
        let places: Vec<(Output, Rectangle<i32, Logical>)> = self
            .places(&window)
            .into_iter()
            .filter(|(output, _)| output.client_outputs(&client).next().is_some())
            .collect();
        let Ok(mut sent) = data.sent.lock() else {
            tracing::warn!("a workspace window object's record is poisoned: dropped");
            return false;
        };
        let mut changed = !sent.done;
        if sent.workspace != workspace {
            if let Some(left) = sent.workspace.take().filter(Resource::is_alive) {
                object.workspace_leave(&left);
            }
            if let Some(entered) = &workspace {
                object.workspace_enter(entered);
            }
            sent.workspace = workspace;
            changed = true;
        }
        for (output, _) in sent
            .places
            .iter()
            .filter(|(output, _)| !places.iter().any(|(now, _)| now == output))
        {
            for wl_output in output.client_outputs(&client) {
                object.geometry(&wl_output, 0, 0, 0, 0);
            }
            changed = true;
        }
        for (output, place) in places.iter().filter(|place| !sent.places.contains(place)) {
            for wl_output in output.client_outputs(&client) {
                object.geometry(
                    &wl_output,
                    place.loc.x,
                    place.loc.y,
                    place.size.w,
                    place.size.h,
                );
            }
            changed = true;
        }
        sent.places = places;
        if changed {
            object.done();
            sent.done = true;
        }
        true
    }
}
