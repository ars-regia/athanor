//! Input from the backend to the seat: keys to the focused surface, the pointer to the surface
//! under it, a click focuses the window it lands on, or the layer surface that asks for the
//! keyboard on demand.

use crate::state::{State, Under};
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
};
use smithay::input::keyboard::FilterResult;
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::utils::SERIAL_COUNTER;

impl State {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                let Some(keyboard) = self.seat.get_keyboard() else {
                    return;
                };
                let serial = SERIAL_COUNTER.next_serial();
                self.last_input_serial = Some(serial);
                keyboard.input::<(), _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    Event::time(&event),
                    |_, _, _| FilterResult::Forward,
                );
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let (Some(area), Some(pointer)) = (self.output_area(), self.seat.get_pointer())
                else {
                    return;
                };
                let location = event.position_transformed(area.size) + area.loc.to_f64();
                let under = self.surface_under(location);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location,
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                if event.state() == ButtonState::Pressed && !pointer.is_grabbed() {
                    match self.under(pointer.current_location()) {
                        Some((Under::Window(window), _)) => self.focus(&window),
                        Some((Under::Layer(layer), _)) => self.focus_layer(&layer),
                        None => {}
                    }
                }
                // After the focus: the button's serial is then newer than the keyboard's enter,
                // so a token made from the click that focused a window is in range.
                let serial = SERIAL_COUNTER.next_serial();
                self.last_input_serial = Some(serial);
                pointer.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state: event.state(),
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let source = event.source();
                let mut frame = AxisFrame::new(event.time()).source(source);
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let discrete = event.amount_v120(axis);
                    let amount = event
                        .amount(axis)
                        .unwrap_or_else(|| discrete.unwrap_or(0.0) * 15.0 / 120.0);
                    if amount != 0.0 {
                        frame = frame.value(axis, amount);
                        if let Some(discrete) = discrete {
                            frame = frame.v120(axis, discrete as i32);
                        }
                    }
                    if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                        frame = frame.stop(axis);
                    }
                }
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}
