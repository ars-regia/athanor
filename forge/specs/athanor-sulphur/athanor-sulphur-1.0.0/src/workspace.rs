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

use crate::handlers::shown;
use crate::identity::Privilege;
use crate::state::{centred, granted, kept_before, place_again, relocate, set_kept_before, State};
use crate::tiling::Tree;
use smithay::backend::input::InputTime;
use smithay::desktop::{find_popup_root_surface, PopupManager, Window};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::workspace::v1::server::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1, GroupCapabilities},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1, WorkspaceCapabilities},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::InvalidId;
use smithay::reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::utils::{Logical, Point, Rectangle, Serial, SERIAL_COUNTER};
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

/// The states a tile is configured with, so that its client squares its corners (D8).
pub(crate) const TILED: [xdg_toplevel::State; 4] = [
    xdg_toplevel::State::TiledLeft,
    xdg_toplevel::State::TiledRight,
    xdg_toplevel::State::TiledTop,
    xdg_toplevel::State::TiledBottom,
];

/// Where a tile stood floating, from its entry in a tree, for the end of the tiling (D7); empty
/// for a window born tiled, which `placed` places then.
#[derive(Default)]
struct Floating(Cell<Option<Rectangle<i32, Logical>>>);

/// D8 in `window`'s pending state, out of its tree: no tiled states, and the size it had
/// floating, unless it is maximised or full screen. That geometry, which it no longer keeps;
/// empty for a window born tiled.
fn floating_state(window: &Window) -> Rectangle<i32, Logical> {
    let floating = window
        .user_data()
        .get::<Floating>()
        .and_then(|kept| kept.0.take())
        .unwrap_or_default();
    let sized = kept_before(window).is_none();
    if let Some(toplevel) = window.toplevel() {
        toplevel.with_pending_state(|state| {
            for tiled in TILED {
                state.states.unset(tiled);
            }
            if sized {
                state.size = (!floating.is_empty()).then_some(floating.size);
            }
        });
    }
    floating
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

    /// The workspaces of the output, in order.
    pub(crate) fn ids(&self) -> &[WorkspaceId] {
        &self.list
    }
}

/// The user data of the global, of the managers and of the groups.
pub struct Workspaces;

/// The user data of a workspace's handle: the workspace it stands for.
pub struct Handle(WorkspaceId);

impl Handle {
    pub(crate) fn workspace(&self) -> WorkspaceId {
        self.0
    }
}

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

    /// The whole of `window`'s home output (`home_output`): where it is full-screen.
    pub(crate) fn home_output_area(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        self.home_output(window)
            .and_then(|output| self.space.output_geometry(&output))
    }

    /// Whether `area` of `window` is shown on its home output (`home_output`).
    pub fn shown_at_home(&self, window: &Window, area: Rectangle<i32, Logical>) -> bool {
        self.home_output_area(window)
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

    /// The workspace `window` is on, or, before it is placed, the one `place_on_workspace` puts
    /// it on: the active workspace of the first output, where a new window is placed.
    pub fn placement_workspace(&self, window: &Window) -> Option<WorkspaceId> {
        on(window).or_else(|| self.active_workspace(self.space.outputs().next()?))
    }

    /// Puts `window`, placed now, on its `placement_workspace`.
    pub fn place_on_workspace(&mut self, window: &Window) {
        let Some(id) = self.placement_workspace(window) else {
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
        let from = on.0.get();
        if self.output_of_workspace(from).as_ref() == Some(&output) {
            return;
        }
        if let Some(id) = self.active_workspace(&output) {
            // Under the pointer's lock (`MoveGrab::button`): it stays where it was dropped, and
            // the grab's end lays its new workspace out.
            let left = self.leave_tree(window, from);
            on.0.set(id);
            if left && !self.is_tiled(id) {
                self.untile(window, false);
            }
            self.enter_tile(window, false);
            if left {
                self.tile_layout(from);
            }
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
        let from = on.0.get();
        if from == id {
            return true;
        }
        let left = self.leave_tree(window, from);
        crate::handlers::moved(window);
        on.0.set(id);
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
        // D9: a tile leaves its tree, and a window the user sees enters the target's.
        if left && !self.is_tiled(id) {
            self.untile(window, true);
        }
        let entered = self.enter_tile(window, false);
        self.fit_maximised();
        if left {
            self.tile_layout(from);
        }
        if let Some(id) = entered {
            self.tile_layout(id);
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
        let mut tiled = Vec::new();
        for (window, at) in back {
            let at = self.reachable(&window, at);
            self.space.map_element(window.clone(), at, false);
            // A move answered while it was out applies now.
            self.relocated(&window);
            // One that hid and drew again while out takes its tile now.
            self.enter_tile(&window, false);
            if let Some(id) = on(&window).filter(|id| self.is_tiled(*id)) {
                if !tiled.contains(&id) {
                    tiled.push(id);
                }
            }
        }
        for id in tiled {
            self.tile_layout(id);
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
        let home = self.workspaces.first().and_then(Strip::active_id);
        let mut rehomed = Vec::new();
        if let Some(home) = home {
            for window in self.windows() {
                if let Some(on) = window.user_data().get::<OnWorkspace>() {
                    let id = on.0.get();
                    if !self.workspaces.iter().any(|strip| strip.list.contains(&id)) {
                        on.0.set(home);
                        crate::handlers::moved(window);
                        rehomed.push(window.clone());
                    }
                }
            }
            if !rehomed.is_empty() {
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
        let gone: Vec<WorkspaceId> = self
            .tilings
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !self.workspaces.iter().any(|strip| strip.list.contains(id)))
            .collect();
        for id in gone {
            self.float(id);
        }
        // Out of the trees gone, the re-homed windows take a tile of their new workspace's.
        let entered: Vec<WorkspaceId> = rehomed
            .iter()
            .filter_map(|window| self.enter_tile(window, false))
            .collect();
        if let Some(home) = entered.first() {
            self.tile_layout(*home);
        }
        self.end_gone_tilings();
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

/// The automatic tiling of a workspace (OS12, `docs/superpowers/plans/2026-10-31-compositor-tiling.md`).
impl State {
    pub fn is_tiled(&self, id: WorkspaceId) -> bool {
        self.tilings.iter().any(|(kept, _)| *kept == id)
    }

    /// Whether `window` holds a tile of its workspace's tree.
    pub fn is_tile(&self, window: &Window) -> bool {
        on(window)
            .and_then(|id| self.tilings.iter().find(|(kept, _)| *kept == id))
            .is_some_and(|(_, tree)| tree.holds(window))
    }

    /// Tiles the workspace `id`, or floats it again (D7).
    pub fn set_tiling(&mut self, id: WorkspaceId, tiled: bool) {
        if tiled == self.is_tiled(id) {
            return;
        }
        if tiled {
            self.tile(id);
        } else {
            self.float(id);
        }
        self.send_tiling(id);
    }

    /// Takes every window of `id` the user can see into a new tree, the topmost first, each
    /// beside the one before (D2), and out of the cascade (`handlers::moved`). A maximised or
    /// full-screen window stays so, its tile what it returns to, and is sent nothing.
    fn tile(&mut self, id: WorkspaceId) {
        let Some(area) = self
            .output_of_workspace(id)
            .and_then(|output| self.window_area_on(&output))
        else {
            return;
        };
        // Bottom to top: the space shows the workspace, or `parked` keeps its windows.
        let stack: Vec<Window> = self
            .space
            .elements()
            .chain(self.parked.iter().map(|(window, _)| window))
            .filter(|window| on(window) == Some(id) && shown(window))
            .cloned()
            .collect();
        let mut tree = Tree::default();
        for window in stack.iter().rev() {
            let floating = kept_before(window).unwrap_or_else(|| self.settled_geometry(window));
            let kept = window.user_data().get_or_insert(Floating::default);
            kept.0.set(Some(floating));
            crate::handlers::moved(window);
            tree.insert(window.clone(), area);
        }
        // The order of focus, which the walk left the stack's reversed: the topmost last, then
        // the window with the keyboard.
        for window in &stack {
            tree.focus(window);
        }
        if let Some(focused) = self.focused_window() {
            tree.focus(&focused);
        }
        self.tilings.push((id, tree));
        self.tile_layout(id);
    }

    /// Ends the tiling of `id`: each window goes back to where it stood floating, at the size it
    /// had (D7, D8).
    fn float(&mut self, id: WorkspaceId) {
        let Some(index) = self.tilings.iter().position(|(kept, _)| *kept == id) else {
            return;
        };
        let (_, tree) = self.tilings.remove(index);
        for window in tree.windows() {
            self.untile(window, true);
        }
    }

    /// D8 for `window`, out of its tree: a configure without the tiled states and with the size
    /// it had floating, and with `moves` the window goes back to where it was at the commit that
    /// answers it; without, it stays where it is. One born tiled is placed then, at the size it
    /// answers with. A maximised or full-screen window is sent nothing, and returns to that place
    /// when it is neither.
    fn untile(&mut self, window: &Window, moves: bool) {
        let floating = floating_state(window);
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        if kept_before(window).is_some() {
            set_kept_before(window, floating);
            return;
        }
        let serial = toplevel.send_pending_configure();
        if !moves {
            return;
        }
        if floating.is_empty() {
            place_again(window, serial);
        } else {
            let at = self.kept_place(window, floating);
            self.go_to(window, at, serial);
        }
    }

    /// Gives each tile of `id` its rectangle: a configure with its size and the tiled states,
    /// when they are not those last sent, and the window goes to its tile's corner at the commit
    /// that answers it. A maximised or full-screen window, and one a grab holds, keep theirs.
    pub(crate) fn tile_layout(&mut self, id: WorkspaceId) {
        let Some(area) = self
            .output_of_workspace(id)
            .and_then(|output| self.window_area_on(&output))
        else {
            return;
        };
        let Some((_, tree)) = self.tilings.iter().find(|(kept, _)| *kept == id) else {
            return;
        };
        let tiles: Vec<(Window, Rectangle<i32, Logical>)> = tree
            .layout(area)
            .into_iter()
            .map(|(window, tile)| (window.clone(), tile))
            .collect();
        for (window, tile) in tiles {
            let Some(toplevel) = window.toplevel() else {
                continue;
            };
            // Maximised or full screen, it keeps that size and returns to its tile, with the
            // tiled states it is sent then (D6, D7).
            if kept_before(&window).is_some() {
                toplevel.with_pending_state(|state| {
                    for tiled in TILED {
                        state.states.set(tiled);
                    }
                });
                set_kept_before(&window, tile);
                continue;
            }
            if crate::grabs::is_held(&window) {
                continue;
            }
            // Set and never unset: the states compare in order, so an unset and a set would
            // send a configure at every layout.
            toplevel.with_pending_state(|state| {
                state.size = Some(tile.size);
                for tiled in TILED {
                    state.states.set(tiled);
                }
            });
            let serial = toplevel.send_pending_configure();
            if self.settled_geometry(&window).loc != tile.loc {
                self.go_to(&window, tile.loc, serial);
            }
        }
        self.restack(Some(id));
    }

    /// Moves `window` to `at`: at the commit that answers the configure of `serial` when there is
    /// one, else as `move_window` does. A parked window comes back there.
    fn go_to(&mut self, window: &Window, at: Point<i32, Logical>, serial: Option<Serial>) {
        for (kept, place) in &mut self.parked {
            if kept == window {
                *place = at;
            }
        }
        match serial {
            Some(serial) => relocate(window, at, Some(serial)),
            None => self.move_window(window, at),
        }
    }

    /// The focus went to `window`: its tile is the one the next tile splits (D2).
    pub(crate) fn focus_tile(&mut self, window: &Window) {
        let Some(id) = on(window) else {
            return;
        };
        if let Some((_, tree)) = self.tilings.iter_mut().find(|(kept, _)| *kept == id) {
            tree.focus(window);
        }
    }

    /// Every workspace an output shows laid out again, after the outputs or the layer surfaces'
    /// zones changed the area.
    pub(crate) fn fit_tiles(&mut self) {
        let shown: Vec<WorkspaceId> = self.workspaces.iter().filter_map(Strip::active_id).collect();
        for id in shown {
            self.tile_layout(id);
        }
    }

    /// D2 for `window`, on a tiled workspace, shown, in no tile yet and not one D5 floats: it
    /// splits the tile focused last, and with `behind` the next tile still splits that one. Where it stood is kept for
    /// the end of the tiling, unless it is kept already, and it leaves the cascade
    /// (`handlers::moved`). The workspace, to lay out.
    fn enter_tile(&mut self, window: &Window, behind: bool) -> Option<WorkspaceId> {
        let id = on(window).filter(|id| self.is_tiled(*id))?;
        if !shown(window) || self.is_tile(window) || crate::handlers::floats(window) {
            return None;
        }
        let area = self
            .output_of_workspace(id)
            .and_then(|output| self.window_area_on(&output))?;
        let floating = kept_before(window).unwrap_or_else(|| self.settled_geometry(window));
        let kept = window.user_data().get_or_insert(Floating::default);
        if kept.0.get().is_none() {
            kept.0.set(Some(floating));
        }
        let (_, tree) = self.tilings.iter_mut().find(|(kept, _)| *kept == id)?;
        crate::handlers::moved(window);
        if behind {
            tree.insert_behind(window.clone(), area);
        } else {
            tree.insert(window.clone(), area);
        }
        Some(id)
    }

    /// Takes `window` out of the tree of `id`; its sibling takes its space (D3). False when that
    /// tree does not hold it.
    fn leave_tree(&mut self, window: &Window, id: WorkspaceId) -> bool {
        let Some((_, tree)) = self.tilings.iter_mut().find(|(kept, _)| *kept == id) else {
            return false;
        };
        let held = tree.holds(window);
        tree.remove(window);
        held
    }

    /// A window drawn again, or brought back: it takes a tile if its workspace is tiled.
    pub(crate) fn tile_shown(&mut self, window: &Window) {
        match self.enter_tile(window, false) {
            Some(id) => self.tile_layout(id),
            None => self.restack(on(window)),
        }
    }

    /// A tile minimised, or one D5 floats now: it leaves its tree with D8's configure, and its
    /// workspace is laid out (D9).
    pub(crate) fn leave_tile(&mut self, window: &Window) {
        let Some(id) = on(window).filter(|id| self.leave_tree(window, *id)) else {
            return;
        };
        self.untile(window, true);
        self.tile_layout(id);
    }

    /// A tile that a change of its parent or its size limits makes one D5 floats: it leaves its
    /// tree, as `leave_tile` takes it out, and the layout's `restack` stands it above the tiles.
    pub(crate) fn float_tile(&mut self, window: &Window) {
        if self.is_tile(window) && crate::handlers::floats(window) {
            self.leave_tile(window);
        }
    }

    /// A tile destroyed: its workspace is laid out without it.
    pub(crate) fn drop_tile(&mut self, window: &Window) {
        if let Some(id) = on(window).filter(|id| self.leave_tree(window, *id)) {
            self.tile_layout(id);
        }
    }

    /// A tile that committed a null buffer leaves its tree, and the rest of its workspace is laid
    /// out. The workspace being still tiled, the initial configure the commit then sends carries
    /// the tile `preview_tile` gives the window and the tiled states, in place of the floating
    /// ones set here; a maximised or full-screen window keeps its size, without the tiled states.
    /// It goes back to where it stood floating, or is placed at its next draw when it was born
    /// tiled, and `tile_shown` gives it a tile again at that draw.
    pub(crate) fn hide_tile(&mut self, window: &Window) {
        let Some(id) = on(window).filter(|id| self.leave_tree(window, *id)) else {
            return;
        };
        let floating = floating_state(window);
        if floating.is_empty() {
            place_again(window, None);
        } else {
            relocate(window, self.kept_place(window, floating), None);
        }
        self.tile_layout(id);
    }

    /// After `restore` or `leave_full_screen` sent a tile back to its kept geometry: the
    /// layout's configure and move replace those (D6), and its `restack` puts the windows D5
    /// floats back above it.
    pub(crate) fn back_to_tile(&mut self, window: &Window) {
        if self.is_tile(window) {
            if let Some(id) = on(window) {
                self.tile_layout(id);
            }
        }
    }

    /// The tile a new window would take by D2 on the workspace it will be placed on, in the
    /// pending state of its initial configure, with D8's states. A window that asked to be
    /// maximised or full screen keeps what it asked, and one D5 floats is sent no tile.
    pub(crate) fn preview_tile(&self, window: &Window) {
        if kept_before(window).is_some() || crate::handlers::floats(window) {
            return;
        }
        let Some(id) = self
            .placement_workspace(window)
            .filter(|id| self.is_tiled(*id))
        else {
            return;
        };
        let Some(tile) = self
            .output_of_workspace(id)
            .and_then(|output| self.window_area_on(&output))
            .zip(self.tilings.iter().find(|(kept, _)| *kept == id))
            .and_then(|(area, (_, tree))| tree.preview(window, area))
        else {
            return;
        };
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.size = Some(tile.size);
                for tiled in TILED {
                    state.states.set(tiled);
                }
            });
        }
    }

    /// `place` put `window` on its workspace. On a tiled one a window D5 floats stands above the
    /// tiles, with or without the keyboard, and any other restores the other maximised
    /// windows there (D6), takes its tile, behind the one focused last when it did not take the
    /// keyboard, and stands there at once unless it asked to be maximised or full screen; it was
    /// born tiled. A window sent a tile in its initial configure that takes none, on a floating
    /// workspace since the toggle went off or one D5 floats since it set a parent or its size
    /// limits, is sent D8's floating configure and placed at the size it answers with.
    pub(crate) fn tile_placed(&mut self, window: &Window, focused: bool) {
        let Some(id) = on(window) else {
            return;
        };
        let floats = self.is_tiled(id) && crate::handlers::floats(window);
        if floats {
            self.restack(Some(id));
        }
        if !self.is_tiled(id) || floats {
            let Some(toplevel) = window.toplevel() else {
                return;
            };
            let tiled = toplevel.with_pending_state(|state| {
                TILED.iter().any(|tiled| state.states.contains(*tiled))
            });
            if tiled && kept_before(window).is_none() {
                toplevel.with_pending_state(|state| {
                    for tiled in TILED {
                        state.states.unset(tiled);
                    }
                    state.size = None;
                });
                let serial = toplevel.send_pending_configure();
                place_again(window, serial);
            }
            return;
        }
        let maximised: Vec<Window> = self
            .space
            .elements()
            .filter(|other| {
                *other != window
                    && on(other) == Some(id)
                    && crate::state::is_maximised(other)
                    && !crate::state::is_full_screen(other)
            })
            .cloned()
            .collect();
        for other in &maximised {
            self.restore(other);
        }
        window
            .user_data()
            .get_or_insert(Floating::default)
            .0
            .set(Some(Rectangle::default()));
        if self.enter_tile(window, !focused).is_none() {
            return;
        }
        if kept_before(window).is_none() {
            let tile = self
                .output_of_workspace(id)
                .and_then(|output| self.window_area_on(&output))
                .zip(self.tilings.iter().find(|(kept, _)| *kept == id))
                .and_then(|(area, (_, tree))| tree.preview(window, area));
            if let Some(tile) = tile {
                self.space.relocate_element(window, tile.loc);
            }
        }
        self.tile_layout(id);
    }

    /// The end of the last grab of `window` (`grabs::Held`): its workspace's layout, which the
    /// grab held off, and the pointer's focus at the next idle, since the pointer is held while
    /// a grab ends.
    pub(crate) fn grab_ended(&mut self, window: &Window) {
        let Some(id) = on(window).filter(|id| self.is_tiled(*id)) else {
            return;
        };
        self.tile_layout(id);
        let window = window.clone();
        self.loop_handle
            .insert_idle(move |state| state.repoint_moved(&window));
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
