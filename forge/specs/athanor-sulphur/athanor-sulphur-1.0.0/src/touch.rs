//! wl_touch (doc_own_stack.md decision 33, wave 2a): the touchscreens libinput finds, on the
//! seat's own `wl_touch`, with no global of their own. A touch goes to the surface under the
//! finger, and a touch-down focuses as a button press does (`State::click_focus`). Each
//! touchscreen spans its own output under that output's transform, never the first output: the
//! output libinput names for it, else the built-in panel; a touch with neither reaches nothing.
//! While the session is locked a touch reaches only a lock surface, and the lock cancels every
//! touch held before it (`State::cancel_touches`). Injected touch is libei's (wave 2c), not here.
//!
//! The event handlers are read in `State` methods that take plain values, as `tablet.rs` has
//! them, so that the fixture drives them without libinput.

use crate::state::State;
use smithay::backend::input::{
    AbsolutePositionEvent, Event, InputBackend, InputEvent, InputTime, TouchEvent, TouchSlot,
};
use smithay::input::touch::{
    DownEvent, GrabStartData, MotionEvent, OrientationEvent, ShapeEvent, TouchGrab,
    TouchInnerHandle, UpEvent,
};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, Size, SERIAL_COUNTER};
use std::any::Any;
use std::collections::HashMap;

/// The touch points the seat was told of.
#[derive(Debug, Default)]
pub struct Touches {
    /// Each touch point the seat was given a down for and not yet its up, where it is now.
    held: HashMap<TouchSlot, Point<f64, Logical>>,
    /// Whether the seat was given an event since its last frame or cancel.
    unframed: bool,
}

/// The names DRM gives the connectors of a built-in panel (`udev.rs`), as cosmic-comp takes them.
const BUILT_IN: [&str; 3] = ["eDP-", "LVDS-", "DSI-"];

impl State {
    /// The output a touchscreen spans: the one libinput names for it (udev's `WL_OUTPUT`), else
    /// the built-in panel. None when the named output is not shown, or when nothing is named and
    /// no output is built in: the first output is never assumed, since a touch on one screen would
    /// land on another.
    pub(crate) fn touch_output(&self, named: Option<&str>) -> Option<Output> {
        let mut outputs = self.space.outputs();
        match named {
            Some(name) => outputs.find(|output| output.name() == name),
            None => outputs.find(|output| {
                let name = output.name();
                BUILT_IN.iter().any(|prefix| name.starts_with(prefix))
            }),
        }
        .cloned()
    }

    /// Where in the space a touchscreen whose output libinput names `named` is touched: it reports
    /// `position` on its panel, which the output's transform turns under what the output shows.
    pub(crate) fn touch_location(
        &self,
        named: Option<&str>,
        position: impl FnOnce(Size<i32, Logical>) -> Point<f64, Logical>,
    ) -> Option<Point<f64, Logical>> {
        let output = self.touch_output(named);
        let location = output
            .as_ref()
            .and_then(|output| self.on_output(output, position));
        if location.is_none() {
            tracing::debug!(output = named, "a touch on no output shown is dropped");
        }
        location
    }

    /// A finger touched the screen at `location`. It goes to the surface under it, and the first
    /// finger focuses as a button press does. While locked, a touch where no lock surface is
    /// reaches nothing, and neither do its motion and its release.
    pub(crate) fn touch_down(
        &mut self,
        slot: TouchSlot,
        location: Point<f64, Logical>,
        time: InputTime,
    ) {
        self.super_tap = None;
        self.aids_pointer_pressed();
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        if self.lock.is_some() && self.surface_under(location).is_none() {
            return;
        }
        // As a press while a button is held: a finger after the first, or one during a move,
        // changes no focus.
        if !touch.is_grabbed() {
            self.a11y.set_pointing(true);
            self.click_focus(location);
            self.a11y.set_pointing(false);
        }
        // After the focus, as for a button: the serial is newer than the keyboard's enter.
        let under = self.surface_under(location);
        let serial = SERIAL_COUNTER.next_serial();
        self.last_input_serial = Some(serial);
        let to = under.as_ref().and_then(|(surface, _)| surface.client());
        self.grab_serials.touch = to.map(|client| (serial, client.id()));
        self.touches.held.insert(slot, location);
        self.touches.unframed = true;
        touch.down(
            self,
            under,
            &DownEvent {
                slot,
                location,
                serial,
                time,
            },
        );
    }

    /// A finger moved to `location`; it stays with the surface it touched.
    pub(crate) fn touch_motion(
        &mut self,
        slot: TouchSlot,
        location: Point<f64, Logical>,
        time: InputTime,
    ) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        let Some(at) = self.touches.held.get_mut(&slot) else {
            return;
        };
        *at = location;
        self.touches.unframed = true;
        let under = self.surface_under(location);
        touch.motion(
            self,
            under,
            &MotionEvent {
                slot,
                location,
                time,
            },
        );
    }

    /// A finger left the screen. The release of a touch the seat was never given, refused while
    /// locked or cancelled by the lock, is no event at all.
    pub(crate) fn touch_up(&mut self, slot: TouchSlot, time: InputTime) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        if self.touches.held.remove(&slot).is_none() {
            return;
        }
        self.touches.unframed = true;
        let serial = SERIAL_COUNTER.next_serial();
        touch.up(self, &UpEvent { slot, serial, time });
    }

    /// The end of a set of touch events; none when the seat was given none of them.
    pub(crate) fn touch_frame(&mut self) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        if std::mem::take(&mut self.touches.unframed) {
            touch.frame(self);
        }
    }

    /// Cancels every touch held: its surface is sent `wl_touch.cancel`, and its later motion and
    /// release reach no one. A move or resize from a touch ends. Smithay cancels only the touch
    /// points with an event since the last frame, so each is first given a motion where it
    /// already is, which the cancel tells its client to discard.
    pub(crate) fn cancel_touches(&mut self) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        touch.unset_grab(self);
        let held = std::mem::take(&mut self.touches.held);
        self.touches.unframed = false;
        if held.is_empty() {
            return;
        }
        let time = InputTime::now();
        for (slot, location) in held {
            touch.motion(
                self,
                None,
                &MotionEvent {
                    slot,
                    location,
                    time,
                },
            );
        }
        touch.cancel(self);
    }

    /// A touch event from the backend, on the output libinput names for its device.
    pub(crate) fn touch_event<I: InputBackend>(&mut self, event: InputEvent<I>)
    where
        I::Device: 'static,
    {
        match event {
            InputEvent::TouchDown { event } => {
                let named = output_name(&event.device());
                let location =
                    self.touch_location(named.as_deref(), |panel| event.position_transformed(panel));
                if let Some(location) = location {
                    self.touch_down(event.slot(), location, event.time());
                }
            }
            InputEvent::TouchMotion { event } => {
                let named = output_name(&event.device());
                let location =
                    self.touch_location(named.as_deref(), |panel| event.position_transformed(panel));
                if let Some(location) = location {
                    self.touch_motion(event.slot(), location, event.time());
                }
            }
            InputEvent::TouchUp { event } => self.touch_up(event.slot(), event.time()),
            InputEvent::TouchCancel { .. } => self.cancel_touches(),
            InputEvent::TouchFrame { .. } => self.touch_frame(),
            _ => {}
        }
    }
}

/// The output libinput names for `device` (udev's `WL_OUTPUT`); none for another backend's.
fn output_name<D: 'static>(device: &D) -> Option<String> {
    let device: &dyn Any = device;
    let device = device.downcast_ref::<smithay::reexports::input::Device>()?;
    device.output_name().map(String::from)
}

/// The seat's grab while a finger is down (`SeatHandler::touch_down_grab`): each finger goes to
/// the surface under it, where Smithay's own grab sends every finger after the first to the
/// first's surface. It holds the first down's serial, which a move, a resize or a drag names,
/// and ends with the last finger.
pub struct Fingers {
    start: GrabStartData<State>,
    down: usize,
}

impl Fingers {
    pub fn new(start: GrabStartData<State>) -> Self {
        Self { start, down: 1 }
    }
}

impl TouchGrab<State> for Fingers {
    fn down(
        &mut self,
        data: &mut State,
        handle: &mut TouchInnerHandle<'_, State>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &DownEvent,
    ) {
        handle.down(data, focus, event);
        self.down = self.down.saturating_add(1);
    }

    fn up(&mut self, data: &mut State, handle: &mut TouchInnerHandle<'_, State>, event: &UpEvent) {
        handle.up(data, event);
        self.down = self.down.saturating_sub(1);
        if self.down == 0 {
            handle.unset_grab(self, data);
        }
    }

    fn motion(
        &mut self,
        data: &mut State,
        handle: &mut TouchInnerHandle<'_, State>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, focus, event);
    }

    fn frame(&mut self, data: &mut State, handle: &mut TouchInnerHandle<'_, State>) {
        handle.frame(data);
    }

    fn cancel(&mut self, data: &mut State, handle: &mut TouchInnerHandle<'_, State>) {
        handle.cancel(data);
        handle.unset_grab(self, data);
    }

    fn shape(
        &mut self,
        data: &mut State,
        handle: &mut TouchInnerHandle<'_, State>,
        event: &ShapeEvent,
    ) {
        handle.shape(data, event);
    }

    fn orientation(
        &mut self,
        data: &mut State,
        handle: &mut TouchInnerHandle<'_, State>,
        event: &OrientationEvent,
    ) {
        handle.orientation(data, event);
    }

    fn start_data(&self) -> &GrabStartData<State> {
        &self.start
    }

    fn unset(&mut self, _data: &mut State) {}
}
