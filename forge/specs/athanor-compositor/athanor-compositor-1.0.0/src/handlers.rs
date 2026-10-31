//! Smithay's handler traits: what the compositor does when a client commits, maps a window,
//! opens a popup, imports a dmabuf or asks for activation.

use crate::grabs::{kept_reachable, MoveGrab, ResizeGrab};
use crate::state::{is_full_screen, ClientState, State};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::desktop::{
    find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output, PopupKeyboardGrab,
    PopupKind, PopupPointerGrab, PopupUngrabStrategy, Window, WindowSurfaceType,
};
use smithay::input::pointer::{CursorImageStatus, Focus, GrabStartData, PointerHandle};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::Output;
use smithay::reexports::calloop::{EventSource, Interest, RegistrationToken};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::protocol::ProtocolError;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::{
    wl_buffer, wl_output::WlOutput, wl_seat, wl_surface::WlSurface,
};
use smithay::reexports::wayland_server::{Client, DisplayHandle, Resource};
use smithay::utils::{Logical, Point, Rectangle, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states, Blocker,
    BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
};
use smithay::wayland::dmabuf::{
    get_dmabuf, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier,
};
use smithay::wayland::drm_syncobj::{DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState};
use smithay::wayland::foreign_toplevel_list::ForeignToplevelHandle;
use smithay::wayland::fractional_scale::{with_fractional_scale, FractionalScaleHandler};
use smithay::wayland::idle_notify::{IdleNotifierHandler, IdleNotifierState};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::security_context::{
    SecurityContext, SecurityContextHandler, SecurityContextListenerSource,
};
use smithay::wayland::selection::data_device::{
    set_data_device_focus, DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler,
};
use smithay::wayland::selection::primary_selection::set_primary_focus;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::{
    Configure, PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    XdgToplevelSurfaceData,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::xdg_activation::{
    XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;

/// How long an activation token stays good.
const ACTIVATION_TOKEN_LIFETIME: Duration = Duration::from_secs(10);

/// How many commits one client may hold waiting on a fence. Each holds an event source with a
/// file descriptor and pins its buffer; a client keeps a few frames in flight per surface, and
/// one that queues more without its fences signalling is flooding the compositor, which
/// disconnects it rather than run out of file descriptors.
pub const MAX_HELD_COMMITS: usize = 64;

/// `wl_display.error.no_memory`, the reason recorded for a client closed for holding too many
/// commits. wayland-server sends no `wl_display.error` on `Client::kill`, and `post_error` reads
/// the code against the interface of the object it is posted on, which for `wl_surface` defines
/// no such error: the client sees its socket close, and the reason reaches `disconnected`.
const NO_MEMORY: u32 = 2;

/// A commit held on a fence: its client, and the event source that waits on the fence.
pub struct HeldCommit {
    client: ClientId,
    seq: u64,
    token: RegistrationToken,
}

/// Marks a window the compositor has placed: placement happens once, at its first sized commit.
struct Placed;

/// Whether the user can see `window` in the space: placed, and still drawing something, since a
/// client can unmap a placed window by committing a null buffer.
pub(crate) fn shown(window: &Window) -> bool {
    let size = window.geometry().size;
    window.user_data().get::<Placed>().is_some() && size.w > 0 && size.h > 0
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // Every client connects through `listen`, which gives it a ClientState. A client
        // without one cannot exist; were it to, it would share this state rather than end the
        // compositor.
        static UNKNOWN: LazyLock<CompositorClientState> = LazyLock::new(Default::default);
        match client.get_data::<ClientState>() {
            Some(data) => &data.compositor_state,
            None => &UNKNOWN,
        }
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        add_pre_commit_hook::<Self, _>(surface, |state, dh, surface| {
            state.wait_for_fence(dh, surface)
        });
    }

    fn commit(&mut self, surface: &WlSurface) {
        // Also reached outside a dispatch of the clients, when a held commit's fence signals.
        self.request_redraw();
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_or_minimised(&root) {
                window.on_commit();
                // A minimised window, or one on an inactive workspace, is placed and moved when
                // it comes back (`unminimise`, `activate_workspace`).
                if !self.is_minimised(&window) && !self.is_parked(&window) {
                    self.place(&window);
                    let landed = self.relocated(&window);
                    self.anchor_resized(&window);
                    // Also what a full-screen window's answer lifts (`marks::lifted`).
                    if landed {
                        self.refresh_pointer();
                    }
                }
            }
        }
        self.layer_commit(surface);

        // The first commit of a toplevel or a popup gets its initial configure.
        if let Some(toplevel) = self
            .window_or_minimised(surface)
            .as_ref()
            .and_then(Window::toplevel)
        {
            if !toplevel.is_initial_configure_sent() {
                toplevel.send_configure();
            }
        }
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) {
            if !popup.is_initial_configure_sent() {
                if let Err(error) = popup.send_configure() {
                    tracing::warn!(%error, "a popup's initial configure failed");
                }
            }
        }
    }
}

impl State {
    /// Holds a commit with a new buffer until the buffer is ready: until its acquire point of
    /// linux-drm-syncobj-v1 signals or, without one, its dmabuf's implicit fence; as anvil does.
    /// It also counts the commit in the frame statistics.
    fn wait_for_fence(&mut self, dh: &DisplayHandle, surface: &WlSurface) {
        let (buffer, acquire) = with_states(surface, |states| {
            let acquire = states
                .cached_state
                .get::<DrmSyncobjCachedState>()
                .pending()
                .acquire_point
                .clone();
            let buffer = match &states
                .cached_state
                .get::<SurfaceAttributes>()
                .pending()
                .buffer
            {
                Some(BufferAssignment::NewBuffer(buffer)) => Some(get_dmabuf(buffer).cloned().ok()),
                _ => None,
            };
            (buffer, acquire)
        });
        let Some(dmabuf) = buffer else {
            return;
        };
        if let Some(stats) = &mut self.frame_stats {
            let pid = surface
                .client()
                .and_then(|client| client.get_credentials(dh).ok())
                .map(|peer| peer.pid);
            stats.commit(surface.id(), pid, dmabuf.is_some(), acquire.is_some());
        }
        if let Some(point) = acquire {
            match point.generate_blocker() {
                Ok((blocker, source)) => self.hold_commit(surface, blocker, source),
                Err(error) => tracing::warn!(%error, "an acquire point cannot be waited on"),
            }
        } else if let Some(dmabuf) = dmabuf {
            // An error means the fence has already signalled: nothing to wait for.
            if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) {
                self.hold_commit(surface, blocker, source);
            }
        }
    }

    /// Holds the pending commit of `surface` on `blocker` until `source` fires, as anvil does.
    /// A client past `MAX_HELD_COMMITS` is disconnected instead (`NO_MEMORY` says how); a
    /// client's held commits are dropped when it disconnects (`release_held`).
    pub fn hold_commit<B, S>(&mut self, surface: &WlSurface, blocker: B, source: S)
    where
        B: Blocker + Send + 'static,
        S: EventSource<Event = (), Ret = std::io::Result<()>> + 'static,
    {
        let Some(client) = surface.client() else {
            return;
        };
        let id = client.id();
        if self.held.iter().filter(|held| held.client == id).count() >= MAX_HELD_COMMITS {
            tracing::warn!(
                ?id,
                "a client holds too many commits on fences: disconnected"
            );
            client.kill(
                &self.display_handle,
                ProtocolError {
                    code: NO_MEMORY,
                    object_id: 1,
                    object_interface: "wl_display".into(),
                    message: format!("more than {MAX_HELD_COMMITS} commits wait on a fence"),
                },
            );
            return;
        }
        let seq = self.next_held;
        self.next_held += 1;
        let unblock = move |_, _: &mut S::Metadata, state: &mut State| {
            state.held.retain(|held| held.seq != seq);
            let dh = state.display_handle.clone();
            state
                .client_compositor_state(&client)
                .blocker_cleared(state, &dh);
            Ok(())
        };
        match self.loop_handle.insert_source(source, unblock) {
            Ok(token) => {
                self.held.push(HeldCommit {
                    client: id,
                    seq,
                    token,
                });
                add_blocker(surface, blocker);
            }
            Err(error) => tracing::warn!(error = %error.error, "a fence cannot be waited on"),
        }
    }

    /// Drops the commits `client` holds on fences, with their event sources.
    pub fn release_held(&mut self, client: &ClientId) {
        let loop_handle = &self.loop_handle;
        self.held.retain(|held| {
            let keep = &held.client != client;
            if !keep {
                loop_handle.remove(held.token);
            }
            keep
        });
    }

    /// Places a window, at its first commit with a size, in what the layer surfaces' exclusive
    /// zones leave of the output, as cosmic-comp places the first (`state::placed`); the
    /// windows after it are not cascaded as cosmic-comp's are yet (#738). It takes the keyboard
    /// only when no live surface holds it or its own client does: mapping a window is not the
    /// user's input, so a client cannot take the focus from another by opening one; a window
    /// with a parent follows it (`follow_parent`).
    fn place(&mut self, window: &Window) {
        let geometry = window.geometry();
        if geometry.size.w <= 0 || geometry.size.h <= 0 {
            return;
        }
        let Some(area) = self.placement_area() else {
            return;
        };
        if !window.user_data().insert_if_missing(|| Placed) {
            return;
        }
        if let Some(toplevel) = window.toplevel() {
            let (title, app_id) = names(toplevel);
            let handle = self
                .foreign_toplevel_state
                .new_toplevel::<Self>(title, app_id);
            window.user_data().insert_if_missing(|| handle);
        }
        let (x, y) = crate::state::placed(area, geometry).into();
        let owner = |surface: &WlSurface| surface.client().map(|client| client.id());
        let holder = self
            .seat
            .get_keyboard()
            .and_then(|keyboard| keyboard.current_focus())
            .and_then(|surface| owner(&surface));
        let own = window.toplevel().and_then(|t| owner(t.wl_surface()));
        if holder.is_none() || holder == own {
            self.space.map_element(window.clone(), (x, y), true);
            self.focus(window);
        } else {
            // Placed where it stays: at the bottom, under the window being typed into.
            self.space.relocate_element(window, (x, y));
        }
        self.place_on_workspace(window);
        self.follow_parent(window, true);
    }

    /// A window with a parent is stacked above it and above the parent's other dialogs, on the
    /// parent's workspace. With `grant`, which only `place` passes, it also takes the keyboard
    /// when the parent's own surface holds it, as a dialog does: the user is typing into the
    /// parent, so the dialog opened for it is where the typing goes on. The surface is compared,
    /// not the client, so a handle for one window does not take the keyboard from a prompt of
    /// the same client; a client's own dialogs are covered by the rule of `place`. A re-parent
    /// of a placed window moves and stacks it and never takes the keyboard.
    ///
    /// What this accepts: an exported handle is the exporter's grant, so its holder may keep a
    /// dialog above the exporter's window for as long as the handle lives, and a new window
    /// parented through it takes the keyboard from the parent's surface at its first placement,
    /// which is what a portal's file chooser needs. A holder that opens new windows repeatedly
    /// can therefore take the keyboard from the exporter's window each time one is placed; that
    /// is not closed here. A parent on an inactive workspace is hidden: the window follows it
    /// there, is hidden with it, and takes no keyboard; one that is minimised leaves the window
    /// where it is, beneath, with none.
    fn follow_parent(&mut self, window: &Window, grant: bool) {
        let Some(parent) = self.parent_window(window) else {
            return;
        };
        if let Some(id) = self.workspace_of(&parent) {
            self.move_window_to(window, id);
        }
        if self.space.element_location(&parent).is_none()
            || self.space.element_location(window).is_none()
        {
            return;
        }
        let held = self
            .seat
            .get_keyboard()
            .and_then(|keyboard| keyboard.current_focus());
        if held.as_ref() == window.toplevel().map(|t| t.wl_surface()) {
            return;
        }
        if grant && held.as_ref() == parent.toplevel().map(|t| t.wl_surface()) {
            self.focus(window);
            return;
        }
        // Just above the parent and its dialogs, under what is above them now; the window's
        // own dialogs go with it.
        let order: Vec<Window> = self.space.elements().cloned().collect();
        let children = self.children();
        let surface = |kept: &Window| kept.toplevel().map(|t| t.wl_surface().clone());
        let (Some(window_surface), Some(parent_surface)) = (surface(window), surface(&parent))
        else {
            return;
        };
        let own = Self::descendants(&children, &window_surface);
        let family = Self::descendants(&children, &parent_surface);
        let is_own = |kept: &Window| surface(kept).is_some_and(|s| own.contains(&s));
        let anchor = order.iter().rposition(|kept| {
            kept != window
                && !is_own(kept)
                && (*kept == parent || surface(kept).is_some_and(|s| family.contains(&s)))
        });
        let (Some(anchor), Some(at_window)) =
            (anchor, order.iter().position(|kept| kept == window))
        else {
            return;
        };
        if at_window < anchor {
            self.space.raise_element(window, false);
            for kept in order.iter().filter(|kept| is_own(kept)) {
                self.space.raise_element(kept, false);
            }
            for above in order
                .iter()
                .skip(anchor + 1)
                .filter(|kept| *kept != window && !is_own(kept))
            {
                self.space.raise_element(above, false);
            }
        }
    }

    /// The window of `window`'s parent, mapped, minimised or on an inactive workspace.
    fn parent_window(&self, window: &Window) -> Option<Window> {
        let parent = window.toplevel()?.parent()?;
        self.window_or_minimised(&parent)
    }

    /// The children of each toplevel surface, in one pass over the windows; empty when no
    /// window has a parent, the common case.
    fn children(&self) -> HashMap<WlSurface, Vec<WlSurface>> {
        let mut children: HashMap<WlSurface, Vec<WlSurface>> = HashMap::new();
        for toplevel in self.windows().filter_map(|window| window.toplevel()) {
            if let Some(parent) = toplevel.parent() {
                children
                    .entry(parent)
                    .or_default()
                    .push(toplevel.wl_surface().clone());
            }
        }
        children
    }

    /// The surfaces that descend from `root` through `children`, each visited once, so that a
    /// chain of any length costs one pass and a loop ends.
    fn descendants(
        children: &HashMap<WlSurface, Vec<WlSurface>>,
        root: &WlSurface,
    ) -> HashSet<WlSurface> {
        let mut found = HashSet::new();
        let mut pending = vec![root];
        while let Some(at) = pending.pop() {
            for child in children.get(at).into_iter().flatten() {
                if found.insert(child.clone()) {
                    pending.push(child);
                }
            }
        }
        found
    }

    /// The mapped windows that descend from `root`, bottom to top.
    fn subtree(&self, root: &Window) -> Vec<Window> {
        let children = self.children();
        let Some(root) = root.toplevel().map(|t| t.wl_surface()) else {
            return Vec::new();
        };
        if children.is_empty() {
            return Vec::new();
        }
        let found = Self::descendants(&children, root);
        self.space
            .elements()
            .filter(|kept| {
                kept.toplevel()
                    .is_some_and(|t| found.contains(t.wl_surface()))
            })
            .cloned()
            .collect()
    }

    /// The pointer, the start of its grab and the window, when the client of `surface` may
    /// start a window grab with `serial`: the session is not locked, and `serial` is the last
    /// button press, sent to that client and still held. A client cannot take the pointer when
    /// it likes, as the popup grab checks.
    fn window_grab(
        &self,
        surface: &ToplevelSurface,
        seat: &wl_seat::WlSeat,
        serial: Serial,
    ) -> Option<(PointerHandle<Self>, GrabStartData<Self>, Window)> {
        if self.lock.is_some() {
            return None;
        }
        let pointer = Seat::<Self>::from_resource(seat)?.get_pointer()?;
        let client = surface.wl_surface().client()?.id();
        if !self.grab_serials.clicked(serial, &client) || !pointer.has_grab(serial) {
            return None;
        }
        let start = pointer.grab_start_data()?;
        // A full-screen window has no place to be dragged from.
        let window = self
            .window_for(surface.wl_surface())
            .filter(|window| !is_full_screen(window))?;
        Some((pointer, start, window))
    }

    /// Raises `window`, activates it and gives it the keyboard, unless a layer surface holds
    /// the keyboard exclusively; the others are deactivated.
    pub fn focus(&mut self, window: &Window) {
        self.space.raise_element(window, true);
        // The window's own dialogs stay above it.
        for dialog in self.subtree(window) {
            self.space.raise_element(&dialog, false);
        }
        for other in self.space.elements() {
            other.set_activated(other == window);
            if let Some(toplevel) = other.toplevel() {
                toplevel.send_pending_configure();
            }
        }
        let target = self
            .exclusive_layer()
            .or_else(|| window.toplevel().map(|t| t.wl_surface().clone()));
        if target.is_some() {
            self.set_keyboard_focus(target);
        }
    }

    /// When `gone`, a destroyed window or a layer surface that gave the keyboard up, held the
    /// keyboard, the keyboard goes to the surface that holds it exclusively, else to the top
    /// window left that the user can see, so that the next window a client maps does not find
    /// it free (`place`): placed, and still drawing something, since a client can unmap a
    /// placed window by committing a null buffer.
    pub fn refocus(&mut self, gone: &WlSurface) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        if keyboard
            .current_focus()
            .is_some_and(|focus| focus.is_alive() && &focus != gone)
        {
            return;
        }
        self.focus_top();
    }

    /// Gives the keyboard to the surface that holds it exclusively, else to the top window the
    /// user can see, else to nothing.
    pub fn focus_top(&mut self) {
        if let Some(surface) = self.exclusive_layer() {
            self.focus_exclusive(surface);
            return;
        }
        let top = self
            .space
            .elements()
            .rev()
            .find(|window| shown(window))
            .cloned();
        match top {
            Some(window) => self.focus(&window),
            None => self.set_keyboard_focus(None),
        }
    }

    /// Keeps a popup inside the output its root, a window or a layer surface, is shown on.
    pub fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some((origin, output)) = self.popup_root_place(&root) else {
            return;
        };
        // Relative to the popup's parent.
        let mut target = output;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= origin;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    /// The origin in the space of a popup's root, a window or a layer surface, and the area of
    /// the output it is shown on: a window's is the output nearest to its centre
    /// (`output_nearest`), the one it is shown on whether it is settled or being moved.
    fn popup_root_place(
        &self,
        root: &WlSurface,
    ) -> Option<(Point<i32, Logical>, Rectangle<i32, Logical>)> {
        if let Some(window) = self.window_for(root) {
            let geometry = self.space.element_geometry(&window)?;
            let centre = (geometry.loc + geometry.size.downscale(2)).to_f64();
            let output = self.output_nearest(centre)?;
            return Some((geometry.loc, self.space.output_geometry(output)?));
        }
        self.space.outputs().find_map(|output| {
            let area = self.space.output_geometry(output)?;
            let map = layer_map_for_output(output);
            let layer = map.layer_for_surface(root, WindowSurfaceType::TOPLEVEL)?;
            Some((map.layer_geometry(layer)?.loc + area.loc, area))
        })
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn ack_configure(&mut self, surface: WlSurface, _configure: Configure) {
        self.decoration_acked(&surface);
    }

    fn parent_changed(&mut self, surface: ToplevelSurface) {
        self.foreign_parent_set_natively(&surface);
        // Before its first placement `place` stacks the window; a re-parent never gives it the keyboard.
        if let Some(window) = self.window_for(surface.wl_surface()) {
            if window.user_data().get::<Placed>().is_some() {
                self.follow_parent(&window, false);
            }
        }
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // Mapped now at the bottom of the stack, placed at its first sized commit.
        let window = Window::new_wayland_window(surface);
        self.space.map_element(window.clone(), (0, 0), false);
        self.space.lower_element(&window);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_or_minimised(surface.wl_surface()) {
            if let Some(handle) = window.user_data().get::<ForeignToplevelHandle>() {
                self.foreign_toplevel_state.remove_toplevel(handle);
            }
            self.space.unmap_elem(&window);
            self.minimised.retain(|(kept, _)| *kept != window);
            self.parked.retain(|(kept, _)| *kept != window);
            self.workspaces_changed();
        }
        self.foreign_toplevel_destroyed(surface.wl_surface());
        // The keyboard does not leave a destroyed window by itself, nor does the pointer.
        self.refocus(surface.wl_surface());
        self.refresh_pointer();
    }

    /// The window follows the pointer until every button is up. A maximised window is
    /// restored first, its top edge where it was and its centre under the pointer.
    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some((pointer, start, window)) = self.window_grab(&surface, &seat, serial) else {
            return;
        };
        let Some(mut from) = self.space.element_geometry(&window).map(|g| g.loc) else {
            return;
        };
        if let Some(kept) = self.restore(&window) {
            from.x = start
                .location
                .to_i32_round::<i32>()
                .x
                .saturating_sub(kept.size.w / 2);
            if let Some(area) = self.window_area_at(start.location) {
                self.move_window(&window, kept_reachable(area, from, kept.size));
            }
        }
        let grab = MoveGrab {
            start,
            window,
            from,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// The window's `edges` follow the pointer until every button is up. A maximised window
    /// keeps its size.
    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let maximised = surface
            .with_pending_state(|state| state.states.contains(xdg_toplevel::State::Maximized));
        if edges == xdg_toplevel::ResizeEdge::None || maximised {
            return;
        }
        let Some((pointer, start, window)) = self.window_grab(&surface, &seat, serial) else {
            return;
        };
        let Some(from) = self.space.element_geometry(&window) else {
            return;
        };
        let grab = ResizeGrab::new(start, window, edges, from);
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// A full-screen window keeps its output, and is told what it has.
    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            if is_full_screen(&window) {
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
                return;
            }
            self.end_window_grab(&window);
            self.maximise(&window);
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            if is_full_screen(&window) {
                self.forget_maximised(&window);
                if surface.is_initial_configure_sent() {
                    surface.send_configure();
                }
                return;
            }
            self.restore(&window);
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<WlOutput>) {
        if let Some(window) = self.window_or_minimised(surface.wl_surface()) {
            self.end_window_grab(&window);
            self.full_screen(&window, output.as_ref().and_then(Output::from_resource));
            self.refresh_pointer();
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_or_minimised(surface.wl_surface()) {
            self.leave_full_screen(&window);
            self.refresh_pointer();
        }
    }

    /// Refused for a window the user cannot see, one that has not drawn yet included.
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.minimise(&window);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.rename(&surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.rename(&surface);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(error) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!(%error, "a popup could not be tracked");
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    /// A popup grab takes the keyboard and the pointer until the popups close, as anvil's
    /// does. Only a client the user is with gets one, and only for an input event the user
    /// made: it names the serial of the last button press sent to it, while it holds the
    /// pointer, or of the last key press or release sent to it, while it holds the keyboard
    /// (`GrabSerials`, as mutter checks them). A client cannot take the keyboard from another
    /// by opening a popup. The grab's end hands the keyboard to the popups' root, so the grab
    /// takes the keyboard only when that root can keep it: never for a layer surface whose
    /// interactivity is `none`, a bar, and while a layer surface holds the keyboard
    /// exclusively only for its own popups; any other grab takes the pointer only. A grab
    /// refused leaves the popup shown without it, as a popover a client opens at start is,
    /// except one of a window the user cannot see, minimised, on an inactive workspace, not
    /// placed yet or drawing nothing, whose popup is dismissed.
    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        // While locked, the keyboard and the pointer stay with the lock surfaces.
        if self.lock.is_some() {
            return;
        }
        let Some(seat) = Seat::<Self>::from_resource(&seat) else {
            return;
        };
        let popup = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&popup) else {
            return;
        };
        // A popup of a window the user cannot see, minimised, on an inactive workspace, not
        // placed yet or drawing nothing, takes neither the keyboard nor the pointer: it is
        // dismissed.
        if self.popup_root_place(&root).is_none()
            || self.window_for(&root).is_some_and(|window| !shown(&window))
        {
            if let PopupKind::Xdg(surface) = &popup {
                surface.send_popup_done();
            }
            return;
        }
        let (keyboard, pointer) = (seat.get_keyboard(), seat.get_pointer());
        let Some(client) = root.client().map(|client| client.id()) else {
            return;
        };
        let held = |focus: Option<WlSurface>| {
            focus
                .and_then(|surface| surface.client())
                .is_some_and(|holder| holder.id() == client)
        };
        let clicked = self.grab_serials.clicked(serial, &client)
            && pointer.as_ref().is_some_and(|p| held(p.current_focus()));
        let typed = self.grab_serials.typed(serial, &client)
            && keyboard.as_ref().is_some_and(|k| held(k.current_focus()));
        if !(clicked || typed) {
            return;
        }
        // The grab's end hands the keyboard to the popups' root: the grab takes it only when the
        // root can keep it, and while a surface holds it exclusively only when that surface is
        // the root, so that it comes back to a surface that keeps it.
        let takes_keyboard = self.takes_keyboard(&root)
            && !self
                .exclusive_layer()
                .is_some_and(|surface| surface != root);
        let Ok(mut grab) = self.popups.grab_popup(root, popup, &seat, serial) else {
            return;
        };
        if let Some(keyboard) = keyboard.filter(|_| takes_keyboard) {
            let previous = grab.previous_serial().unwrap_or(serial);
            if keyboard.is_grabbed() && !(keyboard.has_grab(serial) || keyboard.has_grab(previous))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            keyboard.set_focus(self, grab.current_grab(), serial);
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(pointer) = pointer {
            let previous = grab.previous_serial().unwrap_or_else(|| grab.serial());
            if pointer.is_grabbed() && !(pointer.has_grab(serial) || pointer.has_grab(previous)) {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        }
    }
}

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    /// A client set the cursor with `wl_pointer.set_cursor` or `wp_cursor_shape_v1`, or Smithay
    /// reset it to the default when the pointer's focus changed. Its owner is the client of the
    /// cursor surface, else the client the pointer is over.
    fn cursor_image(&mut self, seat: &Seat<Self>, image: CursorImageStatus) {
        let owner = match &image {
            CursorImageStatus::Surface(surface) => surface.client(),
            // Smithay resets from inside the pointer's motion, which holds the pointer: asking
            // it for its focus there deadlocks, and the default needs no owner.
            _ if image == CursorImageStatus::default_named() => None,
            _ => seat
                .get_pointer()
                .and_then(|pointer| pointer.current_focus())
                .and_then(|surface| surface.client()),
        };
        self.cursor_status = (image, owner.map(|client| client.id()));
        self.request_redraw();
    }

    /// The clipboard and the primary selection follow the keyboard: the client that gains the
    /// focus is sent an offer of each selection, and no other client is sent one. An offer a client already holds stays
    /// readable after it loses the focus, for as long as the selection's source answers:
    /// Smithay checks no focus on `wl_data_offer.receive` and keeps the offer's object data to
    /// itself, so the compositor cannot withdraw it.
    ///
    /// The text inputs follow the keyboard too, except while the session is locked
    /// (`input_method.rs`).
    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // The focus outline moves.
        self.request_redraw();
        let client = focused.and_then(|surface| surface.client());
        self.aids_focus_changed(focused);
        set_primary_focus(&self.display_handle, seat, client.clone());
        set_data_device_focus(&self.display_handle, seat, client);
        self.text_input_focus(focused.cloned());
        // A lock or a confinement follows the keyboard (decision 30), a trusted prompt focused
        // or left starts or ends the aids' quiet (AX11), and the magnifier follows the keyboard
        // unless the pointer moved it: the view then stays with the pointer. Once the dispatch
        // is done: a popup's grab moves the keyboard while it holds the pointer, whose location
        // the first and the last read, and Smithay calls this holding the keyboard, which the
        // aids read.
        self.loop_handle.insert_idle(|state| {
            // The focus is one of what decides what stands above the top layer (`marks::lifted`).
            if state.space.elements().any(is_full_screen) {
                state.refresh_pointer();
            }
            state.refresh_constraint();
            state.aids_quiet_changed();
        });
        if let Some(surface) = focused.cloned().filter(|_| !self.a11y.pointing) {
            self.loop_handle
                .insert_idle(move |state| state.zoom_to_focus(&surface));
        }
    }
}

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

// ponytail: the default cancels every drag and drop; a DnD grab with its icon when a client needs it.
impl WaylandDndGrabHandler for State {}

impl OutputHandler for State {
    fn output_bound(&mut self, output: Output, wl_output: WlOutput) {
        self.workspace_output_bound(&output, &wl_output);
        // A state object made before its client bound the output is sent it now.
        self.send_toplevel_states();
    }
}

impl SecurityContextHandler for State {
    /// Clients connecting through the context's listener carry it: they are granted none of the
    /// privileged globals, whatever their identity (OS4).
    fn context_created(&mut self, source: SecurityContextListenerSource, context: SecurityContext) {
        let inserted = self
            .loop_handle
            .insert_source(source, move |stream, _, state| {
                let data = state.client_state(&stream, Some(context.clone()));
                if let Err(error) = state.display_handle.insert_client(stream, data) {
                    tracing::warn!(%error, "a sandboxed client could not be added");
                }
            });
        if let Err(error) = inserted {
            tracing::warn!(error = %error.error, "a security context cannot be listened on");
        }
    }
}

impl IdleNotifierHandler for State {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        match self.import_dmabuf(&dmabuf) {
            Ok(()) => {
                if let Err(error) = notifier.successful::<Self>() {
                    tracing::warn!(%error, "an imported dmabuf has no buffer");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "a dmabuf could not be imported");
                notifier.failed();
            }
        }
    }
}

impl DrmSyncobjHandler for State {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj_state.as_mut()
    }
}

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        // The first output's scale until a frame shows the surface: `frame_done` then sends the
        // scale of the output it is shown on, and every later change of it.
        let scale = self
            .space
            .outputs()
            .next()
            .map_or(1.0, |output| output.current_scale().fractional_scale());
        with_states(&surface, |states| {
            with_fractional_scale(states, |fractional| fractional.set_preferred_scale(scale));
        });
    }
}

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    /// A token is good only when the client that asks for it holds the keyboard focus of this
    /// seat, which it gets from the user's click or from mapping a window while nothing else
    /// holds it (`place`). The serial must also be one this compositor sent since that focus;
    /// the focused client always holds its own enter serial, so the range only keeps out serials
    /// never sent, and the focus check is what keeps other clients out.
    fn token_created(&mut self, _token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        self.xdg_activation_state
            .retain_tokens(|_, token| token.timestamp.elapsed() < ACTIVATION_TOKEN_LIFETIME);
        let (Some((serial, seat)), Some(client)) = (data.serial, data.client_id) else {
            return false;
        };
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let Some(enter) = keyboard.last_enter() else {
            return false;
        };
        let focused = keyboard
            .current_focus()
            .and_then(|surface| surface.client())
            .map(|focused| focused.id());
        Seat::from_resource(&seat).as_ref() == Some(&self.seat)
            && focused == Some(client)
            && serial_since_focus(serial, enter, self.last_input_serial)
    }

    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        // A token activates once.
        self.xdg_activation_state.remove_token(&token);
        if data.timestamp.elapsed() < ACTIVATION_TOKEN_LIFETIME {
            // While the session is locked a token restores nothing (L14).
            if self.lock.is_none() {
                // A focused client can make a token at any time: it switches no workspace, and a
                // window on an inactive one stays there.
                if let Some(window) = self
                    .window_or_minimised(&surface)
                    .filter(|window| self.on_active_workspace(window))
                {
                    self.unminimise(&window);
                }
            }
            if let Some(window) = self.window_for(&surface) {
                self.focus(&window);
            }
        }
    }
}

/// The title and the app id `toplevel` has set, empty when it has set none.
fn names(toplevel: &ToplevelSurface) -> (String, String) {
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok())
            .map(|role| {
                (
                    role.title.clone().unwrap_or_default(),
                    role.app_id.clone().unwrap_or_default(),
                )
            })
            .unwrap_or_default()
    })
}

impl State {
    /// Sends the foreign toplevel list `toplevel`'s new title and app id.
    fn rename(&mut self, toplevel: &ToplevelSurface) {
        let Some(window) = self.window_or_minimised(toplevel.wl_surface()) else {
            return;
        };
        if let Some(handle) = window.user_data().get::<ForeignToplevelHandle>() {
            let (title, app_id) = names(toplevel);
            handle.send_title(&title);
            handle.send_app_id(&app_id);
            handle.send_done();
        }
    }
}

/// Whether `serial` is one this compositor has sent since the keyboard entered the focused
/// surface at `enter`: no older than `enter`, and no newer than the last key or button event.
/// Serials wrap, so "older" is the wrapping order Smithay's `Serial` compares in.
fn serial_since_focus(serial: Serial, enter: Serial, last_input: Option<Serial>) -> bool {
    let newest = last_input
        .filter(|last| last.is_no_older_than(&enter))
        .unwrap_or(enter);
    serial.is_no_older_than(&enter) && newest.is_no_older_than(&serial)
}

smithay::delegate_dispatch2!(State);

#[cfg(test)]
mod tests {
    use super::serial_since_focus;
    use smithay::utils::Serial;

    #[test]
    fn only_a_serial_sent_since_the_focus_is_accepted() {
        let s = Serial::from;
        // Focus entered at 10; the last key or button event carried 14.
        assert!(serial_since_focus(s(10), s(10), Some(s(14))));
        assert!(serial_since_focus(s(12), s(10), Some(s(14))));
        assert!(serial_since_focus(s(14), s(10), Some(s(14))));
        // Before the focus, or never sent.
        assert!(!serial_since_focus(s(9), s(10), Some(s(14))));
        assert!(!serial_since_focus(s(15), s(10), Some(s(14))));
        assert!(!serial_since_focus(s(0x4000_0000), s(7), Some(s(14))));
        // No input since the focus: only the focus serial itself.
        assert!(serial_since_focus(s(10), s(10), Some(s(4))));
        assert!(!serial_since_focus(s(11), s(10), None));
        // Across the wrap of the counter.
        assert!(serial_since_focus(s(2), s(u32::MAX - 1), Some(s(3))));
        assert!(!serial_since_focus(s(4), s(u32::MAX - 1), Some(s(3))));
    }
}
