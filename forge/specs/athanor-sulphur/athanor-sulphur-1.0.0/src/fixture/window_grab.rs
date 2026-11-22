//! Fixtures of a window move or resize started only from a press still held (OS12,
//! `window_grab`): a popup's grab keeps the serial of the press that opened it past the release,
//! and a move or a resize named with that serial is refused, as a drag is (`dnd.rs`).

use super::dnd::popup_grabbing;
use super::*;

fn resizing(run: &Running) -> bool {
    run.state
        .seat
        .get_pointer()
        .and_then(|pointer| pointer.with_grab(|_, grab| grab.is::<ResizeGrab>()))
        .unwrap_or(false)
}

/// Presses the left button on `local`'s window: the press's serial.
fn pressed(run: &mut Running, local: &mut Local, id: &ClientId) -> u32 {
    let at = location_of(run, id).to_f64() + Point::from((20.0, 10.0));
    local.client.button = None;
    press(run, at);
    local.pump(run, "the press", |client, _| client.button.is_some());
    local.client.button.expect("a press")
}

/// A move named with the serial of a press already released, which a popup's grab opened after
/// the release still holds, is refused.
#[test]
fn a_move_needs_its_press_held() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let serial = pressed(&mut run, &mut local, &id);
    release(&mut run);
    popup_grabbing(&mut run, &mut local, serial);
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(
        pointer.has_grab(serial.into()),
        "the popup holds the press's serial"
    );
    toplevel._move(&seat, serial);
    local.roundtrip(&mut run);
    assert!(!moving(&run), "a press released is refused");
    run.stop();
}

/// A resize named with the serial of a popup's grab, opened while the press was held and kept
/// past its release, is refused.
#[test]
fn a_resize_through_a_popups_grab_needs_its_press_held() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let serial = pressed(&mut run, &mut local, &id);
    popup_grabbing(&mut run, &mut local, serial);
    release(&mut run);
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    assert!(
        pointer.has_grab(serial.into()),
        "the popup's grab outlasts the release"
    );
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    local.roundtrip(&mut run);
    assert!(!resizing(&run), "a press released is refused");
    run.stop();
}

/// A press still held moves its window, and resizes it, with no popup open.
#[test]
fn a_held_press_still_moves() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let id = local.id.clone().expect("an id");
    let seat = local.seat.clone().expect("a seat");
    let serial = pressed(&mut run, &mut local, &id);
    toplevel._move(&seat, serial);
    local.roundtrip(&mut run);
    assert!(moving(&run), "a press held grants the move");
    release(&mut run);
    assert!(!moving(&run), "the release ends the move");

    let serial = pressed(&mut run, &mut local, &id);
    toplevel.resize(&seat, serial, xdg_toplevel::ResizeEdge::BottomRight);
    local.roundtrip(&mut run);
    assert!(resizing(&run), "a press held grants the resize");
    release(&mut run);
    run.stop();
}
