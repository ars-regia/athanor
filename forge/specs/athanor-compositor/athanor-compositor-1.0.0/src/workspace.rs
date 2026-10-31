//! Dynamic workspaces (doc_own_stack.md OS2) and ext-workspace-v1 (OS9). Each output has its own
//! list of workspaces, one of them active, and always an empty one last, as cosmic-comp keeps
//! them: a window placed on the last one adds an empty one after it, and a workspace emptied that
//! is neither the last nor the active one is removed; the active one emptied stays until it is
//! left. The windows of an inactive workspace leave the space, keeping their place, and come back
//! there when it is activated again, at once: no animation. Each output is a workspace group of
//! the protocol, each workspace offers `activate`, applied at the manager's `commit`, and its
//! coordinates are its place in its group.
// ponytail: no pinned workspace and no reordering yet; the overview's private interface adds them
// (doc_overview.md, plan task 5).

use crate::identity::Privilege;
use crate::state::{centred, granted, State};
use smithay::backend::input::InputTime;
use smithay::desktop::{find_popup_root_surface, PopupManager, Window};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1, GroupCapabilities},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1, WorkspaceCapabilities},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use smithay::reexports::wayland_server::backend::InvalidId;
use smithay::reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::cell::Cell;

/// A workspace, by a number the compositor never gives another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceId(u64);

/// The workspace a window is on, in its user data from its placement on.
struct OnWorkspace(Cell<WorkspaceId>);

/// The workspace `window` is on; none before it is placed.
fn on(window: &Window) -> Option<WorkspaceId> {
    window.user_data().get::<OnWorkspace>().map(|on| on.0.get())
}

/// A step along an output's workspaces, from the keyboard (OV13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Previous,
    Next,
}

/// The workspaces of an output, in order, and the place of the active one.
pub struct Strip {
    output: Output,
    list: Vec<WorkspaceId>,
    active: usize,
}

impl Strip {
    fn active_id(&self) -> Option<WorkspaceId> {
        self.list.get(self.active).copied()
    }
}

/// The user data of the global, of the managers and of the groups.
pub struct Workspaces;

/// The user data of a workspace's handle: the workspace it stands for.
pub struct Handle(WorkspaceId);

/// What a client of the protocol was sent: its manager, a group per output and a handle per
/// workspace, with the activations it asked for since its last `commit`.
pub struct Bound {
    manager: ExtWorkspaceManagerV1,
    groups: Vec<(Output, ExtWorkspaceGroupHandleV1)>,
    sent: Vec<Sent>,
    asked: Vec<WorkspaceId>,
}

/// A workspace's handle, with the place and the state its client was last sent.
struct Sent {
    id: WorkspaceId,
    output: Output,
    handle: ExtWorkspaceHandleV1,
    index: usize,
    active: bool,
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
        _client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        let manager = data_init.init(resource, Workspaces);
        let mut bound = Bound {
            manager: manager.clone(),
            groups: Vec::new(),
            sent: Vec::new(),
            asked: Vec::new(),
        };
        match bound.send(dh, &state.workspaces) {
            // A compositor with no output yet has sent nothing: `done` all the same.
            Ok(false) => manager.done(),
            Ok(true) => {}
            Err(error) => tracing::warn!(%error, "the workspaces could not be sent"),
        }
        state.workspace_clients.push(bound);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::Workspaces)
    }
}

/// A workspace's state as the protocol sends it.
fn shown_as(active: bool) -> ext_workspace_handle_v1::State {
    if active {
        ext_workspace_handle_v1::State::Active
    } else {
        ext_workspace_handle_v1::State::empty()
    }
}

/// A workspace's name and coordinates for its place in its group: its number from one, and its
/// place from zero.
fn send_place(handle: &ExtWorkspaceHandleV1, index: usize) {
    handle.name((index + 1).to_string());
    let at = u32::try_from(index).unwrap_or(u32::MAX);
    handle.coordinates(at.to_ne_bytes().to_vec());
}

impl Bound {
    /// Sends the client what changed of `strips` since it was last sent, and `done` after it:
    /// whether anything was sent. A group or a workspace that went is left and removed first,
    /// as the protocol orders it.
    fn send(&mut self, dh: &DisplayHandle, strips: &[Strip]) -> Result<bool, InvalidId> {
        let Some(client) = self.manager.client() else {
            return Ok(false);
        };
        let version = self.manager.version();
        let mut changed = false;
        let groups = &self.groups;
        self.sent.retain(|sent| {
            let kept = strips
                .iter()
                .any(|strip| strip.output == sent.output && strip.list.contains(&sent.id));
            if !kept {
                if let Some((_, group)) = groups.iter().find(|(output, _)| *output == sent.output) {
                    group.workspace_leave(&sent.handle);
                }
                sent.handle.removed();
                changed = true;
            }
            kept
        });
        self.groups.retain(|(output, group)| {
            let kept = strips.iter().any(|strip| strip.output == *output);
            if !kept {
                group.removed();
                changed = true;
            }
            kept
        });
        for strip in strips {
            let group = match self
                .groups
                .iter()
                .find(|(output, _)| *output == strip.output)
            {
                Some((_, group)) => group.clone(),
                None => {
                    let group = client.create_resource::<ExtWorkspaceGroupHandleV1, _, State>(
                        dh, version, Workspaces,
                    )?;
                    self.manager.workspace_group(&group);
                    group.capabilities(GroupCapabilities::empty());
                    for wl_output in strip.output.client_outputs(&client) {
                        group.output_enter(&wl_output);
                    }
                    self.groups.push((strip.output.clone(), group.clone()));
                    changed = true;
                    group
                }
            };
            for (index, id) in strip.list.iter().enumerate() {
                let active = index == strip.active;
                if let Some(sent) = self.sent.iter_mut().find(|sent| sent.id == *id) {
                    if sent.index != index {
                        send_place(&sent.handle, index);
                        sent.index = index;
                        changed = true;
                    }
                    if sent.active != active {
                        sent.handle.state(shown_as(active));
                        sent.active = active;
                        changed = true;
                    }
                    continue;
                }
                let handle = client.create_resource::<ExtWorkspaceHandleV1, _, State>(
                    dh,
                    version,
                    Handle(*id),
                )?;
                self.manager.workspace(&handle);
                send_place(&handle, index);
                handle.state(shown_as(active));
                handle.capabilities(WorkspaceCapabilities::Activate);
                group.workspace_enter(&handle);
                self.sent.push(Sent {
                    id: *id,
                    output: strip.output.clone(),
                    handle,
                    index,
                    active,
                });
                changed = true;
            }
        }
        if changed {
            self.manager.done();
        }
        Ok(changed)
    }
}

impl State {
    /// The workspaces of `output`, in order; none for an output not in the space. The overview
    /// reads it (doc_overview.md).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn workspaces(&self, output: &Output) -> &[WorkspaceId] {
        self.workspaces
            .iter()
            .find(|strip| strip.output == *output)
            .map_or(&[], |strip| &strip.list)
    }

    /// The active workspace of `output`, if it is in the space.
    pub fn active_workspace(&self, output: &Output) -> Option<WorkspaceId> {
        self.workspaces
            .iter()
            .find(|strip| strip.output == *output)
            .and_then(Strip::active_id)
    }

    /// The workspace `window` is on; none before it is placed.
    pub fn workspace_of(&self, window: &Window) -> Option<WorkspaceId> {
        on(window)
    }

    /// The output `id` belongs to.
    pub fn output_of_workspace(&self, id: WorkspaceId) -> Option<Output> {
        self.workspaces
            .iter()
            .find(|strip| strip.list.contains(&id))
            .map(|strip| strip.output.clone())
    }

    /// Whether `window` is on the active workspace of its output, or on none yet.
    pub fn on_active_workspace(&self, window: &Window) -> bool {
        self.workspace_of(window).is_none_or(|id| {
            self.output_of_workspace(id)
                .and_then(|output| self.active_workspace(&output))
                == Some(id)
        })
    }

    /// The output of `window`'s workspace while that output is in the space, else the first
    /// output, whose active workspace `workspaces_changed` gives the window.
    pub(crate) fn home_output(&self, window: &Window) -> Option<Output> {
        self.workspace_of(window)
            .and_then(|id| self.output_of_workspace(id))
            .filter(|output| self.space.output_geometry(output).is_some())
            .or_else(|| self.space.outputs().next().cloned())
    }

    /// Whether `area` of `window` is shown on its home output (`home_output`).
    pub fn shown_at_home(&self, window: &Window, area: Rectangle<i32, Logical>) -> bool {
        self.home_output(window)
            .and_then(|output| self.space.output_geometry(&output))
            .is_some_and(|shown| shown.overlaps(area))
    }

    /// What the layer surfaces leave of `window`'s home output (`home_output`): where it is
    /// maximised, and centred when its place is not shown.
    pub fn home_area(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        self.home_output(window)
            .and_then(|output| self.window_area_on(&output))
    }

    /// Whether `window` is on an inactive workspace, out of the space.
    pub fn is_parked(&self, window: &Window) -> bool {
        self.parked.iter().any(|(kept, _)| kept == window)
    }

    /// Puts `window`, placed now on the first output, on that output's active workspace.
    pub fn place_on_workspace(&mut self, window: &Window) {
        let Some(id) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.active_workspace(output))
        else {
            return;
        };
        window
            .user_data()
            .insert_if_missing(|| OnWorkspace(Cell::new(id)));
        self.workspaces_changed();
    }

    /// Puts `window`, dropped by a move, on the active workspace of the output under its centre
    /// when that is another output's than its workspace's: it stays where it was dropped.
    pub fn settle(&mut self, window: &Window) {
        let (Some(on), Some(bbox)) = (
            window.user_data().get::<OnWorkspace>(),
            self.space.element_bbox(window),
        ) else {
            return;
        };
        let centre = Point::<i32, Logical>::from((
            bbox.loc.x.saturating_add(bbox.size.w / 2),
            bbox.loc.y.saturating_add(bbox.size.h / 2),
        ));
        let Some(output) = self.space.output_under(centre.to_f64()).next().cloned() else {
            return;
        };
        if self.output_of_workspace(on.0.get()).as_ref() == Some(&output) {
            return;
        }
        if let Some(id) = self.active_workspace(&output) {
            on.0.set(id);
            self.workspaces_changed();
        }
    }

    /// Activates `id` on its output: the windows of the workspace left go out of the space, those
    /// of `id` come back where they were, and the keyboard goes to the top window. False when no
    /// output has `id`.
    pub fn activate_workspace(&mut self, id: WorkspaceId) -> bool {
        let Some((strip, index)) = self.workspaces.iter_mut().find_map(|strip| {
            let index = strip.list.iter().position(|kept| *kept == id)?;
            Some((strip, index))
        }) else {
            return false;
        };
        let left = strip.active_id();
        if left == Some(id) {
            return true;
        }
        strip.active = index;
        if let Some(left) = left {
            let leaving: Vec<Window> = self
                .space
                .elements()
                .filter(|window| self.workspace_of(window) == Some(left))
                .cloned()
                .collect();
            self.park(&leaving);
        }
        self.unpark(|window| on(window) == Some(id));
        self.focus_top();
        self.workspaces_changed();
        true
    }

    /// Moves `window` to the workspace `id`: it leaves the space for an inactive one and comes
    /// back for the active one; onto another output it is centred there. A minimised window stays
    /// minimised. False when no output has `id`, or the window is not placed yet.
    pub fn move_window_to(&mut self, window: &Window, id: WorkspaceId) -> bool {
        let Some(strip) = self
            .workspaces
            .iter()
            .find(|strip| strip.list.contains(&id))
        else {
            return false;
        };
        let (output, active) = (strip.output.clone(), strip.active_id() == Some(id));
        let Some(on) = window.user_data().get::<OnWorkspace>() else {
            return false;
        };
        let from = on.0.replace(id);
        if from == id {
            return true;
        }
        if self.output_of_workspace(from).as_ref() != Some(&output) {
            if let Some(at) = self
                .window_area_on(&output)
                .map(|area| centred(area, window.geometry().size))
            {
                self.space.relocate_element(window, at);
                for (kept, place) in self.minimised.iter_mut().chain(self.parked.iter_mut()) {
                    if kept == window {
                        *place = at;
                    }
                }
            }
        }
        let mapped = self.space.element_location(window).is_some();
        if mapped && !active {
            self.park(std::slice::from_ref(window));
            if let Some(toplevel) = window.toplevel() {
                self.refocus(toplevel.wl_surface());
            }
        } else if !mapped && active {
            self.unpark(|kept| kept == window);
        }
        self.workspaces_changed();
        true
    }

    /// The window with the keyboard focus, if a window or a popup of one has it.
    pub(crate) fn focused_window(&self) -> Option<Window> {
        let surface = self.seat.get_keyboard()?.current_focus()?;
        let root = match self.popups.find_popup(&surface) {
            Some(popup) => find_popup_root_surface(&popup).ok()?,
            None => surface,
        };
        self.window_for(&root)
    }

    /// The workspaces of the output the workspace keys act on: the focused window's, else the
    /// first output's.
    fn keyed_strip(&self) -> Option<&Strip> {
        let output = self
            .focused_window()
            .and_then(|window| self.workspace_of(&window))
            .and_then(|id| self.output_of_workspace(id))
            .or_else(|| self.space.outputs().next().cloned())?;
        self.workspaces.iter().find(|strip| strip.output == output)
    }

    /// The `n`-th workspace, from one, of the output the keys act on; its last when `n` is 0 or
    /// past it.
    fn nth_workspace(&self, n: u8) -> Option<WorkspaceId> {
        let list = &self.keyed_strip()?.list;
        usize::from(n)
            .checked_sub(1)
            .and_then(|index| list.get(index))
            .or(list.last())
            .copied()
    }

    /// The workspace one `step` from the active one of the output the keys act on; none past
    /// either end.
    fn stepped(&self, step: Step) -> Option<WorkspaceId> {
        let strip = self.keyed_strip()?;
        let to = match step {
            Step::Previous => strip.active.checked_sub(1)?,
            Step::Next => strip.active + 1,
        };
        strip.list.get(to).copied()
    }

    /// Super+1 to Super+9 and Super+0: activates the `n`-th workspace (`nth_workspace`).
    pub(crate) fn switch_to(&mut self, n: u8) {
        if let Some(id) = self.nth_workspace(n) {
            self.activate_workspace(id);
        }
    }

    /// Super+Shift+1 to Super+Shift+9 and Super+Shift+0: the focused window moves to the `n`-th
    /// workspace, which is activated with the window on top and the keyboard.
    pub(crate) fn send_focused_to(&mut self, n: u8) {
        if let Some(id) = self.nth_workspace(n) {
            self.carry_to(id);
        }
    }

    /// Super+Ctrl and an arrow or h, j, k, l: activates the workspace one `step` away.
    pub(crate) fn step(&mut self, step: Step) {
        if let Some(id) = self.stepped(step) {
            self.activate_workspace(id);
        }
    }

    /// Super+Shift+Ctrl and an arrow or h, j, k, l: the focused window is carried one `step`.
    pub(crate) fn carry(&mut self, step: Step) {
        if let Some(id) = self.stepped(step) {
            self.carry_to(id);
        }
    }

    /// Moves the focused window to `id` and activates it, the window on top with the keyboard.
    fn carry_to(&mut self, id: WorkspaceId) {
        let Some(window) = self.focused_window() else {
            return;
        };
        if self.move_window_to(&window, id) && self.activate_workspace(id) {
            self.focus(&window);
        }
    }

    /// Takes `windows` out of the space, each keeping its place, bottom to top.
    fn park(&mut self, windows: &[Window]) {
        for window in windows {
            let Some(at) = self.space.element_location(window) else {
                continue;
            };
            self.end_window_grab(window);
            self.dismiss_popups(window);
            window.set_activated(false);
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_pending_configure();
            }
            self.space.unmap_elem(window);
            self.parked.push((window.clone(), at));
        }
        // A button goes to the pointer's focus: a click without a motion must miss the windows.
        self.refresh_pointer();
        self.request_redraw();
    }

    /// Dismisses the popups of `window` and ends the keyboard and pointer grabs they hold: while
    /// a popup grab lasts, Smithay keeps the keyboard on the popup whatever focus is set. The
    /// keyboard is left with no focus, not on the popup, and every caller then gives it to the
    /// window on top (`refocus`, `focus_top`): handing it back to `window`, as Smithay's own end
    /// of a popup grab does, would send an enter to a window about to be hidden.
    pub(crate) fn dismiss_popups(&mut self, window: &Window) {
        if let Some(root) = window.toplevel().map(|toplevel| toplevel.wl_surface().clone()) {
            self.dismiss_popups_of(&root);
        }
    }

    /// `dismiss_popups` for the popups whose root is the surface `root`.
    pub(crate) fn dismiss_popups_of(&mut self, root: &WlSurface) {
        let root = root.clone();
        for (popup, _) in PopupManager::popups_for_surface(&root) {
            if let Err(error) = PopupManager::dismiss_popup(&root, &popup) {
                tracing::warn!(?error, "a popup could not be dismissed");
            }
        }
        if let Some(keyboard) = self.seat.get_keyboard() {
            let rooted = keyboard.with_grab(|_, grab| grab.start_data().focus.as_ref() == Some(&root));
            if rooted == Some(true) {
                keyboard.unset_grab(self);
                self.set_keyboard_focus(None);
            }
        }
        if let Some(pointer) = self.seat.get_pointer() {
            let rooted = pointer.with_grab(|_, grab| {
                grab.start_data().focus.as_ref().map(|(surface, _)| surface) == Some(&root)
            });
            if rooted == Some(true) {
                pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), InputTime::now());
            }
        }
    }

    /// Brings back the parked windows `which` takes where they were, in their order, or centred
    /// on their workspace's output when it no longer shows that place (`reachable`).
    fn unpark(&mut self, which: impl Fn(&Window) -> bool) {
        let (back, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.parked)
            .into_iter()
            .partition(|(window, _)| which(window));
        self.parked = kept;
        if back.is_empty() {
            return;
        }
        for (window, at) in back {
            let at = self.reachable(&window, at);
            self.space.map_element(window.clone(), at, false);
            // A move answered while it was out applies now.
            self.relocated(&window);
        }
        self.fit_maximised();
        self.refresh_pointer();
        self.request_redraw();
    }

    /// After a window was placed, moved or destroyed, or the outputs changed: each output has
    /// its list, a window whose output went joins the first output's active workspace, the
    /// emptied workspaces go, an empty one stays last, and the clients are sent the change.
    pub fn workspaces_changed(&mut self) {
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        self.workspaces
            .retain(|strip| outputs.contains(&strip.output));
        for output in outputs {
            if !self.workspaces.iter().any(|strip| strip.output == output) {
                self.next_workspace += 1;
                self.workspaces.push(Strip {
                    output,
                    list: vec![WorkspaceId(self.next_workspace)],
                    active: 0,
                });
            }
        }
        if let Some(home) = self.workspaces.first().and_then(Strip::active_id) {
            let mut moved = false;
            for window in self.windows() {
                if let Some(on) = window.user_data().get::<OnWorkspace>() {
                    let id = on.0.get();
                    if !self.workspaces.iter().any(|strip| strip.list.contains(&id)) {
                        on.0.set(home);
                        moved = true;
                    }
                }
            }
            if moved {
                self.unpark(|window| on(window) == Some(home));
            }
        }
        let used: Vec<WorkspaceId> = self
            .windows()
            .filter_map(|window| self.workspace_of(window))
            .collect();
        for strip in &mut self.workspaces {
            let active = strip.active_id();
            let last = strip.list.last().copied();
            strip
                .list
                .retain(|id| used.contains(id) || Some(*id) == active || Some(*id) == last);
            strip.active = strip
                .list
                .iter()
                .position(|id| Some(*id) == active)
                .unwrap_or(0);
            if strip.list.last().is_none_or(|id| used.contains(id)) {
                self.next_workspace += 1;
                strip.list.push(WorkspaceId(self.next_workspace));
            }
        }
        self.send_workspaces();
    }

    /// Sends every client of the protocol what changed; the managers gone are dropped.
    fn send_workspaces(&mut self) {
        let dh = self.display_handle.clone();
        let mut clients = std::mem::take(&mut self.workspace_clients);
        clients.retain(|bound| bound.manager.is_alive());
        for bound in &mut clients {
            if let Err(error) = bound.send(&dh, &self.workspaces) {
                tracing::warn!(%error, "the workspaces could not be sent");
            }
        }
        self.workspace_clients = clients;
    }

    /// A client bound `wl_output` after the workspace manager: its groups of that output enter it.
    pub fn workspace_output_bound(&mut self, output: &Output, wl_output: &WlOutput) {
        let client = wl_output.client();
        self.workspace_clients
            .retain(|bound| bound.manager.is_alive());
        for bound in &self.workspace_clients {
            if bound.manager.client() != client {
                continue;
            }
            for (_, group) in bound.groups.iter().filter(|(kept, _)| kept == output) {
                group.output_enter(wl_output);
                bound.manager.done();
            }
        }
    }

    /// Where a parked or minimised window at `at`, its geometry's top left, comes back: there, or centred on its
    /// workspace's output when that output no longer shows that place (`home_area`).
    pub fn reachable(&self, window: &Window, at: Point<i32, Logical>) -> Point<i32, Logical> {
        let size = window.geometry().size;
        let kept = Rectangle::new(at, size);
        match self.home_area(window) {
            Some(area) if !self.shown_at_home(window, kept) => centred(area, size),
            _ => at,
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
        match request {
            ext_workspace_manager_v1::Request::Commit => {
                let asked = state
                    .workspace_clients
                    .iter_mut()
                    .find(|bound| bound.manager == *manager)
                    .map(|bound| std::mem::take(&mut bound.asked))
                    .unwrap_or_default();
                // As the toplevel actions: nothing changes where the user cannot see it (L14).
                if state.drops_virtual_input() {
                    if !asked.is_empty() {
                        tracing::debug!("a workspace activation refused: the session is locked");
                    }
                    return;
                }
                for id in asked {
                    state.activate_workspace(id);
                }
            }
            ext_workspace_manager_v1::Request::Stop => {
                state
                    .workspace_clients
                    .retain(|bound| bound.manager != *manager);
                manager.finished();
            }
            _ => {}
        }
    }

    fn destroyed(
        &self,
        state: &mut State,
        _client: smithay::reexports::wayland_server::backend::ClientId,
        manager: &ExtWorkspaceManagerV1,
    ) {
        state
            .workspace_clients
            .retain(|bound| bound.manager != *manager);
    }
}

impl Dispatch2<ExtWorkspaceGroupHandleV1, State> for Workspaces {
    /// `create_workspace` is not among the group's capabilities; a group destroyed is sent
    /// nothing more, its record kept so that it is not announced again.
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _group: &ExtWorkspaceGroupHandleV1,
        _request: ext_workspace_group_handle_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch2<ExtWorkspaceHandleV1, State> for Handle {
    /// `activate` waits for the manager's `commit`; `deactivate`, `assign` and `remove` are not
    /// among the capabilities.
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        workspace: &ExtWorkspaceHandleV1,
        request: ext_workspace_handle_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        if let ext_workspace_handle_v1::Request::Activate = request {
            if let Some(bound) = state
                .workspace_clients
                .iter_mut()
                .find(|bound| bound.sent.iter().any(|sent| sent.handle == *workspace))
            {
                bound.asked.push(self.0);
            }
        }
    }
}
