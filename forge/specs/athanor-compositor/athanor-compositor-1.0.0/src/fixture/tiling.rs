//! OS12: the automatic tiling of a workspace, from the toggle to the windows' places
//! (`docs/superpowers/plans/2026-10-31-compositor-tiling.md`).

use super::*;
use crate::workspace::WorkspaceId;
use smithay::desktop::Window;
use smithay::utils::Rectangle;

/// The whole first output as one tile, and its two halves, the gaps taken off (D4).
const WHOLE: (i32, i32, i32, i32) = (8, 8, 1264, 784);
const LEFT: (i32, i32, i32, i32) = (8, 8, 628, 784);
const RIGHT: (i32, i32, i32, i32) = (644, 8, 628, 784);

fn rect((x, y, w, h): (i32, i32, i32, i32)) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w, h).into())
}

/// The tiled states `window` was last sent (D8), read in the compositor: the fixture binds
/// `xdg_wm_base` at version 1, to which Smithay sends none of them.
fn tiled_states(window: &Window) -> usize {
    let toplevel = window.toplevel().expect("a toplevel");
    let states = toplevel.with_pending_state(|state| state.states.clone());
    crate::workspace::TILED
        .iter()
        .filter(|tiled| states.contains(**tiled))
        .count()
}

/// Minimises the fixture's window, which never answers a configure, so that the tests' windows
/// are the only ones of the first output's workspace.
fn without_the_fixture(run: &mut Running) {
    let (window, _) = fixture_window(run);
    assert!(
        run.state.minimise(&window),
        "the fixture's window is minimised"
    );
}

/// A window of a new client, on top with the keyboard.
fn window(run: &mut Running) -> (Local, xdg_toplevel::XdgToplevel, Window) {
    let mut local = Local::connect(run, None);
    let toplevel = local.toplevel(run);
    let window = window_of(run, local.id.as_ref().expect("an id"));
    (local, toplevel, window)
}

/// The active workspace of the first output.
fn first_workspace(run: &Running) -> WorkspaceId {
    let output = run.state.space.outputs().next().expect("an output");
    run.state.active_workspace(output).expect("a workspace")
}

/// Runs `change`, then each of `locals` answers the configure it brings: their configures.
fn answer_all(
    run: &mut Running,
    locals: &mut [&mut Local],
    change: impl FnOnce(&mut Running),
) -> Vec<(i32, i32, Vec<u32>)> {
    for local in locals.iter_mut() {
        local.roundtrip(run);
        local.client.configure = None;
    }
    change(run);
    locals.iter_mut().map(|local| local.redraw(run)).collect()
}

fn geometry(run: &Running, window: &Window) -> Rectangle<i32, Logical> {
    run.state
        .space
        .element_geometry(window)
        .expect("the window is mapped")
}

/// The toggle tiles the windows of the workspace: the topmost takes the whole area, the next
/// splits it, side by side on an output wider than high (D2, D7), each with the tiled states.
#[test]
fn two_windows_share_an_output() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut lower, _, below) = window(&mut run);
    let (mut upper, _, above) = window(&mut run);
    let id = first_workspace(&run);

    let answers = answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        run.state.set_tiling(id, true)
    });

    assert!(run.state.is_tiled(id));
    assert_eq!((answers[0].0, answers[0].1), (LEFT.2, LEFT.3));
    assert_eq!((answers[1].0, answers[1].1), (RIGHT.2, RIGHT.3));
    assert_eq!((tiled_states(&above), tiled_states(&below)), (4, 4));
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert_eq!(geometry(&run, &below), rect(RIGHT));
    run.stop();
}

/// The toggle turned off gives each window back the place and the size it had floating, without
/// the tiled states (D7, D8).
#[test]
fn turning_tiling_off_returns_each_window_to_its_place() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut lower, _, below) = window(&mut run);
    let (mut upper, _, above) = window(&mut run);
    run.state.move_window(&below, (100, 300).into());
    let before = [geometry(&run, &above), geometry(&run, &below)];
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        run.state.set_tiling(id, true)
    });

    let answers = answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        run.state.set_tiling(id, false)
    });

    assert!(!run.state.is_tiled(id));
    for ((width, height, _), was) in answers.iter().zip(before) {
        assert_eq!((*width, *height), (was.size.w, was.size.h));
    }
    assert_eq!((tiled_states(&above), tiled_states(&below)), (0, 0));
    assert_eq!([geometry(&run, &above), geometry(&run, &below)], before);
    run.stop();
}

/// No workspace is tiled until the toggle says so, and a new window floats where it is placed.
#[test]
fn a_workspace_is_floating_by_default() {
    let mut run = Running::start(None);
    let outputs: Vec<_> = run.state.space.outputs().cloned().collect();
    for output in &outputs {
        for id in run.state.workspaces(output) {
            assert!(!run.state.is_tiled(*id));
        }
    }
    // Placed as the first window, as the fixture's is, not cascaded from it (#941).
    run.state.spawned.clear();
    let (_local, _, window) = window(&mut run);
    assert_eq!(tiled_states(&window), 0);
    let (fixture, _) = fixture_window(&run);
    assert_eq!(geometry(&run, &window).loc, geometry(&run, &fixture).loc);
    run.stop();
}

/// Tiling is a workspace's own: a window on the next workspace keeps its place and its size, and
/// comes back to them.
#[test]
fn tiling_on_one_workspace_leaves_the_next_alone() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (_kept_client, _, kept) = window(&mut run);
    let (mut tiled_client, _, tile) = window(&mut run);
    let output = run
        .state
        .space
        .outputs()
        .next()
        .cloned()
        .expect("an output");
    let id = first_workspace(&run);
    let next = run.state.workspaces(&output)[1];
    let before = geometry(&run, &kept);
    assert!(run.state.move_window_to(&kept, next));

    answer_all(&mut run, &mut [&mut tiled_client], |run| {
        run.state.set_tiling(id, true)
    });

    assert_eq!(geometry(&run, &tile), rect(WHOLE));
    assert!(!run.state.is_tiled(next));
    assert_eq!(tiled_states(&kept), 0);
    assert!(run.state.activate_workspace(next));
    assert_eq!(geometry(&run, &kept), before);
    run.stop();
}

impl Local {
    /// A tiling object of the workspace handle `handle`, from a new manager, its first state
    /// heard.
    fn tiling(
        &mut self,
        run: &mut Running,
        handle: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
    ) -> athanor_workspace_tiling::athanor_workspace_tiling_v1::AthanorWorkspaceTilingV1 {
        let manager: athanor_workspace_tiling::athanor_workspace_tiling_manager_v1::AthanorWorkspaceTilingManagerV1 =
            self.bind(1);
        let qh = self.queue.handle();
        let object = manager.get_tiling(handle, &qh, ());
        self.roundtrip(run);
        object
    }

    /// The events the tiling object `object` was sent, in order: a state, or `None` for
    /// `removed`.
    fn tilings_of(
        &self,
        object: &athanor_workspace_tiling::athanor_workspace_tiling_v1::AthanorWorkspaceTilingV1,
    ) -> Vec<Option<u32>> {
        self.client
            .tilings
            .iter()
            .filter(|(id, _)| *id == object.id())
            .map(|(_, state)| *state)
            .collect()
    }
}

const FLOATING: Option<u32> = Some(0);
const TILED: Option<u32> = Some(1);

/// `athanor_workspace_tiling_manager_v1` is privileged (OS4, D12): the fixture's bar, granted
/// it, sees it; the bar's identity inside a security context, the control center and a client
/// with no line do not.
#[test]
fn os4_only_the_bar_is_granted_tiling() {
    const MANAGER: &str = "athanor_workspace_tiling_manager_v1";
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

/// A tiling object is sent its workspace's state at once and at every change, whoever made
/// it; its `set_state` applies at once, and an object of another workspace hears nothing.
#[test]
fn the_tiling_state_follows_the_toggle() {
    let mut run = Running::start(None);
    let (mut local, _manager) = workspace_client(&mut run);
    let order = heard(&local.client, 0);
    let (first, second) = (
        workspace_handle(&local.client, &order[0].0),
        workspace_handle(&local.client, &order[1].0),
    );
    let object = local.tiling(&mut run, &first);
    let other = local.tiling(&mut run, &second);
    assert_eq!(local.tilings_of(&object), [FLOATING], "sent at once");
    let id = first_workspace(&run);

    object.set_state(athanor_workspace_tiling::athanor_workspace_tiling_v1::State::Tiled);
    local.roundtrip(&mut run);
    assert!(run.state.is_tiled(id), "applied at once");
    assert_eq!(local.tilings_of(&object), [FLOATING, TILED]);

    run.state.set_tiling(id, false);
    local.roundtrip(&mut run);
    assert_eq!(local.tilings_of(&object), [FLOATING, TILED, FLOATING], "whoever changes it");
    assert_eq!(local.tilings_of(&other), [FLOATING], "another workspace hears nothing");
    run.stop();
}

/// A workspace removed ends its tiling objects with `removed`, and a `set_state` on them
/// afterwards tiles nothing, as for any workspace that is gone.
#[test]
fn a_workspace_gone_ends_its_tiling_object() {
    let mut run = Running::start(None);
    let (mut watcher, _manager) = workspace_client(&mut run);
    let mut app = Local::connect(&mut run, None);
    let other = app.another(&mut run, "moved");
    let window = app.server_window(&run, &other.surface);
    let list = workspaces_of(&run, 0);
    assert!(run.state.move_window_to(&window, list[1]));
    watcher.roundtrip(&mut run);
    let order = heard(&watcher.client, 0);
    let second = workspace_handle(&watcher.client, &order[1].0);
    let object = watcher.tiling(&mut run, &second);
    assert_eq!(watcher.tilings_of(&object), [FLOATING]);

    assert!(run.state.move_window_to(&window, list[0]));
    watcher.roundtrip(&mut run);
    assert_eq!(watcher.tilings_of(&object), [FLOATING, None], "removed");

    object.set_state(athanor_workspace_tiling::athanor_workspace_tiling_v1::State::Tiled);
    watcher.roundtrip(&mut run);
    assert!(!run.state.is_tiled(list[1]), "inert");
    assert_eq!(watcher.tilings_of(&object), [FLOATING, None]);
    let late = watcher.tiling(&mut run, &second);
    assert_eq!(watcher.tilings_of(&late), [None], "made for a workspace already gone");
    run.stop();
}

/// A second tiling object of one workspace from one client is the `already_made` error, which
/// bounds what each change walks; another client makes its own.
#[test]
fn a_second_tiling_object_of_a_workspace_is_an_error() {
    let mut run = Running::start(None);
    let (mut other, _other_manager) = workspace_client(&mut run);
    let (mut local, _manager) = workspace_client(&mut run);
    let first = workspace_handle(&local.client, &heard(&local.client, 0)[0].0);
    let object = local.tiling(&mut run, &first);
    let theirs = workspace_handle(&other.client, &heard(&other.client, 0)[0].0);
    let their_object = other.tiling(&mut run, &theirs);
    assert_eq!(other.tilings_of(&their_object), [FLOATING], "another client's own");
    assert_eq!(local.tilings_of(&object), [FLOATING]);
    assert_eq!(run.state.workspace_tilings.len(), 2);

    let manager: athanor_workspace_tiling::athanor_workspace_tiling_manager_v1::AthanorWorkspaceTilingManagerV1 =
        local.bind(1);
    let qh = local.queue.handle();
    let _second = manager.get_tiling(&first, &qh, ());
    local.connection.flush().expect("flush");
    run.until("the client is disconnected", |state| {
        state.workspace_tilings.len() == 1
    });
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
    assert_eq!(error.object_interface, "athanor_workspace_tiling_manager_v1");
    assert_eq!(
        error.code,
        athanor_workspace_tiling::athanor_workspace_tiling_manager_v1::Error::AlreadyMade as u32
    );
    run.stop();
}

/// The halves of the first output's left tile, one above the other (D2: it is higher than wide).
const LEFT_TOP: (i32, i32, i32, i32) = (8, 8, 628, 388);
const LEFT_BOTTOM: (i32, i32, i32, i32) = (8, 404, 628, 388);
/// The halves of the first output under a bar 30 high (`Local::bar`).
const LEFT_UNDER_BAR: (i32, i32, i32, i32) = (8, 38, 628, 754);
const RIGHT_UNDER_BAR: (i32, i32, i32, i32) = (644, 38, 628, 754);
/// The whole first output, where a window is maximised or full screen.
const OUTPUT: (i32, i32, i32, i32) = (0, 0, 1280, 800);

/// Two windows tiled side by side on the first output's workspace, each answered.
struct Pair {
    lower: Local,
    upper: Local,
    upper_toplevel: xdg_toplevel::XdgToplevel,
    below: Window,
    above: Window,
    id: WorkspaceId,
}

fn tiled_pair(run: &mut Running) -> Pair {
    without_the_fixture(run);
    let (mut lower, _, below) = window(run);
    let (mut upper, upper_toplevel, above) = window(run);
    let id = first_workspace(run);
    answer_all(run, &mut [&mut upper, &mut lower], |run| {
        run.state.set_tiling(id, true)
    });
    assert_eq!(geometry(run, &above), rect(LEFT));
    assert_eq!(geometry(run, &below), rect(RIGHT));
    Pair {
        lower,
        upper,
        upper_toplevel,
        below,
        above,
        id,
    }
}

/// How many windows the tree of `id` holds.
fn tiles(run: &Running, id: WorkspaceId) -> usize {
    run.state
        .tilings
        .iter()
        .find(|(kept, _)| *kept == id)
        .map_or(0, |(_, tree)| tree.windows().len())
}

/// The rectangle the tree of `id` gives `window` in its workspace's area.
fn tile_of(run: &Running, id: WorkspaceId, window: &Window) -> Rectangle<i32, Logical> {
    let area = run
        .state
        .output_of_workspace(id)
        .and_then(|output| run.state.window_area_on(&output))
        .expect("an area");
    let (_, tree) = run
        .state
        .tilings
        .iter()
        .find(|(kept, _)| *kept == id)
        .expect("a tree");
    tree.layout(area)
        .into_iter()
        .find(|(kept, _)| *kept == window)
        .map(|(_, tile)| tile)
        .expect("a tile")
}

/// Whether `local` was sent a configure since it last answered one, after a roundtrip.
fn configured(run: &mut Running, local: &mut Local) -> bool {
    local.roundtrip(run);
    local.client.configure.is_some()
}

/// Where `placed` puts a window of the fixture's size on its home output.
fn placed_new(run: &Running, window: &Window) -> Rectangle<i32, Logical> {
    let area = run.state.home_area(window).expect("an area");
    let size = (WIDTH, HEIGHT).into();
    let at = crate::state::placed(area, Rectangle::new((0, 0).into(), size));
    Rectangle::new(at, size)
}

impl Local {
    /// A toplevel of a new surface, before its first commit; `redraw` maps it at the size of its
    /// initial configure.
    fn unborn(&mut self) -> (wl_surface::WlSurface, xdg_toplevel::XdgToplevel) {
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let wm_base: xdg_wm_base::XdgWmBase = self.bind(1);
        let seat: wl_seat::WlSeat = self.bind(1);
        let qh = self.queue.handle();
        self.pointer = Some(seat.get_pointer(&qh, ()));
        self.seat = Some(seat);
        let surface = compositor.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        self.client.configure = None;
        self.window = Some((surface.clone(), xdg));
        (surface, toplevel)
    }

    /// Answers the next configure with a buffer of `width` by `height`, whatever it asked.
    fn draw_at(&mut self, run: &mut Running, width: i32, height: i32) {
        self.pump(run, "the window is configured", |client, _| {
            client.configure.is_some()
        });
        let (surface, xdg) = self.window.clone().expect("a window");
        xdg.ack_configure(self.client.configure.take().expect("a configure"));
        self.client.toplevel_configure = None;
        let shm: wl_shm::WlShm = self.bind(1);
        let (buffer, _) = self.buffer(&shm, width, height, [0, 0, 0, 255]);
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        self.roundtrip(run);
    }

    /// Commits a null buffer: the window hides.
    fn hide(&mut self, run: &mut Running) {
        let (surface, _) = self.window.clone().expect("a window");
        surface.attach(None, 0, 0);
        surface.commit();
        self.roundtrip(run);
    }

    /// Presses the left button at `at`, over this client's window: the serial it was sent.
    fn press_at(&mut self, run: &mut Running, at: Point<f64, Logical>) -> u32 {
        self.client.button = None;
        press(run, at);
        self.pump(run, "the press", |client, _| client.button.is_some());
        self.client.button.take().expect("a press")
    }
}

/// A move of `toplevel`, granted for its client's press at `at`.
fn move_tile(
    run: &mut Running,
    local: &mut Local,
    toplevel: &xdg_toplevel::XdgToplevel,
    at: Point<f64, Logical>,
) {
    let serial = local.press_at(run, at);
    let seat = local.seat.clone().expect("a seat");
    toplevel._move(&seat, serial);
    local.roundtrip(run);
    assert!(moving(run), "the move is granted");
}

/// A resize of `toplevel` by its bottom right corner, granted for its client's press at `at`.
fn resize_tile(
    run: &mut Running,
    local: &mut Local,
    toplevel: &xdg_toplevel::XdgToplevel,
    at: Point<f64, Logical>,
) {
    let serial = local.press_at(run, at);
    let seat = local.seat.clone().expect("a seat");
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    local.roundtrip(run);
}

fn maximised(window: &Window) -> bool {
    crate::state::is_maximised(window)
}

/// A window born on a tiled workspace, then floating when the toggle turns off, is placed as a
/// new window is, at the size it answers with (D7).
#[test]
fn a_window_born_tiled_is_placed_when_tiling_ends() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut upper, _, _) = window(&mut run);
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, true));
    let mut born = Local::connect(&mut run, None);
    let (surface, _) = born.unborn();
    surface.commit();
    born.redraw(&mut run);
    let window = born.server_window(&run, &surface);
    assert!(run.state.is_tile(&window), "born tiled");
    upper.redraw(&mut run);

    let answers = answer_all(&mut run, &mut [&mut born, &mut upper], |run| {
        run.state.set_tiling(id, false)
    });

    assert_eq!((answers[0].0, answers[0].1), (0, 0), "its own size");
    assert_eq!(tiled_states(&window), 0);
    assert_eq!(geometry(&run, &window), placed_new(&run, &window));
    run.stop();
}

/// A bar's frame lays the tiles out, and a tile whose size did not change is sent nothing, nor
/// is one whose client answered with another size.
#[test]
fn a_bar_frame_sends_no_configure() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        ..
    } = tiled_pair(&mut run);
    let mut shell = Local::connect(&mut run, None);
    upper.roundtrip(&mut run);
    lower.roundtrip(&mut run);
    upper.client.configure = None;
    lower.client.configure = None;
    let bar = shell.bar(&mut run, 30);
    upper.redraw(&mut run);
    lower.draw_at(&mut run, 600, 700);

    bar.commit();
    shell.roundtrip(&mut run);

    assert!(!configured(&mut run, &mut upper), "the same size");
    assert!(!configured(&mut run, &mut lower), "another size answered");
    run.stop();
}

/// A bar's exclusive zone takes its height off the tiles, and gives it back when it goes.
#[test]
fn a_bar_zone_narrows_the_tiles() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        below,
        above,
        ..
    } = tiled_pair(&mut run);
    let mut shell = Local::connect(&mut run, None);
    let mut bar = None;

    let answers = answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        bar = Some(shell.bar(run, 30));
    });
    assert_eq!(
        (answers[0].0, answers[0].1),
        (LEFT_UNDER_BAR.2, LEFT_UNDER_BAR.3)
    );
    assert_eq!(geometry(&run, &above), rect(LEFT_UNDER_BAR));
    assert_eq!(geometry(&run, &below), rect(RIGHT_UNDER_BAR));

    // The bar's client goes, and its layer surface with it (`layer_destroyed`).
    answer_all(&mut run, &mut [&mut upper, &mut lower], move |_| {
        drop((bar, shell));
    });
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert_eq!(geometry(&run, &below), rect(RIGHT));
    run.stop();
}

/// An output that takes another size lays its tiles out again.
#[test]
fn a_tile_fits_a_changed_output() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        below,
        above,
        ..
    } = tiled_pair(&mut run);

    let answers = answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        resize_first(run, 1000, 800)
    });

    assert_eq!((answers[0].0, answers[1].0), (488, 488));
    assert_eq!(geometry(&run, &above), rect((8, 8, 488, 784)));
    assert_eq!(geometry(&run, &below), rect((504, 8, 488, 784)));
    run.stop();
}

/// A client that does not answer its tile's configure stays where it stood, and goes to the
/// tile when it answers: a layout never waits on a client (Review Focus 2).
#[test]
fn a_client_slow_to_answer_keeps_its_place() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut lower, _, below) = window(&mut run);
    let (mut upper, _, above) = window(&mut run);
    let id = first_workspace(&run);
    let before = geometry(&run, &below);
    lower.roundtrip(&mut run);
    lower.client.configure = None;

    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, true));
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert_eq!(geometry(&run, &below), before, "not answered yet");
    run.state.fit_tiles();
    assert_eq!(geometry(&run, &below), before, "a second layout does not move it");

    lower.redraw(&mut run);
    assert_eq!(geometry(&run, &below), rect(RIGHT));
    run.stop();
}

/// A new window on a tiled workspace is sent its tile's size in its initial configure and maps
/// at its tile, with no second configure; the window it splits takes the other half. The
/// output is wide enough that the tile it splits is narrow and its halves side by side, so a
/// cascade from that tile would send the new window a lower height (`spawn`); a whole tile is
/// too wide to cascade from.
#[test]
fn a_new_window_first_shows_at_its_tile() {
    // The halves of the first output 2560 wide, and those of its left half, the inner one
    // wider by the half gap its edge keeps (`gapped`).
    const LEFT: (i32, i32, i32, i32) = (8, 8, 1268, 784);
    const RIGHT: (i32, i32, i32, i32) = (1284, 8, 1268, 784);
    const LEFT_LEFT: (i32, i32, i32, i32) = (8, 8, 628, 784);
    const LEFT_RIGHT: (i32, i32, i32, i32) = (644, 8, 632, 784);
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    resize_first(&mut run, 2560, 800);
    // `placement_area` reads the zones the layer map leaves, laid out again for the new size.
    smithay::desktop::layer_map_for_output(run.state.space.outputs().next().expect("an output"))
        .arrange();
    let (mut lower, _, below) = window(&mut run);
    let (mut upper, _, above) = window(&mut run);
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        run.state.set_tiling(id, true)
    });
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert_eq!(geometry(&run, &below), rect(RIGHT));
    let mut new = Local::connect(&mut run, None);
    let (surface, _) = new.unborn();
    surface.commit();

    let (width, height, _) = new.redraw(&mut run);

    assert_eq!((width, height), (LEFT_RIGHT.2, LEFT_RIGHT.3), "the initial configure");
    let window = new.server_window(&run, &surface);
    assert_eq!(tiled_states(&window), 4);
    assert_eq!(geometry(&run, &window), rect(LEFT_RIGHT), "at its first commit");
    assert!(!configured(&mut run, &mut new), "no second configure");
    let (width, _, _) = upper.redraw(&mut run);
    assert_eq!(width, LEFT_LEFT.2);
    assert_eq!(geometry(&run, &above), rect(LEFT_LEFT));
    assert_eq!(geometry(&run, &below), rect(RIGHT));
    run.stop();
}

/// A window taken into a tile leaves the cascade, as one the user moved does: once the
/// workspace floats again, the next new window is placed as the first, not cascaded from a
/// window that held a tile (`spawn`).
#[test]
fn a_tiling_ends_the_cascade() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        id,
        ..
    } = tiled_pair(&mut run);
    answer_all(&mut run, &mut [&mut upper, &mut lower], |run| {
        run.state.set_tiling(id, false)
    });

    let (_, _, window) = window(&mut run);
    assert_eq!(geometry(&run, &window), placed_new(&run, &window));
    run.stop();
}

/// A window that commits without ever drawing takes no tile: the tiles stay as they are.
#[test]
fn a_window_that_never_draws_holds_no_tile() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let mut new = Local::connect(&mut run, None);
    let (surface, _) = new.unborn();
    surface.commit();
    new.pump(&mut run, "the initial configure", |client, _| {
        client.configure.is_some()
    });

    assert_eq!(tiles(&run, pair.id), 2);
    assert!(!configured(&mut run, &mut pair.upper));
    assert!(!configured(&mut run, &mut pair.lower));
    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    assert_eq!(geometry(&run, &pair.below), rect(RIGHT));
    run.stop();
}

/// Every way a window's place on a workspace ends takes it out of the tree: its destruction, a
/// null buffer, a minimise, a move to another workspace, a drop on another output, and the
/// removal of its output (Review Focus 1).
#[test]
fn no_end_leaves_a_tile_behind() {
    let mut run = Running::start(None);
    let pair = tiled_pair(&mut run);
    let id = pair.id;
    let output = output_at(&run, 0);
    let next = run.state.workspaces(&output)[1];

    let (mut destroyed, toplevel, gone) = window(&mut run);
    assert_eq!(tiles(&run, id), 3, "a new window enters");
    toplevel.destroy();
    destroyed.roundtrip(&mut run);
    assert_eq!(tiles(&run, id), 2, "destroyed");
    assert!(!run.state.is_tile(&gone));

    let (mut hidden, _, hiding) = window(&mut run);
    hidden.hide(&mut run);
    assert!(!run.state.is_tile(&hiding), "a null buffer");

    let (_minimised, _, minimising) = window(&mut run);
    assert!(run.state.minimise(&minimising));
    assert!(!run.state.is_tile(&minimising), "minimised");

    let (_moved, _, moving_on) = window(&mut run);
    assert!(run.state.move_window_to(&moving_on, next));
    assert!(!run.state.is_tile(&moving_on), "moved to another workspace");

    let (_dropped, _, dropping) = window(&mut run);
    let second = output_at(&run, 1);
    let shown = run.state.space.output_geometry(&second).expect("placed");
    run.state
        .space
        .relocate_element(&dropping, shown.loc + Point::from((10, 10)));
    run.state.settle(&dropping);
    let there = run.state.active_workspace(&second).expect("a workspace");
    assert_eq!(run.state.workspace_of(&dropping), Some(there));
    assert!(!run.state.is_tile(&dropping), "dropped on another output");
    assert_eq!(tiles(&run, id), 2);

    run.state.set_tiling(there, true);
    assert!(run.state.is_tile(&dropping));
    run.state.set_tiling(id, false);
    run.state.remove_output(&second);
    assert!(!run.state.is_tile(&dropping), "its output removed");
    assert_eq!(tiled_states(&dropping), 0);
    assert!(run.state.tilings.is_empty());
    run.stop();
}

/// A tile that hides with a null buffer gives its space to its sibling, and takes a tile again
/// at its next draw.
#[test]
fn a_hidden_tile_gives_up_its_place() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        below,
        above,
        id,
        ..
    } = tiled_pair(&mut run);
    upper.client.configure = None;

    lower.hide(&mut run);
    let (width, _, _) = upper.redraw(&mut run);

    assert_eq!(width, WHOLE.2);
    assert_eq!(geometry(&run, &above), rect(WHOLE));
    assert_eq!(tiles(&run, id), 1);
    let (width, _, _) = lower.redraw(&mut run);
    assert_eq!(width, RIGHT.2, "its initial configure again, with its tile");
    assert!(run.state.is_tile(&below));
    upper.redraw(&mut run);
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert_eq!(geometry(&run, &below), rect(RIGHT));
    run.stop();
}

/// A full-screen tile keeps its tile, its neighbour does not change, and it returns to the tile
/// when it leaves full screen (D6, R4).
#[test]
fn a_full_screen_tile_returns_to_its_tile() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        below,
        above,
        ..
    } = tiled_pair(&mut run);

    answer_all(&mut run, &mut [&mut upper], |run| {
        run.state.full_screen(&above, None)
    });
    assert_eq!(geometry(&run, &above), rect(OUTPUT));
    assert!(run.state.is_tile(&above));
    assert!(!configured(&mut run, &mut lower), "the neighbour does not change");
    assert_eq!(geometry(&run, &below), rect(RIGHT));

    let answers = answer_all(&mut run, &mut [&mut upper], |run| {
        run.state.leave_full_screen(&above)
    });
    assert_eq!((answers[0].0, answers[0].1), (LEFT.2, LEFT.3));
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert_eq!(tiled_states(&above), 4);
    run.stop();
}

/// A new tile restores a maximised window of its workspace, which takes its tile (D6).
#[test]
fn a_new_tile_restores_a_maximised_window() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut upper, _, above) = window(&mut run);
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, true));
    answer_all(&mut run, &mut [&mut upper], |run| run.state.maximise(&above));
    assert_eq!(geometry(&run, &above), rect(OUTPUT));
    upper.client.configure = None;

    let (_new, _, window) = window(&mut run);
    let (width, _, _) = upper.redraw(&mut run);

    assert_eq!(width, LEFT.2);
    assert!(!maximised(&above));
    assert_eq!(geometry(&run, &above), rect(LEFT));
    assert!(run.state.is_tile(&window));
    run.stop();
}

/// A window maximised before its first commit keeps that size and takes a tile behind it, and
/// goes to that tile when restored; one maximised while a layout's move waits goes to the tile
/// that move was for when restored (D6).
#[test]
fn a_window_maximised_before_its_first_commit_keeps_that_size() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut upper, upper_toplevel, above) = window(&mut run);
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, true));
    let mut new = Local::connect(&mut run, None);
    let (surface, toplevel) = new.unborn();
    toplevel.set_maximized();
    surface.commit();

    let (width, height, _) = new.redraw(&mut run);

    assert_eq!((width, height), (OUTPUT.2, OUTPUT.3), "the maximised size");
    let window = new.server_window(&run, &surface);
    assert_eq!(geometry(&run, &window), rect(OUTPUT));
    assert!(run.state.is_tile(&window));
    upper.redraw(&mut run);
    assert_eq!(geometry(&run, &above), rect(LEFT));
    let answers = answer_all(&mut run, &mut [&mut new], |run| {
        run.state.restore(&window);
    });
    assert_eq!((answers[0].0, answers[0].1), (RIGHT.2, RIGHT.3));
    assert_eq!(geometry(&run, &window), rect(RIGHT));

    // The upper window's end gives the new one the whole area, and it is maximised before it
    // answers that move.
    new.roundtrip(&mut run);
    new.client.configure = None;
    upper_toplevel.destroy();
    upper.roundtrip(&mut run);
    run.state.maximise(&window);
    // Both configures in, so that the maximise's is the one answered.
    new.roundtrip(&mut run);
    new.redraw(&mut run);
    assert_eq!(geometry(&run, &window), rect(OUTPUT));
    let answers = answer_all(&mut run, &mut [&mut new], |run| {
        run.state.restore(&window);
    });
    assert_eq!((answers[0].0, answers[0].1), (WHOLE.2, WHOLE.3));
    assert_eq!(geometry(&run, &window), rect(WHOLE));
    run.stop();
}

/// A window moved to a tiled workspace not shown splits the tile focused last there, and takes
/// its tile when the workspace is shown (D2, D9).
#[test]
fn a_move_to_a_workspace_not_shown_splits_its_last_focused_tile() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let output = output_at(&run, 0);
    let (_first, _, x) = window(&mut run);
    let (mut second, _, y) = window(&mut run);
    let (mut third, _, w) = window(&mut run);
    let next = run.state.workspaces(&output)[1];
    assert!(run.state.move_window_to(&x, next));
    assert!(run.state.move_window_to(&y, next));
    second.roundtrip(&mut run);
    second.client.configure = None;
    run.state.set_tiling(next, true);
    second.redraw(&mut run);

    answer_all(&mut run, &mut [&mut third, &mut second], |run| {
        assert!(run.state.move_window_to(&w, next));
        assert!(run.state.activate_workspace(next));
    });

    assert_eq!(geometry(&run, &y), rect(LEFT_TOP));
    assert_eq!(geometry(&run, &w), rect(LEFT_BOTTOM));
    assert_eq!(tiles(&run, next), 3);
    run.stop();
}

/// A tile dragged and dropped on its own output goes back to its tile.
#[test]
fn a_dropped_tile_goes_back_to_the_layout() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((28.0, 18.0));
    move_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());
    assert_eq!(geometry(&run, &pair.above).loc, (108, 58).into(), "it follows");

    release(&mut run);
    pair.upper.roundtrip(&mut run);

    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    run.stop();
}

/// A drop that moves the tiles at once gives the pointer to the client under it once they
/// moved: on the other tile, the dragged one, which takes that tile's place (D11).
#[test]
fn a_dropped_tile_reaches_the_client_under_the_pointer() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((28.0, 18.0));
    move_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((700.0, 0.0)), InputTime::now());
    pair.upper.roundtrip(&mut run);
    pair.upper.client.pointer_enter = None;

    release(&mut run);

    pair.upper
        .pump(&mut run, "the pointer enters the dropped tile", |client, _| {
            client.pointer_enter.is_some()
        });
    assert_eq!(geometry(&run, &pair.above), rect(RIGHT));
    run.stop();
}

/// The lock ends a move: the tile goes back to its tile.
#[test]
fn a_grab_replaced_by_the_lock_leaves_no_tile_astray() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((28.0, 18.0));
    move_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());

    let (_locker, _lock) = lock_session(&mut run);

    assert!(!moving(&run));
    assert!(!crate::grabs::is_held(&pair.above));
    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    run.stop();
}

/// A move replaced by another of the same tile keeps it out of the layout until the last ends.
#[test]
fn a_second_grab_keeps_the_tile_out_of_the_layout() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((28.0, 18.0));
    let serial = pair.upper.press_at(&mut run, at);
    let seat = pair.upper.seat.clone().expect("a seat");
    pair.upper_toplevel._move(&seat, serial);
    pair.upper.roundtrip(&mut run);
    pair.upper_toplevel._move(&seat, serial);
    pair.upper.roundtrip(&mut run);
    assert!(moving(&run));
    run.state
        .pointer_to(at + Point::from((100.0, 50.0)), InputTime::now());

    run.state.tile_layout(pair.id);

    assert!(crate::grabs::is_held(&pair.above));
    assert_eq!(geometry(&run, &pair.above).loc, (108, 58).into(), "held");
    release(&mut run);
    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    run.stop();
}

/// A tile resized by its client ends at its tile when the resize ends.
#[test]
fn a_resized_tile_ends_at_its_tile() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((600.0, 700.0));
    resize_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((30.0, -100.0)), InputTime::now());
    let (width, height, _) = pair.upper.redraw(&mut run);
    assert_eq!((width, height), (LEFT.2 + 30, LEFT.3 - 100), "resized");

    release(&mut run);
    let (width, height, _) = pair.upper.redraw(&mut run);

    let tile = tile_of(&run, pair.id, &pair.above);
    assert_eq!((width, height), (tile.size.w, tile.size.h));
    assert_eq!(geometry(&run, &pair.above), tile);
    run.stop();
}

/// A bar's frame during a resize leaves the resized tile alone.
#[test]
fn a_bar_frame_leaves_a_resize_alone() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let mut shell = Local::connect(&mut run, None);
    let mut bar = None;
    answer_all(&mut run, &mut [&mut pair.upper, &mut pair.lower], |run| {
        bar = Some(shell.bar(run, 30));
    });
    let at = Point::from((600.0, 700.0));
    resize_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((30.0, -100.0)), InputTime::now());
    pair.upper.redraw(&mut run);
    let resized = geometry(&run, &pair.above);

    bar.as_ref().expect("a bar").commit();
    shell.roundtrip(&mut run);

    assert!(!configured(&mut run, &mut pair.upper));
    assert_eq!(geometry(&run, &pair.above), resized);
    release(&mut run);
    run.stop();
}

/// A window hidden by a null buffer takes no tile at the toggle (D7).
#[test]
fn a_hidden_window_takes_no_tile_at_the_toggle() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut lower, _, below) = window(&mut run);
    let (mut upper, _, above) = window(&mut run);
    lower.hide(&mut run);
    let id = first_workspace(&run);

    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, true));

    assert_eq!(geometry(&run, &above), rect(WHOLE));
    assert!(!run.state.is_tile(&below));
    assert_eq!(tiles(&run, id), 1);
    run.stop();
}

/// A tile on a workspace not shown that hides leaves the tree, and the workspace is laid out
/// without it when it is shown.
#[test]
fn a_parked_tile_that_hides_leaves_the_tree() {
    let mut run = Running::start(None);
    let Pair {
        mut lower,
        mut upper,
        below,
        above,
        id,
        ..
    } = tiled_pair(&mut run);
    let output = output_at(&run, 0);
    let next = run.state.workspaces(&output)[1];
    assert!(run.state.activate_workspace(next));
    assert!(run.state.is_parked(&below));
    upper.roundtrip(&mut run);
    upper.client.configure = None;

    lower.hide(&mut run);

    assert!(!run.state.is_tile(&below));
    assert_eq!(tiles(&run, id), 1);
    assert!(run.state.activate_workspace(id));
    upper.redraw(&mut run);
    assert_eq!(geometry(&run, &above), rect(WHOLE));
    run.stop();
}

/// A window sent its tile in its initial configure floats when the toggle turns off before its
/// first buffer: a configure without the tiled states, at its own size, and it is placed then.
#[test]
fn a_window_sent_a_tile_floats_when_the_toggle_beats_its_buffer() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (mut upper, _, _) = window(&mut run);
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, true));
    let mut new = Local::connect(&mut run, None);
    let (surface, _) = new.unborn();
    surface.commit();
    new.pump(&mut run, "the initial configure", |client, _| {
        client.configure.is_some()
    });
    answer_all(&mut run, &mut [&mut upper], |run| run.state.set_tiling(id, false));

    let (width, _, _) = new.redraw(&mut run);
    assert_eq!(width, RIGHT.2, "the tile it was sent");
    let window = new.server_window(&run, &surface);
    assert_eq!(tiled_states(&window), 0);
    let (width, height, _) = new.redraw(&mut run);

    assert_eq!((width, height), (0, 0), "its own size");
    assert_eq!(geometry(&run, &window), placed_new(&run, &window));
    assert!(!run.state.is_tile(&window));
    run.stop();
}

/// A request served before a window's first commit moves it to a tiled workspace without
/// entering its tree, and sends no configure early.
#[test]
fn a_request_before_the_first_commit_enters_no_tree() {
    let mut run = Running::start(None);
    let second = output_at(&run, 1);
    let there = run.state.active_workspace(&second).expect("a workspace");
    run.state.set_tiling(there, true);
    let mut new = Local::connect(&mut run, None);
    let (_surface, toplevel) = new.unborn();
    let output = new.output(1);
    new.roundtrip(&mut run);

    toplevel.set_fullscreen(Some(&output));
    new.roundtrip(&mut run);

    assert_eq!(tiles(&run, there), 0);
    assert!(
        new.client.configure.is_none(),
        "no configure before the first commit"
    );
    run.stop();
}

/// The toggle keeps the order of the stack: the next window splits the topmost tile (D7).
#[test]
fn the_toggle_keeps_the_order_of_the_stack() {
    let mut run = Running::start(None);
    without_the_fixture(&mut run);
    let (_a, _, _) = window(&mut run);
    let (_b, _, _) = window(&mut run);
    let (mut c, _, top) = window(&mut run);
    let id = first_workspace(&run);
    answer_all(&mut run, &mut [&mut c], |run| run.state.set_tiling(id, true));
    assert_eq!(geometry(&run, &top), rect(LEFT));

    let mut new = Local::connect(&mut run, None);
    let (surface, _) = new.unborn();
    surface.commit();
    new.redraw(&mut run);
    c.redraw(&mut run);

    let window = new.server_window(&run, &surface);
    assert_eq!(geometry(&run, &top), rect(LEFT_TOP));
    assert_eq!(geometry(&run, &window), rect(LEFT_BOTTOM));
    run.stop();
}

/// The place of `window` in the stack, from the bottom.
fn level(run: &Running, window: &Window) -> usize {
    stack(run)
        .iter()
        .position(|kept| kept == window)
        .expect("in the stack")
}

/// A dialog takes no tile and stands above its parent, whose tile stays as it was, and stays
/// above it when the parent is clicked (D5).
#[test]
fn a_dialog_floats_above_its_parent() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let (surface, toplevel) = pair.upper.unborn();
    toplevel.set_parent(Some(&pair.upper_toplevel));
    surface.commit();

    let (width, height, _) = pair.upper.redraw(&mut run);

    assert_eq!((width, height), (0, 0), "no tile in the initial configure");
    let dialog = pair.upper.server_window(&run, &surface);
    assert_eq!(tiles(&run, pair.id), 2);
    assert_eq!(tiled_states(&dialog), 0);
    assert!(!configured(&mut run, &mut pair.lower));
    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    assert_eq!(geometry(&run, &pair.below), rect(RIGHT));
    assert!(level(&run, &dialog) > level(&run, &pair.above), "above its parent");
    run.state.focus(&pair.above);
    assert!(
        level(&run, &dialog) > level(&run, &pair.above),
        "still above its parent once the parent is clicked"
    );
    run.stop();
}

/// A dialog stays above the tiles when another tile is clicked: a tile never covers a window D5
/// floats.
#[test]
fn a_click_on_another_tile_leaves_a_dialog_on_top() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let (surface, toplevel) = pair.upper.unborn();
    toplevel.set_parent(Some(&pair.upper_toplevel));
    surface.commit();
    pair.upper.redraw(&mut run);
    let dialog = pair.upper.server_window(&run, &surface);

    press(&mut run, Point::from((1200.0, 700.0)));
    release(&mut run);
    pair.lower.roundtrip(&mut run);

    assert_eq!(stack(&run).last(), Some(&dialog), "the dialog is on top");
    assert!(level(&run, &pair.below) > level(&run, &pair.above), "the click raised the tile");
    run.stop();
}

/// A window whose minimum and maximum size are one size takes no tile, and stands above the
/// tiles though it did not take the keyboard (D5).
#[test]
fn a_window_of_one_size_floats() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let mut new = Local::connect(&mut run, None);
    let (surface, toplevel) = new.unborn();
    toplevel.set_min_size(WIDTH, HEIGHT);
    toplevel.set_max_size(WIDTH, HEIGHT);
    surface.commit();

    let (width, height, _) = new.redraw(&mut run);

    assert_eq!((width, height), (0, 0), "no tile in the initial configure");
    let window = new.server_window(&run, &surface);
    assert_eq!(tiles(&run, pair.id), 2);
    assert_eq!(tiled_states(&window), 0);
    assert_eq!(geometry(&run, &window), placed_new(&run, &window));
    let tiles_top = level(&run, &pair.above).max(level(&run, &pair.below));
    assert!(level(&run, &window) > tiles_top, "above the tiles");
    assert!(!configured(&mut run, &mut pair.upper));
    assert!(!configured(&mut run, &mut pair.lower));
    run.stop();
}

/// A tile of a new client, beneath the upper one, then `change`, which D5 floats: it leaves its
/// tile with the floating configure of a window born tiled, is placed at the size it answers
/// with, above the tiles, and the upper tile takes its space back.
fn leaves_its_tile(
    change: impl FnOnce(
        &mut Running,
        &mut Local,
        &xdg_toplevel::XdgToplevel,
        &wl_surface::WlSurface,
    ),
) {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let mut new = Local::connect(&mut run, None);
    let (surface, toplevel) = new.unborn();
    surface.commit();
    new.redraw(&mut run);
    pair.upper.redraw(&mut run);
    let window = new.server_window(&run, &surface);
    assert_eq!(geometry(&run, &window), rect(LEFT_BOTTOM));

    change(&mut run, &mut new, &toplevel, &surface);
    new.roundtrip(&mut run);

    assert_eq!(tiles(&run, pair.id), 2, "it left its tile");
    let (width, height, _) = new.redraw(&mut run);
    assert_eq!((width, height), (0, 0), "no floating size: it was born tiled");
    assert_eq!(tiled_states(&window), 0);
    assert_eq!(geometry(&run, &window), placed_new(&run, &window));
    let tiles_top = level(&run, &pair.above).max(level(&run, &pair.below));
    assert!(level(&run, &window) > tiles_top, "above the tiles");
    let (width, height, _) = pair.upper.redraw(&mut run);
    assert_eq!((width, height), (LEFT.2, LEFT.3));
    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    run.stop();
}

/// A tile given a parent leaves its tile (D5). The parent is a toplevel the client has not
/// mapped, so that the client answers the configures of one window.
#[test]
fn a_window_that_gains_a_parent_leaves_its_tile() {
    leaves_its_tile(|_, new, toplevel, _| {
        let compositor: wl_compositor::WlCompositor = new.bind(4);
        let wm_base: xdg_wm_base::XdgWmBase = new.bind(1);
        let qh = new.queue.handle();
        let parent_surface = compositor.create_surface(&qh, ());
        let parent_xdg = wm_base.get_xdg_surface(&parent_surface, &qh, ());
        let parent = parent_xdg.get_toplevel(&qh, ());
        toplevel.set_parent(Some(&parent));
    });
}

/// A tile that commits a minimum and a maximum of one size leaves its tile (D5).
#[test]
fn a_tile_that_takes_one_size_leaves_its_tile() {
    leaves_its_tile(|_, _, toplevel, surface| {
        toplevel.set_min_size(WIDTH, HEIGHT);
        toplevel.set_max_size(WIDTH, HEIGHT);
        surface.commit();
    });
}

/// A tile dropped on the other tile swaps places with it (D11); of one size, both move at once.
#[test]
fn dropping_a_tile_on_a_tile_swaps_them() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((28.0, 18.0));
    move_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((700.0, 0.0)), InputTime::now());

    release(&mut run);
    pair.upper.roundtrip(&mut run);

    assert_eq!(geometry(&run, &pair.above), rect(RIGHT));
    assert_eq!(geometry(&run, &pair.below), rect(LEFT));
    assert_eq!(tile_of(&run, pair.id, &pair.above), rect(RIGHT));
    assert_eq!(tile_of(&run, pair.id, &pair.below), rect(LEFT));
    run.stop();
}

/// A tile dropped on no tile, here in the gap between the two, goes back to its own (D11).
#[test]
fn a_drop_elsewhere_puts_a_tile_back() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((28.0, 18.0));
    move_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(Point::from((640.0, 400.0)), InputTime::now());

    release(&mut run);
    pair.upper.roundtrip(&mut run);

    assert_eq!(geometry(&run, &pair.above), rect(LEFT));
    assert_eq!(geometry(&run, &pair.below), rect(RIGHT));
    assert!(!configured(&mut run, &mut pair.lower));
    run.stop();
}

/// A tile resized by its client moves the split by its right edge, and the tile beside it gives
/// up what it took; its bottom edge, at the area's, has no split to move (D11).
#[test]
fn resizing_a_tile_moves_its_split() {
    const WIDER: (i32, i32, i32, i32) = (8, 8, 658, 784);
    const NARROWER: (i32, i32, i32, i32) = (674, 8, 598, 784);
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let at = Point::from((600.0, 700.0));
    resize_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((30.0, -100.0)), InputTime::now());
    pair.upper.redraw(&mut run);

    let answers = answer_all(&mut run, &mut [&mut pair.upper, &mut pair.lower], release);

    assert_eq!((answers[0].0, answers[0].1), (WIDER.2, WIDER.3));
    assert_eq!((answers[1].0, answers[1].1), (NARROWER.2, NARROWER.3));
    assert_eq!(tile_of(&run, pair.id, &pair.above), rect(WIDER));
    assert_eq!(geometry(&run, &pair.above), rect(WIDER));
    assert_eq!(geometry(&run, &pair.below), rect(NARROWER));
    run.stop();
}

/// A tile whose client takes a minimum size of `i32::MAX` and is resized moves its split no
/// further than the area: every tile stays inside it, and the compositor goes on.
#[test]
fn a_resize_to_any_size_keeps_the_tiles_in_the_area() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let (surface, _) = pair.upper.window.clone().expect("a window");
    pair.upper_toplevel.set_min_size(i32::MAX, i32::MAX);
    surface.commit();
    pair.upper.roundtrip(&mut run);
    let at = Point::from((600.0, 700.0));
    resize_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((30.0, -100.0)), InputTime::now());
    pair.upper.roundtrip(&mut run);

    release(&mut run);
    pair.upper.roundtrip(&mut run);
    pair.lower.roundtrip(&mut run);

    let area = run
        .state
        .output_of_workspace(pair.id)
        .and_then(|output| run.state.window_area_on(&output))
        .expect("an area");
    for window in [&pair.above, &pair.below] {
        let tile = tile_of(&run, pair.id, window);
        assert!(area.contains_rect(tile), "{tile:?} inside {area:?}");
    }
    assert_eq!(tiles(&run, pair.id), 2);
    run.stop();
}

/// A full-screen tile, focused, stays above a window D5 floats on its workspace (D6).
#[test]
fn a_focused_full_screen_tile_stays_above_a_float() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let mut new = Local::connect(&mut run, None);
    let (surface, toplevel) = new.unborn();
    toplevel.set_min_size(WIDTH, HEIGHT);
    toplevel.set_max_size(WIDTH, HEIGHT);
    surface.commit();
    new.redraw(&mut run);
    let window = new.server_window(&run, &surface);
    answer_all(&mut run, &mut [&mut pair.upper], |run| {
        run.state.full_screen(&pair.above, None)
    });

    run.state.focus(&pair.above);

    assert_eq!(stack(&run).last(), Some(&pair.above), "the full-screen tile is on top");
    assert!(level(&run, &window) > level(&run, &pair.below), "above the other tile");
    run.stop();
}

/// A window sent a tile in its initial configure that D5 floats by its first buffer is sent
/// D8's floating configure: no tiled states, and a size of its own choosing.
#[test]
fn a_window_that_floats_by_its_first_buffer_is_sent_no_tile() {
    let mut run = Running::start(None);
    let pair = tiled_pair(&mut run);
    let mut new = Local::connect(&mut run, None);
    let (surface, toplevel) = new.unborn();
    surface.commit();
    toplevel.set_min_size(WIDTH, HEIGHT);
    toplevel.set_max_size(WIDTH, HEIGHT);
    let (width, height, _) = new.redraw(&mut run);
    assert!(width > 0 && height > 0, "a tile in the initial configure");

    let (width, height, _) = new.redraw(&mut run);

    assert_eq!((width, height), (0, 0), "the floating configure");
    let window = new.server_window(&run, &surface);
    assert_eq!(tiled_states(&window), 0);
    assert_eq!(tiles(&run, pair.id), 2);
    assert_eq!(geometry(&run, &window).size, (WIDTH, HEIGHT).into());
    run.stop();
}

/// A tile whose client draws smaller than its tile is resized from its tile: its right edge
/// moves the split by what the pointer moved, not by the edge the client drew (D11).
#[test]
fn a_tile_drawn_smaller_is_resized_from_its_tile() {
    const WIDER: (i32, i32, i32, i32) = (8, 8, 658, 784);
    const NARROWER: (i32, i32, i32, i32) = (674, 8, 598, 784);
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let (surface, _) = pair.upper.window.clone().expect("a window");
    let shm: wl_shm::WlShm = pair.upper.bind(1);
    let (buffer, _) = pair.upper.buffer(&shm, 600, 700, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    pair.upper.roundtrip(&mut run);
    let at = Point::from((590.0, 690.0));
    resize_tile(&mut run, &mut pair.upper, &pair.upper_toplevel, at);
    run.state
        .pointer_to(at + Point::from((30.0, -100.0)), InputTime::now());
    pair.upper.redraw(&mut run);

    answer_all(&mut run, &mut [&mut pair.upper, &mut pair.lower], release);

    assert_eq!(tile_of(&run, pair.id, &pair.above), rect(WIDER));
    assert_eq!(tile_of(&run, pair.id, &pair.below), rect(NARROWER));
    run.stop();
}

/// A client of one size, floating above the tiles of `pair`'s workspace (D5).
fn one_size(run: &mut Running) -> (Local, Window) {
    let mut new = Local::connect(run, None);
    let (surface, toplevel) = new.unborn();
    toplevel.set_min_size(WIDTH, HEIGHT);
    toplevel.set_max_size(WIDTH, HEIGHT);
    surface.commit();
    new.redraw(run);
    let window = new.server_window(&*run, &surface);
    (new, window)
}

/// A tile that goes full screen stands above a window D5 floats on its workspace (D6).
#[test]
fn a_tile_gone_full_screen_stands_above_a_float() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let (_new, float) = one_size(&mut run);
    assert_eq!(stack(&run).last(), Some(&float));

    answer_all(&mut run, &mut [&mut pair.upper], |run| {
        run.state.full_screen(&pair.above, None)
    });

    assert_eq!(stack(&run).last(), Some(&pair.above), "the full-screen tile is on top");
    run.stop();
}

/// A tile that leaves full screen goes back under a window D5 floats on its workspace.
#[test]
fn a_tile_leaving_full_screen_goes_under_a_float() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    let (_new, float) = one_size(&mut run);
    answer_all(&mut run, &mut [&mut pair.upper], |run| {
        run.state.full_screen(&pair.above, None)
    });
    run.state.focus(&pair.above);
    assert_eq!(stack(&run).last(), Some(&pair.above));

    answer_all(&mut run, &mut [&mut pair.upper], |run| {
        run.state.leave_full_screen(&pair.above)
    });

    assert_eq!(stack(&run).last(), Some(&float), "the float is on top again");
    run.stop();
}

/// A window that floats on a workspace with a full-screen tile, placed without the keyboard,
/// stays under the full-screen tile (D6).
#[test]
fn a_float_placed_under_a_full_screen_tile_stays_under_it() {
    let mut run = Running::start(None);
    let mut pair = tiled_pair(&mut run);
    answer_all(&mut run, &mut [&mut pair.upper], |run| {
        run.state.full_screen(&pair.above, None)
    });
    run.state.focus(&pair.above);

    let (_new, float) = one_size(&mut run);

    assert_eq!(stack(&run).last(), Some(&pair.above), "the full-screen tile is on top");
    assert!(stack(&run).contains(&float), "the float is placed");
    run.stop();
}
