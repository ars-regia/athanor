//! Decision 32 of doc_own_stack.md, as amended on 2026-11-26: athanor-workspace-v1, the
//! overview's control of workspaces, of their windows, and the capture source of a workspace
//! (#1133).

use super::*;
use athanor_workspace::athanor_workspace_manager_v1::AthanorWorkspaceManagerV1;
use athanor_workspace::athanor_workspace_v1::AthanorWorkspaceV1;
use athanor_workspace::athanor_workspace_window_v1::AthanorWorkspaceWindowV1;

/// An event of an athanor_workspace_v1 object.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum WorkspaceEvent {
    State(u32),
    Capabilities(u32),
    Removed,
}

/// An event of an athanor_workspace_window_v1 object.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum WindowEvent {
    Enter(ObjectId),
    Leave(ObjectId),
    Geometry(ObjectId, i32, i32, i32, i32),
    Done,
}

delegate_noop!(Client: AthanorWorkspaceManagerV1);

impl Dispatch<AthanorWorkspaceV1, ()> for Client {
    fn event(
        client: &mut Self,
        object: &AthanorWorkspaceV1,
        event: athanor_workspace::athanor_workspace_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use athanor_workspace::athanor_workspace_v1::Event;
        let event = match event {
            Event::State { state } => WorkspaceEvent::State(state.into()),
            Event::Capabilities { capabilities } => {
                WorkspaceEvent::Capabilities(capabilities.into())
            }
            Event::Removed => WorkspaceEvent::Removed,
        };
        client.workspace_objects.push((object.id(), event));
    }
}

impl Dispatch<AthanorWorkspaceWindowV1, ()> for Client {
    fn event(
        client: &mut Self,
        object: &AthanorWorkspaceWindowV1,
        event: athanor_workspace::athanor_workspace_window_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use athanor_workspace::athanor_workspace_window_v1::Event;
        let event = match event {
            Event::WorkspaceEnter { workspace } => WindowEvent::Enter(workspace.id()),
            Event::WorkspaceLeave { workspace } => WindowEvent::Leave(workspace.id()),
            Event::Geometry {
                output,
                x,
                y,
                width,
                height,
            } => WindowEvent::Geometry(output.id(), x, y, width, height),
            Event::Done => WindowEvent::Done,
        };
        client.workspace_windows.push((object.id(), event));
    }
}

const PIN: u32 = 1;
const MOVE: u32 = 2;
const PINNED: u32 = 1;
const MANAGER: &str = "athanor_workspace_manager_v1";

/// Opaque green and blue, as the client's XRGB8888 buffers hold them and as a capture gives them
/// back, B, G, R, A.
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [255, 0, 0, 255];

impl Local {
    /// The events `object` was sent, in order.
    fn workspace_events(&self, object: &AthanorWorkspaceV1) -> Vec<WorkspaceEvent> {
        self.client
            .workspace_objects
            .iter()
            .filter(|(id, _)| *id == object.id())
            .map(|(_, event)| event.clone())
            .collect()
    }

    /// The events of `object` until its next `done`, which they end with; taken.
    fn window_events(
        &mut self,
        run: &mut Running,
        object: &AthanorWorkspaceWindowV1,
    ) -> Vec<WindowEvent> {
        let id = object.id();
        self.pump(run, "the window object's done", |client, _| {
            client
                .workspace_windows
                .iter()
                .any(|(of, event)| *of == id && *event == WindowEvent::Done)
        });
        let events = &mut self.client.workspace_windows;
        let end = events
            .iter()
            .position(|(of, event)| *of == id && *event == WindowEvent::Done)
            .expect("a done");
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for (index, (of, event)) in events.drain(..).enumerate() {
            if of == id && index <= end {
                taken.push(event);
            } else {
                kept.push((of, event));
            }
        }
        *events = kept;
        taken
    }

    /// The client's handles of the workspaces of output `group`, in order, as last heard.
    fn handles(
        &mut self,
        run: &mut Running,
        group: usize,
    ) -> Vec<ext_workspace_handle_v1::ExtWorkspaceHandleV1> {
        self.roundtrip(run);
        heard(&self.client, group)
            .iter()
            .map(|(id, _, _)| workspace_handle(&self.client, id))
            .collect()
    }

    /// Paints the window of `other` all `bgrx`.
    fn paint(&mut self, run: &mut Running, other: &Other, bgrx: [u8; 4]) {
        let shm: wl_shm::WlShm = self.bind(1);
        let (buffer, _) = self.buffer(&shm, WIDTH, HEIGHT, bgrx);
        other.surface.attach(Some(&buffer), 0, 0);
        other.surface.damage_buffer(0, 0, WIDTH, HEIGHT);
        other.surface.commit();
        self.roundtrip(run);
    }
}

/// A client of ext-workspace-v1 with the workspace manager, as the overview binds them.
fn overview(run: &mut Running) -> (Local, AthanorWorkspaceManagerV1) {
    let (local, _workspaces) = workspace_client(run);
    let manager = local.bind(1);
    (local, manager)
}

/// The geometry of `window`, mapped, relative to output `index`.
fn on_output(
    run: &Running,
    window: &smithay::desktop::Window,
    index: usize,
) -> (i32, i32, i32, i32) {
    let output = run
        .state
        .space
        .output_geometry(&output_at(run, index))
        .expect("the output");
    let geometry = run
        .state
        .space
        .element_geometry(window)
        .expect("the window is mapped");
    let at = geometry.loc - output.loc;
    (at.x, at.y, geometry.size.w, geometry.size.h)
}

/// `athanor_workspace_manager_v1` is privileged by a grant of its own (decision 32): the
/// fixture's client granted it sees it; the same identity inside a security context, the
/// control center and a client with no line do not, and so cannot make a workspace's source.
#[test]
fn os4_only_a_granted_client_sees_the_workspace_manager() {
    let mut run = Running::start(None);
    let granted = Local::connect(&mut run, None);
    assert!(granted.sees(MANAGER), "the client granted it");
    let context = sandbox(&run);
    let sandboxed = Local::connect(&mut run, Some(context));
    assert!(!sandboxed.sees(MANAGER), "inside a context");
    let control_center = Local::control_center(&mut run);
    assert!(!control_center.sees(MANAGER), "the control center");
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.sees(MANAGER), "a client with no line");
    run.stop();
}

/// A workspace object is sent its state and capabilities at once: every workspace can be
/// pinned, and the empty one kept last cannot move until a window makes it no longer last.
#[test]
fn a_workspace_object_is_sent_its_pin_and_capabilities() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let handles = local.handles(&mut run, 0);
    assert_eq!(
        handles.len(),
        2,
        "the fixture's workspace and the empty one"
    );
    let qh = local.queue.handle();
    let first = manager.get_workspace(&handles[0], &qh, ());
    let last = manager.get_workspace(&handles[1], &qh, ());
    local.roundtrip(&mut run);
    use WorkspaceEvent::{Capabilities, State};
    assert_eq!(
        local.workspace_events(&first),
        [State(0), Capabilities(PIN | MOVE)]
    );
    assert_eq!(local.workspace_events(&last), [State(0), Capabilities(PIN)]);

    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    local.roundtrip(&mut run);
    assert_eq!(
        local.workspace_events(&last),
        [State(0), Capabilities(PIN), Capabilities(PIN | MOVE)],
        "no longer last"
    );
    run.stop();
}

/// A pinned workspace stays when its last window leaves it, for the session; unpinned, it goes
/// as any emptied workspace does, and its object is sent `removed`.
#[test]
fn a_pinned_workspace_stays_when_it_empties() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    let handles = local.handles(&mut run, 0);
    let object = manager.get_workspace(&handles[1], &local.queue.handle(), ());
    object.pin();
    local.roundtrip(&mut run);
    use WorkspaceEvent::{Capabilities, Removed, State};
    assert_eq!(
        local.workspace_events(&object),
        [State(0), Capabilities(PIN | MOVE), State(PINNED)]
    );

    assert!(run.state.move_window_to(&window, list[0]));
    local.roundtrip(&mut run);
    assert!(
        workspaces_of(&run, 0).contains(&list[1]),
        "kept, though empty"
    );
    assert_eq!(local.workspace_events(&object).last(), Some(&State(PINNED)));

    object.unpin();
    local.roundtrip(&mut run);
    assert!(
        !workspaces_of(&run, 0).contains(&list[1]),
        "gone once unpinned"
    );
    assert_eq!(local.workspace_events(&object).last(), Some(&Removed));
    run.stop();
}

/// `move_before` and `move_after` reorder a workspace among its own output's workspaces, which
/// ext-workspace-v1's coordinates then follow. A move relative to another output's workspace,
/// one after the empty workspace kept last, and a move of that workspace are refused.
#[test]
fn a_workspace_moves_among_its_outputs_workspaces() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    let list = workspaces_of(&run, 0);
    assert_eq!(list.len(), 3);
    let handles = local.handles(&mut run, 0);
    let qh = local.queue.handle();
    let second = manager.get_workspace(&handles[1], &qh, ());
    let last = manager.get_workspace(&handles[2], &qh, ());

    second.move_before(&handles[0]);
    local.roundtrip(&mut run);
    assert_eq!(
        workspaces_of(&run, 0),
        [list[1], list[0], list[2]],
        "moved before"
    );
    let heard_order: Vec<ObjectId> = local.handles(&mut run, 0).iter().map(|h| h.id()).collect();
    assert_eq!(
        heard_order,
        [handles[1].id(), handles[0].id(), handles[2].id()],
        "the coordinates follow"
    );
    assert_eq!(
        run.state.active_workspace(&output_at(&run, 0)),
        Some(list[0]),
        "the active workspace stays active"
    );

    let moved = workspaces_of(&run, 0);
    second.move_after(&handles[2]);
    last.move_before(&handles[1]);
    let theirs = local.handles(&mut run, 1);
    second.move_after(&theirs[0]);
    local.roundtrip(&mut run);
    assert_eq!(
        workspaces_of(&run, 0),
        moved,
        "every refused move changes nothing"
    );
    assert_eq!(workspaces_of(&run, 1).len(), 1, "nor another output's");

    second.move_after(&handles[0]);
    local.roundtrip(&mut run);
    assert_eq!(workspaces_of(&run, 0), list, "moved after");
    run.stop();
}

/// While the session is locked the workspace requests do nothing; unlocked, they act.
#[test]
fn the_workspace_requests_do_nothing_while_locked() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    let list = workspaces_of(&run, 0);
    let handles = local.handles(&mut run, 0);
    let object = manager.get_workspace(&handles[1], &local.queue.handle(), ());
    local.roundtrip(&mut run);

    let (mut locker, lock) = lock_session(&mut run);
    object.pin();
    object.move_before(&handles[0]);
    local.roundtrip(&mut run);
    use WorkspaceEvent::{Capabilities, State};
    assert_eq!(
        local.workspace_events(&object),
        [State(0), Capabilities(PIN | MOVE)],
        "not pinned"
    );
    assert_eq!(workspaces_of(&run, 0), list, "not moved");

    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });
    object.pin();
    local.roundtrip(&mut run);
    assert_eq!(
        local.workspace_events(&object).last(),
        Some(&State(PINNED)),
        "pinned once unlocked"
    );
    run.stop();
}

/// A window object is sent the window's workspace and its place at once, then each change:
/// moved to an inactive workspace it keeps its place; moved to another output's workspace it
/// leaves the first output and is placed on the second. A move to a workspace with an output
/// that is not the workspace's own is refused.
#[test]
fn a_window_object_follows_its_workspace_and_place() {
    use WindowEvent::{Done, Enter, Geometry, Leave};
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let first_output = local.output(0);
    let second_output = local.output(1);
    let probe = local.another(&mut run, "probe");
    let window = local.server_window(&run, &probe.surface);
    let handle = local.handle(&mut run, "probe");
    let handles = local.handles(&mut run, 0);
    let object = manager.get_workspace_window(&handle, &local.queue.handle(), ());
    let (x, y, w, h) = on_output(&run, &window, 0);
    assert_eq!(
        local.window_events(&mut run, &object),
        [
            Enter(handles[0].id()),
            Geometry(first_output.id(), x, y, w, h),
            Done
        ]
    );

    object.move_to_workspace(&handles[1], &first_output);
    assert_eq!(
        local.window_events(&mut run, &object),
        [Leave(handles[0].id()), Enter(handles[1].id()), Done],
        "parked where it was"
    );
    let list = workspaces_of(&run, 0);
    assert_eq!(run.state.workspace_of(&window), Some(list[1]));
    assert!(run.state.is_parked(&window));

    let theirs = local.handles(&mut run, 1);
    object.move_to_workspace(&theirs[0], &first_output);
    local.roundtrip(&mut run);
    assert_eq!(
        run.state.workspace_of(&window),
        Some(list[1]),
        "refused: not its output"
    );

    object.move_to_workspace(&theirs[0], &second_output);
    let events = local.window_events(&mut run, &object);
    let (x, y, w, h) = on_output(&run, &window, 1);
    assert_eq!(
        events,
        [
            Leave(handles[1].id()),
            Enter(theirs[0].id()),
            Geometry(first_output.id(), 0, 0, 0, 0),
            Geometry(second_output.id(), x, y, w, h),
            Done
        ]
    );
    assert_eq!(
        run.state.workspace_of(&window),
        workspaces_of(&run, 1).first().copied()
    );
    run.stop();
}

/// `move_to_workspace` does nothing while the session is locked, nor with the handle of a
/// workspace that went away.
#[test]
fn a_window_move_is_refused_while_locked_and_on_a_gone_workspace() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let first_output = local.output(0);
    let probe = local.another(&mut run, "probe");
    let window = local.server_window(&run, &probe.surface);
    let handle = local.handle(&mut run, "probe");
    let handles = local.handles(&mut run, 0);
    let object = manager.get_workspace_window(&handle, &local.queue.handle(), ());
    local.window_events(&mut run, &object);
    let list = workspaces_of(&run, 0);

    let (mut locker, lock) = lock_session(&mut run);
    object.move_to_workspace(&handles[1], &first_output);
    local.roundtrip(&mut run);
    assert_eq!(
        run.state.workspace_of(&window),
        Some(list[0]),
        "not while locked"
    );
    lock.unlock_and_destroy();
    locker.pump(&mut run, "the session is unlocked", |_, state| {
        state.lock.is_none()
    });

    // The probe alone on the second workspace, then back: the second workspace goes.
    object.move_to_workspace(&handles[1], &first_output);
    local.roundtrip(&mut run);
    assert_eq!(run.state.workspace_of(&window), Some(list[1]));
    object.move_to_workspace(&handles[0], &first_output);
    local.roundtrip(&mut run);
    assert_eq!(run.state.workspace_of(&window), Some(list[0]));
    assert!(!workspaces_of(&run, 0).contains(&list[1]), "gone");
    object.move_to_workspace(&handles[1], &first_output);
    local.roundtrip(&mut run);
    assert_eq!(
        run.state.workspace_of(&window),
        Some(list[0]),
        "not to a gone workspace"
    );
    run.stop();
}

/// The pixel at `x`,`y` of a frame `width` pixels wide, B, G, R, A.
fn pixel(frame: &[u8], width: i32, x: i32, y: i32) -> [u8; 4] {
    let at = usize::try_from((y * width + x) * 4).expect("in the frame");
    frame[at..at + 4].try_into().expect("four bytes")
}

/// The centre of a mapped window's geometry relative to output `index`, in that output's
/// pixels.
fn centre(run: &Running, window: &smithay::desktop::Window, index: usize) -> (i32, i32) {
    let (x, y, w, h) = on_output(run, window, index);
    let scale = OUTPUTS[index].scale;
    let at = |v: i32| (f64::from(v) * scale).round() as i32;
    (at(x + w / 2), at(y + h / 2))
}

/// A capture session of `workspace` through the workspace manager, its constraints heard: the
/// size of `index`'s output.
fn workspace_session(
    local: &mut Local,
    run: &mut Running,
    manager: &AthanorWorkspaceManagerV1,
    copy: &ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    workspace: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
) -> ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1 {
    let qh = local.queue.handle();
    local.client.capture_size = None;
    let source = manager.get_capture_source(workspace, &qh, ());
    let session = copy.create_session(
        &source,
        ext_image_copy_capture_manager_v1::Options::PaintCursors,
        &qh,
        (),
    );
    local.pump(run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });
    session
}

/// `workspace` captured once, from a session asking for cursors, which a workspace never
/// draws: a frame of output `index`'s size, B, G, R, A.
fn captured_workspace(
    local: &mut Local,
    run: &mut Running,
    manager: &AthanorWorkspaceManagerV1,
    workspace: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
    index: usize,
) -> Vec<u8> {
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let session = workspace_session(local, run, manager, &copy, workspace);
    let (w, h) = (OUTPUTS[index].width, OUTPUTS[index].height);
    assert_eq!(
        local.client.capture_size,
        Some((w as u32, h as u32)),
        "the size of the workspace's output in pixels"
    );
    let (buffer, file) = local.buffer(&shm, w, h, [0x55; 4]);
    local.client.captured = None;
    let frame = session.create_frame(&local.queue.handle(), ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, w, h);
    frame.capture();
    local.pump(run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(local.client.captured, Some(Ok(())), "the frame is ready");
    contents(&file, usize::try_from(w * h * 4).expect("size"))
}

/// Two windows, green and blue, on the active workspace of the first output, the fixture's own
/// minimised, and their client's handles of that output's workspaces.
fn two_windows(
    run: &mut Running,
    local: &mut Local,
) -> (Local, smithay::desktop::Window, smithay::desktop::Window) {
    let (fixture, _) = fixture_window(run);
    assert!(run.state.minimise(&fixture));
    let mut app = Local::connect(run, None);
    let green = app.another(run, "green");
    app.paint(run, &green, GREEN);
    let blue = app.another(run, "blue");
    app.paint(run, &blue, BLUE);
    let (green, blue) = (
        app.server_window(run, &green.surface),
        app.server_window(run, &blue.surface),
    );
    run.state
        .space
        .map_element(green.clone(), (100, 100), false);
    run.state
        .space
        .map_element(blue.clone(), (600, 400), false);
    local.roundtrip(run);
    (app, green, blue)
}

/// A workspace with two windows is captured, and the frame holds both, each where the output
/// shows it, over a transparent background: no layer surface, no pointer. Every opaque pixel is
/// the output's own, so the marks are drawn as on screen.
#[test]
fn a_workspace_with_two_windows_is_captured() {
    let mut run = Running::start(None);
    // The theme's fallback square, which a capture of the output would show at the pointer.
    run.state.cursor = crate::cursor::Cursor::new("athanor-no-such-theme", 24);
    let (mut local, manager) = overview(&mut run);
    let (_app, green, blue) = two_windows(&mut run, &mut local);
    let area = run
        .state
        .space
        .output_geometry(&output_at(&run, 0))
        .expect("mapped");
    run.state.pointer_to(
        (area.loc + Point::from((1200, 20))).to_f64(),
        InputTime::now(),
    );
    let handles = local.handles(&mut run, 0);

    let frame = captured_workspace(&mut local, &mut run, &manager, &handles[0], 0);
    let width = OUTPUTS[0].width;
    let (gx, gy) = centre(&run, &green, 0);
    let (bx, by) = centre(&run, &blue, 0);
    assert_eq!(pixel(&frame, width, gx, gy), GREEN, "the green window");
    assert_eq!(pixel(&frame, width, bx, by), BLUE, "the blue window");
    assert_eq!(
        pixel(&frame, width, 1205, 25),
        [0; 4],
        "no pointer, transparent"
    );
    assert_eq!(
        pixel(&frame, width, 5, 795),
        [0; 4],
        "transparent where no window is"
    );

    let screen: Vec<u8> = run
        .state
        .capture(0)
        .expect("frame of output 0")
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [b, g, r, a])
        .collect();
    for (at, (pixel, shown)) in frame
        .as_chunks::<4>()
        .0
        .iter()
        .zip(screen.as_chunks::<4>().0)
        .enumerate()
    {
        if pixel[3] == 255 {
            assert_eq!(pixel, shown, "as the output shows pixel {at}");
        }
    }
    run.stop();
}

/// An inactive workspace is captured with its windows at the places they keep; the active
/// workspace's frame no longer holds the window moved there.
#[test]
fn an_inactive_workspace_is_captured() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let (_app, green, blue) = two_windows(&mut run, &mut local);
    let (bx, by) = centre(&run, &blue, 0);
    let (gx, gy) = centre(&run, &green, 0);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&blue, list[1]));
    assert!(run.state.is_parked(&blue));
    let handles = local.handles(&mut run, 0);
    let width = OUTPUTS[0].width;

    let inactive = captured_workspace(&mut local, &mut run, &manager, &handles[1], 0);
    assert_eq!(
        pixel(&inactive, width, bx, by),
        BLUE,
        "the parked window, where it was"
    );
    assert_eq!(
        pixel(&inactive, width, gx, gy),
        [0; 4],
        "not the active workspace's"
    );

    let active = captured_workspace(&mut local, &mut run, &manager, &handles[0], 0);
    assert_eq!(pixel(&active, width, gx, gy), GREEN);
    assert_eq!(
        pixel(&active, width, bx, by),
        [0; 4],
        "not the inactive workspace's"
    );
    run.stop();
}

/// A window parked across its output's edge that the output no longer shows once it shrinks is
/// sent, drawn and brought back in one place: centred on its workspace's output (`reachable`).
#[test]
fn a_parked_window_its_output_no_longer_shows_is_sent_where_it_comes_back() {
    use WindowEvent::{Done, Enter, Geometry};
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let first_output = local.output(0);
    let blue = local.another(&mut run, "blue");
    local.paint(&mut run, &blue, BLUE);
    let window = local.server_window(&run, &blue.surface);
    let first = output_at(&run, 0);
    let area = run.state.space.output_geometry(&first).expect("placed");
    // Across the first output's right edge, onto the second.
    let across = area.loc + Point::from((area.size.w - 100, 200));
    run.state.space.relocate_element(&window, across);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    assert!(run.state.is_parked(&window));

    first.change_current_state(None, None, Some(smithay::output::Scale::Integer(2)), None);
    layer_map_for_output(&first).arrange();
    run.state.arrange_outputs();
    let home = run.state.home_area(&window).expect("its output's area");
    let size = window.geometry().size;
    let back = crate::state::centred(home, size);
    let shown = run.state.space.output_geometry(&first).expect("placed");
    assert!(
        !shown.overlaps(smithay::utils::Rectangle::new(across, size)),
        "no longer shown"
    );
    assert_eq!(run.state.parked_at(&window), Some(back));

    let handle = local.handle(&mut run, "blue");
    let handles = local.handles(&mut run, 0);
    let object = manager.get_workspace_window(&handle, &local.queue.handle(), ());
    let relative = back - shown.loc;
    assert_eq!(
        local.window_events(&mut run, &object),
        [
            Enter(handles[1].id()),
            Geometry(first_output.id(), relative.x, relative.y, size.w, size.h),
            Done
        ]
    );

    let frame = captured_workspace(&mut local, &mut run, &manager, &handles[1], 0);
    let (x, y) = (relative.x + size.w / 2, relative.y + size.h / 2);
    assert_eq!(
        pixel(&frame, OUTPUTS[0].width, x * 2, y * 2),
        BLUE,
        "drawn where it comes back"
    );

    assert!(run.state.activate_workspace(list[1]));
    assert_eq!(run.state.space.element_location(&window), Some(back));
    run.stop();
}

/// A workspace's windows keep their places and their marks when another workspace is shown: the
/// window not focused is drawn as before, divider and corners with it, and the focused one as
/// before inside its geometry. The focus outline round it goes once its workspace is left, since
/// the session's focus moves to the workspace shown.
#[test]
fn a_workspace_keeps_its_frame_when_it_is_left() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let (_app, green, blue) = two_windows(&mut run, &mut local);
    let handles = local.handles(&mut run, 0);
    let geometry = |run: &Running, window| run.state.space.element_geometry(window).expect("mapped");
    let (unfocused, focused) = (geometry(&run, &green), geometry(&run, &blue));
    let shown = captured_workspace(&mut local, &mut run, &manager, &handles[0], 0);
    let list = workspaces_of(&run, 0);
    assert!(run.state.activate_workspace(list[1]));
    assert!(run.state.is_parked(&green) && run.state.is_parked(&blue));
    let left = captured_workspace(&mut local, &mut run, &manager, &handles[0], 0);
    let width = OUTPUTS[0].width;
    let same = |x0: i32, y0: i32, x1: i32, y1: i32| {
        for y in y0..y1 {
            for x in x0..x1 {
                assert_eq!(
                    pixel(&left, width, x, y),
                    pixel(&shown, width, x, y),
                    "({x}, {y}) once the workspace is left"
                );
            }
        }
    };
    // Two pixels round the window not focused: its edge, where the divider is, and outside it.
    let (x, y, w, h) = (unfocused.loc.x, unfocused.loc.y, unfocused.size.w, unfocused.size.h);
    same(x - 2, y - 2, x + w + 2, y + h + 2);
    assert_ne!(
        pixel(&left, width, x, y + h / 2),
        GREEN,
        "the divider on the window's left edge"
    );
    assert_eq!(pixel(&left, width, x + 1, y + h / 2), GREEN, "inside the divider");
    // The focused window away from its corners, which the outline's ring overlaps.
    let corner = 16;
    let (x, y, w, h) = (focused.loc.x, focused.loc.y, focused.size.w, focused.size.h);
    same(x + corner, y + corner, x + w - corner, y + h - corner);
    // The middle of each of its sides, where its divider lies inside the outline's ring.
    same(x, y + h / 2, x + 1, y + h / 2 + 1);
    same(x + w - 1, y + h / 2, x + w, y + h / 2 + 1);
    same(x + w / 2, y, x + w / 2 + 1, y + 1);
    same(x + w / 2, y + h - 1, x + w / 2 + 1, y + h);
    assert_ne!(pixel(&left, width, x, y + h / 2), BLUE, "its divider");
    assert_eq!(pixel(&left, width, x + corner, y + corner), BLUE);
    run.stop();
}

/// A tiled workspace that is left keeps its marks: the divider of a tiled window and the two
/// lines of a confined one, tiled beside it, are drawn as on screen.
#[test]
fn a_tiled_workspace_keeps_its_marks_when_it_is_left() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    // The security context names the fixture's client, so its window goes once it is made.
    let (fixture, _) = fixture_window(&run);
    let (mut sandboxed, _sandboxed_toplevel) = super::marks::confined(&mut run);
    assert!(run.state.minimise(&fixture));
    let mut plain = Local::connect(&mut run, None);
    let _plain_toplevel = plain.toplevel(&mut run);
    let confined = window_of(&run, sandboxed.id.as_ref().expect("an id"));
    let tiled = window_of(&run, plain.id.as_ref().expect("an id"));
    let id = workspaces_of(&run, 0)[0];
    for client in [&mut sandboxed, &mut plain] {
        client.roundtrip(&mut run);
        client.client.configure = None;
    }
    run.state.set_tiling(id, true);
    sandboxed.redraw(&mut run);
    plain.redraw(&mut run);
    run.state.focus(&confined);
    local.roundtrip(&mut run);
    let geometry = |run: &Running, window| run.state.space.element_geometry(window).expect("mapped");
    let (bordered, divided) = (geometry(&run, &confined), geometry(&run, &tiled));
    assert!(
        bordered.loc.x != divided.loc.x && bordered.size.w < OUTPUTS[0].width / 2,
        "the two windows tiled side by side: {bordered:?} {divided:?}"
    );
    let handles = local.handles(&mut run, 0);
    let shown = captured_workspace(&mut local, &mut run, &manager, &handles[0], 0);
    assert!(run.state.activate_workspace(workspaces_of(&run, 0)[1]));
    assert!(run.state.is_parked(&confined) && run.state.is_parked(&tiled));
    let left = captured_workspace(&mut local, &mut run, &manager, &handles[0], 0);
    let width = OUTPUTS[0].width;
    let same = |x0: i32, y0: i32, x1: i32, y1: i32| {
        for y in y0..y1 {
            for x in x0..x1 {
                assert_eq!(
                    pixel(&left, width, x, y),
                    pixel(&shown, width, x, y),
                    "({x}, {y}) once the workspace is left"
                );
            }
        }
    };
    // The tiled window, not focused: its divider, just inside its geometry, and round it.
    let (x, y, w, h) = (divided.loc.x, divided.loc.y, divided.size.w, divided.size.h);
    same(x - 2, y - 2, x + w + 2, y + h + 2);
    assert_ne!(
        pixel(&left, width, x, y + h / 2),
        pixel(&left, width, x + w / 2, y + h / 2),
        "the divider on the tiled window's edge"
    );
    // The confined window, focused while shown: its two lines, inside the outline's ring.
    let (x, y, w, h) = (bordered.loc.x, bordered.loc.y, bordered.size.w, bordered.size.h);
    let middle = y + h / 2;
    same(x - 3, middle, x + w + 3, middle + 1);
    same(x + w / 2, y - 3, x + w / 2 + 1, y + h + 3);
    assert_eq!(pixel(&left, width, x - 2, middle), super::marks::WHITE, "the white line");
    assert_eq!(pixel(&left, width, x - 3, middle), super::marks::BLACK, "the black line");
    run.stop();
}

/// A workspace of the second output is captured at that output's mode, its window at the
/// output's scale.
#[test]
fn a_workspace_of_the_second_output_is_captured() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let (_app, green, _blue) = two_windows(&mut run, &mut local);
    let theirs = workspaces_of(&run, 1);
    assert!(run.state.move_window_to(&green, theirs[0]));
    let handles = local.handles(&mut run, 1);

    let frame = captured_workspace(&mut local, &mut run, &manager, &handles[0], 1);
    let (x, y) = centre(&run, &green, 1);
    assert_eq!(pixel(&frame, OUTPUTS[1].width, x, y), GREEN);
    run.stop();
}

/// The empty workspace kept last gives a transparent frame, not a failure.
#[test]
fn an_empty_workspace_gives_a_transparent_frame() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let handles = local.handles(&mut run, 0);
    let frame = captured_workspace(&mut local, &mut run, &manager, &handles[1], 0);
    assert!(
        frame.iter().all(|&byte| byte == 0),
        "every pixel transparent"
    );
    run.stop();
}

/// While the session is locked a workspace is not captured: a session made before the lock has
/// its frame fail, and a new session is refused.
#[test]
fn a_workspace_capture_is_refused_while_locked() {
    let mut run = Running::start(None);
    let (mut local, manager) = overview(&mut run);
    let handles = local.handles(&mut run, 0);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let session = workspace_session(&mut local, &mut run, &manager, &copy, &handles[0]);
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    let (buffer, file) = local.buffer(&shm, w, h, [0x55; 4]);

    let (_locker, _lock) = lock_session(&mut run);
    local.client.captured = None;
    let frame = session.create_frame(&local.queue.handle(), ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, w, h);
    frame.capture();
    local.pump(&mut run, "the frame fails", |client, _| {
        client.captured.is_some()
    });
    assert!(
        matches!(local.client.captured, Some(Err(_))),
        "the frame fails"
    );
    let len = usize::try_from(w * h * 4).expect("size");
    assert!(
        contents(&file, len).iter().all(|&byte| byte == 0x55),
        "nothing reaches the buffer"
    );

    let qh = local.queue.handle();
    let source = manager.get_capture_source(&handles[0], &qh, ());
    let refused = copy
        .create_session(
            &source,
            ext_image_copy_capture_manager_v1::Options::empty(),
            &qh,
            (),
        )
        .id();
    local.pump(
        &mut run,
        "a session is refused while locked",
        |client, _| client.stopped.contains(&refused),
    );
    run.stop();
}
