//! The window actions a client asks for on a button press of its own (doc_own_stack.md, OS12):
//! the pointer grabs that move a window, kept where its title bar can be grabbed again.

use crate::state::State;
use smithay::desktop::Window;
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
    GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab, PointerInnerHandle,
    RelativeMotionEvent,
};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Size};

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
    pub start: GrabStartData<State>,
    pub window: Window,
    /// Where the window's geometry was when the pointer was at the press.
    pub from: Point<i32, Logical>,
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
        let Some(area) = data.window_area() else {
            return;
        };
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
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    passed_on!();

    fn unset(&mut self, _data: &mut State) {}
}

/// `at`, where a window of `size` would go, kept so that its top edge is inside `area` and at
/// least `REACHABLE` pixels of it across: its title bar can always be grabbed again.
pub fn kept_reachable(
    area: Rectangle<i32, Logical>,
    at: Point<i32, Logical>,
    size: Size<i32, Logical>,
) -> Point<i32, Logical> {
    let x =
        at.x.max(area.loc.x + REACHABLE - size.w)
            .min(area.loc.x + area.size.w - REACHABLE);
    let y = at.y.min(area.loc.y + area.size.h - 1).max(area.loc.y);
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
}
