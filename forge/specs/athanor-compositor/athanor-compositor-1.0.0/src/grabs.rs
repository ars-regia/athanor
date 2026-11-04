//! The window actions a client asks for on a button press of its own (doc_own_stack.md, OS12):
//! the pointer grabs that move a window, kept where its title bar can be grabbed again, and
//! that resize it from the edge it names, within the sizes it accepts.

use crate::state::{answered, State};
use smithay::backend::input::InputTime;
use smithay::desktop::Window;
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
    GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab, PointerInnerHandle,
    RelativeMotionEvent,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::{self, ResizeEdge};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Serial, Size, SERIAL_COUNTER};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::SurfaceCachedState;
use std::cell::Cell;

/// How much of a window, in logical pixels, stays inside the window area across.
const REACHABLE: i32 = 32;

/// The events a window grab does not act on, passed on as they come.
macro_rules! passed_on {
    () => {
        fn relative_motion(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            focus: Option<(WlSurface, Point<f64, Logical>)>,
            event: &RelativeMotionEvent,
        ) {
            handle.relative_motion(data, focus, event);
        }

        fn axis(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            details: AxisFrame,
        ) {
            handle.axis(data, details);
        }

        fn frame(&mut self, data: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
            handle.frame(data);
        }

        fn gesture_swipe_begin(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeBeginEvent,
        ) {
            handle.gesture_swipe_begin(data, event);
        }

        fn gesture_swipe_update(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeUpdateEvent,
        ) {
            handle.gesture_swipe_update(data, event);
        }

        fn gesture_swipe_end(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureSwipeEndEvent,
        ) {
            handle.gesture_swipe_end(data, event);
        }

        fn gesture_pinch_begin(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchBeginEvent,
        ) {
            handle.gesture_pinch_begin(data, event);
        }

        fn gesture_pinch_update(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchUpdateEvent,
        ) {
            handle.gesture_pinch_update(data, event);
        }

        fn gesture_pinch_end(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GesturePinchEndEvent,
        ) {
            handle.gesture_pinch_end(data, event);
        }

        fn gesture_hold_begin(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureHoldBeginEvent,
        ) {
            handle.gesture_hold_begin(data, event);
        }

        fn gesture_hold_end(
            &mut self,
            data: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            event: &GestureHoldEndEvent,
        ) {
            handle.gesture_hold_end(data, event);
        }

        fn start_data(&self) -> &GrabStartData<State> {
            &self.start
        }
    };
}

/// A window following the pointer from a press its client was sent, until every button is up.
/// No client has the pointer meanwhile.
pub struct MoveGrab {
    start: GrabStartData<State>,
    window: Window,
    /// Where the window's geometry was when the pointer was at the press.
    from: Point<i32, Logical>,
}

impl MoveGrab {
    /// The grab of `window`, which a layout leaves alone until it ends (`Held`). A move ends the
    /// resize it replaces, whose `unset` runs after this.
    pub fn new(start: GrabStartData<State>, window: Window, from: Point<i32, Logical>) -> Self {
        hold(&window);
        if let Some(resizing) = window.user_data().get::<Resizing>() {
            resizing.0.set(None);
        }
        Self {
            start,
            window,
            from,
        }
    }
}

impl PointerGrab<State> for MoveGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let Some(area) = data.window_area_at(event.location) else {
            return;
        };
        crate::handlers::moved(&self.window);
        let delta = (event.location - self.start.location).to_i32_round();
        let at = kept_reachable(area, self.from + delta, self.window.geometry().size);
        data.move_window(&self.window, at);
    }

    fn button(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            let at = handle.current_location();
            let from = data.workspace_of(&self.window);
            data.settle(&self.window);
            if data.workspace_of(&self.window) == from {
                data.drop_on_tile(&self.window, at);
            }
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    passed_on!();

    fn unset(&mut self, data: &mut State) {
        if let_go(&self.window) {
            data.grab_ended(&self.window);
        }
    }
}

/// A window's `edges` following the pointer from a press its client was sent, until every
/// button is up: each motion sends a configure with the `resizing` state and the size the
/// pointer asks for, and each commit keeps the opposite edges where they were.
pub struct ResizeGrab {
    start: GrabStartData<State>,
    window: Window,
    edges: ResizeEdge,
    /// The window's geometry at the press.
    from: Rectangle<i32, Logical>,
    /// The size the last motion asked for.
    asked: Option<Size<i32, Logical>>,
}

impl ResizeGrab {
    pub fn new(
        start: GrabStartData<State>,
        window: Window,
        edges: ResizeEdge,
        from: Rectangle<i32, Logical>,
    ) -> Self {
        hold(&window);
        let resize = Resize {
            edges,
            from,
            end: None,
        };
        window
            .user_data()
            .get_or_insert(Resizing::default)
            .0
            .set(Some(resize));
        Self {
            start,
            window,
            edges,
            from,
            asked: None,
        }
    }
}

impl PointerGrab<State> for ResizeGrab {
    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let Some(toplevel) = self.window.toplevel() else {
            return;
        };
        let (min, max) = with_states(toplevel.wl_surface(), |states| {
            let mut cached = states.cached_state.get::<SurfaceCachedState>();
            let limits = cached.current();
            (limits.min_size, limits.max_size)
        });
        let delta = event.location - self.start.location;
        let size = resized(self.edges, self.from.size, delta, min, max);
        self.asked = Some(size);
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
            state.size = Some(size);
        });
        toplevel.send_pending_configure();
    }

    fn button(
        &mut self,
        data: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    passed_on!();

    /// The release, or the lock: a configure without the `resizing` state, and the window
    /// stays anchored until the commit that answers it. A grab that replaced this one keeps its
    /// own record; a tile is not anchored, since its layout places it, with the splits the
    /// resize moved (D11).
    fn unset(&mut self, data: &mut State) {
        let last = let_go(&self.window);
        if let Some(toplevel) = self.window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Resizing);
            });
            let end = toplevel.send_pending_configure();
            if last {
                let resizing = self.window.user_data().get_or_insert(Resizing::default);
                let resize = resizing.0.get().zip(end).filter(|_| !data.is_tile(&self.window));
                resizing.0.set(resize.map(|(resize, end)| Resize {
                    end: Some(end),
                    ..resize
                }));
            }
        }
        if last {
            if let Some(size) = self.asked {
                let to = Rectangle::new(anchored(self.edges, self.from, size), size);
                data.resize_tile(&self.window, to);
            }
            data.grab_ended(&self.window);
        }
    }
}

/// How many window grabs hold a window. A grab replaced by another on the same window ends after
/// the new one began, so the count, not the grab installed, says whether one still does.
#[derive(Default)]
struct Held(Cell<u32>);

fn hold(window: &Window) {
    let held = window.user_data().get_or_insert(Held::default);
    held.0.set(held.0.get().saturating_add(1));
}

/// Ends a grab of `window`: whether no other holds it.
fn let_go(window: &Window) -> bool {
    let Some(held) = window.user_data().get::<Held>() else {
        return true;
    };
    let count = held.0.get().saturating_sub(1);
    held.0.set(count);
    count == 0
}

/// Whether a `MoveGrab` or a `ResizeGrab` holds `window`: a layout leaves it where the pointer
/// takes it.
pub fn is_held(window: &Window) -> bool {
    window
        .user_data()
        .get::<Held>()
        .is_some_and(|held| held.0.get() > 0)
}

/// A window's resize, kept in its user data from the press until the commit that answers its
/// last configure, as anvil keeps it.
#[derive(Default)]
struct Resizing(Cell<Option<Resize>>);

#[derive(Clone, Copy)]
struct Resize {
    edges: ResizeEdge,
    from: Rectangle<i32, Logical>,
    /// The serial of the configure that ended it.
    end: Option<Serial>,
}

impl State {
    /// Ends a move or resize of `window`, whose size and place maximising takes over: the
    /// grab, and the anchoring of a resize that ended without its last configure answered.
    pub fn end_window_grab(&mut self, window: &Window) {
        if let Some(pointer) = self.seat.get_pointer() {
            let grabbed = pointer.with_grab(|_, grab| {
                grab.downcast_ref::<MoveGrab>()
                    .map(|grab| &grab.window)
                    .or_else(|| grab.downcast_ref::<ResizeGrab>().map(|grab| &grab.window))
                    == Some(window)
            });
            if grabbed == Some(true) {
                pointer.unset_grab(self, SERIAL_COUNTER.next_serial(), InputTime::now());
            }
        }
        if let Some(resizing) = window.user_data().get::<Resizing>() {
            resizing.0.set(None);
        }
    }

    /// D11 for `window`, a tile dropped with the pointer at `at`: it swaps places with the tile
    /// under the pointer. A drop on no other tile changes nothing, and the layout puts it back.
    fn drop_on_tile(&mut self, window: &Window, at: Point<f64, Logical>) {
        let Some((tree, area)) = self.tree_of(window) else {
            return;
        };
        let under = tree
            .layout(area)
            .into_iter()
            .find(|(kept, tile)| *kept != window && tile.to_f64().contains(at))
            .map(|(kept, _)| kept.clone());
        if let Some(under) = under {
            tree.swap(window, &under);
        }
    }

    /// D11 for `window`, a tile its client resized to `to`: the splits by the sides that moved
    /// move with them, before the grab's end lays the workspace out.
    fn resize_tile(&mut self, window: &Window, to: Rectangle<i32, Logical>) {
        if let Some((tree, area)) = self.tree_of(window) {
            tree.resize(window, to, area);
        }
    }

    /// The tree that holds `window`, a tile, with the area of its workspace.
    fn tree_of(
        &mut self,
        window: &Window,
    ) -> Option<(&mut crate::tiling::Tree<Window>, Rectangle<i32, Logical>)> {
        let id = self.workspace_of(window)?;
        let area = self
            .output_of_workspace(id)
            .and_then(|output| self.window_area_on(&output))?;
        self.tilings
            .iter_mut()
            .find(|(kept, tree)| *kept == id && tree.holds(window))
            .map(|(_, tree)| (tree, area))
    }

    /// Keeps the edges opposite a resize's where they were, at each commit while the resize
    /// lasts and at the commit that answers its last configure, whatever size the client drew.
    pub fn anchor_resized(&mut self, window: &Window) {
        let Some(resizing) = window.user_data().get::<Resizing>() else {
            return;
        };
        let Some(resize) = resizing.0.get() else {
            return;
        };
        let at = anchored(resize.edges, resize.from, window.geometry().size);
        if let (Some(end), Some(toplevel)) = (resize.end, window.toplevel()) {
            if answered(toplevel, end) {
                resizing.0.set(None);
            }
        }
        self.move_window(window, at);
    }
}

fn left(edges: ResizeEdge) -> bool {
    matches!(
        edges,
        ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft
    )
}

fn top(edges: ResizeEdge) -> bool {
    matches!(
        edges,
        ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight
    )
}

fn right(edges: ResizeEdge) -> bool {
    matches!(
        edges,
        ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight
    )
}

fn bottom(edges: ResizeEdge) -> bool {
    matches!(
        edges,
        ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight
    )
}

/// The size a window of size `from` asks for when its `edges` moved by `delta`, within the
/// client's `min` and `max`, where 0 on an axis is no limit, and at least 1x1.
pub fn resized(
    edges: ResizeEdge,
    from: Size<i32, Logical>,
    delta: Point<f64, Logical>,
    min: Size<i32, Logical>,
    max: Size<i32, Logical>,
) -> Size<i32, Logical> {
    let delta = delta.to_i32_round::<i32>();
    let along = |length: i32, before: bool, after: bool, moved: i32| match (before, after) {
        (true, _) => length.saturating_sub(moved),
        (_, true) => length.saturating_add(moved),
        _ => length,
    };
    let fit = |length: i32, min: i32, max: i32| {
        let length = if max > 0 { length.min(max) } else { length };
        length.max(min).max(1)
    };
    let w = along(from.w, left(edges), right(edges), delta.x);
    let h = along(from.h, top(edges), bottom(edges), delta.y);
    (fit(w, min.w, max.w), fit(h, min.h, max.h)).into()
}

/// Where a window of `size` goes so that the edges opposite `edges` stay where they were in
/// `start`.
pub fn anchored(
    edges: ResizeEdge,
    start: Rectangle<i32, Logical>,
    size: Size<i32, Logical>,
) -> Point<i32, Logical> {
    let x = if left(edges) {
        start.loc.x.saturating_add(start.size.w).saturating_sub(size.w)
    } else {
        start.loc.x
    };
    let y = if top(edges) {
        start.loc.y.saturating_add(start.size.h).saturating_sub(size.h)
    } else {
        start.loc.y
    };
    (x, y).into()
}

/// `at`, where a window of `size` would go, kept so that its top edge is inside `area` and at
/// least `REACHABLE` pixels of it across: its title bar can always be grabbed again.
pub fn kept_reachable(
    area: Rectangle<i32, Logical>,
    at: Point<i32, Logical>,
    size: Size<i32, Logical>,
) -> Point<i32, Logical> {
    let right = area.loc.x.saturating_add(area.size.w);
    let x =
        at.x.max(area.loc.x.saturating_add(REACHABLE).saturating_sub(size.w))
            .min(right.saturating_sub(REACHABLE));
    let bottom = area.loc.y.saturating_add(area.size.h);
    let y = at.y.min(bottom.saturating_sub(1)).max(area.loc.y);
    (x, y).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_edge_stays_in_the_area() {
        let area = Rectangle::new((0, 32).into(), (1280, 688).into());
        let size = Size::from((400, 300));
        assert_eq!(
            kept_reachable(area, (100, 0).into(), size),
            (100, 32).into()
        );
        assert_eq!(
            kept_reachable(area, (100, 900).into(), size),
            (100, 719).into()
        );
        assert_eq!(
            kept_reachable(area, (-1000, 100).into(), size),
            (-368, 100).into()
        );
        assert_eq!(
            kept_reachable(area, (1500, 100).into(), size),
            (1248, 100).into()
        );
    }

    /// A client's surface tree can be as large as `i32::MAX`: nothing overflows.
    #[test]
    fn the_largest_window_does_not_overflow() {
        let huge = Size::from((i32::MAX, i32::MAX));
        let none = Size::from((0, 0));
        let grown = resized(ResizeEdge::BottomRight, huge, (1.0, 1.0).into(), none, none);
        assert_eq!(grown, huge);
        let from_corner =
            resized(ResizeEdge::TopLeft, (1, 1).into(), (-1e12, -1e12).into(), none, none);
        assert_eq!(from_corner, (i32::MAX, i32::MAX).into());
        let start = Rectangle::new((0, 32).into(), huge);
        assert_eq!(
            anchored(ResizeEdge::TopLeft, start, (1, 1).into()),
            (i32::MAX - 1, i32::MAX - 1).into()
        );
        let area = Rectangle::new((-1280, 0).into(), (1280, 720).into());
        assert_eq!(
            kept_reachable(area, (i32::MIN, 0).into(), huge),
            (i32::MIN, 0).into()
        );
    }

    #[test]
    fn resizing_respects_the_clients_limits() {
        let from = Size::from((400, 300));
        let (min, none) = (Size::from((200, 150)), Size::from((0, 0)));
        let size = |edges, delta: (f64, f64), max| resized(edges, from, delta.into(), min, max);
        assert_eq!(
            size(ResizeEdge::BottomRight, (50.0, 20.0), none),
            (450, 320).into()
        );
        assert_eq!(
            size(ResizeEdge::BottomRight, (-500.0, -500.0), none),
            (200, 150).into()
        );
        assert_eq!(size(ResizeEdge::Left, (50.0, 0.0), none), (350, 300).into());
        let max = Size::from((800, 0));
        assert_eq!(
            size(ResizeEdge::BottomRight, (900.0, 0.0), max),
            (800, 300).into()
        );
    }

    #[test]
    fn a_resize_from_the_top_left_keeps_the_bottom_right() {
        let start = Rectangle::new((100, 100).into(), (400, 300).into());
        // The client answered with 380x290, not the 350x250 asked for.
        let size = Size::from((380, 290));
        assert_eq!(
            anchored(ResizeEdge::TopLeft, start, size),
            (120, 110).into()
        );
        assert_eq!(
            anchored(ResizeEdge::BottomRight, start, size),
            (100, 100).into()
        );
    }
}
