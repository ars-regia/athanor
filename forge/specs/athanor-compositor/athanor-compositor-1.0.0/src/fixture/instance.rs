//! The fixture's tests of the keyboard at a window's first placement across clients (`place`):
//! a client of the same security context's application instance as the one typed into takes
//! it, as wl-copy run in a terminal does; any other client does not.

use super::parented::Created;
use super::*;

/// A context the launcher's client `creator` made for the instance `instance`.
fn context(creator: ClientId, instance: Option<&str>) -> SecurityContext {
    SecurityContext {
        sandbox_engine: Some("os.athanor".into()),
        app_id: Some("org.gnome.Ptyxis".into()),
        instance_id: instance.map(Into::into),
        creator_client_id: creator,
    }
}

/// What `opens` leaves: whether the command's window took the keyboard, and the clients and
/// windows, kept alive for the test.
struct Opened {
    took: bool,
    _terminal: Local,
    typed: smithay::desktop::Window,
    command: Local,
    created: Created,
}

/// The terminal, inside `terminal`, holding the keyboard; and a window that `command`, another
/// client, places.
fn opens(run: &mut Running, terminal: SecurityContext, mut command: Local) -> Opened {
    let (typing, typed) = focused_window_in(run, Some(terminal));
    assert!(keyboard_on(run, &typed), "the terminal has the keyboard");
    let created = command.create(run);
    let window = command.map(run, &created);
    let took = keyboard_on(run, &window);
    assert!(
        took != keyboard_on(run, &typed),
        "the keyboard is on one of them"
    );
    Opened {
        took,
        _terminal: typing,
        typed,
        command,
        created,
    }
}

/// wl-copy run in the terminal connects through the terminal's socket, so through the same
/// context: its window takes the keyboard, and the terminal has it back once the window is gone.
#[test]
fn a_window_of_the_same_app_instance_takes_the_keyboard() {
    let mut run = Running::start(None);
    let (_, launcher) = fixture_window(&run);
    let terminal = context(launcher.clone(), Some("78560abb"));
    let command = Local::connect(&mut run, Some(terminal.clone()));
    let mut opened = opens(&mut run, terminal, command);
    assert!(opened.took, "the command's window takes the keyboard");
    opened.created.toplevel.destroy();
    opened.command.roundtrip(&mut run);
    assert!(keyboard_on(&run, &opened.typed), "the terminal has it back");
    run.stop();
}

/// Another instance of the same application, from the same launcher, is another application.
#[test]
fn a_window_of_another_instance_does_not_take_the_keyboard() {
    let mut run = Running::start(None);
    let (_, launcher) = fixture_window(&run);
    let command = Local::connect(&mut run, Some(context(launcher.clone(), Some("second"))));
    let opened = opens(&mut run, context(launcher, Some("first")), command);
    assert!(!opened.took, "the keyboard stays with the terminal");
    run.stop();
}

/// The instance id is the creator's: one another client names in its own context is not the
/// launcher's instance.
#[test]
fn an_instance_id_of_another_creator_does_not_take_the_keyboard() {
    let mut run = Running::start(None);
    let (_, launcher) = fixture_window(&run);
    let other = Local::connect(&mut run, None);
    let creator = other.id.clone().expect("an id");
    let command = Local::connect(&mut run, Some(context(creator, Some("78560abb"))));
    let opened = opens(&mut run, context(launcher, Some("78560abb")), command);
    assert!(!opened.took, "the keyboard stays with the terminal");
    run.stop();
}

/// A context without an instance id names no instance, so two of them are not one application.
#[test]
fn contexts_without_an_instance_id_do_not_share_the_keyboard() {
    let mut run = Running::start(None);
    let (_, launcher) = fixture_window(&run);
    let command = Local::connect(&mut run, Some(context(launcher.clone(), None)));
    let opened = opens(&mut run, context(launcher, None), command);
    assert!(!opened.took, "the keyboard stays with the terminal");
    run.stop();
}

/// A client outside any context, and xwayland-satellite's, are not the terminal's instance.
#[test]
fn an_unconfined_or_x11_window_does_not_take_the_keyboard_of_an_instance() {
    for x11 in [false, true] {
        let mut run = Running::start(None);
        let (_, launcher) = fixture_window(&run);
        let command = if x11 {
            let (server_end, client_end) = UnixStream::pair().expect("socket pair");
            let data = run.state.satellite_state();
            let id = run
                .state
                .display_handle
                .insert_client(server_end, data)
                .expect("client")
                .id();
            Local::over(&mut run, client_end, Some(id))
        } else {
            Local::connect(&mut run, None)
        };
        let opened = opens(&mut run, context(launcher, Some("78560abb")), command);
        assert!(!opened.took, "the keyboard stays with the terminal (x11: {x11})");
        run.stop();
    }
}
