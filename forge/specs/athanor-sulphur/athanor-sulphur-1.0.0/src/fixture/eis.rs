//! The fixture's tests of the EIS contexts the portal backend hands to applications (doc_own_stack.md
//! decision 33, wave 2c amendment; doc_portal.md PT15; `eis.rs`). Each context is driven by a
//! reis sender on the test's thread, read without blocking while the compositor's loop runs;
//! an `ei_connection.sync` after each request marks the point where the compositor has handled
//! it. The admission is checked through `check` against a systemd that answers with this
//! process's own pidfd and whatever unit the test sets, and the last tests serve the interface
//! on a private bus.

use super::keyboard_monitor::Bus;
use super::*;
use crate::eis::{
    admit, check, serve, within_ask, Types, Update, ASK, BACKEND, BACKEND_EXECUTABLE, BACKEND_UNIT,
    NAME, PATH,
};
use crate::identity::pidfd_open;
use crate::keyboard_monitor::answer;
use athanor_unit::bus_caller::{Credentials, Manager, Questions, Unit, IMAGE_USER_UNITS};
use smithay::reexports::reis::ei;
use smithay::reexports::reis::enumflags2::BitFlags;
use smithay::reexports::reis::event::{DeviceCapability, EiEvent, EiEventConverter};
use smithay::reexports::reis::handshake::EiHandshaker;
use smithay::reexports::reis::PendingRequestResult;
use std::cell::Cell;
use std::os::fd::AsRawFd;
use std::os::fd::BorrowedFd;
use std::os::unix::fs::MetadataExt;
use std::rc::Rc;
use std::sync::mpsc::channel;
use wayland_client::protocol::wl_touch;
use zbus::fdo::DBusProxy;
use zbus::ObjectPath;
use zbus::OwnedObjectPath;

const KEY_ESC: u32 = 1;
const KEY_T: u32 = 20;
const KEY_LEFTCTRL: u32 = 29;
const KEY_A: u32 = 30;
const KEY_C: u32 = 46;
const KEY_LEFTALT: u32 = 56;
const KEY_LEFTMETA: u32 = 125;
const KEY_LEFTSHIFT: u32 = 42;
const BTN_LEFT: u32 = 0x110;

/// xkb's modifier masks, as `wl_keyboard.modifiers` names them.
const SHIFT: u32 = 1;
const CONTROL: u32 = 1 << 2;
const SUPER: u32 = 1 << 6;

/// The backend's process, as the compositor reads it.
const BACKEND_CGROUP: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/xdg-desktop-portal-athanor.service\n";
const BACKEND_IDENTITY: (&str, &str) = (BACKEND_EXECUTABLE, BACKEND_CGROUP);
/// A process whose identity cannot be read (`Fake`).
const UNREADABLE: (&str, &str) = ("", "");

/// How long a bus call waits.
const WAIT: Duration = Duration::from_secs(10);

/// One EIS context's application end: a reis client.
struct Ei {
    context: ei::Context,
    handshaker: Option<EiHandshaker<'static>>,
    converter: Option<EiEventConverter>,
    /// The devices the compositor added and not removed.
    devices: Vec<smithay::reexports::reis::event::Device>,
    resumed: usize,
    synced: Rc<Cell<u32>>,
    sequence: u32,
    /// The socket read end of stream, or the compositor disconnected the client.
    ended: bool,
}

impl Ei {
    /// The context `handle` with the device types `types`, as the bus thread opens it, and its
    /// application's end, of `context_type`.
    fn open(
        run: &mut Running,
        handle: &str,
        types: u32,
        context_type: ei::handshake::ContextType,
    ) -> Self {
        let (server, client) = UnixStream::pair().expect("a socket pair");
        let types = Types::parse(types).expect("granted types");
        run.state.eis_apply(Update::Open {
            handle: handle.into(),
            types,
            server,
        });
        Self {
            context: ei::Context::new(client).expect("an EI context"),
            handshaker: Some(EiHandshaker::new("athanor-test", context_type)),
            converter: None,
            devices: Vec::new(),
            resumed: 0,
            synced: Rc::new(Cell::new(0)),
            sequence: 0,
            ended: false,
        }
    }

    /// A sender bound to every capability, once `devices` devices are resumed.
    fn sender(run: &mut Running, handle: &str, types: u32, devices: usize) -> Self {
        let mut ei = Self::open(run, handle, types, ei::handshake::ContextType::Sender);
        ei.pump(run, "the devices are resumed", |ei| ei.resumed == devices);
        ei
    }

    fn read(&mut self) {
        loop {
            match self.context.read() {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.ended = true;
                    break;
                }
            }
        }
        while let Some(result) = self.context.pending_event() {
            let PendingRequestResult::Request(event) = result else {
                panic!("the compositor sent a message reis cannot read");
            };
            if let Some(handshaker) = self.handshaker.as_mut() {
                if let Some(done) = handshaker.handle_event(event).expect("the handshake") {
                    self.converter = Some(EiEventConverter::new(&self.context, done));
                    self.handshaker = None;
                }
                continue;
            }
            let converter = self.converter.as_mut().expect("a connection");
            converter.handle_event(event).expect("an EI event");
        }
        let Some(converter) = self.converter.as_mut() else {
            return;
        };
        while let Some(event) = converter.next_event() {
            match event {
                EiEvent::SeatAdded(added) => added.seat.bind_capabilities(BitFlags::all()),
                EiEvent::DeviceAdded(added) => self.devices.push(added.device),
                EiEvent::DeviceResumed(resumed) => {
                    resumed
                        .device
                        .device()
                        .start_emulating(resumed.serial, self.sequence);
                    self.sequence += 1;
                    self.resumed += 1;
                }
                EiEvent::DeviceRemoved(removed) => {
                    self.devices.retain(|device| *device != removed.device)
                }
                EiEvent::Disconnected(_) => self.ended = true,
                _ => {}
            }
        }
        if self.context.flush().is_err() {
            self.ended = true;
        }
    }

    /// Runs the compositor and reads its messages until `done` holds.
    fn pump(&mut self, run: &mut Running, what: &str, done: impl Fn(&Ei) -> bool) {
        let started = Instant::now();
        loop {
            self.read();
            if done(self) {
                return;
            }
            assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
            run.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut run.state)
                .expect("dispatch");
        }
    }

    /// Until the compositor has handled every request sent so far.
    fn sync(&mut self, run: &mut Running) {
        let converter = self.converter.as_mut().expect("a connection");
        let callback = converter.connection().connection().sync(1);
        let synced = self.synced.clone();
        converter.add_callback_handler(callback, move |_| synced.set(synced.get() + 1));
        let before = self.synced.get();
        self.pump(run, "the requests are handled", |ei| {
            ei.synced.get() > before
        });
    }

    fn device(&self, capability: DeviceCapability) -> smithay::reexports::reis::event::Device {
        self.devices
            .iter()
            .find(|device| device.has_capability(capability))
            .cloned()
            .expect("a device of the capability")
    }

    /// Sends `request` on the device of `capability`, framed, and syncs.
    fn send<T: ei::Interface>(
        &mut self,
        run: &mut Running,
        capability: DeviceCapability,
        request: impl FnOnce(&T),
    ) {
        let device = self.device(capability);
        request(&device.interface::<T>().expect("the device's interface"));
        let serial = self
            .converter
            .as_ref()
            .expect("a connection")
            .connection()
            .serial();
        let time = InputTime::now().micros();
        device.device().frame(serial, time);
        self.sync(run);
    }

    fn key(&mut self, run: &mut Running, code: u32, pressed: bool) {
        let state = if pressed {
            ei::keyboard::KeyState::Press
        } else {
            ei::keyboard::KeyState::Released
        };
        self.send(
            run,
            DeviceCapability::Keyboard,
            |keyboard: &ei::Keyboard| keyboard.key(code, state),
        );
    }

    fn keys(&mut self, run: &mut Running, codes: &[u32]) {
        for code in codes {
            self.key(run, *code, true);
        }
        for code in codes.iter().rev() {
            self.key(run, *code, false);
        }
    }

    fn absolute(&mut self, run: &mut Running, x: f32, y: f32) {
        self.send(
            run,
            DeviceCapability::PointerAbsolute,
            |pointer: &ei::PointerAbsolute| pointer.motion_absolute(x, y),
        );
    }

    fn button(&mut self, run: &mut Running, button: u32, pressed: bool) {
        let state = if pressed {
            ei::button::ButtonState::Press
        } else {
            ei::button::ButtonState::Released
        };
        self.send(run, DeviceCapability::Pointer, |buttons: &ei::Button| {
            buttons.button(button, state)
        });
    }

    fn scroll(&mut self, run: &mut Running, dy: f32) {
        self.send(run, DeviceCapability::Pointer, |scroll: &ei::Scroll| {
            scroll.scroll(0.0, dy)
        });
    }

    fn touch_down(&mut self, run: &mut Running, id: u32, x: f32, y: f32) {
        self.send(run, DeviceCapability::Touch, |touch: &ei::Touchscreen| {
            touch.down(id, x, y)
        });
    }
}

/// Moves the fixture's own window out of the way.
fn park_the_fixture(run: &mut Running) {
    let (window, _) = fixture_window(run);
    run.state.space.map_element(window, (40, 600), false);
}

/// A window at `at`, focused, with its seat's keyboard, pointer and touch, of a client that
/// connects as `identity`.
fn window_at(run: &mut Running, at: (i32, i32), identity: (&'static str, &'static str)) -> Local {
    window_in(run, at, identity, None)
}

/// A window as `window_at` makes it, of a client that connects through `context` when one is
/// given.
fn window_in(
    run: &mut Running,
    at: (i32, i32),
    identity: (&'static str, &'static str),
    context: Option<SecurityContext>,
) -> Local {
    *run.identity.lock().expect("identity") = identity;
    let mut local = Local::connect(run, context);
    *run.identity.lock().expect("identity") = BAR;
    let _toplevel = local.toplevel(run);
    let seat = local.seat.clone().expect("a seat");
    let qh = local.queue.handle();
    let _keyboard = seat.get_keyboard(&qh, ());
    let _touch: wl_touch::WlTouch = seat.get_touch(&qh, ());
    let window = window_of(run, local.id.as_ref().expect("an id"));
    run.state.space.map_element(window.clone(), at, false);
    run.state.focus(&window);
    local.pump(run, "the window has the keyboard", |client, _| {
        client.keymaps > 0
    });
    local.roundtrip(run);
    local
}

fn focus(run: &mut Running, local: &mut Local) {
    let window = window_of(run, local.id.as_ref().expect("an id"));
    run.state.focus(&window);
    local.roundtrip(run);
}

/// An injected key reaches the focused window under the context's own modifiers and fires no
/// binding, and the seat's modifiers are told again after it: an injected Super with a hardware
/// C, a hardware Super with an injected C, and Ctrl+C injected whole.
#[test]
fn eis_keys_reach_the_window_and_never_a_binding() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);

    let fired = run.state.fired.len();
    ei.keys(&mut run, &[KEY_LEFTMETA, KEY_C]);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys,
        [KEY_LEFTMETA, KEY_C],
        "Super+C reaches the window"
    );
    assert_eq!(window.client.released, [KEY_C, KEY_LEFTMETA]);
    assert_eq!(run.state.fired.len(), fired, "and fires no binding");

    // An injected Super held, a hardware C: the seat reads C without Super.
    ei.key(&mut run, KEY_LEFTMETA, true);
    sent_to(&mut window, &mut run, &[KEY_C]);
    assert_eq!(
        run.state.fired.len(),
        fired,
        "the hardware's C binds nothing"
    );
    let told = window.client.modifiers.last().copied();
    assert_eq!(
        told.map(|mods| mods & SUPER),
        Some(0),
        "C is read without Super"
    );
    ei.key(&mut run, KEY_LEFTMETA, false);

    // A hardware Super held, an injected C: the client reads C without Super, then Super again.
    key(&mut run.state, KEY_LEFTMETA, true);
    let told = window.client.modifiers.len();
    ei.key(&mut run, KEY_C, true);
    ei.key(&mut run, KEY_C, false);
    assert_eq!(
        run.state.fired.len(),
        fired,
        "an injected key binds nothing"
    );
    // The injected key cancels Super alone, as a hardware key would: its release fires nothing.
    key(&mut run.state, KEY_LEFTMETA, false);
    window.roundtrip(&mut run);
    assert_eq!(
        run.state.fired.len(),
        fired,
        "Super's release after an injected key is not Super alone"
    );
    assert!(
        window.client.modifiers[told..]
            .iter()
            .any(|mods| mods & SUPER == 0),
        "C is read under the context's modifiers"
    );

    // An injected Ctrl held: the client reads it with the injected C, never with a hardware C.
    ei.key(&mut run, KEY_LEFTCTRL, true);
    let told = window.client.modifiers.len();
    ei.key(&mut run, KEY_C, true);
    window.roundtrip(&mut run);
    assert!(
        window.client.modifiers[told..]
            .iter()
            .any(|mods| mods & CONTROL != 0),
        "the injected C is read with Ctrl"
    );
    assert_eq!(
        window.client.modifiers.last().map(|mods| mods & CONTROL),
        Some(0),
        "and the seat's modifiers are told again after it"
    );
    ei.key(&mut run, KEY_C, false);
    sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(
        window.client.modifiers.last().map(|mods| mods & CONTROL),
        Some(0),
        "a hardware key is read without the injected Ctrl"
    );
    ei.key(&mut run, KEY_LEFTCTRL, false);
    run.stop();
}

/// Only an injected key delivered to a client cancels a pending tap of Super (decision 33):
/// with no keyboard focus none is delivered, and the hardware Super's release is still Super
/// alone.
#[test]
fn eis_undelivered_key_keeps_the_super_tap() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);
    run.state.set_keyboard_focus(None);
    window.roundtrip(&mut run);

    let fired = run.state.fired.len();
    key(&mut run.state, KEY_LEFTMETA, true);
    ei.keys(&mut run, &[KEY_C]);
    key(&mut run.state, KEY_LEFTMETA, false);
    window.roundtrip(&mut run);
    assert!(
        !window.client.keys.contains(&KEY_C),
        "the injected key reaches no client"
    );
    assert_eq!(
        run.state.fired.len(),
        fired + 1,
        "and Super's release is still Super alone"
    );
    run.stop();
}

/// The physical input wins (decision 33): a physical press of a key a context holds reaches the
/// client after the release of the context's; the context's modifiers no longer count the key,
/// its next release reaches no client, and a press of it after the physical release reaches the
/// client with its own release. A physical Super pressed over the context's still opens the
/// launcher on its release.
#[test]
fn eis_yields_a_key_to_a_physical_press() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);

    ei.key(&mut run, KEY_LEFTSHIFT, true);
    window.roundtrip(&mut run);
    let from = window.client.input.len();
    key(&mut run.state, KEY_LEFTSHIFT, true);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input[from..],
        [(KEY_LEFTSHIFT, false), (KEY_LEFTSHIFT, true)],
        "the context's Shift is released before the physical press"
    );
    key(&mut run.state, KEY_LEFTSHIFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.last(),
        Some(&(KEY_LEFTSHIFT, false)),
        "the physical release reaches the client"
    );
    let from = window.client.input.len();
    ei.key(&mut run, KEY_LEFTSHIFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.len(),
        from,
        "the context's next release reaches no client"
    );

    let told = window.client.modifiers.len();
    ei.keys(&mut run, &[KEY_A]);
    window.roundtrip(&mut run);
    assert!(
        window.client.modifiers[told..]
            .iter()
            .all(|mods| mods & SHIFT == 0),
        "the context's modifiers no longer count Shift"
    );

    let from = window.client.input.len();
    ei.keys(&mut run, &[KEY_LEFTSHIFT]);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input[from..],
        [(KEY_LEFTSHIFT, true), (KEY_LEFTSHIFT, false)],
        "a press of the context after the physical release reaches the client with its release"
    );

    // A context's press, a physical press and release, and the context's press again with no
    // release between: the last press is the context's own, delivered with its release.
    ei.key(&mut run, KEY_C, true);
    key(&mut run.state, KEY_C, true);
    key(&mut run.state, KEY_C, false);
    window.roundtrip(&mut run);
    let from = window.client.input.len();
    ei.key(&mut run, KEY_C, true);
    ei.key(&mut run, KEY_C, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input[from..],
        [(KEY_C, true), (KEY_C, false)],
        "a press of the context after the physical one is judged anew"
    );

    let fired = run.state.fired.len();
    ei.key(&mut run, KEY_LEFTMETA, true);
    key(&mut run.state, KEY_LEFTMETA, true);
    key(&mut run.state, KEY_LEFTMETA, false);
    window.roundtrip(&mut run);
    assert_eq!(
        run.state.fired.len(),
        fired + 1,
        "a physical Super over the context's opens the launcher on its release"
    );
    ei.key(&mut run, KEY_LEFTMETA, false);
    run.stop();
}

/// The seat wins, physical or virtual (decision 33): a virtual keyboard's press of a key a
/// context holds reaches the client after the release of the context's, and the context's next
/// release reaches no client.
#[test]
fn eis_yields_a_key_to_a_virtual_press() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let typist = VirtualKeyboard::new(&mut window, &mut run, None);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);

    ei.key(&mut run, KEY_LEFTSHIFT, true);
    window.roundtrip(&mut run);
    let from = window.client.input.len();
    typist.key(&mut window, &mut run, KEY_LEFTSHIFT, true);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input[from..],
        [(KEY_LEFTSHIFT, false), (KEY_LEFTSHIFT, true)],
        "the context's Shift is released before the virtual press"
    );
    typist.key(&mut window, &mut run, KEY_LEFTSHIFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.last(),
        Some(&(KEY_LEFTSHIFT, false)),
        "the virtual release reaches the client"
    );
    let from = window.client.input.len();
    ei.key(&mut run, KEY_LEFTSHIFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.len(),
        from,
        "the context's next release reaches no client"
    );
    run.stop();
}

/// The physical input wins for a button as for a key (decision 33), and a button one context
/// holds is neither pressed again nor released by another.
#[test]
fn eis_yields_a_button_to_a_physical_press() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut window = window_at(&mut run, (300, 200), BAR);
    let mut ei = Ei::sender(&mut run, "/session/1", 2, 2);
    let mut other = Ei::sender(&mut run, "/session/2", 2, 2);
    ei.absolute(&mut run, 320.0, 220.0);

    ei.button(&mut run, BTN_LEFT, true);
    window.roundtrip(&mut run);
    let from = window.client.input.len();
    other.button(&mut run, BTN_LEFT, true);
    other.button(&mut run, BTN_LEFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.len(),
        from,
        "a button another context holds is neither pressed again nor released"
    );

    run.state
        .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input[from..],
        [(BTN_LEFT, false), (BTN_LEFT, true)],
        "the context's button is released before the physical press"
    );
    run.state
        .button_input(BTN_LEFT, ButtonState::Released, InputTime::now());
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.last(),
        Some(&(BTN_LEFT, false)),
        "the physical release reaches the client"
    );
    let from = window.client.input.len();
    ei.button(&mut run, BTN_LEFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input.len(),
        from,
        "the context's next release reaches no client"
    );
    ei.button(&mut run, BTN_LEFT, true);
    ei.button(&mut run, BTN_LEFT, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.input[from..],
        [(BTN_LEFT, true), (BTN_LEFT, false)],
        "a press of the context after the physical release reaches the client with its release"
    );
    run.stop();
}

/// A touch point is its device's slot: an injected touch and a physical one never share one,
/// whatever slot numbers their devices give, so neither has to yield to the other. Each is a
/// touch point of its own, and a lift ends only its own.
#[test]
fn eis_touch_points_stay_apart_from_the_seats() {
    use smithay::backend::input::TouchSlot;
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut window = window_at(&mut run, (300, 200), BAR);
    let mut ei = Ei::sender(&mut run, "/session/1", 7, 4);

    ei.touch_down(&mut run, 0, 330.0, 230.0);
    window.roundtrip(&mut run);
    let slot = TouchSlot::from(Some(0));
    run.state
        .touch_down("touchscreen", slot, (340.0, 240.0).into(), InputTime::now());
    run.state.touch_frame();
    window.roundtrip(&mut run);
    let downs = &window.client.touch.downs;
    assert_eq!(
        downs.len(),
        2,
        "the physical touch is a touch point of its own"
    );
    assert_ne!(downs[0].1, downs[1].1, "with its own id");
    let physical = downs[1].1;
    run.state.touch_up("touchscreen", slot, InputTime::now());
    run.state.touch_frame();
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.touch.ups,
        [physical],
        "its lift ends it alone"
    );
    run.stop();
}

/// A protected key of decision 30 is dropped under the context's modifiers, its release with it;
/// a key the seat or another context holds is neither pressed again nor released by the context,
/// and the holder's own release still reaches the client (decision 33).
#[test]
fn eis_drops_protected_and_held_keys() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);
    let mut other = Ei::sender(&mut run, "/session/2", 1, 1);

    ei.keys(&mut run, &[KEY_LEFTMETA, KEY_LEFTALT, KEY_T]);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys,
        [KEY_LEFTMETA, KEY_LEFTALT],
        "the reveal key's T is dropped"
    );
    assert_eq!(
        window.client.released,
        [KEY_LEFTALT, KEY_LEFTMETA],
        "and its release"
    );

    key(&mut run.state, KEY_A, true);
    window.roundtrip(&mut run);
    let (pressed, released) = (window.client.keys.len(), window.client.released.len());
    ei.keys(&mut run, &[KEY_A]);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys.len(),
        pressed,
        "a key the hardware holds is not pressed again"
    );
    assert_eq!(
        window.client.released.len(),
        released,
        "nor released by the context"
    );
    key(&mut run.state, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released[released..],
        [KEY_A],
        "the hardware's release still reaches the client"
    );

    let cs = |codes: &[u32]| codes.iter().filter(|code| **code == KEY_C).count();
    ei.key(&mut run, KEY_C, true);
    other.keys(&mut run, &[KEY_C]);
    window.roundtrip(&mut run);
    assert_eq!(
        cs(&window.client.keys),
        1,
        "a key another context holds is not pressed again"
    );
    assert_eq!(cs(&window.client.released), 0, "nor released by it");
    ei.key(&mut run, KEY_C, false);
    window.roundtrip(&mut run);
    assert_eq!(
        cs(&window.client.released),
        1,
        "the holding context's release still reaches the client"
    );
    run.stop();
}

/// Neither the keyboard monitor nor the keyboard aids see an injected key.
#[test]
fn eis_keys_pass_the_monitor_and_the_aids() {
    use crate::keyboard_monitor::{Change, Update as Watch};
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);
    let (events, queued) = std::sync::mpsc::sync_channel(64);
    run.state.keyboard_monitor.events = Some(events);
    run.state.keyboard_monitor.apply(Watch {
        client: ":1.42".into(),
        change: Change::Watch(true),
    });
    run.state.set_sticky_keys(true, false);

    ei.keys(&mut run, &[KEY_LEFTSHIFT]);
    ei.keys(&mut run, &[KEY_A]);
    window.roundtrip(&mut run);
    assert_eq!(
        queued.try_iter().count(),
        0,
        "the monitor hears no injected key"
    );
    assert!(
        run.state.aids.sent.is_empty(),
        "an injected Shift latches nothing"
    );
    sent_to(&mut window, &mut run, &[KEY_A]);
    assert!(
        queued.try_iter().count() > 0,
        "the hardware's keys are heard"
    );
    run.stop();
}

/// From the lock request on nothing injected is delivered, and what a context held when the
/// lock came is released to the window it was pressed on; a pause releases it too.
#[test]
fn eis_lock_and_pause_release_and_drop() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut window = window_at(&mut run, (300, 200), BAR);
    let mut ei = Ei::sender(&mut run, "/session/1", 7, 4);

    ei.key(&mut run, KEY_A, true);
    ei.absolute(&mut run, 320.0, 220.0);
    ei.button(&mut run, BTN_LEFT, true);
    ei.touch_down(&mut run, 0, 330.0, 230.0);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A]);
    assert!(window.client.button.is_some(), "the button is pressed");
    assert_eq!(window.client.touch.downs.len(), 1, "the finger is down");

    run.state.eis_suspend();
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released,
        [KEY_A],
        "a pause releases the held key"
    );
    assert_eq!(window.client.touch.cancels, 1, "and cancels the held touch");
    ei.key(&mut run, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released,
        [KEY_A],
        "whose late release is dropped"
    );

    ei.key(&mut run, KEY_C, true);
    window.roundtrip(&mut run);
    let (mut locker, _lock) = lock_session(&mut run);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released,
        [KEY_A, KEY_C],
        "the lock releases the held key first"
    );
    ei.keys(&mut run, &[KEY_ESC]);
    ei.touch_down(&mut run, 1, 330.0, 230.0);
    locker.roundtrip(&mut run);
    window.roundtrip(&mut run);
    assert!(
        locker.client.keys.is_empty(),
        "while locked the lock is sent no injected key"
    );
    assert!(
        !window.client.keys.contains(&KEY_ESC),
        "and neither is the window"
    );
    assert_eq!(window.client.touch.downs.len(), 1, "nor any touch");
    run.stop();
}

/// Closing a context releases its keys and buttons and lifts its touches with `wl_touch.up`;
/// the other context, whose device has the same id within its own connection, keeps its own,
/// and the application reads end of stream.
#[test]
fn eis_closing_a_context_releases_its_input_alone() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut window = window_at(&mut run, (300, 200), BAR);
    let mut ei = Ei::sender(&mut run, "/session/1", 5, 2);
    let mut other = Ei::sender(&mut run, "/session/2", 5, 2);

    ei.key(&mut run, KEY_A, true);
    other.key(&mut run, KEY_C, true);
    ei.touch_down(&mut run, 0, 320.0, 220.0);
    other.touch_down(&mut run, 0, 330.0, 230.0);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.touch.downs.len(),
        2,
        "two fingers, one per context"
    );

    let (done, answered) = answer();
    run.state.eis_apply(Update::Close {
        handle: "/session/1".into(),
        done,
    });
    assert_eq!(
        zbus::block_on(answered).map(|done| done.is_ok()),
        Some(true)
    );
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released,
        [KEY_A],
        "the context's key is released, not the other's"
    );
    assert_eq!(window.client.touch.ups.len(), 1, "its touch is lifted");
    assert_eq!(window.client.touch.cancels, 0, "never cancelled");
    ei.pump(&mut run, "the application reads end of stream", |ei| {
        ei.ended
    });
    assert_eq!(run.state.eis.open(), 1, "the other context stays");

    other.key(&mut run, KEY_C, false);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released,
        [KEY_A, KEY_C],
        "the other context still holds its key"
    );
    run.stop();
}

/// The backend's name changing owner ends every context: each releases its keys, and each
/// application reads end of stream.
#[test]
fn eis_closing_every_context() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);
    let mut other = Ei::sender(&mut run, "/session/2", 1, 1);
    ei.key(&mut run, KEY_A, true);
    other.key(&mut run, KEY_C, true);
    run.state.eis_apply(Update::CloseAll);
    window.roundtrip(&mut run);
    let mut released = window.client.released.clone();
    released.sort_unstable();
    assert_eq!(released, [KEY_A, KEY_C], "every context's key is released");
    assert_eq!(run.state.eis.open(), 0);
    ei.pump(&mut run, "the application reads end of stream", |ei| {
        ei.ended
    });
    other.pump(&mut run, "the other too", |ei| ei.ended);
    run.stop();
}

/// A change of the seat's keymap releases what each context holds and gives its keyboard the
/// new keymap again: the session goes on, and its keys still reach the window.
#[test]
fn eis_keymap_change_keeps_the_session() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut ei = Ei::sender(&mut run, "/session/1", 1, 1);
    ei.key(&mut run, KEY_A, true);
    let settings = crate::xkb_config::Settings {
        rules: String::new(),
        model: String::new(),
        layout: "it".into(),
        variant: String::new(),
        options: None,
        repeat_delay: 600,
        repeat_rate: 25,
    };
    run.state.eis_keymap_changed(&settings);
    window.roundtrip(&mut run);
    assert_eq!(window.client.released, [KEY_A], "the held key is released");
    ei.pump(&mut run, "the keyboard is offered again", |ei| {
        ei.resumed == 2
    });
    ei.sync(&mut run);
    assert_eq!(ei.devices.len(), 1, "one keyboard, the old one removed");
    assert_eq!(run.state.eis.open(), 1, "the session goes on");
    ei.key(&mut run, KEY_A, false);
    ei.keys(&mut run, &[KEY_C]);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys,
        [KEY_A, KEY_C],
        "and its keys still reach the window"
    );
    assert_eq!(
        window.client.released,
        [KEY_A, KEY_C],
        "a stale release is dropped"
    );
    run.stop();
}

/// A context is offered the granted devices alone and never a text device; a receiver context
/// is refused and leaves nothing behind.
#[test]
fn eis_offers_the_granted_devices_alone() {
    use DeviceCapability::{Keyboard, Pointer, PointerAbsolute, Text, Touch};
    let mut run = Running::start(None);
    // Each device's kinds, once every device the compositor offers has arrived.
    let offered = |run: &mut Running, handle: &str, types: u32, devices: usize| {
        let mut ei = Ei::sender(run, handle, types, devices);
        ei.sync(run);
        let mut kinds: Vec<Vec<DeviceCapability>> = ei
            .devices
            .iter()
            .map(|device| {
                [Keyboard, Pointer, PointerAbsolute, Touch, Text]
                    .into_iter()
                    .filter(|kind| device.has_capability(*kind))
                    .collect()
            })
            .collect();
        kinds.sort_by_key(|kind| format!("{kind:?}"));
        kinds
    };
    assert_eq!(offered(&mut run, "/session/1", 1, 1), [vec![Keyboard]]);
    assert_eq!(
        offered(&mut run, "/session/2", 2, 2),
        [vec![PointerAbsolute], vec![Pointer]]
    );
    assert_eq!(offered(&mut run, "/session/3", 4, 1), [vec![Touch]]);
    assert_eq!(
        offered(&mut run, "/session/5", 7, 4),
        [
            vec![Keyboard],
            vec![PointerAbsolute],
            vec![Pointer],
            vec![Touch]
        ]
    );
    // Each application above has gone, and so has its context; this one stays.
    let _kept = Ei::sender(&mut run, "/session/6", 1, 1);

    let mut receiver = Ei::open(
        &mut run,
        "/session/4",
        1,
        ei::handshake::ContextType::Receiver,
    );
    receiver.pump(&mut run, "a receiver is disconnected", |ei| ei.ended);
    run.until(
        "the receiver's context and the gone ones are ended",
        |state| state.eis.open() == 1,
    );
    run.stop();
}

/// The backend's surfaces fail closed: a client outside a security context whose identity
/// cannot be read gets no injected key, as the backend gets none; a confined client, which has
/// no identity by design and is never the backend, still gets one.
#[test]
fn eis_counts_an_unreadable_client_as_the_backend() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut unreadable = window_at(&mut run, (300, 200), UNREADABLE);
    let context = sandbox(&run);
    let mut confined = window_in(&mut run, (700, 200), BAR, Some(context));
    let mut ei = Ei::sender(&mut run, "/session/1", 7, 4);

    ei.keys(&mut run, &[KEY_A]);
    confined.roundtrip(&mut run);
    assert_eq!(
        confined.client.keys,
        [KEY_A],
        "a confined client gets an injected key"
    );
    assert_eq!(confined.client.released, [KEY_A], "and its release");

    focus(&mut run, &mut unreadable);
    ei.keys(&mut run, &[KEY_C]);
    unreadable.roundtrip(&mut run);
    assert!(
        unreadable.client.keys.is_empty(),
        "a client whose identity cannot be read gets no injected key"
    );
    assert!(unreadable.client.released.is_empty(), "nor its release");
    run.stop();
}

/// No injected input reaches a surface of the portal backend: a key while it has the keyboard,
/// with the keys held before it took it released to the window they were pressed on; a motion
/// never gives it the pointer; a button, a scroll and a touch aimed at it are dropped.
#[test]
fn eis_never_reaches_the_backend() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut window = window_at(&mut run, (300, 200), BAR);
    let mut backend = window_at(&mut run, (700, 200), BACKEND_IDENTITY);
    let mut ei = Ei::sender(&mut run, "/session/1", 7, 4);

    ei.keys(&mut run, &[KEY_A]);
    backend.roundtrip(&mut run);
    assert!(
        backend.client.keys.is_empty(),
        "a key is dropped while the backend has the keyboard"
    );
    assert!(backend.client.released.is_empty(), "its release too");

    focus(&mut run, &mut window);
    ei.key(&mut run, KEY_C, true);
    focus(&mut run, &mut backend);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.released,
        [KEY_C],
        "a held key is released before the backend has it"
    );
    assert!(
        backend.client.entered_keys.is_empty(),
        "the backend enters with no injected key"
    );
    ei.key(&mut run, KEY_C, false);
    backend.roundtrip(&mut run);
    assert!(backend.client.released.is_empty(), "and is sent no release");
    focus(&mut run, &mut window);
    assert!(
        window.client.entered_keys.is_empty(),
        "the key is not left held"
    );
    ei.keys(&mut run, &[KEY_C]);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys.last(),
        Some(&KEY_C),
        "and is pressed again"
    );

    ei.absolute(&mut run, 720.0, 220.0);
    backend.roundtrip(&mut run);
    assert!(
        backend.client.pointer_enter.is_none(),
        "a motion never gives the backend the pointer"
    );
    let device = "event2".to_owned();
    run.state
        .pointer_moved_to(device, (720.0, 220.0).into(), InputTime::now());
    backend.roundtrip(&mut run);
    assert!(
        backend.client.pointer_enter.is_some(),
        "the hardware's motion does"
    );
    ei.button(&mut run, BTN_LEFT, true);
    ei.button(&mut run, BTN_LEFT, false);
    ei.scroll(&mut run, 10.0);
    ei.touch_down(&mut run, 0, 720.0, 220.0);
    backend.roundtrip(&mut run);
    assert!(
        backend.client.button.is_none(),
        "a button aimed at the backend is dropped"
    );
    assert_eq!(backend.client.axes, 0, "a scroll too");
    assert!(backend.client.touch.downs.is_empty(), "a touch too");

    ei.absolute(&mut run, 320.0, 220.0);
    ei.button(&mut run, BTN_LEFT, true);
    ei.button(&mut run, BTN_LEFT, false);
    ei.scroll(&mut run, 10.0);
    window.roundtrip(&mut run);
    assert!(
        window.client.pointer_enter.is_some(),
        "the window gets the pointer"
    );
    assert!(window.client.button.is_some(), "and the button");
    assert!(window.client.axes > 0, "and the scroll");
    run.stop();
}

/// A button the hardware holds is not pressed again by a context, so the context's release
/// cannot end the user's own drag.
#[test]
fn eis_leaves_a_button_the_seat_holds() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut window = window_at(&mut run, (300, 200), BAR);
    let mut ei = Ei::sender(&mut run, "/session/1", 2, 2);
    ei.absolute(&mut run, 320.0, 220.0);
    run.state
        .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
    window.roundtrip(&mut run);
    let pressed = window.client.button;
    ei.button(&mut run, BTN_LEFT, true);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.button, pressed,
        "the context's press is dropped"
    );
    let (done, answered) = answer();
    run.state.eis_apply(Update::Close {
        handle: "/session/1".into(),
        done,
    });
    assert_eq!(
        zbus::block_on(answered).map(|done| done.is_ok()),
        Some(true)
    );
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(pointer.is_grabbed(), "the hardware's button is still held");
    run.stop();
}

/// Injected input is delivered while the session is neither locked nor paused.
#[test]
fn eis_delivers_unlocked_and_running_alone() {
    use crate::eis::delivers;
    assert!(delivers(false, false));
    assert!(!delivers(true, false));
    assert!(!delivers(false, true));
    assert!(!delivers(true, true));
}

/// The device types: a combination of the portal's three bits, and nothing else.
#[test]
fn eis_types_are_the_portals() {
    for types in 1..=7 {
        assert!(Types::parse(types).is_some(), "{types}");
    }
    for types in [0, 8, 9, u32::MAX] {
        assert!(Types::parse(types).is_none(), "{types}");
    }
}

/// The user's systemd, as the admission asks it: every peer is this process, of our user and
/// with a pidfd, run by a verified manager in the unit `unit`, whose one `ExecStart` is `exec`.
struct Systemd {
    unit: &'static str,
    exec: &'static str,
}

const BACKEND_SYSTEMD: Systemd = Systemd {
    unit: BACKEND_UNIT,
    exec: BACKEND_EXECUTABLE,
};

fn ours() -> u32 {
    std::fs::metadata("/proc/self").expect("this process").uid()
}

fn path(path: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(path).expect("an object path")
}

impl Questions for Systemd {
    async fn user_manager(&self) -> Result<String, String> {
        Ok(":1.2".into())
    }

    async fn credentials(&self, _peer: &str) -> Result<Credentials, String> {
        let pidfd = pidfd_open(std::process::id()).map_err(|error| error.to_string())?;
        Ok(Credentials {
            uid: Some(ours()),
            pid: Some(std::process::id()),
            pidfd: Some(pidfd),
        })
    }

    async fn unit_by_pidfd(
        &self,
        manager: Manager<'_>,
        _pidfd: BorrowedFd<'_>,
    ) -> Result<(OwnedObjectPath, String), String> {
        Ok(match manager {
            Manager::System => (path("/user"), format!("user@{}.service", ours())),
            Manager::User(_) => (path("/unit"), self.unit.into()),
        })
    }

    async fn main_pid(
        &self,
        _manager: Manager<'_>,
        _path: &OwnedObjectPath,
    ) -> Result<u32, String> {
        Ok(std::process::id())
    }

    async fn unit(&self, _manager: Manager<'_>, _path: &OwnedObjectPath) -> Result<Unit, String> {
        Ok(Unit {
            main_pid: std::process::id(),
            exec_start: vec![self.exec.into()],
            fragment_path: format!("{IMAGE_USER_UNITS}/{}", self.unit),
            drop_in_paths: Vec::new(),
        })
    }
}

/// Only the backend, the main process of its unit as systemd tells it, while it owns its name,
/// is admitted.
#[test]
fn eis_admits_the_backend_alone() {
    let (client, other) = (":1.7", ":1.8");
    assert!(
        check(&BACKEND_SYSTEMD, client, client).is_ok(),
        "the backend"
    );
    assert!(
        check(&BACKEND_SYSTEMD, client, other).is_err(),
        "a client that does not own the name"
    );
    let other_unit = Systemd {
        unit: "app-gnome-terminal-1.scope",
        ..BACKEND_SYSTEMD
    };
    assert!(check(&other_unit, client, client).is_err(), "another unit");
    let other_executable = Systemd {
        exec: "/usr/bin/python3",
        ..BACKEND_SYSTEMD
    };
    assert!(
        check(&other_executable, client, client).is_err(),
        "a unit that runs another executable"
    );
}

/// The owner of the backend's name is asked within `ASK`: on a bus that answers nothing, the
/// call is refused once the bound has passed, and the `eis` thread goes on to the next.
#[test]
fn eis_admission_refuses_a_silent_bus_within_its_bound() {
    let bus = Bus::start();
    let asking =
        zbus::block_on(within_ask(bus.builder()).build()).expect("a connection to the bus");
    let proxy = zbus::block_on(DBusProxy::new(&asking)).expect("the bus's proxy");
    bus.stop();
    let (refused, read) = channel();
    std::thread::spawn(move || {
        // The test may have ended, and its receiver with it.
        refused
            .send(admit(&proxy, &BACKEND_SYSTEMD, ":1.7").is_err())
            .ok();
    });
    assert_eq!(
        read.recv_timeout(ASK + WAIT),
        Ok(true),
        "a silent bus refuses the call within the bound"
    );
}

/// Whether `fd` is close-on-exec, read from `/proc/self/fdinfo`.
fn closes_on_exec(fd: i32) -> bool {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).expect("the fd's info");
    let flags = info
        .lines()
        .find_map(|line| line.strip_prefix("flags:"))
        .expect("the fd's flags");
    let flags = u32::from_str_radix(flags.trim(), 8).expect("octal flags");
    flags & 0o2000000 != 0
}

/// The error's name, if the call failed.
fn refused<T>(answer: zbus::Result<T>) -> Option<String> {
    match answer {
        Ok(_) => None,
        Err(zbus::Error::MethodError(name, _, _)) => Some(name.to_string()),
        Err(zbus::Error::FDO(error)) => Some(zbus::DBusError::name(&*error).to_string()),
        Err(error) => panic!("the call did not reach the interface: {error}"),
    }
}

fn connect_to_eis(
    client: &zbus::Connection,
    handle: &str,
    types: u32,
) -> zbus::Result<zbus::OwnedFd> {
    let handle = ObjectPath::try_from(handle).expect("a path");
    let reply = zbus::block_on(client.call_method(
        Some(NAME),
        PATH,
        Some("os.athanor.Eis1"),
        "ConnectToEIS",
        &(handle, types),
    ))?;
    reply.body().deserialize()
}

/// On a real bus the interface admits the backend alone and refuses another client with
/// `AccessDenied`; refuses unknown device types; hands out one close-on-exec socket per session
/// handle; closes a session once the loop has; and a second server, the name taken, stops.
#[test]
fn eis_on_the_bus_serves_the_backend_alone() {
    let bus = Bus::start();
    let (served, asking) = (bus.builder(), bus.connect());
    let (sent, updates) = channel();
    let server = std::thread::spawn(move || {
        let send = |update: Update| {
            sent.send(update)
                .map_err(|_| crate::Error::from("the test has ended"))
        };
        serve(served, &asking, &BACKEND_SYSTEMD, &send).map_err(|error| error.to_string())
    });
    let (backend, other) = (bus.connect(), bus.connect());
    let names = zbus::block_on(DBusProxy::new(&backend)).expect("the bus's proxy");
    let deadline = Instant::now() + WAIT;
    while !zbus::block_on(names.name_has_owner(NAME.try_into().expect("a name")))
        .expect("the bus answers")
    {
        assert!(
            Instant::now() < deadline,
            "the interface took its name in time"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    zbus::block_on(backend.request_name(BACKEND)).expect("the backend's name");

    let denied = Some("org.freedesktop.DBus.Error.AccessDenied".to_owned());
    assert_eq!(
        refused(connect_to_eis(&other, "/session/1", 1)),
        denied,
        "another client"
    );
    let invalid = Some("org.freedesktop.DBus.Error.InvalidArgs".to_owned());
    for types in [0, 8] {
        assert_eq!(
            refused(connect_to_eis(&backend, "/session/1", types)),
            invalid,
            "{types}"
        );
    }
    assert!(updates.try_recv().is_err(), "a refused call opens nothing");

    let fd = connect_to_eis(&backend, "/session/1", 3).expect("a socket");
    assert!(
        closes_on_exec(fd.as_raw_fd()),
        "the application's end is close-on-exec"
    );
    let Ok(Update::Open {
        handle,
        types,
        server: end,
    }) = updates.try_recv()
    else {
        panic!("the context is opened before the answer");
    };
    assert_eq!(
        (handle.as_str(), types),
        ("/session/1", Types::parse(3).expect("types"))
    );
    assert!(closes_on_exec(end.as_raw_fd()), "the compositor's end too");
    let failed = Some("org.freedesktop.DBus.Error.Failed".to_owned());
    assert_eq!(
        refused(connect_to_eis(&backend, "/session/1", 3)),
        failed,
        "one socket per handle"
    );

    let closing = std::thread::spawn(move || {
        let handle = ObjectPath::try_from("/session/1").expect("a path");
        let reply = zbus::block_on(backend.call_method(
            Some(NAME),
            PATH,
            Some("os.athanor.Eis1"),
            "CloseSession",
            &(handle,),
        ));
        (refused(reply), backend)
    });
    let Ok(Update::Close { handle, done }) = updates.recv_timeout(WAIT) else {
        panic!("the loop is asked to close the session");
    };
    assert_eq!(handle, "/session/1");
    done.give(Ok(()));
    let (closed, backend) = closing.join().expect("the closing call");
    assert_eq!(closed, None, "answered once the loop closed it");
    assert!(
        connect_to_eis(&backend, "/session/1", 1).is_ok(),
        "the handle is free again"
    );
    assert!(matches!(
        updates.recv_timeout(WAIT),
        Ok(Update::Open { .. })
    ));
    zbus::block_on(backend.release_name(BACKEND)).expect("the backend lets its name go");
    assert!(
        matches!(updates.recv_timeout(WAIT), Ok(Update::CloseAll)),
        "the backend's name changing owner ends its contexts"
    );

    let second = serve(bus.builder(), &bus.connect(), &BACKEND_SYSTEMD, &|_| Ok(()));
    assert!(second.is_err(), "a second server, the name taken, stops");
    drop(bus);
    let ended = server.join().expect("the interface's thread");
    assert!(
        ended.is_err(),
        "the bus gone, the interface stops: {ended:?}"
    );
}
