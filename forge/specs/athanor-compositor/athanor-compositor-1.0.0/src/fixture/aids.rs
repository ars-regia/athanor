//! The fixture's tests of the keyboard aids (doc_accessibility.md AX11 and AX17, #865): sticky,
//! slow and bounce keys as a client, a binding, the lock surface and a virtual keyboard see
//! them, and what the aids tell (`aids.rs`). The aids are set as `athanor_a11y_v1`'s requests
//! set them; what they tell is read from `Aids::sent`.

use super::devices::pen_over_window;
use super::{
    chord, focused_window, key, lock_session, sent_to, write_xkb, xkb_file, Local, Running,
    VirtualKeyboard, FIXTURE_ALLOW_LIST, HEIGHT, SWAPPED_KEYMAP, WIDTH,
};
use crate::aids::{Aid, InvalidDelay, Notice, SHIFT};
use crate::bindings::Action;
use crate::identity::AllowList;
use crate::session_calls::LAUNCHER;
use smithay::backend::input::{ButtonState, InputTime, TabletToolTipState};
use smithay::reexports::wayland_server::Resource;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::protocol::{wl_compositor, wl_seat, wl_shm};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use wayland_protocols::xdg::shell::client::xdg_wm_base;

const KEY_ESC: u32 = 1;
const KEY_Q: u32 = 16;
const KEY_A: u32 = 30;
const KEY_S: u32 = 31;
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTALT: u32 = 56;
const KEY_TAB: u32 = 15;
const BTN_LEFT: u32 = 0x110;
const KEY_F1: u32 = 59;
const KEY_LEFTMETA: u32 = 125;

/// Shift, Control and Mod3 in xkb's masks.
const XKB_SHIFT: u32 = 1;
const XKB_CTRL: u32 = 4;
const XKB_MOD3: u32 = 0x20;

/// What the aids told since the last read.
fn told(run: &mut Running) -> Vec<Notice> {
    std::mem::take(&mut run.state.aids.sent)
}

/// Presses the key of evdev code `code` and releases it.
fn tap(run: &mut Running, code: u32) {
    key(&mut run.state, code, true);
    key(&mut run.state, code, false);
}

/// Whether the seat's keyboard holds no key pressed.
fn no_key_pressed(run: &Running) -> bool {
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    keyboard.pressed_keys().is_empty()
}

/// Runs the loop for `ms` milliseconds, so that the aids' timers fire.
fn wait(run: &mut Running, ms: u64) {
    let started = Instant::now();
    run.until("the delay", |_| {
        started.elapsed() >= Duration::from_millis(ms)
    });
}

fn modifiers(latched: u32, locked: u32) -> Notice {
    Notice::Modifiers { latched, locked }
}

/// Shift alone latches and applies to the next key only; twice it locks until pressed again;
/// held with another key it latches nothing; sticky keys turned off let go of the latch.
#[test]
fn ax11_sticky_keys_latch_lock_and_let_go_a_modifier() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_sticky_keys(true, false);

    tap(&mut run, KEY_LEFTSHIFT);
    window.roundtrip(&mut run);
    assert_eq!(told(&mut run), [modifiers(SHIFT, 0)], "Shift alone latches");
    assert_eq!(
        window.client.modifier_latches.last(),
        Some(&XKB_SHIFT),
        "the client is sent the latch before the next key"
    );
    key(&mut run.state, KEY_A, true);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys.last(), Some(&KEY_A));
    assert_eq!(
        window.client.modifier_latches.last(),
        Some(&0),
        "the latch goes once the key is read with it"
    );
    key(&mut run.state, KEY_A, false);
    assert_eq!(told(&mut run), [modifiers(0, 0)]);
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [KEY_A]);
    assert_eq!(told(&mut run), [], "the next key has no Shift");

    tap(&mut run, KEY_LEFTSHIFT);
    tap(&mut run, KEY_LEFTSHIFT);
    window.roundtrip(&mut run);
    assert_eq!(
        told(&mut run),
        [modifiers(SHIFT, 0), modifiers(0, SHIFT)],
        "twice, it locks"
    );
    assert_eq!(window.client.modifier_locks.last(), Some(&XKB_SHIFT));
    sent_to(&mut window, &mut run, &[KEY_A, KEY_S]);
    assert_eq!(told(&mut run), [], "locked over several keys");
    assert_eq!(window.client.modifier_locks.last(), Some(&XKB_SHIFT));
    tap(&mut run, KEY_LEFTSHIFT);
    window.roundtrip(&mut run);
    assert_eq!(
        told(&mut run),
        [modifiers(0, 0)],
        "a third time, it lets go"
    );
    assert_eq!(window.client.modifier_locks.last(), Some(&0));

    chord(&mut run.state, &[KEY_LEFTSHIFT, KEY_A]);
    assert_eq!(told(&mut run), [], "held with a key, Shift latches nothing");

    tap(&mut run, KEY_LEFTCTRL);
    assert_eq!(
        told(&mut run),
        [modifiers(2, 0)],
        "Ctrl latches as Shift does"
    );
    run.state.set_sticky_keys(false, false);
    window.roundtrip(&mut run);
    assert_eq!(told(&mut run), [modifiers(0, 0)], "off, the latch goes");
    assert_eq!(window.client.modifier_latches.last(), Some(&0));
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(told(&mut run), [], "and nothing latches");
    run.stop();
}

/// With two-key-off, a key pressed while a modifier is held turns sticky keys off, lets go of
/// the latch and is told as the aid turned off; without it, the chord changes nothing.
#[test]
fn ax11_two_key_off_turns_sticky_keys_off() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_sticky_keys(true, false);
    chord(&mut run.state, &[KEY_LEFTCTRL, KEY_A]);
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(
        told(&mut run),
        [modifiers(SHIFT, 0)],
        "without two-key-off, a chord leaves sticky keys on"
    );

    run.state.set_sticky_keys(true, true);
    chord(&mut run.state, &[KEY_LEFTSHIFT, KEY_A]);
    window.roundtrip(&mut run);
    assert_eq!(
        told(&mut run),
        [modifiers(0, 0), Notice::AidOff(Aid::Sticky)],
        "two keys together turn sticky keys off"
    );
    assert_eq!(window.client.modifier_latches.last(), Some(&0));
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(told(&mut run), [], "and nothing latches after");
    run.stop();
}

/// Slow keys: a key released before the delay never reaches the client, and is told as
/// rejected; held for the delay it is pressed and released; a delay of 0 holds nothing back.
#[test]
fn ax11_slow_keys_reject_a_key_released_before_the_delay() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 200).expect("in bounds");

    let (pressed, released) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(
        (pressed, released),
        (vec![], vec![]),
        "a short press types nothing"
    );
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Slow)]);

    key(&mut run.state, KEY_A, true);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys.last(), None, "not before the delay");
    wait(&mut run, 250);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys,
        [KEY_A],
        "held for the delay, it is pressed"
    );
    key(&mut run.state, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(window.client.released, [KEY_A]);
    assert_eq!(told(&mut run), []);

    run.state.set_slow_keys(true, 0).expect("in bounds");
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [KEY_A], "a delay of 0 holds nothing back");
    run.stop();
}

/// Bounce keys: a press within the delay of the same key's release never reaches the client,
/// and is told as rejected; another key, or the same key after the delay, passes.
#[test]
fn ax11_bounce_keys_reject_a_press_within_the_delay() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_bounce_keys(true, 300).expect("in bounds");

    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [KEY_A], "the first press passes");
    let (pressed, released) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(
        (pressed, released),
        (vec![], vec![]),
        "a bounce types nothing"
    );
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Bounce)]);
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_S]);
    assert_eq!(pressed, [KEY_S], "another key passes");
    wait(&mut run, 350);
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [KEY_A], "after the delay, the key passes");
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// Sticky keys take the launcher's key as well: Super tapped, then A, shows it once, which is
/// the way to the launcher while Super alone latches (AX11).
#[test]
fn ax11_super_then_a_shows_the_launcher() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTMETA);
    window.roundtrip(&mut run);
    assert_eq!(run.state.fired, [], "Super tapped latches, no launcher");
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(
        run.state.fired,
        [Action::Call(&LAUNCHER)],
        "the latched Super takes A, once"
    );
    assert_eq!(pressed, [], "which the client is not sent");
    run.stop();
}

/// A binding sees the keys the aids let through, as the client does: a latched Super takes
/// Super+Q, and Super tapped does not show the launcher; under slow keys a short Super+Q takes
/// nothing, a held one its action.
#[test]
fn ax11_a_binding_sees_what_the_client_sees() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTMETA);
    window.roundtrip(&mut run);
    assert_eq!(run.state.fired, [], "Super tapped latches, no launcher");
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_Q]);
    assert_eq!(
        run.state.fired,
        [Action::CloseFocused],
        "the latched Super takes Q"
    );
    assert_eq!(pressed, [], "which the client is not sent");
    run.state.set_sticky_keys(false, false);
    run.state.fired.clear();

    run.state.set_slow_keys(true, 200).expect("in bounds");
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_LEFTMETA, KEY_Q]);
    assert_eq!(
        (pressed, &run.state.fired),
        (vec![], &vec![]),
        "a short chord"
    );
    key(&mut run.state, KEY_LEFTMETA, true);
    wait(&mut run, 250);
    key(&mut run.state, KEY_Q, true);
    wait(&mut run, 250);
    assert_eq!(run.state.fired, [Action::CloseFocused], "a held one");
    key(&mut run.state, KEY_Q, false);
    key(&mut run.state, KEY_LEFTMETA, false);
    run.stop();
}

/// The Shift gestures are told only while they are on: Shift five times in a row offers sticky
/// keys, another key between starts the count again, and Shift held for eight seconds offers
/// slow keys.
#[test]
fn ax11_the_shift_gestures_are_told_only_while_on() {
    let mut run = Running::start(None);
    // An aid on, one that filters nothing, so that the keys take the aids' path.
    run.state.set_bounce_keys(true, 0).expect("in bounds");
    for _ in 0..5 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    key(&mut run.state, KEY_LEFTSHIFT, true);
    wait(&mut run, 8100);
    key(&mut run.state, KEY_LEFTSHIFT, false);
    assert_eq!(
        told(&mut run),
        [],
        "with the gestures off, nothing is offered"
    );

    run.state.set_aid_gestures(true);
    for _ in 0..4 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    tap(&mut run, KEY_A);
    for _ in 0..4 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    assert_eq!(told(&mut run), [], "another key starts the count again");
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(told(&mut run), [Notice::Gesture(Aid::Sticky)]);
    key(&mut run.state, KEY_LEFTSHIFT, true);
    wait(&mut run, 8100);
    key(&mut run.state, KEY_LEFTSHIFT, false);
    assert_eq!(told(&mut run), [Notice::Gesture(Aid::Slow)]);
    run.stop();
}

/// A gesture offers an aid that is off: five Shift presses to latch and lock offer nothing with
/// sticky keys on, and neither does Shift held once slow keys are on, read when the eight
/// seconds end. Each aid leaves the other's gesture alone.
#[test]
fn ax11_a_gesture_offers_only_an_aid_that_is_off() {
    let offered = |notices: Vec<Notice>| -> Vec<Notice> {
        let gesture = |notice: &Notice| matches!(notice, Notice::Gesture(_));
        notices.into_iter().filter(gesture).collect()
    };
    let mut run = Running::start(None);
    run.state.set_aid_gestures(true);
    run.state.set_sticky_keys(true, false);
    for _ in 0..5 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    assert_eq!(offered(told(&mut run)), [], "sticky keys are on");

    key(&mut run.state, KEY_LEFTSHIFT, true);
    run.state.set_slow_keys(true, 100).expect("in bounds");
    wait(&mut run, 8100);
    key(&mut run.state, KEY_LEFTSHIFT, false);
    assert_eq!(offered(told(&mut run)), [], "slow keys came on meanwhile");

    run.state.set_sticky_keys(false, false);
    for _ in 0..5 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    assert_eq!(
        offered(told(&mut run)),
        [Notice::Gesture(Aid::Sticky)],
        "slow keys on leave sticky keys' offer"
    );
    run.stop();
}

/// A delay above 2000 ms is refused, the protocol's `invalid_value`, and changes nothing.
#[test]
fn ax11_a_delay_above_2000_ms_is_refused() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    assert_eq!(run.state.set_slow_keys(true, 2001), Err(InvalidDelay(2001)));
    assert_eq!(
        run.state.set_bounce_keys(true, 2001),
        Err(InvalidDelay(2001))
    );
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [KEY_A], "slow keys stay off");
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [KEY_A], "bounce keys stay off");
    assert_eq!(run.state.set_slow_keys(true, 2000), Ok(()));
    assert_eq!(run.state.set_bounce_keys(true, 2000), Ok(()));
    run.stop();
}

/// While locked the aids filter the lock surface's keys, the protected keys pass slow and
/// bounce keys at 2000 ms, and no modifiers, rejected-key or gesture event is told; the
/// modifiers are told once the session is unlocked.
#[test]
fn ax11_the_aids_work_on_the_lock_surface_and_tell_nothing() {
    let mut run = Running::start(None);
    let (mut locker, lock) = lock_session(&mut run);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    run.state.set_bounce_keys(true, 2000).expect("in bounds");
    run.state.set_sticky_keys(true, false);
    run.state.set_aid_gestures(true);

    let (pressed, _) = sent_to(&mut locker, &mut run, &[KEY_A]);
    assert_eq!(
        pressed,
        [],
        "a short press types nothing on the lock surface"
    );
    for _ in 0..2 {
        let (pressed, _) = sent_to(&mut locker, &mut run, &[KEY_LEFTMETA, KEY_ESC]);
        assert_eq!(
            pressed,
            [KEY_LEFTMETA, KEY_ESC],
            "Super+Escape passes slow and bounce keys at once"
        );
        let fired = run.state.fired.len();
        chord(&mut run.state, &[KEY_LEFTCTRL, KEY_LEFTALT, KEY_F1]);
        assert_eq!(
            run.state.fired[fired..],
            [Action::Vt(1)],
            "the switch to a text console passes too"
        );
        locker.roundtrip(&mut run);
    }

    run.state.set_slow_keys(false, 0).expect("in bounds");
    run.state.set_bounce_keys(false, 0).expect("in bounds");
    // Latched, locked, let go, latched, locked: the fifth Shift is also a gesture.
    for _ in 0..5 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    locker.roundtrip(&mut run);
    assert_eq!(
        locker.client.modifier_locks.last(),
        Some(&XKB_SHIFT),
        "sticky keys work on the lock surface"
    );
    assert_eq!(told(&mut run), [], "and nothing is told while locked");
    assert_eq!(
        run.state.aid_modifiers(),
        None,
        "nor to a client that binds now"
    );

    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    assert_eq!(
        told(&mut run),
        [modifiers(0, SHIFT)],
        "the modifiers are told after the unlock"
    );
    assert_eq!(run.state.aid_modifiers(), Some(modifiers(0, SHIFT)));
    run.stop();
}

/// Sticky keys turned off twice during one lock are told once, at the unlock.
#[test]
fn ax11_an_aid_turned_off_twice_while_locked_is_told_once() {
    let mut run = Running::start(None);
    let (mut locker, lock) = lock_session(&mut run);
    for _ in 0..2 {
        run.state.set_sticky_keys(true, true);
        chord(&mut run.state, &[KEY_LEFTCTRL, KEY_A]);
    }
    assert_eq!(told(&mut run), [], "nothing is told while locked");
    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    assert_eq!(
        told(&mut run),
        [modifiers(0, 0), Notice::AidOff(Aid::Sticky)]
    );
    run.stop();
}

/// Under a keyboard-shortcuts inhibitor the aids work as without one.
#[test]
fn ax11_the_aids_work_under_a_shortcuts_inhibitor() {
    let mut run = Running::start(None);
    let mut window = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = window.bind(1);
    let _keyboard = seat.get_keyboard(&window.queue.handle(), ());
    window.toplevel(&mut run);
    let (surface, _) = window.window.clone().expect("a window");
    window.pump(&mut run, "the window has the keyboard", |client, state| {
        let focus = state.seat.get_keyboard().and_then(|k| k.current_focus());
        client.keymaps > 0 && focus.is_some()
    });
    let inhibit: ZwpKeyboardShortcutsInhibitManagerV1 = window.bind(1);
    let log = Arc::new(Mutex::new(Vec::new()));
    let _inhibitor =
        inhibit.inhibit_shortcuts(&surface, &seat, &window.queue.handle(), log.clone());
    window.pump(&mut run, "the inhibitor is active", move |_, _| {
        *log.lock().expect("the log") == ["active"]
    });

    run.state.set_slow_keys(true, 200).expect("in bounds");
    let (pressed, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(pressed, [], "slow keys hold a short press back");
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Slow)]);
    run.state.set_slow_keys(false, 0).expect("in bounds");
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTSHIFT);
    window.roundtrip(&mut run);
    assert_eq!(told(&mut run), [modifiers(SHIFT, 0)], "sticky keys latch");
    assert_eq!(window.client.modifier_latches.last(), Some(&XKB_SHIFT));
    run.stop();
}

/// Slow and bounce keys read the hardware keyboard only, and sticky keys a virtual keyboard's
/// keys too: its key repeated at once is neither held back nor dropped, its Shift tapped latches,
/// and the hardware's key is still rejected beside it.
#[test]
fn ax11_a_virtual_keyboard_goes_through_sticky_keys_alone() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    run.state.set_bounce_keys(true, 2000).expect("in bounds");
    run.state.set_sticky_keys(true, false);
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A, KEY_A], "no aid filters them");
    assert_eq!(told(&mut run), [], "and none is told");

    keyboard.modifiers(&mut typist, &mut run, 0);
    keyboard.chord(&mut typist, &mut run, &[KEY_LEFTSHIFT]);
    assert_eq!(told(&mut run), [modifiers(SHIFT, 0)], "sticky keys latch");
    window.roundtrip(&mut run);
    assert_eq!(window.client.modifier_latches.last(), Some(&XKB_SHIFT));

    tap(&mut run, KEY_Q);
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Slow)]);
    run.stop();
}

/// While a client the allow-list marks as a trusted prompt has the keyboard, nothing is told;
/// once it loses it, the modifiers and the aid turned off meanwhile are.
#[test]
fn ax11_a_trusted_prompt_is_told_nothing() {
    let mut run = Running::start(None);
    // The control center's line, marked: the fixture connects it under its identity.
    let marked = FIXTURE_ALLOW_LIST.replace(
        "athanor_toplevel_manager_v1\n",
        "athanor_toplevel_manager_v1 trusted-prompt\n",
    );
    run.state.allow_list = AllowList::parse(&marked).expect("allow-list");
    let mut prompt = Local::control_center(&mut run);
    let seat: wl_seat::WlSeat = prompt.bind(1);
    let _keyboard = seat.get_keyboard(&prompt.queue.handle(), ());
    prompt.toplevel(&mut run);
    let id = prompt.id.clone().expect("a client the fixture inserted");
    prompt.pump(&mut run, "the prompt has the keyboard", |_, state| {
        let focus = state.seat.get_keyboard().and_then(|k| k.current_focus());
        focus.and_then(|surface| surface.client()).map(|c| c.id()) == Some(id.clone())
    });
    run.state.set_sticky_keys(true, true);
    run.state.set_slow_keys(true, 200).expect("in bounds");
    sent_to(&mut prompt, &mut run, &[KEY_A]);
    run.state.set_slow_keys(false, 0).expect("in bounds");
    tap(&mut run, KEY_LEFTSHIFT);
    chord(&mut run.state, &[KEY_LEFTCTRL, KEY_A]);
    assert_eq!(told(&mut run), [], "nothing is told to the prompt's keys");

    *run.identity.lock().expect("identity") = super::BAR;
    *run.executable_file.lock().expect("file") = super::BAR_FILE;
    let _window = focused_window(&mut run);
    run.until("the quiet ends", |state| !state.aids.sent.is_empty());
    assert_eq!(
        told(&mut run),
        [modifiers(0, 0), Notice::AidOff(Aid::Sticky)],
        "the prompt left, what changed is told"
    );
    run.stop();
}

/// Turning the last aid off leaves no press to arrive with no release: a key released within
/// the delay stays rejected, and a key still held comes through, with its release.
#[test]
fn ax11_a_press_held_back_outlives_the_last_aid_turned_off() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 200).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    run.state.set_slow_keys(false, 0).expect("in bounds");
    key(&mut run.state, KEY_A, false);
    wait(&mut run, 250);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys,
        [],
        "released within the delay, it is rejected"
    );
    assert!(no_key_pressed(&run), "and no press is left with no release");
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Slow)]);

    run.state.set_slow_keys(true, 200).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    run.state.set_slow_keys(false, 0).expect("in bounds");
    wait(&mut run, 250);
    key(&mut run.state, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A], "held, it comes through");
    assert_eq!(window.client.released, [KEY_A], "with its release");
    assert!(no_key_pressed(&run));
    run.stop();
}

/// A modifier bounce keys rejected is let go with its release, though the aid went off
/// meanwhile: it does not count as held when a key is judged protected later.
#[test]
fn ax11_a_rejected_modifier_does_not_stay_held_after_the_aid_is_off() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_bounce_keys(true, 2000).expect("in bounds");
    tap(&mut run, KEY_LEFTCTRL);
    key(&mut run.state, KEY_LEFTCTRL, true);
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Bounce)]);
    run.state.set_bounce_keys(false, 0).expect("in bounds");
    key(&mut run.state, KEY_LEFTCTRL, false);

    run.state.set_slow_keys(true, 2000).expect("in bounds");
    chord(&mut run.state, &[KEY_LEFTALT, KEY_F1]);
    assert_eq!(run.state.fired, [], "Alt+F1 is no switch to a text console");
    run.stop();
}

/// A press held back when another client takes the keyboard reaches neither client, and its
/// release is swallowed.
#[test]
fn ax11_a_focus_change_drops_a_press_held_back() {
    let mut run = Running::start(None);
    let (mut first, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 1000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    let (mut second, _) = focused_window(&mut run);
    wait(&mut run, 1100);
    key(&mut run.state, KEY_A, false);
    first.roundtrip(&mut run);
    second.roundtrip(&mut run);
    assert_eq!(first.client.keys, [], "the first window is sent nothing");
    assert_eq!(second.client.keys, [], "and the second no press");
    assert_eq!(second.client.released, [], "and no release");
    assert!(no_key_pressed(&run));
    run.stop();
}

/// The same for another surface of the same client: the focus is a surface's, not a client's.
#[test]
fn ax11_a_focus_change_within_a_client_drops_a_press_held_back() {
    let mut run = Running::start(None);
    let (mut local, first) = focused_window(&mut run);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    run.state.set_slow_keys(true, 1000).expect("in bounds");
    key(&mut run.state, KEY_A, true);

    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    local.client.configure = None;
    surface.commit();
    local.pump(&mut run, "the second window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let held = first.toplevel().expect("a toplevel").wl_surface().clone();
    local.pump(&mut run, "the second window has the keyboard", |_, state| {
        let keyboard = state.seat.get_keyboard().expect("a keyboard");
        keyboard.current_focus().is_some_and(|focus| focus != held)
    });

    wait(&mut run, 1100);
    key(&mut run.state, KEY_A, false);
    local.roundtrip(&mut run);
    assert_eq!(local.client.keys, [], "neither surface is sent the press");
    assert!(no_key_pressed(&run));
    run.stop();
}

/// The same when the keyboard goes to no surface: the press would otherwise be held on the
/// seat, and the next surface would receive it among the keys of its enter event.
#[test]
fn ax11_a_loss_of_focus_drops_a_press_held_back() {
    let mut run = Running::start(None);
    let (mut local, window) = focused_window(&mut run);
    run.state.set_slow_keys(true, 200).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    run.state.set_keyboard_focus(None);
    wait(&mut run, 300);
    assert!(no_key_pressed(&run), "the delay ended on no surface");
    run.state.focus(&window);
    key(&mut run.state, KEY_A, false);
    local.roundtrip(&mut run);
    assert_eq!(local.client.keys, [], "no press");
    assert_eq!(local.client.released, [], "and no release");
    run.stop();
}

/// A press held back when the session locks is dropped at the lock, before a lock surface
/// takes the keyboard.
#[test]
fn ax11_a_lock_drops_a_press_held_back() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 1000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    let mut locker = Local::connect(&mut run, None);
    let locks: ExtSessionLockManagerV1 = locker.bind(1);
    let _lock = locks.lock(&locker.queue.handle(), ());
    locker.pump(&mut run, "the session is locked", |_, state| {
        state.lock.is_some()
    });
    wait(&mut run, 1100);
    assert!(no_key_pressed(&run), "no press lands under the lock");
    key(&mut run.state, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [], "and the window is sent nothing");
    assert_eq!(window.client.released, []);
    run.stop();
}

/// A press held back on the lock surface when the session unlocks does not reach the window
/// the unlock gives the keyboard to: a password's last character stays at the lock.
#[test]
fn ax11_an_unlock_keeps_a_password_key_from_the_window_underneath() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let (mut locker, lock) = lock_session(&mut run);
    run.state.set_slow_keys(true, 1000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    wait(&mut run, 1100);
    key(&mut run.state, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [], "the window is sent no press");
    assert_eq!(window.client.released, [], "and no release");
    run.stop();
}

/// An unlock with no window to go to drops a press held back too: nothing gives the keyboard
/// a new client, and the press would stay on it until a window opens.
#[test]
fn ax11_an_unlock_drops_a_press_held_back() {
    let mut run = Running::start(None);
    let (mut locker, lock) = lock_session(&mut run);
    run.state.set_slow_keys(true, 1000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    wait(&mut run, 1100);
    assert!(no_key_pressed(&run));
    key(&mut run.state, KEY_A, false);
    run.stop();
}

/// With two-key-off a chord being built turns nothing off, and neither does a protected
/// combination: Ctrl+Alt+Tab works and sticky keys stay on.
#[test]
fn ax11_two_key_off_spares_a_protected_combination() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_sticky_keys(true, true);
    chord(&mut run.state, &[KEY_LEFTCTRL, KEY_LEFTALT, KEY_TAB]);
    assert_eq!(
        told(&mut run),
        [],
        "neither Alt nor Tab turns sticky keys off"
    );
    assert_eq!(
        run.state.fired,
        [Action::FocusCycle],
        "and the key still works"
    );
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(told(&mut run), [modifiers(SHIFT, 0)], "they are still on");
    run.stop();
}

/// A virtual keyboard's key is no part of a chord of the hardware's: it neither turns sticky
/// keys off nor keeps the modifier held on the hardware from latching.
#[test]
fn ax11_two_key_off_reads_one_keyboard_at_a_time() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_sticky_keys(true, true);
    key(&mut run.state, KEY_LEFTCTRL, true);
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    assert_eq!(told(&mut run), [], "the aid stays on");
    key(&mut run.state, KEY_LEFTCTRL, false);
    let notices = told(&mut run);
    assert!(
        matches!(notices[..], [Notice::Modifiers { latched, locked: 0 }] if latched != 0),
        "and Ctrl was tapped alone: {notices:?}"
    );
    run.stop();
}

/// The Shift gestures count the hardware keyboard: a virtual keyboard's Shift offers nothing,
/// and a virtual key does not break a run of the hardware's.
#[test]
fn ax11_the_shift_gestures_read_the_hardware_keyboard_only() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_aid_gestures(true);
    for _ in 0..5 {
        keyboard.chord(&mut typist, &mut run, &[KEY_LEFTSHIFT]);
    }
    assert_eq!(told(&mut run), [], "a client's five presses offer nothing");
    for _ in 0..4 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(told(&mut run), [Notice::Gesture(Aid::Sticky)]);
    run.stop();
}

/// A pointer button pressed starts the count of Shift presses again, as another key does: five
/// Shift+clicks offer nothing.
#[test]
fn ax11_a_click_starts_the_shift_count_again() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_aid_gestures(true);
    for _ in 0..5 {
        tap(&mut run, KEY_LEFTSHIFT);
        run.state
            .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
        run.state
            .button_input(BTN_LEFT, ButtonState::Released, InputTime::now());
    }
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// Another key stops the timing of Shift held: nothing is offered eight seconds on.
#[test]
fn ax11_another_key_stops_the_shift_hold() {
    let mut run = Running::start(None);
    run.state.set_aid_gestures(true);
    key(&mut run.state, KEY_LEFTSHIFT, true);
    key(&mut run.state, KEY_A, true);
    wait(&mut run, 8100);
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// A latched or locked modifier counts towards a protected key: Ctrl and Alt tapped, then F1,
/// switches to a text console at once with slow keys at their longest.
#[test]
fn ax11_a_protected_key_reads_the_latched_and_locked_modifiers() {
    for taps in [1, 2] {
        let mut run = Running::start(None);
        let _window = focused_window(&mut run);
        run.state.set_sticky_keys(true, false);
        for _ in 0..taps {
            tap(&mut run, KEY_LEFTCTRL);
            tap(&mut run, KEY_LEFTALT);
        }
        run.state.set_slow_keys(true, 2000).expect("in bounds");
        key(&mut run.state, KEY_F1, true);
        assert_eq!(run.state.fired, [Action::Vt(1)], "after {taps} taps");
        run.stop();
    }
}

/// A latch survives a virtual keyboard with modifiers and no keymap: the seat's modifiers
/// coming back after its key do not take a latch it set away.
#[test]
fn ax11_a_latch_of_a_virtual_keyboard_survives_its_key() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_sticky_keys(true, false);
    keyboard.modifiers(&mut typist, &mut run, 0);
    keyboard.chord(&mut typist, &mut run, &[KEY_LEFTSHIFT]);
    assert_eq!(told(&mut run), [modifiers(SHIFT, 0)]);
    window.roundtrip(&mut run);
    assert_eq!(window.client.modifier_latches.last(), Some(&XKB_SHIFT));
    run.stop();
}

/// A hardware latch a virtual keyboard's key with no keymap uses up does not come back when the
/// seat's modifiers do.
#[test]
fn ax11_a_latch_used_by_a_virtual_keyboard_does_not_come_back() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTSHIFT);
    keyboard.modifiers(&mut typist, &mut run, 0);
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    window.roundtrip(&mut run);
    assert_eq!(window.client.modifier_latches.last(), Some(&0));
    run.stop();
}

/// A hardware latch applies under a virtual keyboard's own keymap: S is Super there, a modifier,
/// which leaves the latch for the next key.
#[test]
fn ax11_a_latch_crosses_a_virtual_keymap() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, Some(SWAPPED_KEYMAP));
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTSHIFT);
    keyboard.key(&mut typist, &mut run, KEY_S, true);
    let seat = run.state.seat.get_keyboard().expect("a keyboard");
    assert_eq!(seat.modifier_state().serialized.latched, XKB_SHIFT);
    run.stop();
}

/// A latch not used under a virtual keyboard's own keymap is there when the seat's comes back.
#[test]
fn ax11_a_latch_comes_back_with_the_seat_keymap() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, Some(SWAPPED_KEYMAP));
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTSHIFT);
    keyboard.key(&mut typist, &mut run, KEY_S, true);
    keyboard.key(&mut typist, &mut run, KEY_S, false);
    key(&mut run.state, KEY_LEFTCTRL, true);
    let seat = run.state.seat.get_keyboard().expect("a keyboard");
    assert_eq!(
        seat.modifier_state().serialized.latched & XKB_SHIFT,
        XKB_SHIFT
    );
    run.stop();
}

/// A latch a key under a virtual keyboard's own keymap uses up does not come back with the
/// seat's keymap, before the next hardware key.
#[test]
fn ax11_a_latch_used_under_a_virtual_keymap_does_not_come_back() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, Some(SWAPPED_KEYMAP));
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTSHIFT);
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    tap(&mut run, KEY_A);
    window.roundtrip(&mut run);
    let seat = run.state.seat.get_keyboard().expect("a keyboard");
    assert_eq!(seat.modifier_state().serialized.latched, 0);
    assert_eq!(window.client.modifier_latches.last(), Some(&0));
    run.stop();
}

/// A chord on a virtual keyboard is a client's, and turns off nothing of the seat's.
#[test]
fn ax11_two_key_off_counts_the_hardware_keyboard_only() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_sticky_keys(true, true);
    keyboard.chord(&mut typist, &mut run, &[KEY_LEFTCTRL, KEY_A]);
    assert_eq!(told(&mut run), [], "the aid stays on");
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(
        told(&mut run),
        [modifiers(SHIFT, 0)],
        "and it still latches"
    );
    run.stop();
}

/// A protected key takes the modifiers still pending though the last aid went off: with slow
/// keys off, Ctrl+Alt held back and F1 switch to a text console.
#[test]
fn ax11_a_protected_key_lets_pending_modifiers_through_after_the_aid_is_off() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    key(&mut run.state, KEY_LEFTCTRL, true);
    key(&mut run.state, KEY_LEFTALT, true);
    run.state.set_slow_keys(false, 0).expect("in bounds");
    key(&mut run.state, KEY_F1, true);
    assert_eq!(run.state.fired, [Action::Vt(1)]);
    run.stop();
}

/// A protected key lets through only the modifiers still pending: Shift, dropped by a change of
/// focus, stays dropped when Ctrl+Alt+F1 switches to a text console.
#[test]
fn ax11_a_protected_key_leaves_a_dropped_modifier_dropped() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    key(&mut run.state, KEY_LEFTSHIFT, true);
    run.state.aids_drop_pending();
    key(&mut run.state, KEY_LEFTCTRL, true);
    key(&mut run.state, KEY_LEFTALT, true);
    key(&mut run.state, KEY_F1, true);
    window.roundtrip(&mut run);
    assert_eq!(run.state.fired, [Action::Vt(1)]);
    assert_eq!(window.client.keys, [KEY_LEFTCTRL, KEY_LEFTALT]);
    run.stop();
}

/// The same for a modifier bounce keys rejected: Shift's second press stays rejected.
#[test]
fn ax11_a_protected_key_leaves_a_bounced_modifier_rejected() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_bounce_keys(true, 2000).expect("in bounds");
    tap(&mut run, KEY_LEFTSHIFT);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    key(&mut run.state, KEY_LEFTSHIFT, true);
    key(&mut run.state, KEY_LEFTCTRL, true);
    key(&mut run.state, KEY_LEFTALT, true);
    key(&mut run.state, KEY_F1, true);
    window.roundtrip(&mut run);
    assert_eq!(run.state.fired, [Action::Vt(1)]);
    assert_eq!(
        window.client.keys,
        [KEY_LEFTSHIFT, KEY_LEFTCTRL, KEY_LEFTALT]
    );
    run.stop();
}

/// A modifier dropped counts for nothing: Alt pending and F1, with Ctrl dropped, are no switch
/// to a text console, and nothing is let through.
#[test]
fn ax11_a_dropped_modifier_does_not_make_a_key_protected() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    key(&mut run.state, KEY_LEFTCTRL, true);
    run.state.aids_drop_pending();
    key(&mut run.state, KEY_LEFTALT, true);
    key(&mut run.state, KEY_F1, true);
    window.roundtrip(&mut run);
    assert_eq!(run.state.fired, []);
    assert_eq!(window.client.keys, []);
    run.stop();
}

/// A tablet's tip pressed starts the count of Shift presses again, as a pointer button does.
#[test]
fn ax11_a_tip_down_starts_the_shift_count_again() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    run.state.set_aid_gestures(true);
    for _ in 0..5 {
        tap(&mut run, KEY_LEFTSHIFT);
        run.state
            .tool_tip(&tool, TabletToolTipState::Down, InputTime::now());
        run.state
            .tool_tip(&tool, TabletToolTipState::Up, InputTime::now());
    }
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// The release of a press slow keys rejected is no release bounce keys read: the key held next
/// passes slow keys, though bounce keys' delay would still be running.
#[test]
fn ax11_a_press_slow_keys_rejected_arms_no_bounce() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_bounce_keys(true, 2000).expect("in bounds");
    run.state.set_slow_keys(true, 200).expect("in bounds");
    tap(&mut run, KEY_A);
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Slow)]);
    key(&mut run.state, KEY_A, true);
    wait(&mut run, 250);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A]);
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// Neither does the release of a press dropped by a change of focus.
#[test]
fn ax11_a_dropped_press_arms_no_bounce() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_bounce_keys(true, 2000).expect("in bounds");
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    run.state.aids_drop_pending();
    key(&mut run.state, KEY_A, false);
    run.state.set_slow_keys(false, 0).expect("in bounds");
    tap(&mut run, KEY_A);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A]);
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// A bounced press's release restarts the delay, so a chattering key stays rejected.
#[test]
fn ax11_a_bounced_release_restarts_the_delay() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_bounce_keys(true, 500).expect("in bounds");
    tap(&mut run, KEY_A);
    key(&mut run.state, KEY_A, true);
    wait(&mut run, 400);
    key(&mut run.state, KEY_A, false);
    wait(&mut run, 200);
    key(&mut run.state, KEY_A, true);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A], "the third press is a bounce");
    assert_eq!(
        told(&mut run),
        [Notice::Rejected(Aid::Bounce), Notice::Rejected(Aid::Bounce)]
    );
    run.stop();
}

/// A second press of a key held back, which a second keyboard sends, starts no timer of its
/// own: the release cancels the one that runs, and a later press waits its whole delay.
#[test]
fn ax11_a_second_press_keeps_the_timer_of_the_first() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 2000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    key(&mut run.state, KEY_A, true);
    key(&mut run.state, KEY_A, false);
    wait(&mut run, 500);
    key(&mut run.state, KEY_A, true);
    wait(&mut run, 1600);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [], "the first timer was cancelled");
    wait(&mut run, 500);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A], "the delay ends the press");
    run.stop();
}

/// The keymap of `VirtualKeyboard::new` that puts Control_L on code 31, S, with Mod3.
const CONTROL_ON_MOD3: &str = r#"xkb_keymap {
    xkb_keycodes "mod3" { minimum = 8; maximum = 255; <ESCA> = 38; <LCTL> = 39; };
    xkb_types "mod3" { include "complete" };
    xkb_compatibility "mod3" { include "complete" };
    xkb_symbols "mod3" {
        key <ESCA> { [ Escape ] };
        key <LCTL> { symbols[Group1] = [ Control_L ], actions[Group1] = [ SetMods(modifiers=Mod3) ] };
        modifier_map Mod3 { <LCTL> };
    };
};"#;

/// The modifiers the seat has latched and locked.
fn seat_latched(run: &Running) -> (u32, u32) {
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    let mods = keyboard.modifier_state().serialized;
    (mods.latched, mods.locked)
}

/// A modifier latched on the hardware is set under a virtual keymap as that keymap maps it, and
/// as the seat's does when the seat's comes back.
#[test]
fn ax11_a_hardware_latch_follows_the_keymap() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, Some(CONTROL_ON_MOD3));
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTCTRL);
    assert_eq!(seat_latched(&run), (XKB_CTRL, 0));
    keyboard.key(&mut typist, &mut run, KEY_S, true);
    assert_eq!(
        seat_latched(&run),
        (XKB_MOD3, 0),
        "under the virtual keymap"
    );
    key(&mut run.state, KEY_LEFTALT, true);
    assert_eq!(seat_latched(&run), (XKB_CTRL, 0), "under the seat's again");
    run.stop();
}

/// The same for a modifier latched on the virtual keyboard: its bit is its keymap's, and the
/// hardware reads the seat's.
#[test]
fn ax11_a_virtual_latch_follows_the_keymap() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, Some(CONTROL_ON_MOD3));
    run.state.set_sticky_keys(true, false);
    keyboard.chord(&mut typist, &mut run, &[KEY_S]);
    assert_eq!(
        seat_latched(&run),
        (XKB_MOD3, 0),
        "under the virtual keymap"
    );
    key(&mut run.state, KEY_LEFTALT, true);
    assert_eq!(seat_latched(&run), (XKB_CTRL, 0), "under the seat's");
    run.stop();
}

/// A keymap from the configuration keeps what sticky keys hold, under its own masks.
#[test]
fn ax11_a_new_keymap_keeps_the_latched_and_locked_modifiers() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_sticky_keys(true, false);
    tap(&mut run, KEY_LEFTSHIFT);
    tap(&mut run, KEY_LEFTCTRL);
    tap(&mut run, KEY_LEFTCTRL);
    let file = xkb_file("aids");
    write_xkb(&file, "it");
    run.state.load_xkb_config(&file);
    assert_eq!(seat_latched(&run), (XKB_SHIFT, XKB_CTRL));
    run.stop();
}

/// A session paused drops a press held back, whose timer stops, and forgets it: the key's next
/// press waits its own delay and is typed.
#[test]
fn ax11_a_pause_forgets_a_press_held_back() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    run.state.set_slow_keys(true, 1000).expect("in bounds");
    key(&mut run.state, KEY_A, true);
    run.state.pause();
    wait(&mut run, 300);
    key(&mut run.state, KEY_A, true);
    wait(&mut run, 800);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [], "the first timer stopped");
    wait(&mut run, 400);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [KEY_A], "the second press was timed");
    run.stop();
}

/// A modifier held across a pause counts as held no more: the next key is no chord.
#[test]
fn ax11_a_pause_forgets_a_modifier_held() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_sticky_keys(true, true);
    key(&mut run.state, KEY_LEFTCTRL, true);
    run.state.pause();
    key(&mut run.state, KEY_A, true);
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// A pause ends a run of Shift presses.
#[test]
fn ax11_a_pause_starts_the_shift_count_again() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_aid_gestures(true);
    for _ in 0..4 {
        tap(&mut run, KEY_LEFTSHIFT);
    }
    run.state.pause();
    tap(&mut run, KEY_LEFTSHIFT);
    assert_eq!(told(&mut run), []);
    run.stop();
}

/// A pause stops the timing of Shift held: its release may never come here.
#[test]
fn ax11_a_pause_stops_the_shift_hold() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    run.state.set_aid_gestures(true);
    key(&mut run.state, KEY_LEFTSHIFT, true);
    run.state.pause();
    wait(&mut run, 8100);
    assert_eq!(told(&mut run), []);
    run.stop();
}
