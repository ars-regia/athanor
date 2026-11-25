//! VL9 (`workspace/slide.rs`): the workspace switch slides for 200 ms, read from the frame,
//! and AX9's reduced motion makes it at once.

use super::full_screen::{enter, window};
use super::marks::{at, clear_the_fixture, frame, paint};
use super::*;
use crate::workspace::slide::{DURATION, advance, right_to_left, under_way};
use crate::workspace::WorkspaceId;

/// Where the window of the first workspace stands, and the one of the second.
const FIRST: (i32, i32) = (800, 300);
const SECOND: (i32, i32) = (200, 300);
const SIZE: i32 = 200;

/// A switch's two workspaces: each with a window of its own colour, the second shown and its
/// slide over.
struct Two {
    _first: Local,
    _second: Local,
    first: ClientId,
    list: Vec<WorkspaceId>,
    /// The colours the windows show at rest.
    red: [u8; 4],
    blue: [u8; 4],
}

/// The client's window, painted `bgrx`, at `(x, y)` with the keyboard: the colour it shows.
fn painted(run: &mut Running, (x, y): (i32, i32), bgrx: [u8; 4]) -> (Local, [u8; 4]) {
    let mut local = Local::connect(run, None);
    local.toplevel(run);
    paint(run, &mut local, SIZE, SIZE, bgrx);
    let window = window_of(run, local.id.as_ref().expect("an id"));
    run.state.space.map_element(window.clone(), (x, y), true);
    run.state.focus(&window);
    let shown = at(&frame(run), x + SIZE / 2, y + SIZE / 2);
    (local, shown)
}

fn two_workspaces(run: &mut Running) -> Two {
    let (window, _) = fixture_window(run);
    assert!(
        run.state.minimise(&window),
        "the fixture's window is minimised"
    );
    let (first, red) = painted(run, FIRST, [0, 0, 255, 255]);
    let list = workspaces_of(run, 0);
    assert!(run.state.activate_workspace(list[1]));
    advance(DURATION);
    let (second, blue) = painted(run, SECOND, [255, 0, 0, 255]);
    assert_ne!(red, blue);
    let first_id = first.id.clone().expect("an id");
    Two {
        _first: first,
        _second: second,
        first: first_id,
        list,
        red,
        blue,
    }
}

/// What the frame shows at the middle of the first window's place, and of the second's.
fn at_places(run: &mut Running) -> ([u8; 4], [u8; 4]) {
    let shown = frame(run);
    let middle = |(x, y): (i32, i32)| at(&shown, x + SIZE / 2, y + SIZE / 2);
    (middle(FIRST), middle(SECOND))
}

/// Half way back to the first workspace, which comes in from the left: each window is drawn
/// half the output's width, 640 pixels, from its place, the first to the left of its own and
/// the second to the right of its own.
fn half_way(run: &mut Running, two: &Two) {
    assert!(run.state.activate_workspace(two.list[0]));
    advance(DURATION / 2);
    let shown = frame(run);
    let middle = |(x, y): (i32, i32), by: i32| at(&shown, x + SIZE / 2 + by, y + SIZE / 2);
    assert_eq!(middle(FIRST, -640), two.red, "the first, coming in");
    assert_eq!(middle(SECOND, 640), two.blue, "the second, leaving");
}

/// Mid-slide both workspaces are drawn, one output's width apart, and once it is over only the
/// workspace activated, at its place.
#[test]
fn the_switch_slides_both_workspaces() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    half_way(&mut run, &two);
    advance(DURATION);
    let (first, second) = at_places(&mut run);
    assert_eq!(first, two.red, "at its place once over");
    assert!(background(&second), "the second is no longer drawn");
    run.stop();
}

/// A switch back during a slide starts where the slide stands: neither workspace jumps.
#[test]
fn a_second_switch_starts_where_the_slide_stands() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    half_way(&mut run, &two);
    assert!(run.state.activate_workspace(two.list[1]));
    let shown = frame(&mut run);
    let middle = |(x, y): (i32, i32), by: i32| at(&shown, x + SIZE / 2 + by, y + SIZE / 2);
    assert_eq!(middle(FIRST, -640), two.red, "the first, where it stood");
    assert_eq!(middle(SECOND, 640), two.blue, "the second, where it stood");
    run.stop();
}

/// The input goes to the workspace activated at once: a click mid-slide on the first window's
/// place, where the second is still drawn, reaches the first.
#[test]
fn a_click_mid_slide_reaches_the_window_activated() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    assert!(run.state.activate_workspace(two.list[0]));
    advance(DURATION / 2);
    let (first, _) = at_places(&mut run);
    assert_eq!(
        first, two.blue,
        "the second is drawn over the first's place"
    );
    press(&mut run, (900.0, 400.0).into());
    release(&mut run);
    let (_, to) = run.state.grab_serials.button.clone().expect("a press");
    assert_eq!(to, two.first, "the press went to the first window");
    run.stop();
}

/// Under reduced motion the switch shows at once, with no slide; without it, it slides.
#[test]
fn reduced_motion_switches_at_once() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    half_way(&mut run, &two);
    advance(DURATION);
    assert!(run.state.activate_workspace(two.list[1]));
    advance(DURATION);
    run.state.bell.reduced_motion = true;
    assert!(run.state.activate_workspace(two.list[0]));
    let (first, second) = at_places(&mut run);
    assert_eq!(first, two.red, "the first at its place at once");
    assert!(background(&second), "the second is not drawn");
    run.stop();
}

/// Under a right-to-left language the slide mirrors: the earlier workspace comes in from the
/// right. Three quarters through, the ease leaves a sixteenth to go, 80 pixels.
#[test]
fn a_right_to_left_language_mirrors_the_slide() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    right_to_left(true);
    assert!(run.state.activate_workspace(two.list[0]));
    advance(DURATION * 3 / 4);
    let shown = frame(&mut run);
    let (x, y) = (FIRST.0, FIRST.1 + SIZE / 2);
    assert_eq!(at(&shown, x + SIZE + 40, y), two.red, "80 pixels right");
    assert!(background(&at(&shown, x + 40, y)), "nothing left of it");
    right_to_left(false);
    run.stop();
}

/// A third switch during a slide keeps the first workspace left on its way out: it is drawn
/// where it stood, and the second beside the one activated.
#[test]
fn a_third_switch_keeps_the_first_leaving() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    assert!(run.state.activate_workspace(two.list[0]));
    advance(DURATION);
    let list = workspaces_of(&run, 0);
    assert_eq!(list.len(), 3, "an empty workspace after the second");
    assert!(run.state.activate_workspace(list[1]));
    advance(DURATION / 2);
    assert!(run.state.activate_workspace(list[2]));
    let shown = frame(&mut run);
    let middle = |(x, y): (i32, i32), by: i32| at(&shown, x + SIZE / 2 + by, y + SIZE / 2);
    assert_eq!(middle(FIRST, -640), two.red, "the first, where it stood");
    assert_eq!(middle(SECOND, 640), two.blue, "the second, where it stood");
    run.stop();
}

/// Once the slide is over nothing of the workspace left is drawn, and its timer is gone, so no
/// more frames are asked for.
#[test]
fn the_slide_ends_and_asks_for_no_more_frames() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    let output = output_at(&run, 0);
    assert!(run.state.activate_workspace(two.list[0]));
    assert!(under_way(&output), "the slide has its timer");
    advance(DURATION);
    // The timer ticks on the loop's own clock, every 4 ms.
    for _ in 0..5 {
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }
    assert!(!under_way(&output), "the timer is gone");
    let (first, second) = at_places(&mut run);
    assert_eq!(first, two.red, "at its place");
    assert!(background(&second), "the second is not drawn");
    run.stop();
}

/// A slide on one output leaves the frame of the other still.
#[test]
fn a_slide_leaves_the_other_output_still() {
    let mut run = Running::start(None);
    let two = two_workspaces(&mut run);
    let second = output_at(&run, 1);
    let area = run.state.space.output_geometry(&second).expect("placed");
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    paint(&mut run, &mut local, SIZE, SIZE, [0, 255, 0, 255]);
    let window = window_of(&run, local.id.as_ref().expect("an id"));
    let place = area.loc + Point::from((100, 100));
    run.state.space.map_element(window.clone(), place, false);
    run.state.settle(&window);
    assert_eq!(
        run.state.workspace_of(&window),
        run.state.active_workspace(&second)
    );
    // The switch moves the keyboard, and the outline with it, at once: the frame half way
    // through is the one after.
    assert!(run.state.activate_workspace(two.list[0]));
    advance(DURATION / 2);
    let during = run.state.capture(1).expect("a frame");
    advance(DURATION);
    let after = run.state.capture(1).expect("a frame");
    let green = during.as_chunks::<4>().0.contains(&[0, 255, 0, 255]);
    assert!(green, "the second output shows its window");
    assert!(during == after, "the second output's frame is the same");
    run.stop();
}

/// The black behind a full-screen client smaller than its output moves with it: half way out,
/// the half of the output it left shows the workspace activated.
#[test]
fn the_black_of_a_full_screen_client_slides_with_it() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel, _) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    paint(&mut run, &mut local, 400, 300, [0x60, 0x20, 0x60, 255]);
    let black = [0, 0, 0, 255];
    assert_eq!(at(&frame(&mut run), 1000, 50), black, "black at rest");
    let list = workspaces_of(&run, 0);
    assert!(run.state.activate_workspace(list[1]));
    advance(DURATION / 2);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 100, 50), black, "the black, 640 pixels out");
    assert!(background(&at(&shown, 1000, 50)), "the workspace activated");
    run.stop();
}
