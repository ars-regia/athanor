//! Decision 37 of `doc_own_stack.md`: a toplevel attached to a drag follows the pointer or the
//! touch point at the client's offset, takes no part in choosing the drop target, is detached
//! when it unmaps, keeps its border, and stays where a lock's cancel leaves it; the protocol's
//! three errors are raised as it defines them (`toplevel_drag.rs`).

use super::super::devices::protocol_error;
use super::*;
use wayland_protocols::xdg::toplevel_drag::v1::client::{
    xdg_toplevel_drag_manager_v1::XdgToplevelDragManagerV1, xdg_toplevel_drag_v1::XdgToplevelDragV1,
};

delegate_noop!(Client: XdgToplevelDragManagerV1);
delegate_noop!(Client: XdgToplevelDragV1);

/// A toplevel drag of a new source, with the manager that made it.
struct Carry {
    manager: XdgToplevelDragManagerV1,
    drag: XdgToplevelDragV1,
    source: wl_data_source::WlDataSource,
    log: DragSource,
}

impl Party {
    /// A new source of `TEXT` and a toplevel drag made of it.
    fn carry(&mut self, run: &mut Running) -> Carry {
        let manager: XdgToplevelDragManagerV1 = self.local.bind(1);
        let (source, log) = self.source();
        let drag = manager.get_xdg_toplevel_drag(&source, &self.local.queue.handle(), ());
        self.local.roundtrip(run);
        Carry {
            manager,
            drag,
            source,
            log,
        }
    }

    /// Starts the drag of `source` from the client's window, with `serial` and no icon.
    fn start(&mut self, run: &mut Running, source: &wl_data_source::WlDataSource, serial: u32) {
        self.device
            .start_drag(Some(source), &self.surface, None, serial);
        self.local.roundtrip(run);
    }
}

/// Where the window of the client `id` whose surface is `surface` is in the space.
fn location_of_surface(
    run: &Running,
    id: &ClientId,
    surface: &wl_surface::WlSurface,
) -> Point<i32, Logical> {
    let window = run
        .state
        .space
        .elements()
        .find(|window| {
            owned_by(window, id)
                && window.toplevel().is_some_and(|toplevel| {
                    toplevel.wl_surface().id().protocol_id() == surface.id().protocol_id()
                })
        })
        .expect("the window");
    run.state
        .space
        .element_location(window)
        .expect("a location")
}

/// The window follows the pointer at the offset, by an absolute motion and by a relative one,
/// stays where the drop leaves it, and no longer follows; its drag object may go once the drag
/// has ended.
#[test]
fn an_attached_toplevel_follows_the_pointer() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);

    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());
    assert_eq!(
        location_of(&run, &from.id),
        (580, 380).into(),
        "at the pointer, less the offset"
    );
    let by = Point::from((10.0, 5.0));
    run.state.pointer_moved_by(by, by, InputTime::now());
    assert_eq!(
        location_of(&run, &from.id),
        (590, 385).into(),
        "a relative motion"
    );

    release(&mut run);
    from.local.roundtrip(&mut run);
    assert!(carry.log.sourced().cancelled, "dropped on nothing");
    run.state
        .pointer_to((900.0, 600.0).into(), InputTime::now());
    assert_eq!(
        location_of(&run, &from.id),
        (590, 385).into(),
        "where the drop left it"
    );
    carry.drag.destroy();
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "a drag object destroyed after its drag"
    );
    run.stop();
}

/// A toplevel attached after the drag started follows the touch point the drag follows.
#[test]
fn an_attached_toplevel_follows_a_touch() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let seat = from.local.seat.clone().expect("a seat");
    let _touch = seat.get_touch(&from.local.queue.handle(), ());
    from.local.roundtrip(&mut run);
    down(&mut run, 0, (120.0, 120.0).into());
    from.local.pump(&mut run, "the touch", |client, _| {
        client.touch.downs.len() == 1
    });
    let serial = from.local.client.touch.downs[0].3;
    let carry = from.carry(&mut run);
    from.start(&mut run, &carry.source, serial);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.local.roundtrip(&mut run);

    motion(&mut run, 0, (600.0, 400.0).into());
    assert_eq!(
        location_of(&run, &from.id),
        (580, 380).into(),
        "at the finger, less the offset"
    );
    up(&mut run, 0);
    from.local.roundtrip(&mut run);
    assert!(carry.log.sourced().cancelled, "dropped on nothing");
    assert_eq!(location_of(&run, &from.id), (580, 380).into(), "it stays");
    run.stop();
}

/// The protocol's workflow: a toplevel made during the drag and attached before its map is
/// mapped at the pointer, less the offset, and follows it.
#[test]
fn a_toplevel_attached_before_its_map_lands_at_the_pointer() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    from.start(&mut run, &carry.source, serial);
    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());

    let compositor: wl_compositor::WlCompositor = from.local.bind(4);
    let shm: wl_shm::WlShm = from.local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = from.local.bind(1);
    let qh = from.local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    carry.drag.attach(&toplevel, 30, 10);
    surface.commit();
    let configure = from.local.first_configure(&mut run, &xdg);
    xdg.ack_configure(configure);
    let (buffer, _) = from.local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    from.local.roundtrip(&mut run);
    assert_eq!(
        location_of_surface(&run, &from.id, &surface),
        (570, 390).into(),
        "mapped at the pointer, less the offset"
    );
    run.state
        .pointer_to((700.0, 500.0).into(), InputTime::now());
    assert_eq!(
        location_of_surface(&run, &from.id, &surface),
        (670, 490).into(),
        "it follows"
    );
    release(&mut run);
    run.stop();
}

/// Another client's window under the attached one, and under the pointer, is the target: the
/// attached window is never entered.
#[test]
fn the_drop_target_is_chosen_without_the_attached_window() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut to = party(&mut run, (500, 100), None);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);

    // The attached window, on top, now covers the pointer and the other window there.
    run.state
        .pointer_to((550.0, 150.0).into(), InputTime::now());
    assert_eq!(location_of(&run, &from.id), (530, 130).into());
    let (surface, _, _) = to.entered(&mut run, 1);
    assert_eq!(
        surface,
        to.surface.id(),
        "the window under the attached one"
    );
    from.local.roundtrip(&mut run);
    assert!(
        from.heard
            .heard()
            .entered
            .iter()
            .all(|(entered, _, _)| *entered != from.surface.id()),
        "the attached window is never a target"
    );
    release(&mut run);
    run.stop();
}

/// A window unmapped during the drag is detached from it: another toplevel can then be
/// attached, and follows the pointer.
#[test]
fn a_window_unmapped_during_the_drag_is_detached() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);
    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());
    assert_eq!(location_of(&run, &from.id), (580, 380).into());

    from.surface.attach(None, 0, 0);
    from.surface.commit();
    from.local.roundtrip(&mut run);
    let second = from.local.toplevel(&mut run);
    let (surface, _) = from.local.window.clone().expect("a window");
    carry.drag.attach(&second, 20, 20);
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "no toplevel is attached once the first unmapped"
    );
    run.state
        .pointer_to((700.0, 500.0).into(), InputTime::now());
    assert_eq!(
        location_of_surface(&run, &from.id, &surface),
        (680, 480).into(),
        "the toplevel attached since follows"
    );
    release(&mut run);
    run.stop();
}

/// A lock request cancels the drag: the client is told, and the window stays where the drag
/// last placed it.
#[test]
fn a_lock_request_cancels_the_toplevel_drag() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);
    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());
    assert_eq!(location_of(&run, &from.id), (580, 380).into());

    let mut locker = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let _lock = locks.lock(&locker.queue.handle(), ());
    locker.roundtrip(&mut run);
    from.local.roundtrip(&mut run);
    assert!(carry.log.sourced().cancelled, "the client is told");
    run.state
        .pointer_to((900.0, 600.0).into(), InputTime::now());
    assert_eq!(
        location_of(&run, &from.id),
        (580, 380).into(),
        "where the drag last placed it"
    );
    carry.drag.destroy();
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "the drag ended at the cancel"
    );
    release(&mut run);
    run.stop();
}

/// A confined client's window keeps its border at each place the drag takes it to.
#[test]
fn an_attached_window_keeps_its_border() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let context = sandbox(&run);
    let mut from = party(&mut run, (100, 100), Some(context));
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);
    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());
    assert_eq!(location_of(&run, &from.id), (580, 380).into());

    let shown = frame(&mut run);
    let middle = 380 + HEIGHT / 2;
    assert_eq!(at(&shown, 578, middle), WHITE, "the white line");
    assert_eq!(at(&shown, 577, middle), BLACK, "the outer line");
    assert!(background(&at(&shown, 98, 150)), "no border where it was");
    release(&mut run);
    run.stop();
}

/// The `invalid_source` of `local`'s manager.
fn invalid_source(run: &mut Running, local: &mut Local) {
    let error = protocol_error(run, local);
    assert_eq!(
        (error.code, error.object_interface.as_str()),
        (0, "xdg_toplevel_drag_manager_v1"),
        "{}",
        error.message
    );
}

/// A source set as the selection is not one for a drag and drop.
#[test]
fn a_selected_source_is_an_invalid_source() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let (source, _log) = from.source();
    from.device.set_selection(Some(&source), serial);
    let manager: XdgToplevelDragManagerV1 = from.local.bind(1);
    let _drag = manager.get_xdg_toplevel_drag(&source, &from.local.queue.handle(), ());
    invalid_source(&mut run, &mut from.local);
    release(&mut run);
    run.stop();
}

/// The source of a toplevel drag set as the selection.
#[test]
fn the_source_of_a_toplevel_drag_is_not_a_selection() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    from.device.set_selection(Some(&carry.source), serial);
    invalid_source(&mut run, &mut from.local);
    release(&mut run);
    run.stop();
}

/// A source serves one toplevel drag, even once the drag object that had it is gone.
#[test]
fn a_source_serves_one_toplevel_drag() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let carry = from.carry(&mut run);
    carry.drag.destroy();
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "a drag object whose drag never started"
    );
    let _drag = carry
        .manager
        .get_xdg_toplevel_drag(&carry.source, &from.local.queue.handle(), ());
    invalid_source(&mut run, &mut from.local);
    run.stop();
}

/// A second toplevel attached while the first still has its role.
#[test]
fn a_second_toplevel_is_refused_while_one_is_attached() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    let second = from.local.toplevel(&mut run);
    carry.drag.attach(&second, 20, 20);
    let error = protocol_error(&mut run, &mut from.local);
    assert_eq!(
        (error.code, error.object_interface.as_str()),
        (0, "xdg_toplevel_drag_v1"),
        "toplevel_attached: {}",
        error.message
    );
    run.stop();
}

/// The drag object destroyed while its drag goes on.
#[test]
fn a_toplevel_drag_is_kept_until_its_drag_ends() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);
    carry.drag.destroy();
    let error = protocol_error(&mut run, &mut from.local);
    // libwayland-client names no object for an error on one its client destroyed, and the drag
    // object is the one this client destroyed: its error 1 is `ongoing_drag`.
    assert_eq!(
        (error.code, error.object_id, error.object_interface.as_str()),
        (1, 0, ""),
        "ongoing_drag"
    );
    release(&mut run);
    run.stop();
}

/// A full-screen window attached to its own drag is no drop target either, though it is lifted
/// above the top layers and its black hides what is below it.
#[test]
fn an_attached_full_screen_window_is_not_the_target() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut to = party(&mut run, (500, 100), None);
    let mut from = party(&mut run, (100, 100), None);
    from.local.roundtrip(&mut run);
    from.local.client.configure = None;
    from.toplevel.set_fullscreen(None);
    from.local.redraw(&mut run);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);

    run.state
        .pointer_to((550.0, 150.0).into(), InputTime::now());
    let (surface, _, _) = to.entered(&mut run, 1);
    assert_eq!(surface, to.surface.id(), "the window below the full-screen one");
    from.local.roundtrip(&mut run);
    assert!(
        from.heard
            .heard()
            .entered
            .iter()
            .all(|(entered, _, _)| *entered != from.surface.id()),
        "the attached window is never a target"
    );
    release(&mut run);
    run.stop();
}

/// A toplevel destroyed during the drag is no error, and nothing of it is kept once the drag
/// has ended.
#[test]
fn a_toplevel_destroyed_during_the_drag_is_forgotten() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);
    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());
    assert_eq!(location_of(&run, &from.id), (580, 380).into());

    from.toplevel.destroy();
    from.local.roundtrip(&mut run);
    run.state
        .pointer_to((700.0, 500.0).into(), InputTime::now());
    release(&mut run);
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "a toplevel destroyed while attached"
    );
    let [drags, attached, started, _, _] = run.state.toplevel_drags.sizes();
    assert_eq!((drags, attached, started), (1, 0, 0), "the drag object alone");
    carry.drag.destroy();
    from.local.roundtrip(&mut run);
    assert!(from.local.connection.protocol_error().is_none());
    assert_eq!(run.state.toplevel_drags.sizes()[0], 0, "no drag object");
    run.stop();
}

/// A source destroyed during the drag ends nothing in error, and the drag in progress is
/// forgotten once it has ended.
#[test]
fn a_source_destroyed_during_the_drag_is_forgotten() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let carry = from.carry(&mut run);
    carry.drag.attach(&from.toplevel, 20, 20);
    from.start(&mut run, &carry.source, serial);
    run.state
        .pointer_to((600.0, 400.0).into(), InputTime::now());
    assert_eq!(location_of(&run, &from.id), (580, 380).into());

    carry.source.destroy();
    from.local.roundtrip(&mut run);
    run.state
        .pointer_to((700.0, 500.0).into(), InputTime::now());
    release(&mut run);
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "a source destroyed during its drag"
    );
    let [_, attached, started, _, _] = run.state.toplevel_drags.sizes();
    assert_eq!((attached, started), (0, 0), "no drag in progress");
    run.stop();
}

/// The same source set as the selection again and again, and many sources each set and gone,
/// or each given to a toplevel drag and gone, leave a bounded number of marks: a client cannot
/// make a request cost more than the last.
#[test]
fn the_marks_of_sources_stay_bounded() {
    let mut run = Running::start(None);
    park_the_fixture(&mut run);
    let mut from = party(&mut run, (100, 100), None);
    let serial = from.press(&mut run);
    let (source, _log) = from.source();
    for _ in 0..1000 {
        from.device.set_selection(Some(&source), serial);
    }
    from.local.roundtrip(&mut run);
    assert_eq!(
        run.state.toplevel_drags.sizes()[4],
        1,
        "one mark for one source"
    );

    let manager: XdgToplevelDragManagerV1 = from.local.bind(1);
    for _ in 0..500 {
        let (selected, _log) = from.source();
        from.device.set_selection(Some(&selected), serial);
        selected.destroy();
        let (used, _log) = from.source();
        let drag = manager.get_xdg_toplevel_drag(&used, &from.local.queue.handle(), ());
        drag.destroy();
        used.destroy();
    }
    from.local.roundtrip(&mut run);
    assert!(
        from.local.connection.protocol_error().is_none(),
        "every source and drag object was its own"
    );
    let [drags, _, _, used, selected] = run.state.toplevel_drags.sizes();
    assert_eq!(drags, 0, "no drag object left");
    assert!(used <= 64, "the marks of used sources gone are dropped: {used}");
    assert!(selected <= 64, "the marks of selected sources gone are dropped: {selected}");
    release(&mut run);
    run.stop();
}
