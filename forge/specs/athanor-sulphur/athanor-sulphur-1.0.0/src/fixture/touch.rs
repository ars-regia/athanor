//! Fixtures of wl_touch (doc_own_stack.md decision 33, wave 2a). libinput cannot be driven here,
//! so the tests call the `State` methods the touch events end in.

use super::devices::show_lock_surface;
use super::*;
use crate::grabs::{MoveGrab, ResizeGrab};
use smithay::backend::input::TouchSlot;
use smithay::utils::Transform;
use std::sync::atomic::AtomicBool;
use wayland_client::protocol::{wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_touch};

/// The touch grab start, the parameter of a window grab that follows a finger.
type TouchStart = smithay::input::touch::GrabStartData<State>;

/// What a client's wl_touch was sent.
#[derive(Default)]
pub(super) struct Seen {
    /// Each down: the surface, the touch point, where on the surface, and its serial.
    downs: Vec<(ObjectId, i32, (f64, f64), u32)>,
    /// Each motion: the touch point and where on its surface.
    motions: Vec<(i32, (f64, f64))>,
    /// The touch point of each up.
    ups: Vec<i32>,
    frames: usize,
    cancels: usize,
}

impl Dispatch<wl_touch::WlTouch, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let seen = &mut client.touch;
        match event {
            wl_touch::Event::Down {
                serial,
                surface,
                id,
                x,
                y,
                ..
            } => seen.downs.push((surface.id(), id, (x, y), serial)),
            wl_touch::Event::Motion { id, x, y, .. } => seen.motions.push((id, (x, y))),
            wl_touch::Event::Up { id, .. } => seen.ups.push(id),
            wl_touch::Event::Frame => seen.frames += 1,
            wl_touch::Event::Cancel => seen.cancels += 1,
            _ => {}
        }
    }
}

fn slot(id: u32) -> TouchSlot {
    TouchSlot::from(Some(id))
}

fn down(run: &mut Running, id: u32, at: Point<f64, Logical>) {
    run.state.touch_down(slot(id), at, InputTime::now());
    run.state.touch_frame();
}

fn motion(run: &mut Running, id: u32, at: Point<f64, Logical>) {
    run.state.touch_motion(slot(id), at, InputTime::now());
    run.state.touch_frame();
}

fn up(run: &mut Running, id: u32) {
    run.state.touch_up(slot(id), InputTime::now());
    run.state.touch_frame();
}

/// Moves the fixture's own window out of the way, to the bottom left of the first output.
fn park_the_fixture(run: &mut Running) {
    let (window, _) = fixture_window(run);
    run.state.space.map_element(window, (40, 600), false);
}

/// A new client with a window at `at`, on top, and the seat's touch bound: the window's
/// toplevel and surface.
fn window_at(
    run: &mut Running,
    at: (i32, i32),
) -> (Local, xdg_toplevel::XdgToplevel, wl_surface::WlSurface) {
    let mut local = Local::connect(run, None);
    let toplevel = local.toplevel(run);
    let seat = local.seat.clone().expect("a seat");
    let _touch = seat.get_touch(&local.queue.handle(), ());
    let window = window_of(run, local.id.as_ref().expect("an id"));
    run.state.space.map_element(window, at, false);
    local.roundtrip(run);
    let (surface, _) = local.window.clone().expect("a window");
    (local, toplevel, surface)
}

/// A locker with its touch, the session locked and its surface shown on the first output.
fn lock_with_touch(run: &mut Running) -> (Local, ext_session_lock_v1::ExtSessionLockV1) {
    let mut locker = Local::connect(run, None);
    let compositor: wl_compositor::WlCompositor = locker.bind(4);
    let output: wl_output::WlOutput = locker.bind(4);
    let seat: wl_seat::WlSeat = locker.bind(1);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let qh = locker.queue.handle();
    let _touch = seat.get_touch(&qh, ());
    let lock = locks.lock(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    locker.roundtrip(run);
    show_lock_surface(run, &mut locker, &lock, &surface, &output);
    (locker, lock)
}

/// Each finger goes to the surface under it, not to the surface of the first finger, with its
/// own position there; its motion and its release go to the surface it touched.
#[test]
fn a_touch_reaches_the_surface_under_each_finger() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let (mut a, _, a_surface) = window_at(&mut run, (300, 200));
    let (mut b, _, b_surface) = window_at(&mut run, (700, 200));

    down(&mut run, 0, (320.0, 210.0).into());
    down(&mut run, 1, (730.0, 240.0).into());
    a.pump(&mut run, "the first finger", |client, _| {
        !client.touch.downs.is_empty() && client.touch.frames > 0
    });
    b.pump(&mut run, "the second finger", |client, _| {
        !client.touch.downs.is_empty() && client.touch.frames > 0
    });
    assert_eq!(
        a.client.touch.downs.len(),
        1,
        "one finger on the first window"
    );
    let (surface, id, at, _) = a.client.touch.downs[0].clone();
    assert_eq!((surface, id, at), (a_surface.id(), 0, (20.0, 10.0)));
    let (surface, id, at, _) = b.client.touch.downs[0].clone();
    assert_eq!((surface, id, at), (b_surface.id(), 1, (30.0, 40.0)));

    motion(&mut run, 1, (740.0, 250.0).into());
    b.pump(&mut run, "the second finger moves", |client, _| {
        !client.touch.motions.is_empty()
    });
    assert_eq!(b.client.touch.motions, [(1, (40.0, 50.0))]);
    up(&mut run, 1);
    up(&mut run, 0);
    a.pump(&mut run, "the first finger is lifted", |client, _| {
        !client.touch.ups.is_empty()
    });
    b.pump(&mut run, "the second finger is lifted", |client, _| {
        !client.touch.ups.is_empty()
    });
    assert_eq!(a.client.touch.ups, [0]);
    assert_eq!(b.client.touch.ups, [1]);
    assert!(
        a.client.touch.motions.is_empty(),
        "no motion of another finger"
    );
    run.stop();
}

/// A touch-down gives the window under the finger the keyboard, as a button press does; a
/// finger after the first, as a second button, changes no focus.
#[test]
fn a_touch_down_focuses_the_window_under_the_finger() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let (a, _, _) = window_at(&mut run, (300, 200));
    let (b, _, _) = window_at(&mut run, (700, 200));
    let focused = |run: &Running| keyboard_focus(run).and_then(|surface| surface.client()).map(|client| client.id());
    run.state.set_keyboard_focus(None);

    down(&mut run, 0, (320.0, 210.0).into());
    assert_eq!(focused(&run), a.id, "the touched window");
    down(&mut run, 1, (720.0, 210.0).into());
    assert_eq!(focused(&run), a.id, "a second finger keeps it");
    up(&mut run, 0);
    up(&mut run, 1);
    down(&mut run, 0, (720.0, 210.0).into());
    assert_eq!(focused(&run), b.id, "a new touch moves it");
    run.stop();
}

/// While the session is locked, a touch where no lock surface is reaches no client, neither the
/// window under it nor the lock, and the seat holds nothing for it; its motion and its release
/// reach no one either. A touch on the lock surface reaches it.
#[test]
fn a_touch_off_the_lock_surface_reaches_nothing() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let second = run
        .state
        .space
        .output_geometry(&output_at(&run, 1))
        .expect("placed");
    let (mut window, _, _) = window_at(&mut run, (second.loc.x + 20, second.loc.y + 20));
    let (mut locker, _lock) = lock_with_touch(&mut run);
    let off: Point<f64, Logical> = (
        f64::from(second.loc.x) + 40.0,
        f64::from(second.loc.y) + 30.0,
    )
        .into();
    let on: Point<f64, Logical> = (100.0, 100.0).into();
    assert!(
        run.state.surface_under(off).is_none(),
        "no lock surface on the second output"
    );
    assert!(
        run.state.surface_under(on).is_some(),
        "the lock surface on the first output"
    );

    down(&mut run, 0, off);
    let touch = run.state.seat.get_touch().expect("a touch");
    assert!(
        !touch.is_grabbed(),
        "the seat holds nothing for the refused touch"
    );
    motion(&mut run, 0, off + Point::from((5.0, 5.0)));
    up(&mut run, 0);
    down(&mut run, 1, on);
    locker.pump(&mut run, "the touch on the lock surface", |client, _| {
        !client.touch.downs.is_empty()
    });
    up(&mut run, 1);
    locker.pump(&mut run, "its release", |client, _| {
        !client.touch.ups.is_empty()
    });
    window.roundtrip(&mut run);

    let seen = &window.client.touch;
    assert!(seen.downs.is_empty() && seen.motions.is_empty() && seen.ups.is_empty());
    assert_eq!(seen.frames, 0, "the window was sent nothing");
    let seen = &locker.client.touch;
    let ids: Vec<i32> = seen.downs.iter().map(|(_, id, _, _)| *id).collect();
    assert_eq!(ids, [1], "the lock had the touch on it alone");
    assert_eq!(seen.ups, [1]);
    assert!(seen.motions.is_empty(), "no motion of the refused touch");
    run.stop();
}

/// A finger held on a window when the session locks: the window is sent `cancel`, and the
/// finger's later motion and release reach neither the window nor the lock surface.
#[test]
fn a_touch_held_at_lock_is_cancelled_and_its_release_reaches_no_one() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let (mut window, _, _) = window_at(&mut run, (300, 200));
    down(&mut run, 0, (320.0, 210.0).into());
    window.pump(&mut run, "the touch", |client, _| {
        !client.touch.downs.is_empty()
    });

    let (mut locker, _lock) = lock_with_touch(&mut run);
    window.roundtrip(&mut run);
    let motions = window.client.touch.motions.len();
    motion(&mut run, 0, (330.0, 220.0).into());
    up(&mut run, 0);
    window.roundtrip(&mut run);
    locker.roundtrip(&mut run);

    assert!(
        window.client.touch.ups.is_empty(),
        "the release does not reach the window"
    );
    assert_eq!(window.client.touch.motions.len(), motions, "nor its motion");
    assert_eq!(
        window.client.touch.cancels, 1,
        "the lock cancelled the touch"
    );
    let seen = &locker.client.touch;
    assert!(seen.downs.is_empty() && seen.ups.is_empty() && seen.motions.is_empty());
    assert!(!run.state.seat.get_touch().expect("a touch").is_grabbed());
    run.stop();
}

/// A touchscreen spans the output libinput names for it, under that output's own transform,
/// never the first output; with no name, the built-in panel; with neither, no output at all.
#[test]
fn a_touchscreen_maps_to_its_own_output() {
    let mut run = Running::start(None);
    let second = output_at(&run, 1);
    second.change_current_state(None, Some(Transform::_180), None, None);
    run.state.arrange_outputs();
    let area = run.state.space.output_geometry(&second).expect("placed");
    let corner = |dx: i32, dy: i32| -> Point<f64, Logical> {
        (
            f64::from(area.loc.x + area.size.w - dx),
            f64::from(area.loc.y + area.size.h - dy),
        )
            .into()
    };
    let named = run
        .state
        .touch_location(Some("headless-1"), |_| (10.0, 20.0).into());
    assert_eq!(named, Some(corner(10, 20)), "on the second output, turned");
    let origin = run
        .state
        .touch_location(Some("headless-1"), |_| (0.0, 0.0).into());
    assert_eq!(origin, Some(corner(1, 1)), "kept on it");

    assert_eq!(
        run.state.touch_location(None, |_| (0.0, 0.0).into()),
        None,
        "no built-in panel"
    );
    assert_eq!(
        run.state
            .touch_location(Some("DP-9"), |_| (0.0, 0.0).into()),
        None,
        "an output not shown"
    );
    let panel = crate::state::build_output(
        "eDP-1",
        &crate::state::placeholder_identity("fixture"),
        (800, 600).into(),
        60_000,
        Transform::Normal,
        1.0,
    );
    run.state.map_output(&panel);
    let built_in = run.state.space.output_geometry(&panel).expect("placed");
    let point = run.state.touch_location(None, |_| (5.0, 6.0).into());
    assert_eq!(point, Some(built_in.loc.to_f64() + Point::from((5.0, 6.0))));
    run.stop();
}

/// Whether the seat's touch is grabbed by `G`.
fn touch_grab_is<G: smithay::input::touch::TouchGrab<State>>(run: &Running) -> bool {
    run.state
        .seat
        .get_touch()
        .and_then(|touch| touch.with_grab(|_, grab| grab.is::<G>()))
        .unwrap_or(false)
}

/// A window moves with the finger from a touch its client was sent, as from a press, and
/// resizes from one; a serial no touch sent it is refused. The release reaches the client.
#[test]
fn a_touch_moves_and_resizes_a_window() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let (mut local, toplevel, _) = window_at(&mut run, (300, 200));
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");

    toplevel._move(&seat, SERIAL_COUNTER.next_serial().into());
    local.roundtrip(&mut run);
    assert!(
        !touch_grab_is::<MoveGrab<TouchStart>>(&run),
        "a serial no touch sent"
    );

    down(&mut run, 0, (320.0, 210.0).into());
    local.pump(&mut run, "the touch", |client, _| {
        !client.touch.downs.is_empty()
    });
    let serial = local.client.touch.downs[0].3;
    toplevel._move(&seat, serial);
    local.roundtrip(&mut run);
    assert!(
        touch_grab_is::<MoveGrab<TouchStart>>(&run),
        "the client's own touch moves it"
    );
    motion(&mut run, 0, (370.0, 230.0).into());
    assert_eq!(
        location_of(&run, &id),
        Point::from((350, 220)),
        "the window follows"
    );
    up(&mut run, 0);
    local.pump(&mut run, "the release", |client, _| {
        !client.touch.ups.is_empty()
    });
    assert!(
        !touch_grab_is::<MoveGrab<TouchStart>>(&run),
        "the release ends the move"
    );
    motion(&mut run, 0, (420.0, 260.0).into());
    assert_eq!(
        location_of(&run, &id),
        Point::from((350, 220)),
        "and it stays"
    );

    local.client.touch.downs.clear();
    down(&mut run, 0, (370.0, 230.0).into());
    local.pump(&mut run, "a new touch", |client, _| {
        !client.touch.downs.is_empty()
    });
    let serial = local.client.touch.downs[0].3;
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    local.roundtrip(&mut run);
    assert!(
        touch_grab_is::<ResizeGrab<TouchStart>>(&run),
        "the client's own touch resizes it"
    );
    motion(&mut run, 0, (400.0, 250.0).into());
    let (width, height, _) = local.redraw(&mut run);
    assert_eq!((width, height), (WIDTH + 30, HEIGHT + 20));
    up(&mut run, 0);
    local.roundtrip(&mut run);
    assert!(
        !touch_grab_is::<ResizeGrab<TouchStart>>(&run),
        "the release ends the resize"
    );
    run.stop();
}

/// Whether a drag source was cancelled.
type Cancelled = Arc<AtomicBool>;

impl Dispatch<wl_data_source::WlDataSource, Cancelled> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        cancelled: &Cancelled,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_source::Event::Cancelled = event {
            cancelled.store(true, Ordering::SeqCst);
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for Client {
    fn event(
        _: &mut Self,
        _: &wl_data_device::WlDataDevice,
        _: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(Client, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

delegate_noop!(Client: ignore wl_data_offer::WlDataOffer);

/// A drag starts from a touch its client was sent, as from a press, while any finger is still
/// down: the seat takes it, and, as for a press, cancels the source (drag and drop is not offered
/// yet). A serial no touch sent is refused, and the source is left alone.
#[test]
fn a_drag_starts_from_a_touch() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let (mut local, _, surface) = window_at(&mut run, (300, 200));
    let seat = local.seat.clone().expect("a seat");
    let qh = local.queue.handle();
    let manager: wl_data_device_manager::WlDataDeviceManager = local.bind(3);
    let device = manager.get_data_device(&seat, &qh, ());
    let drag = |local: &mut Local, run: &mut Running, serial: u32| -> bool {
        let cancelled = Cancelled::default();
        let source = manager.create_data_source(&local.queue.handle(), cancelled.clone());
        source.offer("text/plain".into());
        device.start_drag(Some(&source), &surface, None, serial);
        local.roundtrip(run);
        cancelled.load(Ordering::SeqCst)
    };

    down(&mut run, 0, (320.0, 210.0).into());
    down(&mut run, 1, (340.0, 220.0).into());
    local.pump(&mut run, "the touches", |client, _| client.touch.downs.len() == 2);
    let serial = local.client.touch.downs[0].3;
    assert!(!drag(&mut local, &mut run, serial.wrapping_sub(1)), "a serial no touch sent");
    up(&mut run, 1);
    assert!(drag(&mut local, &mut run, serial), "the first touch, held, starts a drag");
    up(&mut run, 0);
    assert!(!drag(&mut local, &mut run, serial), "the lifted touch no longer does");
    run.stop();
}
