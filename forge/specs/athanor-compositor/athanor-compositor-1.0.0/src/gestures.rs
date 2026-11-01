//! zwp_pointer_gestures_v1 (doc_own_stack.md OS17 and decision 30, standard): the swipe, pinch
//! and hold of a touchpad, from libinput, to the surface the pointer is over. The gesture goes
//! to that surface only: no binding of the compositor takes a gesture yet, so none is withheld
//! from it. The gesture events reach the seat's pointer, which hands each to the surface it has
//! the focus on at that moment. When the focus moves off the surface a gesture began on (a lock,
//! an unlock, a workspace switch), Smithay ends the gesture there as cancelled
//! (`pointer_gestures::leave`); the update and end that libinput sends after are dropped
//! by Smithay, which no longer has the gesture in progress.
//!
//! The event handlers are ported from anvil, Smithay's reference compositor.
//!
//! Copyright (c) 2017 Victor Berger and Victoria Brekenfeld (anvil, MIT)
//! Source: Smithay revision 023b78b1c5a05ca512767cacc07efca55fae8be4, `anvil/src/input_handler.rs`
//! (`on_gesture_swipe_begin` to `on_gesture_hold_end`), under the MIT licence below.
//! Changed by the Athanor authors on 2026-10-29: the events are read in `State` methods that
//! take plain values, so that the fixture drives them without libinput; the pointer is the
//! seat's own; the session's idle and redraw bookkeeping stays in `input.rs`.
//!
//! The MIT License
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of this
//! software and associated documentation files (the "Software"), to deal in the Software
//! without restriction, including without limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//! to whom the Software is furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all copies or
//! substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
//! INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//! PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
//! FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.

use crate::state::State;
use smithay::backend::input::{
    Event, GestureBeginEvent, GestureEndEvent, GesturePinchUpdateEvent as _,
    GestureSwipeUpdateEvent as _, InputBackend, InputEvent, InputTime,
};
use smithay::input::pointer::{
    GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent,
    GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent,
};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use smithay::wayland::pointer_gestures::PointerGesturesState;

/// Offers the global to every client.
pub fn global(dh: &DisplayHandle) {
    PointerGesturesState::new::<State>(dh);
}

/// The gesture in progress.
#[derive(Clone, Copy)]
pub enum Kind {
    Swipe,
    Pinch,
    Hold,
}

impl State {
    pub(crate) fn swipe_begin(&mut self, time: InputTime, fingers: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        self.gesture = Some(Kind::Swipe);
        let serial = SERIAL_COUNTER.next_serial();
        pointer.gesture_swipe_begin(
            self,
            &GestureSwipeBeginEvent {
                serial,
                time,
                fingers,
            },
        );
    }

    pub(crate) fn swipe_update(&mut self, time: InputTime, delta: Point<f64, Logical>) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !matches!(self.gesture, Some(Kind::Swipe)) {
            return;
        }
        pointer.gesture_swipe_update(self, &GestureSwipeUpdateEvent { time, delta });
    }

    pub(crate) fn swipe_end(&mut self, time: InputTime, cancelled: bool) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !matches!(self.gesture, Some(Kind::Swipe)) {
            return;
        }
        self.gesture = None;
        let serial = SERIAL_COUNTER.next_serial();
        pointer.gesture_swipe_end(
            self,
            &GestureSwipeEndEvent {
                serial,
                time,
                cancelled,
            },
        );
    }

    pub(crate) fn pinch_begin(&mut self, time: InputTime, fingers: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        self.gesture = Some(Kind::Pinch);
        let serial = SERIAL_COUNTER.next_serial();
        pointer.gesture_pinch_begin(
            self,
            &GesturePinchBeginEvent {
                serial,
                time,
                fingers,
            },
        );
    }

    pub(crate) fn pinch_update(
        &mut self,
        time: InputTime,
        delta: Point<f64, Logical>,
        scale: f64,
        rotation: f64,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !matches!(self.gesture, Some(Kind::Pinch)) {
            return;
        }
        pointer.gesture_pinch_update(
            self,
            &GesturePinchUpdateEvent {
                time,
                delta,
                scale,
                rotation,
            },
        );
    }

    pub(crate) fn pinch_end(&mut self, time: InputTime, cancelled: bool) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !matches!(self.gesture, Some(Kind::Pinch)) {
            return;
        }
        self.gesture = None;
        let serial = SERIAL_COUNTER.next_serial();
        pointer.gesture_pinch_end(
            self,
            &GesturePinchEndEvent {
                serial,
                time,
                cancelled,
            },
        );
    }

    pub(crate) fn hold_begin(&mut self, time: InputTime, fingers: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        self.gesture = Some(Kind::Hold);
        let serial = SERIAL_COUNTER.next_serial();
        pointer.gesture_hold_begin(
            self,
            &GestureHoldBeginEvent {
                serial,
                time,
                fingers,
            },
        );
    }

    pub(crate) fn hold_end(&mut self, time: InputTime, cancelled: bool) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !matches!(self.gesture, Some(Kind::Hold)) {
            return;
        }
        self.gesture = None;
        let serial = SERIAL_COUNTER.next_serial();
        pointer.gesture_hold_end(
            self,
            &GestureHoldEndEvent {
                serial,
                time,
                cancelled,
            },
        );
    }

    /// Reads a gesture event of the backend and hands it to the pointer; any other event is not
    /// a gesture and is left alone.
    pub(crate) fn gesture_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        use InputEvent::*;
        match event {
            GestureSwipeBegin { event } => self.swipe_begin(event.time(), event.fingers()),
            GestureSwipeUpdate { event } => self.swipe_update(event.time(), event.delta()),
            GestureSwipeEnd { event } => self.swipe_end(event.time(), event.cancelled()),
            GesturePinchBegin { event } => self.pinch_begin(event.time(), event.fingers()),
            GesturePinchUpdate { event } => {
                self.pinch_update(event.time(), event.delta(), event.scale(), event.rotation())
            }
            GesturePinchEnd { event } => self.pinch_end(event.time(), event.cancelled()),
            GestureHoldBegin { event } => self.hold_begin(event.time(), event.fingers()),
            GestureHoldEnd { event } => self.hold_end(event.time(), event.cancelled()),
            _ => {}
        }
    }
}
