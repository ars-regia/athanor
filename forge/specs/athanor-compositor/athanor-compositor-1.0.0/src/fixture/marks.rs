//! Decision 31 (`marks.rs`, `focus_theme.rs`): the focus outline and the border of confined
//! windows, read from the frame.

use super::*;
use crate::focus_theme;
use smithay::desktop::PopupManager;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as Configured;
use wayland_client::protocol::wl_subcompositor;

/// The factory accent, which the outline takes before a theme is read, as wide as
/// `focus_theme::FACTORY_WIDTH`.
const ACCENT: [u8; 4] = [0x91, 0x41, 0xac, 255];
const OUTLINE: i32 = 3;
const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const GREY: [u8; 4] = [0x80, 0x80, 0x80, 255];
/// What the border's lines add around a window.
const BORDER: i32 = 3;
/// Where the tests put a window: on the first output, clear of its edges.
const AT: (i32, i32) = (400, 300);

fn frame(run: &mut Running) -> Vec<u8> {
    run.state.capture(0).expect("a frame")
}

fn at(frame: &[u8], x: i32, y: i32) -> [u8; 4] {
    pixel(frame, OUTPUTS[0].width, x, y)
}

/// The client's window, at `(x, y)`, with the keyboard and on top.
fn focused_at(run: &mut Running, local: &Local, (x, y): (i32, i32)) {
    let window = window_of(run, local.id.as_ref().expect("an id"));
    run.state.space.map_element(window.clone(), (x, y), true);
    run.state.focus(&window);
}

/// A new window of a client inside a security context, at `AT` and with the keyboard.
fn confined(run: &mut Running) -> (Local, xdg_toplevel::XdgToplevel) {
    let context = sandbox(run);
    let mut local = Local::connect(run, Some(context));
    let toplevel = local.toplevel(run);
    focused_at(run, &local, AT);
    (local, toplevel)
}

/// Draws a buffer of `width` by `height` pixels of `bgrx` on the client's window.
fn paint(run: &mut Running, local: &mut Local, width: i32, height: i32, bgrx: [u8; 4]) {
    let shm: wl_shm::WlShm = local.bind(1);
    let (buffer, _) = local.buffer(&shm, width, height, bgrx);
    let (surface, _) = local.window.clone().expect("a window");
    surface.attach(Some(&buffer), 0, 0);
    surface.damage(0, 0, width, height);
    surface.commit();
    local.roundtrip(run);
}

/// The point an absolute device names on a turned panel is what the panel shows there.
fn a_panel_shows_what_a_device_points_at(transform: smithay::utils::Transform) {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let first = run.state.space.outputs().next().cloned().expect("an output");
    first.change_current_state(None, Some(transform), None, None);
    run.state.arrange_outputs();
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    paint(&mut run, &mut local, 40, 40, [0, 0, 255, 255]);

    let (x, y) = (OUTPUTS[0].width / 4, OUTPUTS[0].height / 10);
    let pointed = run.state.on_first_panel(|panel| {
        assert_eq!(panel, (OUTPUTS[0].width, OUTPUTS[0].height).into());
        (f64::from(x), f64::from(y)).into()
    });
    let pointed = pointed.expect("an output");
    focused_at(&mut run, &local, (pointed.x as i32 - 20, pointed.y as i32 - 20));

    assert_eq!(at(&frame(&mut run), x, y), [255, 0, 0, 255]);
    run.stop();
}

#[test]
fn a_panel_turned_90_shows_what_a_device_points_at() {
    a_panel_shows_what_a_device_points_at(smithay::utils::Transform::_90);
}

#[test]
fn a_panel_turned_270_shows_what_a_device_points_at() {
    a_panel_shows_what_a_device_points_at(smithay::utils::Transform::_270);
}

#[test]
fn a_flipped_panel_shows_what_a_device_points_at() {
    a_panel_shows_what_a_device_points_at(smithay::utils::Transform::Flipped90);
}

/// The fixture's window, moved away from `AT` so that nothing lies around either.
fn clear_the_fixture(run: &mut Running) -> smithay::desktop::Window {
    let (window, _) = fixture_window(run);
    run.state.space.map_element(window.clone(), (40, 40), false);
    window
}

/// The focused window, and no other, is outlined, in the accent and outside its geometry; a
/// window outside a security context has no border.
#[test]
fn the_focused_window_alone_is_outlined() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    focused_at(&mut run, &local, AT);
    let (x, y) = AT;
    let middle = y + HEIGHT / 2;

    let shown = frame(&mut run);
    for offset in 1..=OUTLINE {
        assert_eq!(at(&shown, x - offset, middle), ACCENT, "left, {offset}");
        let right = x + WIDTH + offset - 1;
        assert_eq!(at(&shown, right, middle), ACCENT, "right, {offset}");
        let above = x + WIDTH / 2;
        assert_eq!(at(&shown, above, y - offset), ACCENT, "top, {offset}");
        let below = y + HEIGHT + offset - 1;
        assert_eq!(at(&shown, above, below), ACCENT, "bottom, {offset}");
    }
    assert!(background(&at(&shown, x - OUTLINE - 1, middle)));
    assert!(background(&at(&shown, 39, 40 + HEIGHT / 2)), "no mark");
    assert_eq!(at(&shown, x, middle), BLACK, "the window itself");

    run.state.focus(&fixture);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 39, 40 + HEIGHT / 2), ACCENT);
    assert!(background(&at(&shown, x - 1, middle)), "the focus left");
    run.stop();
}

/// Focused or not, maximised or full-screen, a confined window shows its two lines, and only
/// with the focus and room around it the outline, outside them.
#[test]
fn a_confined_window_is_bordered_in_every_state() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (mut local, toplevel) = confined(&mut run);
    let (x, y) = AT;
    let middle = y + HEIGHT / 2;

    let shown = frame(&mut run);
    assert_eq!(at(&shown, x - 2, middle), WHITE, "focused, the white line");
    assert_eq!(at(&shown, x - BORDER, middle), BLACK, "focused, outside");
    assert_eq!(at(&shown, x - BORDER - 1, middle), ACCENT, "the outline");
    let far = x - BORDER - OUTLINE;
    assert_eq!(at(&shown, far, middle), ACCENT, "its far side");
    assert!(background(&at(&shown, far - 1, middle)));

    run.state.focus(&fixture);
    let shown = frame(&mut run);
    assert_eq!(
        at(&shown, x - 2, middle),
        WHITE,
        "unfocused, the white line"
    );
    assert_eq!(at(&shown, x - BORDER, middle), BLACK, "unfocused, outside");
    assert!(
        background(&at(&shown, x - BORDER - 1, middle)),
        "no outline"
    );

    focused_at(&mut run, &local, AT);
    local.roundtrip(&mut run);
    local.client.configure = None;
    toplevel.set_maximized();
    local.redraw(&mut run);
    let area = (OUTPUTS[0].width, OUTPUTS[0].height);
    run.until("the window is maximised", |state| {
        state
            .space
            .elements()
            .any(|window| window.geometry().size == area.into())
    });
    let shown = frame(&mut run);
    for (place, what) in [
        ((1, 400), "left"),
        ((area.0 - 2, 400), "right"),
        ((640, 1), "top"),
        ((640, area.1 - 2), "bottom"),
    ] {
        assert_eq!(at(&shown, place.0, place.1), WHITE, "maximised, {what}");
    }
    assert_eq!(at(&shown, 0, 400), BLACK, "the outer line, not the outline");

    let window = window_of(&run, local.id.as_ref().expect("an id"));
    let surface = window.toplevel().expect("a toplevel");
    surface.with_pending_state(|state| {
        state.states.unset(Configured::Maximized);
        state.states.set(Configured::Fullscreen);
    });
    surface.send_configure();
    local.client.configure = None;
    local.redraw(&mut run);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 1, 400), WHITE, "full-screen");
    assert_eq!(at(&shown, 0, 400), BLACK, "full-screen");
    run.stop();
}

/// A maximised window loses the outline even when it does not fill the output.
#[test]
fn a_maximised_window_has_no_outline() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    focused_at(&mut run, &local, AT);
    let (x, y) = AT;
    let middle = y + HEIGHT / 2;
    assert_eq!(at(&frame(&mut run), x - 1, middle), ACCENT, "before");
    local.roundtrip(&mut run);
    local.client.configure = None;
    toplevel.set_maximized();
    local.redraw(&mut run);
    paint(&mut run, &mut local, WIDTH, HEIGHT, [0, 0, 0, 255]);
    focused_at(&mut run, &local, AT);
    assert!(
        background(&at(&frame(&mut run), x - 1, middle)),
        "maximised"
    );
    run.stop();
}

/// A subsurface at the far end of the coordinates ends nothing, focused or not, and the border
/// around the tree stays on the output.
#[test]
fn a_tree_at_the_edge_of_the_coordinates_crashes_nothing() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (mut local, _toplevel) = confined(&mut run);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let (surface, _) = local.window.clone().expect("a window");
    let child = compositor.create_surface(&qh, ());
    let subsurface = subcompositor.get_subsurface(&child, &surface, &qh, ());
    let (buffer, _) = local.buffer(&shm, 1, 1, [0, 0, 0, 0]);
    child.attach(Some(&buffer), 0, 0);
    child.commit();
    let (x, y) = AT;
    for (far_x, far_y) in [(i32::MAX - 500, y), (i32::MIN + 500, i32::MIN + 500)] {
        subsurface.set_position(far_x, far_y);
        surface.commit();
        local.roundtrip(&mut run);
        focused_at(&mut run, &local, AT);
        frame(&mut run);
        run.state.focus(&fixture);
        frame(&mut run);
    }
    subsurface.set_position(i32::MAX - 500, 0);
    surface.commit();
    local.roundtrip(&mut run);
    let shown = frame(&mut run);
    let middle = y + HEIGHT / 2;
    assert_eq!(at(&shown, x - 2, middle), WHITE, "the near side");
    assert_eq!(
        at(&shown, OUTPUTS[0].width - 2, middle),
        WHITE,
        "the cut side"
    );
    run.stop();
}

/// Each line is a whole number of the output's pixels, the same number for the three.
#[test]
fn the_border_keeps_its_lines_at_a_fractional_scale() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (local, _toplevel) = confined(&mut run);
    let second = run.state.space.outputs().nth(1).expect("an output").clone();
    let origin = run
        .state
        .space
        .output_geometry(&second)
        .expect("mapped")
        .loc;
    focused_at(&mut run, &local, (origin.x + 100, origin.y + 100));
    run.state.focus(&fixture);
    let shown = run.state.capture(1).expect("frame of output 1");
    // 100 logical pixels are 150 at scale 1.5, and a line of 1 is 2.
    for (column, colour) in [
        (144, BLACK),
        (145, BLACK),
        (146, WHITE),
        (147, WHITE),
        (148, BLACK),
        (149, BLACK),
    ] {
        assert_eq!(
            pixel(&shown, OUTPUTS[1].width, column, 200),
            colour,
            "{column}"
        );
    }
    run.stop();
}

/// A window across two outputs shows the border at the seam of each.
#[test]
fn the_border_is_cut_on_each_output() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (local, _toplevel) = confined(&mut run);
    let second = run.state.space.outputs().nth(1).expect("an output").clone();
    let origin = run
        .state
        .space
        .output_geometry(&second)
        .expect("mapped")
        .loc;
    let y = AT.1;
    focused_at(&mut run, &local, (origin.x - WIDTH / 2, y));
    run.state.focus(&fixture);
    let middle = y + HEIGHT / 2;
    let first = frame(&mut run);
    assert_eq!(at(&first, OUTPUTS[0].width - 1, middle), BLACK);
    assert_eq!(at(&first, OUTPUTS[0].width - 2, middle), WHITE);
    let shown = run.state.capture(1).expect("frame of output 1");
    let row = middle * 3 / 2;
    assert_eq!(pixel(&shown, OUTPUTS[1].width, 0, row), BLACK);
    assert_eq!(pixel(&shown, OUTPUTS[1].width, 2, row), WHITE);
    run.stop();
}

/// A window under an exclusive bar shows the border at the bar's edge, not under it.
#[test]
fn the_border_stays_out_from_under_the_bar() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let mut shell = Local::connect(&mut run, None);
    let _bar = shell.bar(&mut run, 30);
    let (local, _toplevel) = confined(&mut run);
    focused_at(&mut run, &local, (AT.0, 0));
    run.state.focus(&fixture);
    let shown = frame(&mut run);
    let column = AT.0 + WIDTH / 2;
    assert_eq!(at(&shown, column, 30), BLACK, "the outer line");
    assert_eq!(at(&shown, column, 31), WHITE, "at the bar's edge");
    run.stop();
}

/// The lock covers the windows and their marks.
#[test]
fn no_mark_is_drawn_while_the_lock_holds() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (_local, _toplevel) = confined(&mut run);
    let (x, y) = AT;
    let beside = (x - 2, y + HEIGHT / 2);
    assert_eq!(at(&frame(&mut run), beside.0, beside.1), WHITE, "before");

    let mut locker = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let compositor: wl_compositor::WlCompositor = locker.bind(4);
    let shm: wl_shm::WlShm = locker.bind(1);
    let output: wl_output::WlOutput = locker.bind(4);
    let qh = locker.queue.handle();
    let lock = locks.lock(&qh, ());
    locker.pump(&mut run, "the session is locked", |client, _| client.locked);
    let surface = compositor.create_surface(&qh, ());
    let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    locker.pump(&mut run, "the lock surface is configured", |client, _| {
        client.lock_configure.is_some()
    });
    let (serial, width, height) = locker.client.lock_configure.expect("a configure");
    lock_surface.ack_configure(serial);
    let (buffer, _) = locker.buffer(&shm, width as i32, height as i32, [0, 0, 255, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    locker.roundtrip(&mut run);
    assert_eq!(at(&frame(&mut run), beside.0, beside.1), RED, "the lock");
    run.stop();
}

/// Neither the grey between black and white nor a white edge on a dark backdrop hides the white
/// line, which touches neither.
#[test]
fn the_border_shows_on_a_grey_backdrop() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let mut backdrop = Local::connect(&mut run, None);
    backdrop.toplevel(&mut run);
    focused_at(&mut run, &backdrop, (300, 250));
    let (mut local, _toplevel) = confined(&mut run);
    let (x, y) = AT;
    let middle = y + HEIGHT / 2;
    for (around, edge) in [
        (GREY, [0, 0, 0, 255]),
        ([0x18, 0x10, 0x10, 255], [255, 255, 255, 255]),
    ] {
        paint(&mut run, &mut backdrop, 600, 400, around);
        paint(&mut run, &mut local, WIDTH, HEIGHT, edge);
        run.state.focus(&fixture);
        let shown = frame(&mut run);
        assert_eq!(at(&shown, x - 4, middle)[0], around[2], "the backdrop");
        assert_eq!(at(&shown, x - BORDER, middle), BLACK);
        assert_eq!(at(&shown, x - 2, middle), WHITE);
        assert_eq!(at(&shown, x - 1, middle), BLACK);
        assert_eq!(at(&shown, x, middle)[0], edge[2], "the window");
    }
    run.stop();
}

/// A window as large as the output, or larger, which asked for no state, leaves no room around
/// it, so the border lies inside it, on the output.
#[test]
fn a_confined_window_as_large_as_the_output_is_bordered() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, _toplevel) = confined(&mut run);
    let (width, height) = (OUTPUTS[0].width, OUTPUTS[0].height);
    for (w, h) in [(width, height), (width + 200, height + 100)] {
        paint(&mut run, &mut local, w, h, [0, 0, 0, 255]);
        focused_at(&mut run, &local, (0, 0));
        let shown = frame(&mut run);
        assert_eq!(at(&shown, 1, 400), WHITE, "left of {w}x{h}");
        assert_eq!(at(&shown, width - 2, 400), WHITE, "right of {w}x{h}");
        assert_eq!(at(&shown, 640, 1), WHITE, "top of {w}x{h}");
        assert_eq!(at(&shown, 640, height - 2), WHITE, "bottom of {w}x{h}");
        assert_eq!(at(&shown, 2, 400), BLACK, "the inner line of {w}x{h}");
    }
    run.stop();
}

/// A client's geometry does not hide a prompt from the border: a shadow stays outside it, and a
/// tree that reaches past a shadow's margin is inside it.
#[test]
fn a_border_follows_a_tree_larger_than_its_geometry() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, _toplevel) = confined(&mut run);
    let (surface, xdg) = local.window.clone().expect("a window");

    xdg.set_window_geometry(15, 12, WIDTH - 30, HEIGHT - 24);
    surface.commit();
    local.roundtrip(&mut run);
    focused_at(&mut run, &local, AT);
    let (x, y) = AT;
    let shown = frame(&mut run);
    assert_eq!(at(&shown, x - 2, y + 20), WHITE, "around the geometry");

    // One pixel of geometry in a tree within the shadow's margin collapses nothing.
    xdg.set_window_geometry(WIDTH / 2, HEIGHT / 2, 1, 1);
    surface.commit();
    local.roundtrip(&mut run);
    focused_at(&mut run, &local, AT);
    let shown = frame(&mut run);
    let (left, top) = (x - WIDTH / 2, y - HEIGHT / 2);
    assert_eq!(at(&shown, left - 2, y), WHITE, "left of the sliver's tree");
    assert_eq!(at(&shown, x, top - 2), WHITE, "above the sliver's tree");

    let (tree_w, tree_h) = (600, 300);
    let (left, top) = (100, 100);
    xdg.set_window_geometry(left, top, 50, 50);
    paint(&mut run, &mut local, tree_w, tree_h, [0, 0, 0, 255]);
    focused_at(&mut run, &local, AT);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, x - left - 2, y), WHITE, "left of the tree");
    assert_eq!(at(&shown, x - left + tree_w + 1, y), WHITE, "right of it");
    assert_eq!(at(&shown, x, y - top - 2), WHITE, "above the tree");
    assert_eq!(at(&shown, x, y - top + tree_h + 1), WHITE, "below it");
    run.stop();
}

/// A popup of a confined client carries the border, drawn over its window's.
#[test]
fn a_confined_popup_is_bordered() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, _toplevel) = confined(&mut run);
    let (x, y) = popup(&mut run, &mut local, AT, 0);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, x, y + 20), RED, "the popup");
    assert_eq!(at(&shown, x - 2, y + 20), WHITE, "left");
    assert_eq!(at(&shown, x + 61, y + 20), WHITE, "right");
    assert_eq!(at(&shown, x + 30, y - 2), WHITE, "top");
    assert_eq!(at(&shown, x + 30, y + 41), WHITE, "bottom");
    run.stop();
}

/// A popup declaring far more than it paints would put its border at the output's edges, with a
/// painted dialog inside it and no line beside that.
#[test]
fn a_popups_border_stays_at_what_it_paints() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, _toplevel) = confined(&mut run);
    let (x, y) = popup_as(&mut run, &mut local, AT, 0, Some((4000, 4000)), true);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, x + 61, y + 20), WHITE, "right");
    assert_eq!(at(&shown, x + 30, y + 41), WHITE, "bottom");
    run.stop();
}

/// A popup with no buffer shows nothing, and a client gets no mark to place by opening one.
#[test]
fn a_popup_with_nothing_mapped_has_no_border() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, _toplevel) = confined(&mut run);
    let before = frame(&mut run);
    popup_as(&mut run, &mut local, AT, 0, None, false);
    assert!(frame(&mut run) == before, "the screen changed");
    run.stop();
}

/// A menu holds the keyboard through its popup, and its window keeps the outline.
#[test]
fn the_outline_stays_while_a_popup_has_the_keyboard() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    focused_at(&mut run, &local, AT);
    popup(&mut run, &mut local, AT, 0);
    let window = window_of(&run, local.id.as_ref().expect("an id"));
    let (menu, _) = PopupManager::popups_for_surface(&surface_of(&window))
        .next()
        .expect("a popup");
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    let serial = SERIAL_COUNTER.next_serial();
    keyboard.set_focus(&mut run.state, Some(menu.wl_surface().clone()), serial);
    let (x, y) = AT;
    assert_eq!(at(&frame(&mut run), x - 1, y + HEIGHT / 2), ACCENT);
    run.stop();
}

/// A client places a popup where it likes, and that can be in the bar's zone, past the window
/// area, where the bar's own surface leaves it in view.
#[test]
fn a_popup_in_the_bars_zone_is_bordered() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let mut shell = Local::connect(&mut run, None);
    let _bar = shell.zoned_bar(&mut run, 10, 100);
    let (mut local, _toplevel) = confined(&mut run);
    let origin = (AT.0, 100);
    focused_at(&mut run, &local, origin);
    run.state.focus(&fixture);
    let (x, y) = popup(&mut run, &mut local, origin, -(HEIGHT / 2 + 50));
    assert!((10..58).contains(&y), "in the zone, at {y}");
    let shown = frame(&mut run);
    assert_eq!(at(&shown, x, y + 20), RED, "the popup");
    assert_eq!(at(&shown, x - 2, y + 20), WHITE, "left");
    assert_eq!(at(&shown, x + 30, y - 2), WHITE, "top");
    run.stop();
}

/// A zone that leaves no room is ignored when windows are placed, and when they are marked.
#[test]
fn a_zone_that_leaves_no_room_hides_no_border() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let mut shell = Local::connect(&mut run, None);
    let _bar = shell.zoned_bar(&mut run, 10, 20_000);
    let (local, _toplevel) = confined(&mut run);
    focused_at(&mut run, &local, AT);
    run.state.focus(&fixture);
    let shown = frame(&mut run);
    assert_eq!(at(&shown, AT.0 - 2, AT.1 + HEIGHT / 2), WHITE, "left");
    run.stop();
}

/// A blue popup of 60 by 40 on `local`'s window at `origin`, centred on the window and moved by
/// `shift` upwards or downwards; where it lies. The compositor draws its surface red.
fn popup(run: &mut Running, local: &mut Local, origin: (i32, i32), shift: i32) -> (i32, i32) {
    popup_as(run, local, origin, shift, None, true)
}

/// As `popup`, with the size the client declares as its geometry, and with no buffer at all
/// unless `painted`.
fn popup_as(
    run: &mut Running,
    local: &mut Local,
    origin: (i32, i32),
    shift: i32,
    declared: Option<(i32, i32)>,
    painted: bool,
) -> (i32, i32) {
    let (parent, parent_xdg) = local.window.clone().expect("a window");
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let shm: wl_shm::WlShm = local.bind(1);
    let qh = local.queue.handle();
    let positioner = wm_base.create_positioner(&qh, ());
    positioner.set_size(60, 40);
    positioner.set_anchor_rect(0, 0, WIDTH, HEIGHT);
    positioner.set_offset(0, shift);
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let _popup = xdg.get_popup(Some(&parent_xdg), &positioner, &qh, ());
    local.client.configure = None;
    surface.commit();
    local.pump(run, "the popup is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.take().expect("a configure"));
    if let Some((width, height)) = declared {
        xdg.set_window_geometry(0, 0, width, height);
    }
    if painted {
        let (buffer, _) = local.buffer(&shm, 60, 40, [0, 0, 255, 255]);
        surface.attach(Some(&buffer), 0, 0);
    }
    surface.commit();
    parent.commit();
    local.roundtrip(run);

    let window = window_of(run, local.id.as_ref().expect("an id"));
    let (_, offset) = PopupManager::popups_for_surface(&surface_of(&window))
        .next()
        .expect("a popup");
    (origin.0 + offset.x, origin.1 + offset.y)
}

/// The bottom and background layers would show a surface unmarked where no window covers it, so
/// a confined client has no layer shell to ask.
#[test]
fn a_confined_client_gets_no_layer_surface() {
    let mut run = Running::start(None);
    let context = sandbox(&run);
    let confined = Local::connect(&mut run, Some(context));
    assert!(!confined.sees("zwlr_layer_shell_v1"));
    let unconfined = Local::connect(&mut run, None);
    assert!(unconfined.sees("zwlr_layer_shell_v1"));
    run.stop();
}

/// The marks stand with their window: a window above covers them.
#[test]
fn a_window_above_covers_the_marks_below() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (local, _toplevel) = confined(&mut run);
    let (x, y) = AT;
    let beside = (x - 2, y + HEIGHT / 2);
    assert_eq!(at(&frame(&mut run), beside.0, beside.1), WHITE);

    let mut above = Local::connect(&mut run, None);
    above.toplevel(&mut run);
    focused_at(&mut run, &above, (x - 40, y - 40));
    paint(&mut run, &mut above, 400, 300, [0, 0, 255, 255]);
    assert_eq!(at(&frame(&mut run), beside.0, beside.1), RED);
    drop(local);
    run.stop();
}

/// A change of the theme's files reaches the frame without a new client: the width and the
/// colour of the outline, and the setting that turns it off.
#[test]
fn the_outline_follows_a_theme_change() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    focused_at(&mut run, &local, AT);
    let (x, y) = AT;
    let middle = y + HEIGHT / 2;

    let dir = std::env::temp_dir().join(format!("athanor-marks-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let theme = dir.join(focus_theme::DARK).join("v2");
    let setting = dir.join(focus_theme::COMP);
    std::fs::create_dir_all(&theme).expect("the theme");
    std::fs::create_dir_all(&setting).expect("the setting");
    let handle = run.state.loop_handle.clone();
    focus_theme::follow(&mut run.state, &handle, dir.clone());

    std::fs::write(theme.join("active_hint"), "6").expect("a width");
    std::fs::write(theme.join("window_hint"), "Some(\"#00FF00FF\")").expect("a colour");
    let green = [0, 255, 0, 255];
    settled(&mut run, "the new outline", |shown| {
        at(shown, x - 6, middle) == green
    });

    std::fs::write(setting.join("active_hint"), "false").expect("the setting");
    settled(&mut run, "no outline", |shown| {
        background(&at(shown, x - 1, middle))
    });
    run.stop();
    std::fs::remove_dir_all(dir).expect("the directory");
}

/// Runs the compositor until the frame shows `done`.
fn settled(run: &mut Running, what: &str, done: impl Fn(&[u8]) -> bool) {
    let started = Instant::now();
    while !done(&frame(run)) {
        assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }
}
