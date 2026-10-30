//! The fixture's tests of Orca's keyboard monitor (doc_own_stack.md OS4, doc_accessibility.md
//! AX4; `keyboard_monitor.rs`). A call is admitted as the monitor's thread admits it, through
//! `check` with the bus's credentials of this test's own process, read as whatever executable
//! and cgroup the test sets; the signals are read from the queue the loop sends them to.

use super::{
    focused_window, lock_session, sent_to, Local, Running, VirtualKeyboard,
};
use crate::identity::{pidfd_open, FileId, Process};
use crate::keyboard_monitor::{check, Change, KeyEvent, Update};
use smithay::input::keyboard::Keysym;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use wayland_client::protocol::wl_seat;
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

    // Super+Alt+T, the reveal key, is the compositor's: the window never sees T, Orca does.
    let reveal = [KEY_LEFTMETA, KEY_LEFTALT, KEY_T];
    let (presses, _) = sent_to(&mut window, &mut run, &reveal);
    assert_eq!(presses, [KEY_LEFTMETA, KEY_LEFTALT], "the binding keeps T");
    assert_eq!(
        heard(&queued),
        [
            (false, Keysym::Super_L),
            (false, Keysym::Alt_L),
            (false, Keysym::t),
            (true, Keysym::t),
            (true, Keysym::Alt_L),
            (true, Keysym::Super_L),
        ],
        "Orca hears a bound key too"
    );

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
/// keys reach the window. Caps Lock as the virtual modifier leaves no lock behind.
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
    assert_eq!(heard(&queued).len(), 6, "and Orca hears it");

    assert_eq!(
        sent_to(&mut window, &mut run, &[KEY_A]),
        (vec![], vec![]),
        "Orca's grab takes A"
    );
    assert_eq!(heard(&queued), pressed(Keysym::a), "and Orca hears it");
    run.stop();
}
