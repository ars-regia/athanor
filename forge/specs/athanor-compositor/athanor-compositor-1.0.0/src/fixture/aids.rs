//! The fixture's tests of the keyboard aids (doc_accessibility.md AX11 and AX17, #865): sticky,
//! slow and bounce keys as a client, a binding, the lock surface and a virtual keyboard see
//! them, and what the aids tell (`aids.rs`). The aids are set as `athanor_a11y_v1`'s requests
//! set them; what they tell is read from `Aids::sent`.

use super::{
    chord, focused_window, key, lock_session, sent_to, Local, Running, VirtualKeyboard,
    FIXTURE_ALLOW_LIST,
};
use crate::aids::{Aid, InvalidDelay, Notice, SHIFT};
use crate::bindings::Action;
use crate::identity::AllowList;
use smithay::reexports::wayland_server::Resource;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_seat;
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;

const KEY_ESC: u32 = 1;
const KEY_Q: u32 = 16;
const KEY_A: u32 = 30;
const KEY_S: u32 = 31;
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_LEFTALT: u32 = 56;
const KEY_F1: u32 = 59;
const KEY_LEFTMETA: u32 = 125;

/// Shift in xkb's masks.
const XKB_SHIFT: u32 = 1;

/// What the aids told since the last read.
fn told(run: &mut Running) -> Vec<Notice> {
    std::mem::take(&mut run.state.aids.sent)
}

/// Presses the key of evdev code `code` and releases it.
fn tap(run: &mut Running, code: u32) {
    key(&mut run.state, code, true);
    key(&mut run.state, code, false);
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
    chord(&mut run.state, &[KEY_LEFTCTRL, KEY_A]);
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
    assert_eq!(run.state.aid_modifiers(), None, "nor to a client that binds now");

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

/// A virtual keyboard's keys go through the aids as the hardware's do.
#[test]
fn ax11_a_virtual_keyboard_goes_through_the_aids() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let mut typist = Local::connect(&mut run, None);
    let keyboard = VirtualKeyboard::new(&mut typist, &mut run, None);
    run.state.set_slow_keys(true, 200).expect("in bounds");
    keyboard.chord(&mut typist, &mut run, &[KEY_A]);
    window.roundtrip(&mut run);
    assert_eq!(window.client.keys, [], "a short press types nothing");
    assert_eq!(told(&mut run), [Notice::Rejected(Aid::Slow)]);
    keyboard.key(&mut typist, &mut run, KEY_A, true);
    wait(&mut run, 250);
    window.roundtrip(&mut run);
    assert_eq!(
        window.client.keys,
        [KEY_A],
        "held for the delay, it is pressed"
    );
    keyboard.key(&mut typist, &mut run, KEY_A, false);
    window.roundtrip(&mut run);
    assert_eq!(window.client.released, [KEY_A]);
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
