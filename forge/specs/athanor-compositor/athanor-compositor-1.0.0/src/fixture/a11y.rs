//! AX10 (`a11y.rs`, `look.rs`, `screen_filter.rs`): the magnifier and the colour filters,
//! switched through athanor-a11y-v1 and the keys, drawn on the outputs and not on captures, and
//! stored where cosmic-comp stores them.

use super::*;
use crate::bindings::Action;
use crate::screen_filter::{filter_text, parse_filter, ScreenFilter};
use athanor_a11y::athanor_a11y_manager_v1::{self, ActiveState, AthanorA11yManagerV1, Filter};

/// An event of the manager, its enums as numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seen {
    Magnifier(u32, f64),
    Filter(u32, u32),
    Modifiers(u32, u32),
    Other,
}

/// The events a manager was sent, in order.
type Log = Arc<Mutex<Vec<Seen>>>;

impl Dispatch<AthanorA11yManagerV1, Log> for Client {
    fn event(
        _: &mut Self,
        _: &AthanorA11yManagerV1,
        event: athanor_a11y_manager_v1::Event,
        log: &Log,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use athanor_a11y_manager_v1::Event;
        let seen = match event {
            Event::Magnifier { active, level } => Seen::Magnifier(active.into(), level),
            Event::ScreenFilter { inverted, filter } => {
                Seen::Filter(inverted.into(), filter.into())
            }
            Event::Modifiers { latched, locked } => Seen::Modifiers(latched.into(), locked.into()),
            _ => Seen::Other,
        };
        log.lock().expect("the events").push(seen);
    }
}

const MANAGER: &str = "athanor_a11y_manager_v1";

/// What a manager is sent when it is bound with everything off.
const AT_BIND: [Seen; 3] = [
    Seen::Magnifier(0, 1.0),
    Seen::Filter(0, 0),
    Seen::Modifiers(0, 0),
];

/// `local`'s manager, and its events once the first roundtrip has brought them.
fn manager(local: &mut Local, run: &mut Running) -> (AthanorA11yManagerV1, Log) {
    let name = local
        .client
        .globals
        .iter()
        .find(|(_, interface, _)| interface == MANAGER)
        .map(|(name, _, _)| *name)
        .expect("the manager's global");
    let log = Log::default();
    let manager = local
        .registry
        .bind(name, 1, &local.queue.handle(), log.clone());
    local.roundtrip(run);
    (manager, log)
}

/// The events of `log` since the last call.
fn taken(log: &Log) -> Vec<Seen> {
    std::mem::take(&mut *log.lock().expect("the events"))
}

/// The middle of the fixture's window, red above it and blue below.
fn window_middle(run: &Running) -> Point<i32, Logical> {
    let (window, _) = fixture_window(run);
    let area = run.state.space.element_geometry(&window).expect("mapped");
    area.loc + Point::from((WIDTH / 2, HEIGHT / 2))
}

/// A scratch directory of its own for test `name`, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("athanor-a11y-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir
}

/// `athanor_a11y_manager_v1` is privileged (OS4, OS5): the fixture's bar, granted it, sees it;
/// the bar's identity inside a security context, the control center and a client with no line
/// do not.
#[test]
fn the_a11y_manager_is_privileged() {
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.sees(MANAGER), "the bar, granted it");
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(MANAGER), "the bar inside a context");
    let control_center = Local::control_center(&mut run);
    assert!(!control_center.sees(MANAGER), "the control center");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(MANAGER), "a client with no line");
    run.stop();
}

/// A manager is sent the state at once; set_magnifier, zoom_in and zoom_out step the level by
/// cosmic-comp's increment, between 1 and 256, every manager hears each change, and the
/// magnifier comes back on at its last level.
#[test]
fn the_magnifier_is_switched_and_stepped() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let (manager, log) = manager(&mut bar, &mut run);
    let mut other = Local::connect(&mut run, None);
    let (_other, other_log) = self::manager(&mut other, &mut run);
    assert_eq!(taken(&log), AT_BIND, "the state at bind");
    taken(&other_log);

    manager.set_magnifier(ActiveState::Enabled);
    bar.roundtrip(&mut run);
    assert_eq!(taken(&log), [Seen::Magnifier(1, 1.5)], "on at one step");
    other.roundtrip(&mut run);
    assert_eq!(
        taken(&other_log),
        [Seen::Magnifier(1, 1.5)],
        "the other hears it"
    );
    manager.zoom_in();
    manager.zoom_out();
    manager.zoom_out();
    manager.zoom_out();
    bar.roundtrip(&mut run);
    assert_eq!(
        taken(&log),
        [
            Seen::Magnifier(1, 2.25),
            Seen::Magnifier(1, 1.5),
            Seen::Magnifier(0, 1.0),
        ],
        "a step each way; at 1 off, and no further"
    );
    manager.zoom_in();
    manager.zoom_in();
    manager.set_magnifier(ActiveState::Disabled);
    manager.set_magnifier(ActiveState::Enabled);
    bar.roundtrip(&mut run);
    assert_eq!(
        taken(&log),
        [
            Seen::Magnifier(1, 1.5),
            Seen::Magnifier(1, 2.25),
            Seen::Magnifier(0, 1.0),
            Seen::Magnifier(1, 2.25),
        ],
        "back on at the last level"
    );
    for _ in 0..20 {
        manager.zoom_in();
    }
    bar.roundtrip(&mut run);
    let seen = taken(&log);
    assert_eq!(seen.last(), Some(&Seen::Magnifier(1, 256.0)), "at most 256");
    assert_eq!(run.state.a11y.level, 256.0);
    run.stop();
}

/// cosmic-comp's increment sets the step; an increment of 0 would never change the level and is
/// 50.
#[test]
fn the_step_is_cosmic_comps_increment() {
    let dir = scratch("increment");
    let path = dir.join("accessibility_zoom");
    let record = |increment: u32| {
        format!(
            "(start_on_login: false, show_overlay: true, increment: {increment}, \
             view_moves: Continuously, enable_mouse_zoom_shortcuts: true)"
        )
    };
    let mut run = Running::start(None);
    std::fs::write(&path, record(25)).expect("the record");
    run.state.load_zoom(&path);
    run.state.zoom_step(true);
    assert_eq!(run.state.a11y.level, 1.25, "a quarter");
    run.state.zoom_step(false);
    std::fs::write(&path, record(0)).expect("the record");
    run.state.load_zoom(&path);
    run.state.zoom_step(true);
    assert_eq!(run.state.a11y.level, 1.5, "0 is 50");
    run.stop();
}

/// Super+= and Super+- step the magnifier, the window is sent neither key, and they act under a
/// shortcut inhibitor too (decision 30, `constraints::an_inhibitor_keeps_the_way_out`). While
/// the session is locked they do nothing.
#[test]
fn super_equal_and_minus_step_the_magnifier() {
    let mut run = Running::start(None);
    let (mut window, _) = focused_window(&mut run);
    let zoom_in = [KEY_LEFTMETA, KEY_EQUAL];
    let zoom_out = [KEY_LEFTMETA, KEY_MINUS];
    acts(
        &mut window,
        &mut run,
        &zoom_in,
        Some(Action::ZoomIn),
        "Super+=",
    );
    assert_eq!(run.state.a11y.level, 1.5);
    acts(
        &mut window,
        &mut run,
        &zoom_in,
        Some(Action::ZoomIn),
        "Super+=",
    );
    assert_eq!(run.state.a11y.level, 2.25);
    acts(
        &mut window,
        &mut run,
        &zoom_out,
        Some(Action::ZoomOut),
        "Super+-",
    );
    assert_eq!(run.state.a11y.level, 1.5);

    let (_locker, _lock) = lock_session(&mut run);
    let fired = run.state.fired.len();
    chord(&mut run.state, &zoom_in);
    chord(&mut run.state, &zoom_out);
    assert_eq!(run.state.fired.len(), fired, "locked, nothing fires");
    assert_eq!(run.state.a11y.level, 1.5, "and the level stays");
    run.stop();
}

/// Super with the scroll wheel zooms, five percent a notch, and the scroll is not sent on;
/// without Super, or with cosmic-comp's mouse shortcuts off, it does nothing to the level.
#[test]
fn super_scroll_zooms() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = bar.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = bar.bind(2);
    let device = manager.create_virtual_pointer(Some(&seat), &bar.queue.handle(), ());
    let notch = |bar: &mut Local, run: &mut Running, up: bool| {
        let (value, discrete) = if up { (-15.0, -1) } else { (15.0, 1) };
        device.axis_source(wl_pointer::AxisSource::Wheel);
        device.axis_discrete(0, wl_pointer::Axis::VerticalScroll, value, discrete);
        device.frame();
        bar.roundtrip(run);
    };

    notch(&mut bar, &mut run, true);
    assert_eq!(run.state.a11y.level, 1.0, "without Super");
    key(&mut run.state, KEY_LEFTMETA, true);
    notch(&mut bar, &mut run, true);
    assert!((run.state.a11y.level - 1.05).abs() < 1e-9, "a notch up with Super");
    notch(&mut bar, &mut run, true);
    assert!((run.state.a11y.level - 1.1025).abs() < 1e-9, "and another");
    notch(&mut bar, &mut run, false);
    assert!(
        (run.state.a11y.level - 1.05).abs() < 1e-9,
        "a notch down: {}",
        run.state.a11y.level
    );
    run.state.a11y.zoom.mouse_shortcuts = false;
    notch(&mut bar, &mut run, true);
    assert!(
        (run.state.a11y.level - 1.05).abs() < 1e-9,
        "the mouse shortcuts off"
    );
    key(&mut run.state, KEY_LEFTMETA, false);
    run.stop();
}

/// The magnified frame is scaled about the pointer: the window's red and blue reach points of
/// the output that are background at level 1.
#[test]
fn the_magnifier_scales_the_frame_about_the_pointer() {
    let mut run = Running::start(None);
    let middle = window_middle(&run);
    run.state.pointer_to(middle.to_f64(), InputTime::now());
    let w = OUTPUTS[0].width;
    let (above, below) = ((middle.x, middle.y - 80), (middle.x, middle.y + 80));
    let frame = run.state.capture(0).expect("frame of output 0");
    assert!(
        background(&pixel(&frame, w, above.0, above.1)),
        "above, at level 1"
    );
    assert!(
        background(&pixel(&frame, w, below.0, below.1)),
        "below, at level 1"
    );

    run.state.zoom_step(true);
    run.state.zoom_step(true);
    assert_eq!(run.state.a11y.level, 2.25);
    let frame = run.state.capture(0).expect("frame of output 0");
    assert_eq!(pixel(&frame, w, above.0, above.1), RED, "above, magnified");
    assert_eq!(pixel(&frame, w, below.0, below.1), BLUE, "below, magnified");
    run.stop();
}

/// The view follows the keyboard's focus (`doc_accessibility.md` section 5, item 8): with the
/// pointer in a corner, the keyboard going to the window brings its middle into view, as
/// `view_moves` brings the pointer; the next motion of the pointer takes the view back.
#[test]
fn the_magnifier_follows_the_keyboard_focus() {
    let mut run = Running::start(None);
    let middle = window_middle(&run);
    let w = OUTPUTS[0].width;
    let (above, below) = ((middle.x, middle.y - 80), (middle.x, middle.y + 80));
    run.state
        .pointer_to(Point::from((1.0, 1.0)), InputTime::now());
    run.state.zoom_step(true);
    run.state.zoom_step(true);
    let frame = run.state.capture(0).expect("frame of output 0");
    assert_ne!(pixel(&frame, w, above.0, above.1), RED, "the corner is magnified");

    let (window, _) = fixture_window(&run);
    run.state.set_keyboard_focus(None);
    run.state.focus(&window);
    // The view moves once the dispatch that moved the keyboard is done.
    run.event_loop
        .dispatch(Some(Duration::ZERO), &mut run.state)
        .expect("dispatch");
    let frame = run.state.capture(0).expect("frame of output 0");
    assert_eq!(pixel(&frame, w, above.0, above.1), RED, "above, the focus magnified");
    assert_eq!(pixel(&frame, w, below.0, below.1), BLUE, "below, the focus magnified");

    run.state
        .pointer_to(Point::from((2.0, 2.0)), InputTime::now());
    let frame = run.state.capture(0).expect("frame of output 0");
    assert_ne!(pixel(&frame, w, above.0, above.1), RED, "the pointer followed again");
    run.stop();
}

/// A click outside a popup that grabs the seat, with the magnifier on, ends the grab and gives
/// the keyboard back to the popup's root: Smithay moves the keyboard from inside the pointer's
/// button, and the magnifier reads the pointer's location only once that is done. Read inside,
/// it would lock the pointer the button holds, and the compositor would hang.
#[test]
fn a_click_outside_a_grabbing_popup_with_the_magnifier_on() {
    let mut run = Running::start(None);
    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        focus(state)
            .is_some_and(|surface| surface.client().map(|c| c.id()) == Some(shell_id.clone()))
    });
    let exclusive = focus(&run.state).expect("the exclusive surface");
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, shell_id.clone()));
    steps
        .send(Step::Popup(serial.into()))
        .expect("the shell's client");
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    run.until("the popup's grab takes the pointer", |_| {
        pointer.is_grabbed()
    });

    run.state.zoom_step(true);
    let corner = Point::from((
        f64::from(OUTPUTS[0].width - 2),
        f64::from(OUTPUTS[0].height - 2),
    ));
    run.state.pointer_to(corner, InputTime::now());
    // A hang is no failure the harness sees: the test ends the process if the click does not
    // come back.
    let (back, watch) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if watch.recv_timeout(TIMEOUT) == Err(mpsc::RecvTimeoutError::Timeout) {
            eprintln!("a_click_outside_a_grabbing_popup_with_the_magnifier_on: the click hung");
            std::process::exit(101);
        }
    });
    run.state
        .button_input(BTN_LEFT, ButtonState::Pressed, InputTime::now());
    drop(back);
    release(&mut run);
    run.until("the keyboard is back on the popup's root", |state| {
        focus(state).as_ref() == Some(&exclusive)
    });
    assert!(!pointer.is_grabbed(), "the click ends the grab");

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// A device that points at the screen names, while the magnifier is on, the point shown under
/// it: an absolute motion 90 pixels right of the pointer at level 2.25 moves it 40.
#[test]
fn an_absolute_motion_points_through_the_magnifier() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = bar.bind(1);
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = bar.bind(2);
    let device = manager.create_virtual_pointer(Some(&seat), &bar.queue.handle(), ());
    let area = run
        .state
        .output_areas()
        .into_iter()
        .reduce(|all, area| all.merge(area))
        .expect("the outputs");
    let (w, h) = (area.size.w as u32, area.size.h as u32);
    let mut point = |run: &mut Running, at: Point<i32, Logical>| {
        let local = at - area.loc;
        device.motion_absolute(0, local.x as u32, local.y as u32, w, h);
        device.frame();
        bar.roundtrip(run);
        run.state
            .seat
            .get_pointer()
            .expect("a pointer")
            .current_location()
    };
    let middle = window_middle(&run);
    let right = middle + Point::from((90, 0));
    assert_eq!(point(&mut run, right), right.to_f64(), "at level 1");

    run.state.pointer_to(middle.to_f64(), InputTime::now());
    run.state.zoom_step(true);
    run.state.zoom_step(true);
    // Drawn, so that the view follows the pointer.
    run.state.capture(0).expect("frame of output 0");
    assert_eq!(
        point(&mut run, right),
        (middle + Point::from((40, 0))).to_f64(),
        "at level 2.25"
    );
    run.stop();
}

/// The inversion and the filters are one pass on the frame shown: the window's red inverted is
/// cyan, and greyscale is its luma.
#[test]
fn the_filters_are_drawn_on_the_output() {
    use crate::a11y::Affine;
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let (manager, _log) = manager(&mut bar, &mut run);
    let middle = window_middle(&run);
    let w = OUTPUTS[0].width;
    let red = (middle.x, middle.y - HEIGHT / 4);

    manager.set_screen_filter(ActiveState::Enabled, Filter::None);
    bar.roundtrip(&mut run);
    let frame = run.state.capture(0).expect("frame of output 0");
    assert_eq!(
        pixel(&frame, w, red.0, red.1),
        [0, 255, 255, 255],
        "inverted"
    );

    manager.set_screen_filter(ActiveState::Disabled, Filter::Greyscale);
    bar.roundtrip(&mut run);
    let frame = run.state.capture(0).expect("frame of output 0");
    let grey = pixel(&frame, w, red.0, red.1);
    let expected = Affine::of(&run.state.a11y.screen)
        .expect("a pass")
        .apply(RED);
    assert_eq!(grey, expected, "greyscale");
    assert!(
        grey[0] == grey[1] && grey[1] == grey[2] && grey[0] < 255,
        "{grey:?}"
    );

    manager.set_screen_filter(ActiveState::Disabled, Filter::None);
    bar.roundtrip(&mut run);
    let frame = run.state.capture(0).expect("frame of output 0");
    assert_eq!(pixel(&frame, w, red.0, red.1), RED, "off");
    run.stop();
}

/// A capture is the screen's contents, not the user's view of it: with the inversion and the
/// magnifier on, a capture of the output holds the window's red where it is unmagnified.
#[test]
fn a_capture_has_neither_the_filter_nor_the_magnifier() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let (manager, _log) = manager(&mut bar, &mut run);
    let middle = window_middle(&run);
    run.state.pointer_to(middle.to_f64(), InputTime::now());
    manager.set_screen_filter(ActiveState::Enabled, Filter::None);
    manager.zoom_in();
    manager.zoom_in();
    bar.roundtrip(&mut run);
    let shm: wl_shm::WlShm = bar.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = bar.bind(1);
    let (_source, session) = output_session(&mut bar, &mut run, &copy, 0);
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    let (buffer, file) = bar.buffer(&shm, w, h, [0x55; 4]);
    let frame = session.create_frame(&bar.queue.handle(), ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, w, h);
    frame.capture();
    bar.pump(&mut run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(bar.client.captured, Some(Ok(())), "the frame is ready");
    let captured = contents(&file, (w * h * 4) as usize);
    let at = |x: i32, y: i32| {
        let [b, g, r, a] = pixel(&captured, w, x, y);
        [r, g, b, a]
    };
    assert_eq!(at(middle.x, middle.y - HEIGHT / 4), RED, "not inverted");
    assert!(background(&at(middle.x, middle.y - 80)), "not magnified");
    let shown = run.state.capture(0).expect("frame of output 0");
    assert_ne!(
        pixel(&shown, w, middle.x, middle.y - HEIGHT / 4),
        RED,
        "the output itself is"
    );
    run.stop();
}

/// The inversion and the filter are read from cosmic-comp's file and written back to it in its
/// format at each change; a filter this compositor does not name is sent as unknown, not drawn,
/// and kept when only the inversion changes.
#[test]
fn the_filter_is_stored_as_cosmic_comp_stores_it() {
    let dir = scratch("stored");
    let path = dir.join("a11y_screen_filter.ron");
    std::fs::write(
        &path,
        "(\n    inverted: true,\n    color_filter: Some(Greyscale),\n)",
    )
    .expect("cosmic-comp's file");
    let mut run = Running::start(None);
    run.state.load_screen_filter(path.clone());
    let mut bar = Local::connect(&mut run, None);
    let (manager, log) = manager(&mut bar, &mut run);
    assert_eq!(
        taken(&log)[1],
        Seen::Filter(1, Filter::Greyscale as u32),
        "read at start"
    );

    manager.set_screen_filter(ActiveState::Disabled, Filter::Protanopia);
    bar.roundtrip(&mut run);
    assert_eq!(taken(&log), [Seen::Filter(0, Filter::Protanopia as u32)]);
    let stored = std::fs::read_to_string(&path).expect("the file");
    let expected = ScreenFilter {
        inverted: false,
        filter: Some("Protanopia".into()),
    };
    assert_eq!(stored, filter_text(&expected), "cosmic-comp's format");
    assert_eq!(parse_filter(&stored).expect("readable"), expected);

    std::fs::write(&path, "(inverted: false, color_filter: Some(Sepia))").expect("a newer name");
    run.state.load_screen_filter(path.clone());
    let (_again, again) = self::manager(&mut bar, &mut run);
    assert_eq!(
        taken(&again)[1],
        Seen::Filter(0, Filter::Unknown as u32),
        "unknown"
    );
    assert!(
        crate::a11y::Affine::of(&run.state.a11y.screen).is_none(),
        "not drawn"
    );
    manager.set_screen_filter(ActiveState::Enabled, Filter::Unknown);
    bar.roundtrip(&mut run);
    let stored = std::fs::read_to_string(&path).expect("the file");
    assert_eq!(
        parse_filter(&stored).expect("readable"),
        ScreenFilter {
            inverted: true,
            filter: Some("Sepia".into()),
        },
        "the unknown filter kept"
    );
    run.stop();
}

/// A file that cannot be read changes nothing and is not written over until the user changes
/// the filter.
#[test]
fn a_malformed_filter_file_is_left_alone() {
    let dir = scratch("malformed");
    let path = dir.join("a11y_screen_filter.ron");
    std::fs::write(&path, "(inverted: maybe)").expect("a malformed file");
    let mut run = Running::start(None);
    run.state.load_screen_filter(path.clone());
    assert_eq!(
        run.state.a11y.screen,
        ScreenFilter::default(),
        "nothing changes"
    );
    run.state.zoom_step(true);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file"),
        "(inverted: maybe)",
        "not written"
    );
    let mut bar = Local::connect(&mut run, None);
    let (manager, _log) = manager(&mut bar, &mut run);
    manager.set_screen_filter(ActiveState::Enabled, Filter::Unknown);
    bar.roundtrip(&mut run);
    assert_eq!(
        parse_filter(&std::fs::read_to_string(&path).expect("the file")).expect("readable"),
        ScreenFilter {
            inverted: true,
            filter: None,
        },
        "written once the user changes it"
    );
    run.stop();
}

/// A slow or bounce keys delay above 2000 ms is the protocol error invalid_value; 2000 is not.
#[test]
fn a_delay_above_two_seconds_is_a_protocol_error() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let (manager, _log) = manager(&mut bar, &mut run);
    manager.set_slow_keys(ActiveState::Enabled, 2000);
    manager.set_bounce_keys(ActiveState::Enabled, 2000);
    bar.roundtrip(&mut run);
    assert!(bar.connection.protocol_error().is_none(), "2000 is valid");
    manager.set_bounce_keys(ActiveState::Enabled, 2001);
    bar.connection.flush().expect("flush");
    let started = Instant::now();
    let error = loop {
        if let Some(error) = bar.connection.protocol_error() {
            break error;
        }
        assert!(started.elapsed() < TIMEOUT, "the error: not in time");
        if let Some(guard) = bar.queue.prepare_read() {
            drop(guard.read());
        }
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    };
    assert_eq!(error.object_interface, MANAGER);
    assert_eq!(
        error.code,
        athanor_a11y_manager_v1::Error::InvalidValue as u32
    );
    run.stop();
}

/// A manager bound while the session is locked is not sent the modifiers.
#[test]
fn no_modifiers_are_sent_while_locked() {
    let mut run = Running::start(None);
    let (_locker, _lock) = lock_session(&mut run);
    let mut bar = Local::connect(&mut run, None);
    let (_manager, log) = manager(&mut bar, &mut run);
    assert_eq!(
        taken(&log),
        AT_BIND[..2],
        "the magnifier and the filter only"
    );
    run.stop();
}
