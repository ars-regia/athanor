//! Decision 38: the user's `input.toml`, `windows.toml` and `bindings.toml`, read at start and
//! followed live.
//! The fixture has no libinput device (`devices.rs`), so these read what the compositor holds;
//! a key reaching a device is the resolution `input_settings.rs` tests.

use super::*;
use crate::input_settings::{self, Class};

/// A directory of its own for each test, and the file in it.
fn settings_file(name: &str, file: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("athanor-settings-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a directory");
    dir.join(file)
}

/// Runs the loop until `done` holds.
fn until(run: &mut Running, what: &str, done: impl Fn(&State) -> bool) {
    let started = Instant::now();
    while !done(&run.state) {
        assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }
}

/// Runs the loop a while, for a change that must not happen.
fn idle(run: &mut Running) {
    for _ in 0..20 {
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }
}

fn mouse_speed(state: &State) -> Option<f64> {
    state.input_settings().resolve(Class::Mouse, "a mouse").speed
}

/// A change is followed without a restart; a malformed or too large file leaves the last good
/// settings; a file removed gives the defaults back.
#[test]
fn input_toml_is_followed_live() {
    let mut run = Running::start(None);
    let file = settings_file("input", "input.toml");
    let handle = run.event_loop.handle();
    input_settings::follow(&mut run.state, &handle, file.clone());
    assert_eq!(*run.state.input_settings(), Default::default(), "no file");

    write_raw(&file, "schema = 1\n[mouse]\nspeed = 0.5\n");
    until(&mut run, "the speed", |state| {
        mouse_speed(state) == Some(0.5)
    });

    write_raw(&file, "schema = 1\n[mouse\nspeed = -0.5\n");
    idle(&mut run);
    assert_eq!(
        mouse_speed(&run.state),
        Some(0.5),
        "a malformed file changes nothing"
    );

    let large = format!(
        "schema = 1\n[mouse]\nspeed = -0.5\n#{}\n",
        "x".repeat(64 * 1024)
    );
    write_raw(&file, &large);
    idle(&mut run);
    assert_eq!(
        mouse_speed(&run.state),
        Some(0.5),
        "a large file changes nothing"
    );

    write_raw(
        &file,
        "schema = 1\n[mouse]\nspeed = 3\nnatural_scroll = true\n",
    );
    until(&mut run, "the other key", |state| {
        state
            .input_settings()
            .resolve(Class::Mouse, "a mouse")
            .natural_scroll
            == Some(true)
    });
    assert_eq!(
        mouse_speed(&run.state),
        None,
        "a refused key is skipped alone"
    );

    std::fs::remove_file(&file).expect("removed");
    until(&mut run, "the defaults", |state| {
        *state.input_settings() == Default::default()
    });
    run.stop();
    std::fs::remove_dir_all(file.parent().expect("a directory")).expect("clean");
}

/// Num Lock is set once at the start of the session, and a keymap reload keeps it.
#[test]
fn num_lock_at_the_start_survives_a_keymap_reload() {
    let mut run = Running::start(None);
    let file = settings_file("numlock", "input.toml");
    write_raw(
        &file,
        "schema = 1\n[keyboard]\nnumlock_state = \"boot_on\"\n",
    );
    run.state.load_input_settings(&file);
    run.state.set_num_lock_at_start();
    let num_lock = |state: &State| {
        let keyboard = state.seat.get_keyboard().expect("a keyboard");
        keyboard.modifier_state().num_lock
    };
    assert!(num_lock(&run.state), "on at the start");

    let xkb = xkb_file("numlock");
    write_xkb(&xkb, "us,it");
    run.state.load_xkb_config(&xkb);
    assert!(num_lock(&run.state), "kept through a new keymap");

    // A later change of the file does not touch Num Lock: it is the start's.
    write_raw(
        &file,
        "schema = 1\n[keyboard]\nnumlock_state = \"boot_off\"\n",
    );
    run.state.load_input_settings(&file);
    assert!(num_lock(&run.state), "not turned off by a change");
    run.stop();
    std::fs::remove_dir_all(file.parent().expect("a directory")).expect("clean");
    std::fs::remove_dir_all(xkb.parent().expect("a directory")).expect("clean");
}

/// Without `[keyboard]`, Num Lock stays off.
#[test]
fn num_lock_is_off_by_default() {
    let mut run = Running::start(None);
    run.state.set_num_lock_at_start();
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    assert!(!keyboard.modifier_state().num_lock);
    run.stop();
}
/// `active_hint` turns the focus outline off and on without a restart; a malformed file keeps
/// the last value, a refused key is skipped alone, and a file removed gives the default back.
#[test]
fn windows_toml_is_followed_live() {
    let mut run = Running::start(None);
    let file = settings_file("windows", "windows.toml");
    let handle = run.event_loop.handle();
    crate::window_settings::follow(&mut run.state, &handle, file.clone());
    assert!(
        run.state.focus_theme.outline().is_some(),
        "no file: the outline is on"
    );

    write_raw(&file, "schema = 1\nactive_hint = false\n");
    until(&mut run, "the hint off", |state| {
        state.focus_theme.outline().is_none()
    });

    write_raw(&file, "schema = 1\nactive_hint = \n");
    idle(&mut run);
    assert!(
        run.state.focus_theme.outline().is_none(),
        "a malformed file changes nothing"
    );

    write_raw(
        &file,
        "schema = 1\nactive_hint = true\nedge_snap_threshold = 8\n",
    );
    until(&mut run, "the hint on", |state| {
        state.focus_theme.outline().is_some()
    });

    write_raw(&file, "schema = 1\nactive_hint = false\n");
    until(&mut run, "the hint off again", |state| {
        state.focus_theme.outline().is_none()
    });
    std::fs::remove_file(&file).expect("removed");
    until(&mut run, "the default", |state| {
        state.focus_theme.outline().is_some()
    });
    run.stop();
    std::fs::remove_dir_all(file.parent().expect("a directory")).expect("clean");
}

/// `bindings.toml` is followed live: a user's command runs from its key as `/bin/sh -c`, a
/// system action the user replaced runs the user's command while unlocked and the default
/// while locked, where no binding of the user's acts; a malformed file keeps the last good set,
/// and a change is followed.
#[test]
fn bindings_toml_is_followed_live() {
    use crate::bindings::Action;
    use crate::session_calls::{Job, SessionCalls, MUTE};
    let mut run = Running::start(None);
    let (calls, requests) = SessionCalls::detached();
    run.state.session_calls = Some(calls);
    let file = settings_file("bindings", "bindings.toml");
    let handle = run.event_loop.handle();
    crate::user_bindings::follow(&mut run.state, &handle, file.clone());
    let (mut window, _) = focused_window(&mut run);
    let super_t = [KEY_LEFTMETA, KEY_T];
    let job = || requests.try_recv().ok().map(|request| request.job);

    write_raw(
        &file,
        "schema = 1\n[system_actions]\nmute = \"my-mute\"\n\
         [[binding]]\nkeys = \"Super+t\"\naction = \"spawn\"\ncommand = \"foot\"\n",
    );
    until(&mut run, "the user's bindings", |state| {
        state.user_bindings.command(1) == Some("foot")
    });
    acts(&mut window, &mut run, &super_t, Some(Action::Spawn(1)), "Super+t");
    assert_eq!(job(), Some(Job::Shell("foot".to_owned())));
    acts(&mut window, &mut run, &[KEY_MUTE], Some(Action::Spawn(0)), "mute");
    assert_eq!(job(), Some(Job::Shell("my-mute".to_owned())));

    write_raw(&file, "schema = 1\n[[binding]\n");
    idle(&mut run);
    assert_eq!(
        run.state.user_bindings.command(1),
        Some("foot"),
        "a malformed file changes nothing"
    );

    let (mut locker, _lock) = lock_session(&mut run);
    acts(
        &mut locker,
        &mut run,
        &[KEY_MUTE],
        Some(Action::Run(&MUTE)),
        "locked, mute",
    );
    assert_eq!(job(), Some(Job::Run(&MUTE)), "the default, not the user's command");
    reaches(&mut locker, &mut run, &super_t, "locked, Super+t");
    assert_eq!(job(), None);

    write_raw(
        &file,
        "schema = 1\n[[binding]]\nkeys = \"Super+t\"\naction = \"spawn\"\ncommand = \"kitty\"\n",
    );
    until(&mut run, "the change", |state| {
        state.user_bindings.command(0) == Some("kitty")
    });
    std::fs::remove_file(&file).expect("removed");
    until(&mut run, "the defaults", |state| {
        state.user_bindings.command(0).is_none()
    });
    run.stop();
    std::fs::remove_dir_all(file.parent().expect("a directory")).expect("clean");
}
