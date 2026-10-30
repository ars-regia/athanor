//! Input from the backend to the seat: keys to the focused surface, the pointer to the surface
//! under it, a click focuses the window it lands on, or the layer surface that asks for the
//! keyboard on demand. The compositor keeps the keys of its bindings for itself, which no client
//! sees (`bindings.rs`). A virtual keyboard's keys and a virtual pointer's motion and buttons, of
//! the clients the allow-list grants them, take the same path (OS4, OS9 task 6).

use crate::bindings;
use crate::keyboard_monitor::Taken;
use crate::state::{identity_of, State, Under};
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Device, Event, InputBackend, InputEvent,
    InputTime, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    PointerMotionEvent,
};
use smithay::input::keyboard::{
    FilterResult, KeyboardHandle, KeyboardSource, Keycode, Xkb, XkbConfig,
};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER};
use smithay::wayland::virtual_keyboard::{
    VirtualKeyboardBackend, VirtualKeyboardDevice, VirtualKeyboardHandler,
    VirtualKeyboardSpecialEvent,
};
use smithay::wayland::virtual_pointer::{VirtualPointerBackend, VirtualPointerHandler};
use std::sync::{Arc, Mutex, PoisonError};

impl State {
    /// A click at `location` focuses the window it lands on, or the layer surface there that
    /// asks for the keyboard on demand. A click on a layer surface that takes the keyboard, of a
    /// identity Super+Escape took the keyboard from, gives its exclusive surfaces the keyboard
    /// back; a click on its windows, or on its surfaces that take no keyboard, does not.
    pub fn click_focus(&mut self, location: Point<f64, Logical>) {
        // A click on a surface of a client Ctrl+Alt+Tab suspended ends the suspension.
        if let Some(client) = self
            .surface_under(location)
            .and_then(|(surface, _)| surface.client())
        {
            self.lift(&client.id());
        }
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
        self.typed(code, key_state, time, source, false);
        self.send_keyboard_group();
    }

    /// As `key_input`, and a key `past_grab` goes to the focused surface past the keyboard's
    /// grab: the input method's, which forwards it through its own virtual keyboard.
    fn typed(
        &mut self,
        code: Keycode,
        key_state: KeyState,
        time: InputTime,
        source: KeyboardSource,
        past_grab: bool,
    ) {
        // A dropped press never reaches the seat: it changes neither the keys held nor the
        // modifiers a hardware key is read with. A release is never dropped: a key held when the
        // session locked is let go, or a window would see it held after unlock.
        if key_state == KeyState::Pressed && self.drops_key(source) {
            return;
        }
        // A hardware key is read under the seat's keymap, which its clients are sent first.
        if source == KeyboardSource::Physical {
            self.restore_seat_keymap();
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let to = keyboard
            .current_focus()
            .and_then(|surface| surface.client());
        // Read before the filter, which runs with the keyboard held.
        let inhibited = self.shortcuts_inhibited();
        let locked = keyboard.modifier_state().serialized.locked;
        // One of Orca's virtual modifiers, pressed or let go: it changes no modifier.
        let mut virtual_modifier = false;
        // Let go, it changed the locked modifiers (Caps Lock): those before its press.
        let mut relock = None;
        let filtered =
            keyboard.input_intercept(source, self, code, key_state, |state, modifiers, handle| {
                let found = match key_state {
                    KeyState::Pressed => bindings::find(modifiers, &handle, state.lock.is_some()),
                    KeyState::Released => None,
                };
                // The keyboard aids of AX11 take their place here, before the monitor (#865).
                // Orca is sent every key once: the input method's forwarded keys are the
                // hardware's it was sent already. The monitor is not a binding: an inhibitor
                // leaves it working (decision 30). A protected key is the compositor's alone,
                // Orca's too (OS4; the maintainer's decision of 2026-10-30, #863), locked or not:
                // the bindings the lock sets aside are protected all the same.
                if !past_grab {
                    let released = key_state == KeyState::Released;
                    let protected = !released
                        && bindings::find(modifiers, &handle, false)
                            .is_some_and(|action| action.never_inhibited());
                    state
                        .keyboard_monitor
                        .observe(modifiers, &handle, released, protected);
                }
                match key_state {
                    KeyState::Pressed => {
                        // Any other press disarms Super alone (D4). Only a hardware key counts:
                        // the input method sends the keys it forwards a second time, as a
                        // virtual keyboard's.
                        // A second Super while one is held is not Super alone either, and
                        // nothing arms while the session is locked or the focused surface
                        // inhibits the bindings.
                        if source == KeyboardSource::Physical {
                            state.super_tap = (state.super_tap.is_none()
                                && state.lock.is_none()
                                && !inhibited
                                && bindings::arms_super(modifiers, &handle))
                            .then_some(code);
                        }
                        // Under an inhibitor only the user's way out stays the compositor's
                        // (decision 30).
                        match found.filter(|action| !inhibited || action.never_inhibited()) {
                            Some(action) => {
                                state.kept_keys.push(code);
                                FilterResult::Intercept(Some(action))
                            }
                            // After the bindings, Orca's grabs, an inhibitor or not (decision
                            // 30): a grab never takes a protected key. While the session is
                            // locked no grab or virtual modifier takes a key from the lock
                            // surface, and Orca, watching, is still sent every key, the lock
                            // surface's included, as cosmic-comp does, so that it speaks during
                            // the unlock: the maintainer's decision of 2026-10-30 (#863), not an
                            // oversight.
                            None if !past_grab && state.lock.is_none() => {
                                match state.keyboard_monitor.takes(modifiers, &handle, locked) {
                                    Some(taken) => {
                                        virtual_modifier = taken == Taken::Modifier;
                                        state.kept_keys.push(code);
                                        FilterResult::Intercept(None)
                                    }
                                    None => FilterResult::Forward,
                                }
                            }
                            None => FilterResult::Forward,
                        }
                    }
                    KeyState::Released => {
                        let before = state.keyboard_monitor.released(&handle);
                        virtual_modifier = before.is_some();
                        relock = before.filter(|before| *before != modifiers.serialized.locked);
                        // The release of a kept press is kept: the client never saw the key
                        // go down.
                        match state.kept_keys.iter().position(|k| *k == code) {
                            Some(at) => {
                                state.kept_keys.swap_remove(at);
                                FilterResult::Intercept(None)
                            }
                            None => FilterResult::Forward,
                        }
                    }
                }
            });
        let filtered = match filtered {
            Some((FilterResult::Intercept(kept), mods_changed)) => {
                // A key kept from the focus for Orca changed the modifiers, as Caps Lock under
                // its grab does: the focus, sent no key, is sent the modifiers, so that it reads
                // the next key as the compositor does. A virtual modifier's change is undone
                // instead (below).
                if kept.is_none() && mods_changed && !virtual_modifier {
                    keyboard.advertise_modifier_state(self);
                }
                Some(kept)
            }
            Some((FilterResult::Forward, mods_changed)) => {
                if past_grab {
                    // The focus is sent no modifiers while a grab holds the keyboard: they are
                    // sent with the key, whether it changed them or not.
                    keyboard
                        .input_forward_bypassing_grab(self, code, key_state, serial, time, true);
                    // The grab was not sent the key: it is sent what the key changed.
                    if mods_changed {
                        self.input_method_modifiers();
                    }
                } else {
                    keyboard.input_forward(self, code, key_state, serial, time, mods_changed);
                }
                None
            }
            // A key another source holds already: there is nothing to send.
            None => None,
        };
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
            Some(Some(action)) => self.fire_by_key(action),
            Some(None) => {}
        }
        if let Some(before) = relock {
            // Caps Lock as Orca's modifier: pressed again and let go, kept from every client as
            // the first press was, it gives back the lock from before its press, on or off.
            for key_state in [KeyState::Pressed, KeyState::Released] {
                let serial = SERIAL_COUNTER.next_serial();
                keyboard.input(self, code, key_state, serial, time, |_, _, _| {
                    FilterResult::Intercept(())
                });
            }
            if keyboard.modifier_state().serialized.locked != before {
                tracing::warn!("Orca's modifier changed the keyboard's locks for good");
            }
            // The focus may have been sent the lock with a key forwarded meanwhile.
            keyboard.advertise_modifier_state(self);
        }
        // Super alone, released: the launcher, after the release has reached the client, which
        // saw Super pressed (D4). Not while locked (`State::fire`), nor when an inhibitor became
        // active while Super was held (decision 30).
        if key_state == KeyState::Released && self.super_tap == Some(code) {
            self.super_tap = None;
            if !self.shortcuts_inhibited() {
                self.fire(bindings::Action::Call(&crate::session_calls::LAUNCHER));
            }
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
        self.refresh_constraint();
    }

    /// A button pressed or released: a press first focuses what is under the pointer.
    pub fn button_input(&mut self, button: u32, button_state: ButtonState, time: InputTime) {
        self.super_tap = None;
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
                self.pointer_moved_to(event.device().id(), location, event.time());
            }
            // A mouse or a touchpad on a console: moved by its delta, kept on the outputs.
            InputEvent::PointerMotion { event, .. } => {
                self.pointer_moved_by(event.delta(), event.delta_unaccel(), event.time());
            }
            InputEvent::PointerButton { event, .. } => {
                self.button_input(event.button_code(), event.state(), event.time());
            }
            InputEvent::PointerAxis { event, .. } => {
                self.super_tap = None;
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
            // A gesture goes to the focused surface; one still disarms Super alone (D4).
            event @ (InputEvent::GestureSwipeBegin { .. }
            | InputEvent::GestureSwipeUpdate { .. }
            | InputEvent::GestureSwipeEnd { .. }
            | InputEvent::GesturePinchBegin { .. }
            | InputEvent::GesturePinchUpdate { .. }
            | InputEvent::GesturePinchEnd { .. }
            | InputEvent::GestureHoldBegin { .. }
            | InputEvent::GestureHoldEnd { .. }) => {
                if matches!(
                    event,
                    InputEvent::GestureSwipeBegin { .. }
                        | InputEvent::GesturePinchBegin { .. }
                        | InputEvent::GestureHoldBegin { .. }
                ) {
                    self.super_tap = None;
                }
                self.gesture_event(event);
            }
            event @ (InputEvent::DeviceAdded { .. }
            | InputEvent::DeviceRemoved { .. }
            | InputEvent::TabletToolAxis { .. }
            | InputEvent::TabletToolProximity { .. }
            | InputEvent::TabletToolTip { .. }
            | InputEvent::TabletToolButton { .. }) => self.tablet_event(event),
            _ => {}
        }
    }
}

impl State {
    /// Where the outputs are in the space.
    pub(crate) fn output_areas(&self) -> Vec<Rectangle<i32, Logical>> {
        self.space
            .outputs()
            .filter_map(|output| self.space.output_geometry(output))
            .collect()
    }

    /// Runs `input` under `device`'s keymap and modifiers. The keymap follows the keyboard that
    /// sent the last key, as wlroots has it: a virtual keyboard's own keymap is loaded, and sent
    /// to the clients, at its first key after another keyboard's, and stays until a key of
    /// another keyboard, so that a client repeating a held key reads it under the keymap it was
    /// pressed with; what a virtual keyboard uploads never reads a hardware key. A virtual
    /// keyboard with no keymap of its own types under the seat's, and its modifiers apply to its
    /// own keys only, the seat's coming back after each.
    fn with_keymap_of(&mut self, device: &VirtualKeyboardDevice, input: impl FnOnce(&mut Self)) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        if !self.load_keymap_of(device, &keyboard) {
            return;
        }
        let modifiers = self.virtual_modifiers.get(device).copied();
        let seat_modifiers = (self.virtual_keymap.is_none() && modifiers.is_some())
            .then(|| keyboard.modifier_state().serialized);
        if let Some(modifiers) = modifiers {
            self.set_modifiers(&keyboard, modifiers);
        }
        input(self);
        if let Some(saved) = seat_modifiers {
            let mask = [
                saved.depressed,
                saved.latched,
                saved.locked,
                saved.layout_effective,
            ];
            self.set_modifiers(&keyboard, mask);
        }
        self.send_keyboard_group();
    }

    /// Sets the seat's modifiers to `[depressed, latched, locked, group]`, a virtual keyboard's,
    /// with no key: the focused surface is sent them when they change, and so is the input
    /// method's grab, which a key would otherwise have carried them to.
    fn set_modifiers(&mut self, keyboard: &KeyboardHandle<Self>, mask: [u32; 4]) {
        let [depressed, latched, locked, group] = mask;
        let before = keyboard.modifier_state().serialized;
        keyboard.with_xkb_state(self, |mut context| {
            context.set_modifier_mask(depressed, latched, locked, group)
        });
        if keyboard.modifier_state().serialized != before {
            self.input_method_modifiers();
        }
    }

    /// Gives the seat `device`'s keymap unless it has it already, or the seat's own when
    /// `device` has none of its own; false when `device`'s keymap does not load, and its key is
    /// dropped.
    fn load_keymap_of(
        &mut self,
        device: &VirtualKeyboardDevice,
        keyboard: &KeyboardHandle<Self>,
    ) -> bool {
        let Some(keymap) = device.keymap() else {
            self.restore_seat_keymap();
            return true;
        };
        let current = self.virtual_keymap.as_ref();
        if current.is_some_and(|current| current.device == *device && current.keymap == keymap) {
            return true;
        }
        let (seat_keymap, seat_modifiers) = match current {
            Some(current) => (current.seat_keymap.clone(), current.seat_modifiers),
            None => {
                let saved = keyboard.modifier_state().serialized;
                (
                    keyboard.with_xkb_state(self, |context| keymap_text(context.xkb())),
                    [
                        saved.depressed,
                        saved.latched,
                        saved.locked,
                        saved.layout_effective,
                    ],
                )
            }
        };
        if *keymap == *seat_keymap {
            self.restore_seat_keymap();
            return true;
        }
        if let Err(error) = keyboard.set_keymap_from_string(self, keymap.to_string()) {
            tracing::warn!(
                ?error,
                "a virtual keyboard's keymap did not load: its key is dropped"
            );
            return false;
        }
        self.virtual_keymap = Some(VirtualKeymap {
            device: device.clone(),
            keymap,
            seat_keymap,
            seat_modifiers,
        });
        self.input_method_keymap_changed();
        true
    }

    /// Gives the seat its own keymap and modifiers back when a virtual keyboard's is current:
    /// before a hardware key, before a key of a virtual keyboard with no keymap of its own, and
    /// when that virtual keyboard goes.
    pub(crate) fn restore_seat_keymap(&mut self) {
        let (Some(current), Some(keyboard)) =
            (self.virtual_keymap.take(), self.seat.get_keyboard())
        else {
            return;
        };
        if let Err(error) = keyboard.set_keymap_from_string(self, current.seat_keymap) {
            // The seat's configuration, as `State::new` gives it, so that the seat never stays
            // on a client's keymap.
            tracing::error!(
                ?error,
                "the seat's keymap did not load again: back to the default"
            );
            if let Err(error) = keyboard.set_xkb_config(self, XkbConfig::default()) {
                tracing::error!(?error, "the default keymap did not load");
            }
        }
        // A new keymap starts with no modifier locked: the seat's Caps Lock and layout return.
        let [depressed, latched, locked, layout] = current.seat_modifiers;
        keyboard.with_xkb_state(self, |mut context| {
            context.set_modifier_mask(depressed, latched, locked, layout)
        });
        self.input_method_keymap_changed();
        self.send_keyboard_group();
    }
}

/// A virtual keyboard's keymap the seat has in place of its own (`State::with_keymap_of`).
pub struct VirtualKeymap {
    device: VirtualKeyboardDevice,
    keymap: Arc<str>,
    /// The seat's keymap and its modifiers (depressed, latched, locked, layout) when the
    /// virtual keymap was loaded, which `State::restore_seat_keymap` gives back.
    seat_keymap: String,
    pub(crate) seat_modifiers: [u32; 4],
}

/// The keymap of `xkb`, as the clients are sent it.
pub(crate) fn keymap_text(xkb: &Mutex<Xkb>) -> String {
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
                // The release of a key forwarded past the input method's grab goes past it too,
                // to the surface that saw the press, even once the grab or its client is gone:
                // smithay lets go of a gone keyboard's keys when its client may be gone too.
                let forwarded = key_state == KeyState::Released
                    && self
                        .past_grab_keys
                        .get_mut(&device)
                        .is_some_and(|held| held.remove(&code));
                // While locked a press is dropped before anything, and a release lets go of a
                // key the seat holds under the keymap the seat has: no client is sent this
                // client's keymap, and the idle timers keep running.
                if self.drops_virtual_input() {
                    if key_state == KeyState::Released {
                        self.key_input(code, key_state, time, device.source());
                    }
                    return;
                }
                self.activity();
                self.request_redraw();
                let past_grab = forwarded || self.holds_keyboard_grab(device.client());
                self.with_keymap_of(&device, |state| {
                    state.typed(code, key_state, time, device.source(), past_grab)
                });
                // A code outside evdev's keys is not kept: its release takes the grab's way.
                if past_grab && key_state == KeyState::Pressed && KEYS.contains(&code.raw()) {
                    self.past_grab_keys.entry(device).or_default().insert(code);
                }
            }
            InputEvent::Special(VirtualKeyboardSpecialEvent::Modifiers {
                device,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            }) => {
                // While this keyboard's keymap is current the seat reads its modifiers, and a
                // change with no key after it, a Shift let go, reaches the focused client at
                // once; otherwise its next key applies it.
                let current = self
                    .virtual_keymap
                    .as_ref()
                    .is_some_and(|current| current.device == device);
                if current && !self.drops_virtual_input() {
                    if let Some(keyboard) = self.seat.get_keyboard() {
                        let mask = [mods_depressed, mods_latched, mods_locked, group];
                        self.set_modifiers(&keyboard, mask);
                    }
                }
                self.virtual_modifiers
                    .insert(device, [mods_depressed, mods_latched, mods_locked, group]);
            }
            InputEvent::DeviceRemoved { device } => {
                self.virtual_modifiers.remove(&device);
                self.past_grab_keys.remove(&device);
                if self
                    .virtual_keymap
                    .as_ref()
                    .is_some_and(|current| current.device == device)
                {
                    self.restore_seat_keymap();
                }
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
            // A pointer coming or going is no input: no activity, so a client that makes and
            // drops pointers in a loop does not keep the session awake.
            InputEvent::DeviceAdded { .. } => return,
            InputEvent::DeviceRemoved { device } => {
                self.absolute_device_gone(&device.id());
                self.virtual_buttons.remove(device);
                return;
            }
            InputEvent::PointerButton { event } => {
                let (device, button) = (event.device(), event.button_code());
                // A code outside evdev's buttons is no button: neither the seat nor the count
                // takes it, so a pointer holds at most every button.
                if !BUTTONS.contains(&button) {
                    return;
                }
                let holds = self
                    .virtual_buttons
                    .get(&device)
                    .is_some_and(|held| held.contains(&button));
                match event.state() {
                    ButtonState::Pressed if locked => return,
                    ButtonState::Pressed => {
                        if !holds {
                            self.virtual_buttons.entry(device).or_default().push(button);
                        }
                    }
                    ButtonState::Released if holds => {
                        if let Some(held) = self.virtual_buttons.get_mut(&device) {
                            held.retain(|b| *b != button);
                        }
                        // Not through `process_input_event`: letting go is no activity, and the
                        // locked session keeps going idle.
                        if locked {
                            return self.button_input(button, ButtonState::Released, event.time());
                        }
                    }
                    ButtonState::Released if locked => return,
                    ButtonState::Released => {}
                }
            }
            InputEvent::PointerMotion { .. }
            | InputEvent::PointerMotionAbsolute { .. }
            | InputEvent::PointerAxis { .. } => {
                if locked {
                    return;
                }
            }
            // Smithay's virtual pointer sends nothing else; anything it may add is no input here.
            _ => return,
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
        // Clamped first: the far edge, x == x_extent, maps to the first column past the area,
        // which a neighbouring output holds.
        let to = clamp_into(
            area,
            event.position_transformed(area.size) + area.loc.to_f64(),
        );
        if let Some(location) = confine(&outputs, pointer.current_location(), to) {
            self.pointer_moved_to(event.device().id(), location, event.time());
        }
    }
}

/// The evdev codes of buttons, BTN_MISC to KEY_MAX.
const BUTTONS: std::ops::RangeInclusive<u32> = 0x100..=0x2ff;

/// The xkb codes of evdev's keys, 0 to KEY_MAX, each the evdev code plus 8.
const KEYS: std::ops::RangeInclusive<u32> = 8..=0x2ff + 8;

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
    Some(clamp_into(*area, to))
}

/// The point of `area` nearest `to`: from its first pixel to its last, never its far edge.
fn clamp_into(area: Rectangle<i32, Logical>, to: Point<f64, Logical>) -> Point<f64, Logical> {
    let (min, max) = (area.loc.to_f64(), (area.loc + area.size).to_f64());
    Point::from((
        to.x.clamp(min.x, max.x - 1.0),
        to.y.clamp(min.y, max.y - 1.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::confine;
    use smithay::utils::{Logical, Point, Rectangle};

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
