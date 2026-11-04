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
