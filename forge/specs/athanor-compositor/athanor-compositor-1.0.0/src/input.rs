//! Input from the backend to the seat: keys to the focused surface, the pointer to the surface
//! under it, a click focuses the window it lands on, or the layer surface that asks for the
//! keyboard on demand. The compositor keeps two kinds of key for itself, which no client sees:
//! Super+Escape locks the session (C1 task 8, doc_lock_and_prompts.md LP5), and Ctrl+Alt+F1 to
//! F12 switch to another virtual terminal on a console.

use crate::state::{identity_of, State, Under};
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
    InputTime, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    PointerMotionEvent,
};
use smithay::input::keyboard::{FilterResult, Keycode, Keysym, ModifiersState};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};

impl State {
    /// A click at `location` focuses the window it lands on, or the layer surface there that
    /// asks for the keyboard on demand. A click on a layer surface that takes the keyboard, of a
    /// identity Super+Escape took the keyboard from, gives its exclusive surfaces the keyboard
    /// back; a click on its windows, or on its surfaces that take no keyboard, does not.
    pub fn click_focus(&mut self, location: Point<f64, Logical>) {
        let under = self.under(location);
        if let Some((Under::Layer(layer), _)) = &under {
            if layer.can_receive_keyboard_focus() {
                if let Some(identity) = layer.wl_surface().client().and_then(|c| identity_of(&c)) {
                    self.escaped.retain(|escaped| *escaped != identity);
                }
            }
        }
        match under {
            Some((Under::Window(window), _)) => self.focus(&window),
            Some((Under::Layer(layer), _)) => self.focus_layer(&layer),
            None => {}
        }
    }

    /// A key pressed or released: to the focused surface, unless the compositor keeps it.
    pub fn key_input(&mut self, code: Keycode, key_state: KeyState, time: InputTime) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let to = keyboard
            .current_focus()
            .and_then(|surface| surface.client());
        let filtered = keyboard.input(
            self,
            code,
            key_state,
            serial,
            time,
            |state, modifiers, handle| match key_state {
                KeyState::Pressed => {
                    match kept(modifiers, handle.modified_sym(), state.lock.is_some()) {
                        Some(kept) => {
                            state.kept_keys.push(code);
                            FilterResult::Intercept(Some(kept))
                        }
                        None => FilterResult::Forward,
                    }
                }
                // The release of a kept press is kept: the client never saw the key go down.
                KeyState::Released => match state.kept_keys.iter().position(|k| *k == code) {
                    Some(at) => {
                        state.kept_keys.swap_remove(at);
                        FilterResult::Intercept(None)
                    }
                    None => FilterResult::Forward,
                },
            },
        );
        match filtered {
            // Sent to the client: a popup's grab may name its serial.
            None => {
                self.last_input_serial = Some(serial);
                let sent = to.map(|client| (serial, client.id()));
                match key_state {
                    KeyState::Pressed => self.grab_serials.key_press = sent,
                    KeyState::Released => self.grab_serials.key_release = sent,
                }
            }
            Some(Some(Kept::Lock)) => self.request_lock(),
            Some(Some(Kept::Vt(vt))) => self.switch_vt(vt),
            Some(None) => {}
        }
    }

    /// Moves the pointer to `location` and gives it to the surface under it.
    fn pointer_to(&mut self, location: Point<f64, Logical>, time: InputTime) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let under = self.surface_under(location);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
    }

    /// Input from the backend; it may change what is on screen, the pointer or the stacking.
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        self.activity();
        self.request_redraw();
        match event {
            InputEvent::Keyboard { event, .. } => {
                self.key_input(event.key_code(), event.state(), Event::time(&event));
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let Some(area) = self.output_area() else {
                    return;
                };
                let location = event.position_transformed(area.size) + area.loc.to_f64();
                self.pointer_to(location, event.time());
            }
            // A mouse or a touchpad on a console: moved by its delta, kept on the outputs.
            InputEvent::PointerMotion { event, .. } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let outputs: Vec<Rectangle<i32, Logical>> = self
                    .space
                    .outputs()
                    .filter_map(|output| self.space.output_geometry(output))
                    .collect();
                let from = pointer.current_location();
                if let Some(location) = confine(&outputs, from, from + event.delta()) {
                    self.pointer_to(location, event.time());
                }
            }
            InputEvent::PointerButton { event, .. } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                if event.state() == ButtonState::Pressed && !pointer.is_grabbed() {
                    self.click_focus(pointer.current_location());
                }
                // After the focus: the button's serial is then newer than the keyboard's enter,
                // so a token made from the click that focused a window is in range.
                let serial = SERIAL_COUNTER.next_serial();
                self.last_input_serial = Some(serial);
                if event.state() == ButtonState::Pressed {
                    let to = pointer.current_focus().and_then(|surface| surface.client());
                    self.grab_serials.button = to.map(|client| (serial, client.id()));
                }
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

/// A key the compositor keeps for itself.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kept {
    /// Super+Escape, alone: lock the session.
    Lock,
    /// Ctrl+Alt+F1 to F12, which the keymap turns into XF86Switch_VT_1 to 12.
    Vt(i32),
}

/// Whether the compositor keeps `sym`, pressed with `modifiers`. Escape only with Super alone,
/// and not while `locked`: the lock surface takes it as any key. Super+Alt+Escape (terminate)
/// and Super+Shift+Escape are the client's.
fn kept(modifiers: &ModifiersState, sym: Keysym, locked: bool) -> Option<Kept> {
    if sym == Keysym::Escape {
        let alone = modifiers.logo && !(modifiers.ctrl || modifiers.alt || modifiers.shift);
        return (alone && !locked).then_some(Kept::Lock);
    }
    if !(modifiers.ctrl && modifiers.alt) {
        return None;
    }
    let vt = sym.raw().checked_sub(Keysym::XF86_Switch_VT_1.raw())?;
    (vt < 12).then(|| Kept::Vt(vt as i32 + 1))
}

/// Where a relative motion from `from` to `to` leaves the pointer: at `to` when an output of
/// `outputs` shows it, else at the edge of the output it is on (the first one when on none),
/// so that it neither leaves the outputs nor enters a gap between two of different sizes.
fn confine(
    outputs: &[Rectangle<i32, Logical>],
    from: Point<f64, Logical>,
    to: Point<f64, Logical>,
) -> Option<Point<f64, Logical>> {
    if outputs.iter().any(|area| area.to_f64().contains(to)) {
        return Some(to);
    }
    let area = outputs
        .iter()
        .find(|area| area.to_f64().contains(from))
        .or_else(|| outputs.first())?;
    let (min, max) = (area.loc.to_f64(), (area.loc + area.size).to_f64());
    Some(Point::from((
        to.x.clamp(min.x, max.x - 1.0),
        to.y.clamp(min.y, max.y - 1.0),
    )))
}

#[cfg(test)]
mod tests {
    use super::{confine, kept, Kept};
    use smithay::input::keyboard::{Keysym, ModifiersState};
    use smithay::utils::{Logical, Point, Rectangle};

    #[test]
    fn the_compositor_keeps_its_escape_and_the_terminal_switches() {
        let logo = ModifiersState {
            logo: true,
            ..ModifiersState::default()
        };
        let ctrl_alt = ModifiersState {
            ctrl: true,
            alt: true,
            ..ModifiersState::default()
        };
        assert_eq!(kept(&logo, Keysym::Escape, false), Some(Kept::Lock));
        assert_eq!(
            kept(&logo, Keysym::Escape, true),
            None,
            "locked, it is the lock surface's"
        );
        assert_eq!(
            kept(&ModifiersState::default(), Keysym::Escape, false),
            None
        );
        for (other, what) in [
            (ModifiersState { alt: true, ..logo }, "Super+Alt+Escape"),
            (
                ModifiersState {
                    shift: true,
                    ..logo
                },
                "Super+Shift+Escape",
            ),
            (ModifiersState { ctrl: true, ..logo }, "Super+Ctrl+Escape"),
            (ctrl_alt, "Ctrl+Alt+Escape"),
        ] {
            assert_eq!(
                kept(&other, Keysym::Escape, false),
                None,
                "{what} is the client's"
            );
        }
        assert_eq!(
            kept(&ctrl_alt, Keysym::XF86_Switch_VT_1, false),
            Some(Kept::Vt(1))
        );
        assert_eq!(
            kept(&ctrl_alt, Keysym::XF86_Switch_VT_12, true),
            Some(Kept::Vt(12))
        );
        assert_eq!(
            kept(&ModifiersState::default(), Keysym::XF86_Switch_VT_1, false),
            None,
            "a switch the keymap sends without Ctrl+Alt switches nothing"
        );
        assert_eq!(
            kept(
                &ctrl_alt,
                Keysym::from(Keysym::XF86_Switch_VT_12.raw() + 1),
                false
            ),
            None
        );
        assert_eq!(kept(&ctrl_alt, Keysym::a, false), None);
    }

    #[test]
    fn the_pointer_moves_across_the_outputs_and_stays_on_them() {
        // 1000x800 at the origin, 600x400 to its right.
        let outputs: [Rectangle<i32, Logical>; 2] = [
            Rectangle::new((0, 0).into(), (1000, 800).into()),
            Rectangle::new((1000, 0).into(), (600, 400).into()),
        ];
        let at = |x: f64, y: f64| Point::<f64, Logical>::from((x, y));
        assert_eq!(
            confine(&outputs, at(990.0, 100.0), at(1100.0, 100.0)),
            Some(at(1100.0, 100.0)),
            "onto the second output"
        );
        assert_eq!(
            confine(&outputs, at(1100.0, 100.0), at(1700.0, 100.0)),
            Some(at(1599.0, 100.0)),
            "not past its right edge"
        );
        assert_eq!(
            confine(&outputs, at(1100.0, 390.0), at(1100.0, 600.0)),
            Some(at(1100.0, 399.0)),
            "not into the gap below the smaller output"
        );
        assert_eq!(
            confine(&outputs, at(10.0, 10.0), at(-50.0, -50.0)),
            Some(at(0.0, 0.0))
        );
        assert_eq!(confine(&[], at(0.0, 0.0), at(1.0, 1.0)), None, "no output");
    }
}
