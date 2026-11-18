//! `athanor-idle`'s Wayland side on Sulphur (doc_session_daemons.md, SD4): its notifications,
//! the overlay that fades the outputs to black with a single-pixel buffer, and the outputs'
//! power. The daemon connects as the bar does, whose allow-list line grants the overlay layer
//! and the output power, as `athanor-idle`'s own line will.

use super::*;
use athanor_idle::wayland::{Display, Notice, Which};

const BLACK: [u8; 4] = [0, 0, 0, 255];

/// `athanor-idle`'s display, connected, with its globals bound.
fn daemon(run: &mut Running) -> Display {
    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let data = run.state.client_state(&server_end, None);
    run.state
        .display_handle
        .insert_client(server_end, data)
        .expect("client");
    let mut display = Display::new(Connection::from_socket(client_end).expect("connection"));
    let mut seen = Vec::new();
    pump(
        run,
        &mut display,
        &mut seen,
        "the globals are bound",
        |display, _, _| display.check().is_ok(),
    );
    // The outputs, then their power objects.
    settle(run, &mut display, &mut seen);
    display
}

/// Runs the daemon's side and the compositor until `done` holds, gathering the notices.
fn pump(
    run: &mut Running,
    display: &mut Display,
    seen: &mut Vec<Notice>,
    what: &str,
    done: impl Fn(&Display, &[Notice], &State) -> bool,
) {
    let started = Instant::now();
    loop {
        match display.read() {
            Err(error) if error.kind() != std::io::ErrorKind::WouldBlock => {
                panic!("{what}: {error}")
            }
            _ => {}
        }
        seen.extend(display.dispatch().expect("dispatch"));
        if done(display, seen, &run.state) {
            return;
        }
        assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
        run.state.refresh_idle_inhibit();
    }
}

/// Lets 50 ms of requests and events go by.
fn settle(run: &mut Running, display: &mut Display, seen: &mut Vec<Notice>) {
    let started = Instant::now();
    pump(run, display, seen, "a pause", |_, _, _| {
        started.elapsed() >= Duration::from_millis(50)
    });
}

/// Each output's middle, as the first output's frame shows it.
fn middle(run: &mut Running) -> [u8; 4] {
    let frame = run.state.capture(0).expect("a frame");
    pixel(
        &frame,
        OUTPUTS[0].width,
        OUTPUTS[0].width / 2,
        OUTPUTS[0].height / 2,
    )
}

/// `idled`, the overlay that fades to opaque black over the fixture's window, the outputs off,
/// input that resumes, and the outputs on with the overlay gone.
#[test]
fn the_blank_fades_to_black_and_turns_the_outputs_off() {
    let mut run = Running::start(None);
    let before = middle(&mut run);
    assert_ne!(before, BLACK, "the window or the background shows");
    let mut idle = daemon(&mut run);
    let mut seen = Vec::new();
    idle.set_timeout(Which::Blank, Some(Duration::from_millis(100)));
    pump(&mut run, &mut idle, &mut seen, "idled", |_, seen, _| {
        seen == [Notice::Idled(Which::Blank)]
    });

    let fade = Duration::from_millis(300);
    idle.show_overlay(fade);
    let shown = Instant::now();
    pump(
        &mut run,
        &mut idle,
        &mut seen,
        "the fade ends",
        |_, _, _| shown.elapsed() >= fade + Duration::from_millis(100),
    );
    assert_eq!(middle(&mut run), BLACK, "opaque black on top");

    idle.set_outputs_off(true);
    pump(
        &mut run,
        &mut idle,
        &mut seen,
        "the outputs go off",
        |_, _, state| {
            outputs_of(state)
                .iter()
                .all(|output| !state.output_power.is_on(output))
        },
    );

    run.state.activity();
    pump(
        &mut run,
        &mut idle,
        &mut seen,
        "input resumes",
        |_, seen, _| seen == [Notice::Idled(Which::Blank), Notice::Resumed(Which::Blank)],
    );
    idle.set_outputs_off(false);
    idle.remove_overlay();
    pump(
        &mut run,
        &mut idle,
        &mut seen,
        "the outputs come on",
        |_, _, state| {
            outputs_of(state)
                .iter()
                .all(|output| state.output_power.is_on(output))
        },
    );
    settle(&mut run, &mut idle, &mut seen);
    assert_eq!(middle(&mut run), before, "the overlay is gone");
    assert_eq!(outputs_of(&run.state).len(), OUTPUTS.len());
    drop(idle);
    run.stop();
}

fn outputs_of(state: &State) -> Vec<smithay::output::Output> {
    state.space.outputs().cloned().collect()
}

/// With no fade the overlay is opaque at once, and it takes neither the keyboard nor the
/// pointer from the window under it.
#[test]
fn the_overlay_without_a_fade_is_opaque_at_once_and_takes_no_input() {
    let mut run = Running::start(None);
    let focus = keyboard_focus(&run);
    let mut idle = daemon(&mut run);
    let mut seen = Vec::new();
    idle.show_overlay(Duration::ZERO);
    settle(&mut run, &mut idle, &mut seen);
    assert_eq!(middle(&mut run), BLACK);
    assert_eq!(
        keyboard_focus(&run),
        focus,
        "the keyboard stays where it was"
    );
    idle.remove_overlay();
    settle(&mut run, &mut idle, &mut seen);
    assert_ne!(middle(&mut run), BLACK);
    run.stop();
}

/// A timeout changed before it ran out creates the notification again, which counts from zero;
/// `None` creates none; the suspend notification is a second one.
#[test]
fn a_changed_timeout_counts_again_and_none_creates_no_notification() {
    let mut run = Running::start(None);
    let mut idle = daemon(&mut run);
    let mut seen = Vec::new();
    idle.set_timeout(Which::Blank, Some(Duration::from_millis(200)));
    idle.set_timeout(Which::Blank, Some(Duration::from_secs(60)));
    idle.set_timeout(Which::Suspend, None);
    let started = Instant::now();
    pump(
        &mut run,
        &mut idle,
        &mut seen,
        "past the first timeout",
        |_, _, _| started.elapsed() >= Duration::from_millis(400),
    );
    assert_eq!(seen, [], "neither notification idled");
    idle.set_timeout(Which::Blank, None);
    idle.set_timeout(Which::Suspend, Some(Duration::from_millis(100)));
    pump(
        &mut run,
        &mut idle,
        &mut seen,
        "the suspend notification idles",
        |_, seen, _| seen == [Notice::Idled(Which::Suspend)],
    );
    run.stop();
}
