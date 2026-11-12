//! Decision 35: the dock's intellihide, athanor-dock-overlap-v1 (`dock_overlap.rs`). Each rule
//! of the decision has its test: the grant, the answer on the rectangle sent, the minimised
//! window, the other workspace and the other output, the output's coordinates, the lock, and
//! the surface that is no layer surface.

use super::*;
use athanor_dock_overlap::athanor_dock_overlap_manager_v1::AthanorDockOverlapManagerV1;
use athanor_dock_overlap::athanor_dock_overlap_v1::AthanorDockOverlapV1;
use smithay::desktop::Window;

/// The dock's height, and the rectangle it shows itself in on the first output: along its
/// bottom edge.
const DOCK: i32 = 64;
const SHOWN: (i32, i32, i32, i32) = (0, OUTPUTS[0].height - DOCK, OUTPUTS[0].width, DOCK);

/// Minimises the fixture's window, so that the tests' windows are the only ones shown.
fn without_the_fixture(run: &mut Running) {
    let (window, _) = fixture_window(run);
    assert!(
        run.state.minimise(&window),
        "the fixture's window is minimised"
    );
}

/// A window of a new client, on the first output's current workspace, at `(x, y)`.
fn window_at(run: &mut Running, x: i32, y: i32) -> (Local, Window) {
    let mut local = Local::connect(run, None);
    local.toplevel(run);
    let window = window_of(run, local.id.as_ref().expect("an id"));
    run.state.move_window(&window, (x, y).into());
    (local, window)
}

impl Local {
    /// A dock: a layer surface on the top layer of `output`, or of the first output, anchored
    /// at `anchor`, `DOCK` high, drawn black.
    fn dock(
        &mut self,
        run: &mut Running,
        output: Option<&wl_output::WlOutput>,
        anchor: zwlr_layer_surface_v1::Anchor,
    ) -> wl_surface::WlSurface {
        use zwlr_layer_surface_v1::Anchor;
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let shm: wl_shm::WlShm = self.bind(1);
        let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = self.bind(4);
        let qh = self.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(
            &surface,
            output,
            zwlr_layer_shell_v1::Layer::Top,
            "dock".into(),
            &qh,
            (),
        );
        layer.set_anchor(anchor | Anchor::Left | Anchor::Right);
        layer.set_size(0, DOCK.unsigned_abs());
        assert!(self.answer(run, &surface), "the dock is configured");
        let (serial, width, _) = self.client.layer_configure.expect("a configure");
        layer.ack_configure(serial);
        let width = i32::try_from(width).expect("a width");
        let (buffer, _) = self.buffer(&shm, width, DOCK, [0, 0, 0, 255]);
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        self.roundtrip(run);
        surface
    }

    /// The overlap object of `surface`, from a new manager, with `rectangle` set.
    fn overlap(
        &mut self,
        run: &mut Running,
        surface: &wl_surface::WlSurface,
        (x, y, width, height): (i32, i32, i32, i32),
    ) -> AthanorDockOverlapV1 {
        let manager: AthanorDockOverlapManagerV1 = self.bind(1);
        let qh = self.queue.handle();
        let object = manager.get_overlap(surface, &qh, ());
        object.set_rectangle(x, y, width, height);
        self.roundtrip(run);
        object
    }

    /// The answers `object` was sent, in order.
    fn overlaps_of(&self, object: &AthanorDockOverlapV1) -> Vec<u32> {
        self.client
            .overlaps
            .iter()
            .filter(|(id, _)| *id == object.id())
            .map(|(_, overlapped)| *overlapped)
            .collect()
    }

    /// Runs the compositor until `object` has been sent `answers`, in order.
    fn heard_overlaps(
        &mut self,
        run: &mut Running,
        object: &AthanorDockOverlapV1,
        answers: &[u32],
    ) {
        let id = object.id();
        let expected = answers.to_vec();
        self.pump(run, "the overlap answers", |client, _| {
            let heard: Vec<u32> = client
                .overlaps
                .iter()
                .filter(|(of, _)| *of == id)
                .map(|(_, overlapped)| *overlapped)
                .collect();
            heard.len() >= expected.len()
        });
        assert_eq!(self.overlaps_of(object), answers);
    }
}

/// `athanor_dock_overlap_manager_v1` is privileged (OS4, decision 35): the fixture's bar, granted
/// it, sees it; the bar's identity inside a security context, the control center and a client
/// with no line do not.
#[test]
fn d35_only_a_granted_client_sees_the_overlap() {
    const MANAGER: &str = "athanor_dock_overlap_manager_v1";
    let mut run = Running::start(None);
    let granted = Local::connect(&mut run, None);
    assert!(granted.sees(MANAGER), "the granted client");
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(
        !sandboxed.sees(MANAGER),
        "the granted client inside a context"
    );
    let control_center = Local::control_center(&mut run);
    assert!(!control_center.sees(MANAGER), "the control center");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(MANAGER), "a client with no line");
    run.stop();
}

/// The answer is sent after the first rectangle, then at each change: true while a window
/// intersects the rectangle, false when none does.
#[test]
fn d35_a_window_over_the_rectangle_overlaps_it() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let mut dock = Local::connect(&mut run, None);
    let surface = dock.dock(&mut run, None, zwlr_layer_surface_v1::Anchor::Bottom);
    let (_app, window) = window_at(&mut run, 100, 100);
    let object = dock.overlap(&mut run, &surface, SHOWN);
    dock.heard_overlaps(&mut run, &object, &[0]);

    run.state.move_window(&window, (100, SHOWN.1 - 20).into());
    dock.heard_overlaps(&mut run, &object, &[0, 1]);

    // Touching the rectangle's edge is not intersecting it.
    run.state
        .move_window(&window, (100, SHOWN.1 - HEIGHT).into());
    dock.heard_overlaps(&mut run, &object, &[0, 1, 0]);
    dock.frames(&mut run, 3);
    assert_eq!(
        dock.overlaps_of(&object),
        [0, 1, 0],
        "sent at a change only"
    );
    run.stop();
}

/// The compositor answers on the rectangle sent, whatever the surface's own geometry: a dock
/// drawn at the top that sends a rectangle at the bottom is overlapped by a window at the
/// bottom, not by one over the surface. A rectangle without area is overlapped by nothing.
#[test]
fn d35_the_answer_is_on_the_rectangle_not_the_surface() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let mut dock = Local::connect(&mut run, None);
    let surface = dock.dock(&mut run, None, zwlr_layer_surface_v1::Anchor::Top);
    let (_app, window) = window_at(&mut run, 100, SHOWN.1 - 20);
    let object = dock.overlap(&mut run, &surface, SHOWN);
    dock.heard_overlaps(&mut run, &object, &[1]);

    run.state.move_window(&window, (100, 0).into());
    dock.heard_overlaps(&mut run, &object, &[1, 0]);

    object.set_rectangle(0, 0, OUTPUTS[0].width, 0);
    dock.roundtrip(&mut run);
    dock.frames(&mut run, 3);
    assert_eq!(dock.overlaps_of(&object), [1, 0], "an empty rectangle");
    run.stop();
}

/// A minimised window does not overlap, nor one of another workspace.
#[test]
fn d35_a_minimised_window_or_another_workspace_does_not_overlap() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let mut dock = Local::connect(&mut run, None);
    let surface = dock.dock(&mut run, None, zwlr_layer_surface_v1::Anchor::Bottom);
    let (_app, window) = window_at(&mut run, 100, SHOWN.1 - 20);
    let object = dock.overlap(&mut run, &surface, SHOWN);
    dock.heard_overlaps(&mut run, &object, &[1]);

    assert!(run.state.minimise(&window));
    dock.heard_overlaps(&mut run, &object, &[1, 0]);
    assert!(run.state.unminimise(&window));
    dock.heard_overlaps(&mut run, &object, &[1, 0, 1]);

    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    dock.heard_overlaps(&mut run, &object, &[1, 0, 1, 0]);
    run.stop();
}

/// The rectangle is in its output's logical coordinates, and only a window of that output's
/// current workspace counts: a window of the first output's workspace moved over the second
/// output's dock does not overlap it until it moves to the second output's workspace.
#[test]
fn d35_the_rectangle_is_on_the_surfaces_output() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .cloned()
        .expect("a second output");
    let area = run.state.space.output_geometry(&second).expect("placed");
    let mut dock = Local::connect(&mut run, None);
    let name = dock
        .client
        .globals
        .iter()
        .filter(|(_, interface, _)| interface == "wl_output")
        .nth(1)
        .map(|(name, _, _)| *name)
        .expect("a second wl_output");
    let qh = dock.queue.handle();
    let output: wl_output::WlOutput = dock.registry.bind(name, 4, &qh, ());
    let surface = dock.dock(
        &mut run,
        Some(&output),
        zwlr_layer_surface_v1::Anchor::Bottom,
    );
    let shown = (0, area.size.h - DOCK, area.size.w, DOCK);
    let (_app, window) = window_at(&mut run, 100, SHOWN.1 - 20);
    let object = dock.overlap(&mut run, &surface, shown);
    dock.heard_overlaps(&mut run, &object, &[0]);

    let over = (area.loc.x + 20, area.loc.y + shown.1 - 20);
    run.state.move_window(&window, over.into());
    dock.frames(&mut run, 3);
    assert_eq!(dock.overlaps_of(&object), [0], "another output's workspace");

    let theirs = run.state.active_workspace(&second).expect("a workspace");
    assert!(run.state.move_window_to(&window, theirs));
    run.state.move_window(&window, over.into());
    dock.heard_overlaps(&mut run, &object, &[0, 1]);
    run.stop();
}

/// While the session is locked nothing is sent, and at unlock the current answer is, changed
/// or not.
#[test]
fn d35_nothing_is_sent_while_locked() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let mut dock = Local::connect(&mut run, None);
    let surface = dock.dock(&mut run, None, zwlr_layer_surface_v1::Anchor::Bottom);
    let (_app, window) = window_at(&mut run, 100, SHOWN.1 - 20);
    let object = dock.overlap(&mut run, &surface, SHOWN);
    dock.heard_overlaps(&mut run, &object, &[1]);

    let (mut locker, lock) = lock_session(&mut run);
    run.state.move_window(&window, (100, 100).into());
    object.set_rectangle(SHOWN.0, SHOWN.1, SHOWN.2, SHOWN.3);
    dock.roundtrip(&mut run);
    dock.frames(&mut run, 3);
    assert_eq!(dock.overlaps_of(&object), [1], "nothing while locked");

    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    dock.heard_overlaps(&mut run, &object, &[1, 0]);

    let (mut locker, lock) = lock_session(&mut run);
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    dock.heard_overlaps(&mut run, &object, &[1, 0, 0]);
    run.stop();
}

/// An overlap object of a surface that is no layer surface is the `not_layer_surface` error.
#[test]
fn d35_a_surface_without_the_layer_role_is_an_error() {
    let mut run = Running::start(None);
    let local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let manager: AthanorDockOverlapManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let _object = manager.get_overlap(&surface, &qh, ());
    local.connection.flush().expect("flush");
    let started = Instant::now();
    let error = loop {
        if let Some(error) = local.connection.protocol_error() {
            break error;
        }
        assert!(started.elapsed() < TIMEOUT, "the error: not in time");
        if let Some(guard) = local.queue.prepare_read() {
            drop(guard.read());
        }
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    };
    assert_eq!(error.object_interface, "athanor_dock_overlap_manager_v1");
    assert_eq!(error.code, 0);
    assert!(run.state.dock_overlaps.is_empty());
    run.stop();
}
