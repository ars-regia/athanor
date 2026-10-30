//! The fixture's tests of Orca's keyboard monitor (doc_own_stack.md OS4, doc_accessibility.md
//! AX4; `keyboard_monitor.rs`). A call is admitted as the monitor's thread admits it, through
//! `check` with the bus's credentials of this test's own process, read as whatever executable
//! and cgroup the test sets; the signals are read from the queue the loop sends them to. The
//! last test serves the interface on a private bus, as the monitor's thread does.

use super::{
    focused_window, lock_session, sent_to, Local, Running, VirtualKeyboard,
};
use crate::bindings::Action;
use crate::identity::{pidfd_open, FileId, Process};
use crate::keyboard_monitor::{check, serve, Change, KeyEvent, Update, INTERFACE, NAME, PATH};
use smithay::backend::input::{InputTime, KeyState};
use smithay::input::keyboard::{KeyboardSource, Keysym};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_seat;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::connection::Builder;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::{MatchRule, Message};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use zbus::fdo::ConnectionCredentials;

const KEY_T: u32 = 20;
const KEY_LEFTALT: u32 = 56;
const KEY_LEFTMETA: u32 = 125;
const KEY_A: u32 = 30;
const KEY_C: u32 = 46;
const KEY_S: u32 = 31;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_CAPSLOCK: u32 = 58;
const KEY_INSERT: u32 = 110;
const KEY_ESC: u32 = 1;
const KEY_TAB: u32 = 15;
const KEY_LEFTCTRL: u32 = 29;
const KEY_F1: u32 = 59;

/// The Lock modifier of xkb's masks: Caps Lock's.
const LOCK: u32 = 1 << 1;
/// How long the bus test waits for an answer.
const WAIT: Duration = Duration::from_secs(10);

/// Orca's unique name on the bus, and another client's.
const ORCA: &str = ":1.42";
const OTHER: &str = ":1.43";

/// Orca's executable: `/usr/bin/orca` is a Python script, which the interpreter runs.
const PYTHON: &str = "/usr/bin/python3.14";
const PYTHON_FILE: FileId = FileId { dev: 1, ino: 9 };
const ORCA_CGROUP: &str =
    "0::/user.slice/user-1000.slice/user@1000.service/session.slice/orca.service\n";
const APP_CGROUP: &str =
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-terminal-1.scope\n";

/// The caller's process as the compositor reads it; every path of the compositor's file system
/// is the interpreter, so that only the check of `/usr` refuses a copy of it elsewhere.
struct Peer {
    executable: &'static str,
    cgroup: &'static str,
    file: FileId,
}

/// Orca, as `orca.service` runs it.
const ORCA_PROCESS: Peer = Peer {
    executable: PYTHON,
    cgroup: ORCA_CGROUP,
    file: PYTHON_FILE,
};

impl Process for Peer {
    fn executable(&self, _pid: i32) -> std::io::Result<PathBuf> {
        Ok(self.executable.into())
    }
    fn cgroup(&self, _pid: i32) -> std::io::Result<String> {
        Ok(self.cgroup.into())
    }
    fn executable_file(&self, _pid: i32) -> std::io::Result<FileId> {
        Ok(self.file)
    }
    fn file(&self, _path: &Path) -> std::io::Result<FileId> {
        Ok(PYTHON_FILE)
    }
}

/// The credentials a bus with `ProcessFD` returns for this process.
fn credentials() -> ConnectionCredentials {
    let pidfd = pidfd_open(std::process::id()).expect("a pidfd of this process");
    ConnectionCredentials::default().set_process_fd(pidfd.into())
}

/// `client` calls the monitor while `owner` owns Orca's name: admitted, its change reaches the
/// loop, as the monitor's thread sends it. Whether it was admitted.
fn call(run: &mut Running, client: &str, owner: &str, peer: &Peer, change: Change) -> bool {
    let admitted = check(client, owner, &credentials(), peer).is_ok();
    if admitted {
        let client = client.to_owned();
        run.state.keyboard_monitor.apply(Update { client, change });
    }
    admitted
}

/// The queue the monitor's signals go to.
fn listen(run: &mut Running) -> Receiver<KeyEvent> {
    let (events, queued) = sync_channel(64);
    run.state.keyboard_monitor.events = Some(events);
    queued
}

/// The keys sent since the last read, as (released, keysym), all to Orca.
fn heard(queued: &Receiver<KeyEvent>) -> Vec<(bool, Keysym)> {
    queued
        .try_iter()
        .map(|event| {
            assert_eq!(event.destination, ORCA, "a key is sent to its client alone");
            (event.released, Keysym::from(event.keysym))
        })
        .collect()
}

/// Pressed and released, in order.
fn pressed(keysym: Keysym) -> [(bool, Keysym); 2] {
    [(false, keysym), (true, keysym)]
}

/// Super and Alt, pressed and released around a key Orca is not sent.
const SUPER_ALT: [(bool, Keysym); 4] = [
    (false, Keysym::Super_L),
    (false, Keysym::Alt_L),
    (true, Keysym::Alt_L),
    (true, Keysym::Super_L),
];

/// One key, pressed or let go, from the hardware keyboard.
fn key(run: &mut Running, code: u32, key_state: KeyState) {
    let code = (code + 8).into();
    let time = InputTime::now();
    run.state
        .key_input(code, key_state, time, KeyboardSource::Physical);
}

/// The window's keyboard is the compositor's for Super+Alt+T, the reveal key, whatever Orca
/// grabs: Orca is sent neither its press nor its release, and the key is revealed.
fn reveal_is_protected(window: &mut Local, run: &mut Running, queued: &Receiver<KeyEvent>) {
    let reveal = [KEY_LEFTMETA, KEY_LEFTALT, KEY_T];
    let t = Keysym::t.raw();
    let super_alt = 0x40 | 0x08;
    let grabs = [
        ("the whole keyboard", Change::Grab(true), SUPER_ALT.to_vec()),
        ("Super+Alt+T", Change::KeyGrabs(vec![], vec![(t, super_alt)]), vec![]),
    ];
    for (what, grab, expected) in grabs {
        assert!(call(run, ORCA, ORCA, &ORCA_PROCESS, grab), "{what}");
        let fired = run.state.fired.len();
        sent_to(window, run, &reveal);
        assert_eq!(
            run.state.fired[fired..],
            [Action::Reveal],
            "{what}: Orca's grab does not take the reveal key"
        );
        assert_eq!(heard(queued), expected, "{what}: Orca is not sent T");
        for ungrab in [Change::Grab(false), Change::KeyGrabs(vec![], vec![])] {
            assert!(call(run, ORCA, ORCA, &ORCA_PROCESS, ungrab));
        }
    }
}

/// An admitted watcher is sent every key, a bound one too, and the window is still sent the keys
/// no binding and no grab takes.
#[test]
fn ax4_a_watcher_receives_every_key_and_the_window_still_gets_it() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let queued = listen(&mut run);
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Watch(true)
    ));

    let (presses, releases) = sent_to(&mut window, &mut run, &[KEY_LEFTSHIFT, KEY_A]);
    assert_eq!(
        presses,
        [KEY_LEFTSHIFT, KEY_A],
        "the window is sent the keys"
    );
    assert_eq!(releases, [KEY_A, KEY_LEFTSHIFT]);
    assert_eq!(
        heard(&queued),
        [
            (false, Keysym::Shift_L),
            (false, Keysym::A),
            (true, Keysym::A),
            (true, Keysym::Shift_L),
        ],
        "and Orca is sent each of them"
    );

    // Super+C, bound, is the compositor's: the window never sees C, Orca does.
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_LEFTMETA, KEY_C]);
    assert_eq!(presses, [KEY_LEFTMETA], "the binding keeps C");
    assert_eq!(
        heard(&queued),
        [
            (false, Keysym::Super_L),
            (false, Keysym::c),
            (true, Keysym::c),
            (true, Keysym::Super_L),
        ],
        "Orca hears a bound key too"
    );
    // Super+Alt+T, the reveal key, is protected: the window never sees T, nor does Orca.
    let reveal = [KEY_LEFTMETA, KEY_LEFTALT, KEY_T];
    let (presses, _) = sent_to(&mut window, &mut run, &reveal);
    assert_eq!(presses, [KEY_LEFTMETA, KEY_LEFTALT], "the binding keeps T");
    assert_eq!(heard(&queued), SUPER_ALT, "Orca is not sent a protected key");

    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Watch(false)
    ));
    sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(heard(&queued), [], "unwatched, Orca is sent nothing");
    run.stop();
}

/// A key Orca grabs, alone or with its virtual modifier, reaches Orca and not the window; other
/// keys reach the window. Caps Lock as the virtual modifier leaves no lock behind, and a letter
/// Orca grabs is still its own while Caps Lock capitalizes it.
#[test]
fn ax4_a_grabbed_key_reaches_orca_and_not_the_window() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let queued = listen(&mut run);
    let a = Keysym::a.raw();
    let insert = Keysym::Insert.raw();
    let grabs = Change::KeyGrabs(vec![insert], vec![(a, 0), (Keysym::s.raw(), 1 << 15)]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, grabs));

    assert_eq!(sent_to(&mut window, &mut run, &[KEY_A]), (vec![], vec![]));
    assert_eq!(heard(&queued), pressed(Keysym::a), "A is Orca's");

    // S is grabbed with Insert only; Insert, Orca's modifier, reaches no client.
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_S]);
    assert_eq!(presses, [KEY_S], "S alone is the window's");
    assert_eq!(heard(&queued), [], "and not Orca's");
    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_INSERT, KEY_S]),
        (vec![], vec![])
    );
    assert_eq!(
        heard(&queued),
        [(false, Keysym::s), (true, Keysym::s)],
        "Insert+S is Orca's"
    );
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_S]);
    assert_eq!(presses, [KEY_S], "Insert let go, S is the window's again");

    // Ungrabbed, A is the window's again.
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::KeyGrabs(vec![], vec![])
    ));
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(presses, [KEY_A]);

    // The whole keyboard, grabbed, then let go.
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Grab(true)
    ));
    assert_eq!(sent_to(&mut window, &mut run, &[KEY_S]), (vec![], vec![]));
    assert_eq!(
        heard(&queued),
        pressed(Keysym::s),
        "a grab sends Orca every key"
    );
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Grab(false)
    ));
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_S]);
    assert_eq!(presses, [KEY_S]);

    let caps = Keysym::Caps_Lock.raw();
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::KeyGrabs(vec![caps], vec![])
    ));
    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_CAPSLOCK]),
        (vec![], vec![]),
        "Caps Lock as Orca's modifier reaches no client"
    );
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    assert!(
        !keyboard.modifier_state().caps_lock,
        "Caps Lock as Orca's modifier locks nothing"
    );
    let s = Keysym::s.raw();
    let grabs = Change::KeyGrabs(vec![caps], vec![(s, 1 << 15)]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, grabs));
    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_CAPSLOCK, KEY_S]),
        (vec![], vec![]),
        "Caps Lock+S is Orca's"
    );
    assert_eq!(
        heard(&queued),
        pressed(Keysym::S),
        "though Caps Lock, pressed, capitalizes the S Orca grabs"
    );
    assert!(!keyboard.modifier_state().caps_lock, "and locks nothing");

    // A client that goes is forgotten.
    let forget = Update {
        client: ORCA.into(),
        change: Change::Forget,
    };
    run.state.keyboard_monitor.apply(forget);
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_INSERT]);
    assert_eq!(presses, [KEY_INSERT], "Orca gone, Insert is the window's");
    run.stop();
}

/// A caller outside `orca.service`, one that runs another executable, one that does not own
/// Orca's name, or one the bus names no process of, is refused: no key is sent to it and its
/// grab takes nothing. Orca itself, known by its pidfd or by its pid, is admitted.
#[test]
fn ax4_only_orca_in_its_unit_is_admitted() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let queued = listen(&mut run);
    let outside = Peer {
        cgroup: APP_CGROUP,
        ..ORCA_PROCESS
    };
    let elsewhere = Peer {
        executable: "/home/user/.local/bin/python3.14",
        ..ORCA_PROCESS
    };
    let impostor = Peer {
        file: FileId { dev: 2, ino: 9 },
        ..ORCA_PROCESS
    };
    let refused: [(&str, &str, &Peer); 4] = [
        ("outside orca.service", ORCA, &outside),
        ("an executable outside /usr", ORCA, &elsewhere),
        ("another file at the interpreter's path", ORCA, &impostor),
        ("a client that does not own the name", OTHER, &ORCA_PROCESS),
    ];
    for (what, owner, peer) in refused {
        for change in [Change::Watch(true), Change::Grab(true)] {
            assert!(
                !call(&mut run, ORCA, owner, peer, change),
                "{what} is refused"
            );
        }
        let (presses, _) = sent_to(&mut window, &mut run, &[KEY_A]);
        assert_eq!(presses, [KEY_A], "{what}: its grab takes nothing");
        assert_eq!(heard(&queued), [], "{what}: it is sent no key");
    }
    let nameless = ConnectionCredentials::default();
    assert!(
        check(ORCA, ORCA, &nameless, &ORCA_PROCESS).is_err(),
        "no process, no admission"
    );
    let by_pid = ConnectionCredentials::default().set_process_id(std::process::id());
    assert!(
        check(ORCA, ORCA, &by_pid, &ORCA_PROCESS).is_ok(),
        "a pid stands in for a pidfd"
    );

    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Watch(true)
    ));
    sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(heard(&queued), pressed(Keysym::a), "Orca is admitted");
    run.stop();
}

/// A virtual keyboard's key reaches Orca as the hardware's does.
#[test]
fn ax4_a_virtual_key_reaches_orca() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let queued = listen(&mut run);
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Watch(true)
    ));
    let typist = VirtualKeyboard::new(&mut window, &mut run, None);
    let keys = window.client.keys.len();
    typist.chord(&mut window, &mut run, &[KEY_A]);
    assert_eq!(
        window.client.keys[keys..],
        [KEY_A],
        "the window is sent the key"
    );
    assert_eq!(heard(&queued), pressed(Keysym::a), "and so is Orca");
    run.stop();
}

/// While the session is locked, Orca's grab takes no key from the lock surface, and Orca still
/// hears the keys.
#[test]
fn ax4_while_locked_a_grab_takes_no_key_from_the_lock_surface() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let queued = listen(&mut run);
    let a = Keysym::a.raw();
    let insert = Keysym::Insert.raw();
    let grabs = Change::KeyGrabs(vec![insert], vec![(a, 0)]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, grabs));
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Grab(true)
    ));
    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_S]),
        (vec![], vec![]),
        "unlocked, the grab takes S"
    );
    heard(&queued);

    let (mut locker, _lock) = lock_session(&mut run);
    for codes in [&[KEY_A][..], &[KEY_S][..], &[KEY_INSERT, KEY_A][..]] {
        let (presses, releases) = sent_to(&mut locker, &mut run, codes);
        assert_eq!(presses, codes, "the lock surface is sent every key");
        let reversed: Vec<u32> = codes.iter().rev().copied().collect();
        assert_eq!(releases, reversed);
    }
    assert_eq!(heard(&queued).len(), 8, "and Orca hears them");
    run.stop();
}

/// Under an active shortcuts inhibitor Orca is still sent every key: a binding the inhibitor
/// hands the window, a protected one the compositor keeps, and the others; and Orca's grab still
/// takes its key, which is not a binding (doc_own_stack.md decision 30).
#[test]
fn ax4_an_inhibitor_leaves_the_monitor_working() {
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

    let queued = listen(&mut run);
    let grabs = Change::KeyGrabs(vec![], vec![(Keysym::a.raw(), 0)]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, grabs));
    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Watch(true)
    ));

    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_LEFTMETA, KEY_C]);
    assert_eq!(
        presses,
        [KEY_LEFTMETA, KEY_C],
        "the inhibitor hands the window Super+C"
    );
    assert_eq!(
        heard(&queued),
        [
            (false, Keysym::Super_L),
            (false, Keysym::c),
            (true, Keysym::c),
            (true, Keysym::Super_L),
        ],
        "and Orca hears it"
    );

    let reveal = [KEY_LEFTMETA, KEY_LEFTALT, KEY_T];
    let (presses, _) = sent_to(&mut window, &mut run, &reveal);
    assert_eq!(
        presses,
        [KEY_LEFTMETA, KEY_LEFTALT],
        "the compositor keeps Super+Alt+T"
    );
    assert_eq!(heard(&queued), SUPER_ALT, "and Orca is not sent T");

    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_A]),
        (vec![], vec![]),
        "Orca's grab takes A"
    );
    assert_eq!(heard(&queued), pressed(Keysym::a), "and Orca hears it");

    assert!(call(
        &mut run,
        ORCA,
        ORCA,
        &ORCA_PROCESS,
        Change::Watch(false)
    ));
    reveal_is_protected(&mut window, &mut run, &queued);
    run.stop();
}

/// Neither a grab of the whole keyboard nor a grab of the key takes a protected key from the
/// compositor, nor is Orca sent it (the maintainer's decision of 2026-10-30, #863).
#[test]
fn ax4_a_grab_never_takes_a_protected_key() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let queued = listen(&mut run);
    reveal_is_protected(&mut window, &mut run, &queued);
    run.stop();
}

/// A virtual modifier is held by its key, not by its keysym: S as Orca's modifier, pressed as s
/// and let go under Shift as S, is let go, and A alone is the window's again.
#[test]
fn ax4_a_virtual_modifier_is_let_go_by_its_key() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let grabs = Change::KeyGrabs(vec![Keysym::s.raw()], vec![(Keysym::a.raw(), 1 << 15)]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, grabs));
    let queued = listen(&mut run);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, Change::Watch(true)));
    key(&mut run, KEY_S, KeyState::Pressed);
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(presses, [], "S held, A is Orca's");
    key(&mut run, KEY_LEFTSHIFT, KeyState::Pressed);
    key(&mut run, KEY_S, KeyState::Released);
    key(&mut run, KEY_LEFTSHIFT, KeyState::Released);
    window.roundtrip(&mut run);
    let letter: Vec<_> = heard(&queued)
        .into_iter()
        .filter(|(_, sym)| [Keysym::s, Keysym::S].contains(sym))
        .collect();
    assert_eq!(
        letter,
        [(false, Keysym::s), (true, Keysym::S)],
        "the key is let go under Shift as S"
    );
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_A]);
    assert_eq!(presses, [KEY_A], "S let go, A is the window's");
    run.stop();
}

/// While the session is locked a protected key is still sent to no client, Orca watching
/// included, though the lock leaves some to the lock surface (the maintainer's decision of
/// 2026-10-30, #863).
#[test]
fn ax4_while_locked_orca_is_sent_no_protected_key() {
    let mut run = Running::start(None);
    let _window = focused_window(&mut run);
    let queued = listen(&mut run);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, Change::Watch(true)));
    let (mut locker, _lock) = lock_session(&mut run);
    let chords: [(&[u32], &str); 6] = [
        (&[KEY_LEFTMETA, KEY_ESC], "Super+Escape"),
        (&[KEY_LEFTMETA, KEY_LEFTALT, KEY_ESC], "Super+Alt+Escape"),
        (&[KEY_LEFTMETA, KEY_LEFTALT, KEY_T], "Super+Alt+T"),
        (&[KEY_LEFTMETA, KEY_LEFTALT, KEY_S], "Super+Alt+S"),
        (&[KEY_LEFTCTRL, KEY_LEFTALT, KEY_TAB], "Ctrl+Alt+Tab"),
        (&[KEY_LEFTCTRL, KEY_LEFTALT, KEY_F1], "Ctrl+Alt+F1"),
    ];
    let modifiers = [Keysym::Super_L, Keysym::Alt_L, Keysym::Control_L];
    for (codes, what) in chords {
        sent_to(&mut locker, &mut run, codes);
        let sent = heard(&queued);
        assert_eq!(sent.len(), 2 * (codes.len() - 1), "{what}: Orca hears the modifiers");
        for (_, sym) in sent {
            assert!(modifiers.contains(&sym), "{what}: Orca is sent {sym:?}");
        }
    }
    run.stop();
}

/// Caps Lock as Orca's modifier gives back the lock it found, on or off, and the window is told;
/// Caps Lock under Orca's grab of the keyboard locks the keyboard, and the window, sent no key,
/// is told of the lock.
#[test]
fn ax4_caps_lock_leaves_the_lock_as_the_window_knows_it() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    let (presses, _) = sent_to(&mut window, &mut run, &[KEY_CAPSLOCK]);
    assert_eq!(presses, [KEY_CAPSLOCK]);
    assert!(keyboard.modifier_state().caps_lock, "Caps Lock locks");

    let caps = Keysym::Caps_Lock.raw();
    let grabs = Change::KeyGrabs(vec![caps], vec![]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, grabs));
    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_CAPSLOCK]),
        (vec![], vec![])
    );
    assert!(
        keyboard.modifier_state().caps_lock,
        "Caps Lock as Orca's modifier leaves the lock on"
    );
    let told = window.client.modifier_locks.last().copied();
    assert_eq!(told.map(|locks| locks & LOCK), Some(LOCK), "as the window knows");

    let ungrab = Change::KeyGrabs(vec![], vec![]);
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, ungrab));
    assert!(call(&mut run, ORCA, ORCA, &ORCA_PROCESS, Change::Grab(true)));
    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_CAPSLOCK]),
        (vec![], vec![]),
        "the grab takes Caps Lock"
    );
    assert!(!keyboard.modifier_state().caps_lock, "which unlocks");
    let told = window.client.modifier_locks.last().copied();
    assert_eq!(told.map(|locks| locks & LOCK), Some(0), "as the window is told");
    run.stop();
}

/// A private session bus: a `dbus-daemon` of its own, stopped and removed when dropped.
struct Bus {
    daemon: Child,
    address: String,
    dir: PathBuf,
}

impl Bus {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!("athanor-compositor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the bus's directory");
        // A configuration of its own: `--session` reads the host's, which on a system running
        // dbus-broker has no <listen> element.
        let config = dir.join("bus.conf");
        let policy = "<policy context=\"default\"><allow send_destination=\"*\"/>\
                      <allow receive_sender=\"*\"/><allow own=\"*\"/></policy>";
        let listen = format!("unix:path={}", dir.join("bus").display());
        let text = format!("<busconfig><type>session</type><listen>{listen}</listen>{policy}</busconfig>");
        std::fs::write(&config, text).expect("bus.conf");
        let mut daemon = Command::new("dbus-daemon")
            .args(["--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--config-file={}", config.display()))
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon on PATH");
        let mut address = String::new();
        let stdout = daemon.stdout.take().expect("the daemon's output");
        BufReader::new(stdout)
            .read_line(&mut address)
            .expect("the bus's address");
        let address = address.trim().to_owned();
        Self {
            daemon,
            address,
            dir,
        }
    }

    fn builder(&self) -> Builder<'static> {
        Builder::address(self.address.as_str()).expect("the bus's address")
    }

    fn connect(&self) -> Connection {
        self.builder().build().expect("a connection to the bus")
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// The error's name, if the call failed.
fn refused(answer: zbus::Result<Message>) -> Option<String> {
    match answer {
        Ok(_) => None,
        Err(zbus::Error::MethodError(name, _, _)) => Some(name.to_string()),
        Err(error) => panic!("the call did not reach the monitor: {error}"),
    }
}

/// `method` of the monitor, with no arguments, called by `client`: the error's name, if it fails.
fn ask(client: &Connection, method: &str) -> Option<String> {
    refused(client.call_method(Some(NAME), PATH, Some(INTERFACE), method, &()))
}

/// The next `KeyEvent` `signals` delivers, within `WAIT`.
fn next_key(signals: MessageIterator) -> (MessageIterator, Message) {
    let (sent, received) = channel();
    std::thread::spawn(move || {
        let mut signals = signals;
        let next = signals.next();
        // The test may have given up waiting.
        sent.send((signals, next)).ok();
    });
    let (signals, next) = received.recv_timeout(WAIT).expect("a key, in time");
    (signals, next.expect("the bus").expect("a message"))
}

/// The `KeyEvent` signals addressed to `client`.
fn key_events(client: &Connection) -> MessageIterator {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(INTERFACE)
        .and_then(|rule| rule.member("KeyEvent"))
        .map(|rule| rule.build())
        .expect("a match rule");
    MessageIterator::for_match_rule(rule, client, None).expect("the match rule added")
}

/// On a real bus the monitor admits Orca's calls, and Orca's alone, before it answers them;
/// refuses another client's with `AccessDenied`; sends a key to its client alone; forgets Orca
/// when it lets its name go; and when the bus goes, forgets every client and stops.
#[test]
fn ax4_on_the_bus_the_monitor_serves_orca_alone() {
    use crate::keyboard_monitor::ORCA as ORCA_NAME;
    let bus = Bus::start();
    let (served, asking) = (bus.builder(), bus.connect());
    let (sent, updates) = channel();
    let (events, queued) = sync_channel(8);
    let server = std::thread::spawn(move || {
        let send = |update: Update| {
            sent.send(update)
                .map_err(|_| crate::Error::from("the test has ended"))
        };
        serve(served, &asking, &ORCA_PROCESS, &send, queued).map_err(|error| error.to_string())
    });
    let (orca, other) = (bus.connect(), bus.connect());
    let names = DBusProxy::new(&orca).expect("the bus's proxy");
    let deadline = Instant::now() + WAIT;
    while !names
        .name_has_owner(NAME.try_into().expect("a name"))
        .expect("the bus answers")
    {
        assert!(Instant::now() < deadline, "the monitor took its name in time");
        std::thread::sleep(Duration::from_millis(10));
    }
    orca.request_name(ORCA_NAME).expect("Orca's name");
    let orca_name = orca.unique_name().expect("Orca's unique name").to_string();
    let other_name = other.unique_name().expect("a unique name").to_string();

    assert_eq!(ask(&orca, "WatchKeyboard"), None, "Orca is admitted");
    let watch = Update {
        client: orca_name.clone(),
        change: Change::Watch(true),
    };
    assert_eq!(updates.try_recv().ok(), Some(watch), "before it is answered");
    let denied = Some("org.freedesktop.DBus.Error.AccessDenied".to_owned());
    for method in ["WatchKeyboard", "GrabKeyboard"] {
        assert_eq!(ask(&other, method), denied, "{method}: another client is refused");
    }
    let body = (vec![1_u32], vec![(2_u32, 3_u32)]);
    let answer = orca.call_method(Some(NAME), PATH, Some(INTERFACE), "SetKeyGrabs", &body);
    assert_eq!(refused(answer), None);
    let grabs = Update {
        client: orca_name.clone(),
        change: Change::KeyGrabs(vec![1], vec![(2, 3)]),
    };
    assert_eq!(updates.try_recv().ok(), Some(grabs), "a refused call changes nothing");

    let (to_orca, to_other) = (key_events(&orca), key_events(&other));
    for (destination, released) in [(&orca_name, false), (&other_name, true)] {
        let event = KeyEvent {
            destination: destination.clone(),
            released,
            state: 1,
            keysym: Keysym::a.raw(),
            unichar: 'a'.into(),
            keycode: 38,
        };
        events.send(event).expect("the signals' thread");
    }
    let (_, signal) = next_key(to_orca);
    let destination = signal.header().destination().map(|name| name.to_string());
    assert_eq!(destination, Some(orca_name.clone()), "a key is addressed to Orca");
    let body: (bool, u32, u32, u32, u16) = signal.body().deserialize().expect("a KeyEvent");
    assert_eq!(body, (false, 1, Keysym::a.raw(), 'a'.into(), 38));
    let (_, signal) = next_key(to_other);
    let body: (bool, u32, u32, u32, u16) = signal.body().deserialize().expect("a KeyEvent");
    assert!(body.0, "another client is sent its own key, not Orca's");

    orca.release_name(ORCA_NAME).expect("Orca lets its name go");
    let forget = Update {
        client: orca_name.clone(),
        change: Change::Forget,
    };
    assert_eq!(updates.recv_timeout(WAIT).ok(), Some(forget), "Orca is forgotten");

    orca.request_name(ORCA_NAME).expect("Orca's name");
    assert_eq!(ask(&orca, "WatchKeyboard"), None, "Orca is admitted again");
    updates.try_recv().expect("its change");
    drop(bus);
    let forget = Update {
        client: orca_name,
        change: Change::Forget,
    };
    assert_eq!(updates.recv_timeout(WAIT).ok(), Some(forget), "the bus gone, Orca is forgotten");
    let ended = server.join().expect("the monitor's thread");
    assert!(ended.is_err(), "and the monitor stops: {ended:?}");
}
