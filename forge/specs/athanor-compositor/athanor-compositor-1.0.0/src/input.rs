//! Input from the backend to the seat: keys to the focused surface, the pointer to the surface
//! under it, a click focuses the window it lands on, or the layer surface that asks for the
//! keyboard on demand. The compositor keeps two kinds of key for itself, which no client sees:
//! Super+Escape locks the session (C1 task 8, doc_lock_and_prompts.md LP5), and Ctrl+Alt+F1 to
//! F12 switch to another virtual terminal on a console. A virtual keyboard's keys and a virtual
//! pointer's motion and buttons, of the clients the allow-list grants them, take the same path
//! (OS4, OS9 task 6).

use crate::state::{identity_of, State, Under};
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
    InputTime, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    PointerMotionEvent,
};
use smithay::input::keyboard::{
    FilterResult, KeyboardSource, Keycode, Keysym, ModifiersState, Xkb, XkbConfig,
};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};
use smithay::wayland::virtual_keyboard::{
    VirtualKeyboardBackend, VirtualKeyboardDevice, VirtualKeyboardHandler,
    VirtualKeyboardSpecialEvent,
};
use smithay::wayland::virtual_pointer::{VirtualPointerBackend, VirtualPointerHandler};
use std::sync::{Mutex, PoisonError};

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

    /// A key pressed or released, from the hardware keyboard or a virtual one (`source`): to
    /// the focused surface, unless the compositor keeps it or drops it (`drops_key`).
    pub fn key_input(
        &mut self,
        code: Keycode,
        key_state: KeyState,
        time: InputTime,
        source: KeyboardSource,
    ) {
        // A dropped press never reaches the seat: it changes neither the keys held nor the
        // modifiers a hardware key is read with. A release is never dropped: a key held when the
        // session locked is let go, or a window would see it held after unlock.
        if key_state == KeyState::Pressed && self.drops_key(source) {
            return;
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let to = keyboard
            .current_focus()
            .and_then(|surface| surface.client());
        let filtered = keyboard.input_from_source(
            source,
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
    pub(crate) fn pointer_to(&mut self, location: Point<f64, Logical>, time: InputTime) {
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

    /// A button pressed or released: a press first focuses what is under the pointer.
    pub fn button_input(&mut self, button: u32, button_state: ButtonState, time: InputTime) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
            self.click_focus(pointer.current_location());
        }
        // After the focus: the button's serial is then newer than the keyboard's enter, so a
        // token made from the click that focused a window is in range.
        let serial = SERIAL_COUNTER.next_serial();
        self.last_input_serial = Some(serial);
        if button_state == ButtonState::Pressed {
            let to = pointer.current_focus().and_then(|surface| surface.client());
            self.grab_serials.button = to.map(|client| (serial, client.id()));
        }
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state: button_state,
                serial,
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
                self.key_input(
                    event.key_code(),
                    event.state(),
                    Event::time(&event),
                    KeyboardSource::Physical,
                );
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
                let outputs = self.output_areas();
                let from = pointer.current_location();
                if let Some(location) = confine(&outputs, from, from + event.delta()) {
                    self.pointer_to(location, event.time());
                }
            }
            InputEvent::PointerButton { event, .. } => {
                self.button_input(event.button_code(), event.state(), event.time());
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

impl State {
    /// Where the outputs are in the space.
    fn output_areas(&self) -> Vec<Rectangle<i32, Logical>> {
        self.space
            .outputs()
            .filter_map(|output| self.space.output_geometry(output))
            .collect()
    }

    /// Runs `input` under `device`'s keymap and modifiers, then gives the seat back its own: what
    /// a virtual keyboard uploads applies to its own keys only, never to the hardware's, and the
    /// clients are sent the seat's keymap again.
    fn with_keymap_of(&mut self, device: &VirtualKeyboardDevice, input: impl FnOnce(&mut Self)) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let seat_keymap = keyboard.with_xkb_state(self, |context| keymap_text(context.xkb()));
        let own = device.keymap().filter(|keymap| **keymap != *seat_keymap);
        let modifiers = self.virtual_modifiers.get(device).copied();
        if own.is_none() && modifiers.is_none() {
            return input(self);
        }
        let saved = keyboard.modifier_state().serialized;
        if let Some(keymap) = &own {
            if let Err(error) = keyboard.set_keymap_from_string(self, keymap.to_string()) {
                tracing::warn!(
                    ?error,
                    "a virtual keyboard's keymap did not load: its key is dropped"
                );
                return;
            }
        }
        if let Some([depressed, latched, locked, group]) = modifiers {
            keyboard.with_xkb_state(self, |mut context| {
                context.set_modifier_mask(depressed, latched, locked, group)
            });
        }
        input(self);
        if own.is_some() {
            if let Err(error) = keyboard.set_keymap_from_string(self, seat_keymap) {
                // The seat's configuration, as `State::new` gives it, so that the seat never
                // stays on a client's keymap.
                tracing::error!(
                    ?error,
                    "the seat's keymap did not load again: back to the default"
                );
                if let Err(error) = keyboard.set_xkb_config(self, XkbConfig::default()) {
                    tracing::error!(?error, "the default keymap did not load");
                }
            }
        }
        // A new keymap starts with no modifier locked: the seat's Caps Lock and layout return.
        keyboard.with_xkb_state(self, |mut context| {
            context.set_modifier_mask(
                saved.depressed,
                saved.latched,
                saved.locked,
                saved.layout_effective,
            )
        });
    }
}

/// The keymap of `xkb`, as the clients are sent it.
fn keymap_text(xkb: &Mutex<Xkb>) -> String {
    xkb.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .keymap_as_string()
}

impl VirtualKeyboardHandler for State {
    /// A virtual keyboard's key enters `key_input`, as a hardware key does, under the keymap and
    /// the modifiers the keyboard set; the modifiers alone are kept for its next keys.
    fn process_virtual_keyboard_event(&mut self, event: InputEvent<VirtualKeyboardBackend>) {
        match event {
            InputEvent::Keyboard { event } => {
                let device = event.device();
                let (code, key_state, time) =
                    (event.key_code(), event.state(), Event::time(&event));
                // While locked a press is dropped before anything, and a release lets go of a
                // key the seat holds under the seat's keymap: no client is sent this client's
                // keymap, and the idle timers keep running.
                if self.drops_virtual_input() {
                    if key_state == KeyState::Released {
                        self.key_input(code, key_state, time, device.source());
                    }
                    return;
                }
                self.activity();
                self.request_redraw();
                self.with_keymap_of(&device, |state| {
                    state.key_input(code, key_state, time, device.source())
                });
            }
            InputEvent::Special(VirtualKeyboardSpecialEvent::Modifiers {
                device,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            }) => {
                self.virtual_modifiers
                    .insert(device, [mods_depressed, mods_latched, mods_locked, group]);
            }
            InputEvent::DeviceRemoved { device } => {
                self.virtual_modifiers.remove(&device);
            }
            _ => {}
        }
    }
}

impl VirtualPointerHandler for State {
    /// A virtual pointer's motion, buttons and scrolling take the hardware's path, unless the
    /// session is locked (`drops_virtual_input`); its absolute motion spans the output it names,
    /// or all of them, and stays on the outputs.
    fn process_virtual_pointer_event(&mut self, event: InputEvent<VirtualPointerBackend>) {
        // While locked only the release of a button the same virtual pointer holds goes on, or a
        // click on the lock surface would keep its grab after unlock. Smithay keeps the seat's
        // held buttons to its grabs, so the compositor counts each virtual pointer's itself.
        let locked = self.drops_virtual_input();
        match &event {
            InputEvent::DeviceRemoved { device } => {
                self.virtual_buttons.remove(device);
            }
            InputEvent::PointerButton { event } => {
                let (device, button) = (event.device(), event.button_code());
                let held = self.virtual_buttons.entry(device).or_default();
                let holds = held.contains(&button);
                match event.state() {
                    ButtonState::Pressed if locked => return,
                    ButtonState::Pressed if !holds => held.push(button),
                    ButtonState::Released if holds => {
                        held.retain(|b| *b != button);
                        // Not through `process_input_event`: letting go is no activity, and the
                        // locked session keeps going idle.
                        if locked {
                            return self.button_input(button, ButtonState::Released, event.time());
                        }
                    }
                    _ if locked => return,
                    _ => {}
                }
            }
            _ if locked => return,
            _ => {}
        }
        let InputEvent::PointerMotionAbsolute { event } = event else {
            return self.process_input_event(event);
        };
        self.activity();
        self.request_redraw();
        let outputs = self.output_areas();
        let area = match event.device().output() {
            Some(output) => self.space.output_geometry(&output),
            None => outputs.iter().copied().reduce(|all, area| all.merge(area)),
        };
        let (Some(area), Some(pointer)) = (area, self.seat.get_pointer()) else {
            return;
        };
        let to = event.position_transformed(area.size) + area.loc.to_f64();
        if let Some(location) = confine(&outputs, pointer.current_location(), to) {
            self.pointer_to(location, event.time());
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
pub(crate) fn confine(
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
