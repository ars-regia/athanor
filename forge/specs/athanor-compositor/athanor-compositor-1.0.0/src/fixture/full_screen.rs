//! OS12: `xdg_toplevel.set_fullscreen` and `unset_fullscreen`, from the client's configure to the
//! pixels of the frame.

use super::marks::{at, clear_the_fixture, confined, frame, lock_the_session, paint};
use super::*;
use smithay::desktop::Window;
use smithay::utils::Rectangle;

/// What the tests paint a window with; the same read back, red and blue being equal.
const PAINT: [u8; 4] = [0x60, 0x20, 0x60, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const OVERLAY: [u8; 4] = [0x80, 0x80, 0x80, 255];
const FULLSCREEN: u32 = xdg_toplevel::State::Fullscreen as u32;
const MAXIMIZED: u32 = xdg_toplevel::State::Maximized as u32;
const BAR: i32 = 40;
const OUTPUT: (i32, i32) = (OUTPUTS[0].width, OUTPUTS[0].height);

/// A window of a new client, with the keyboard.
fn window(run: &mut Running) -> (Local, xdg_toplevel::XdgToplevel, Window) {
    let mut local = Local::connect(run, None);
    let toplevel = local.toplevel(run);
    let window = window_of(run, local.id.as_ref().expect("an id"));
    (local, toplevel, window)
}

/// The bar of a new client, on the top layer, drawn black.
fn bar(run: &mut Running) -> (Local, wl_surface::WlSurface) {
    let mut local = Local::connect(run, None);
    let surface = local.bar(run, BAR);
    (local, surface)
}

/// Asks for `output`, or for none, and answers: the configure's width, height and states.
fn enter(
    run: &mut Running,
    local: &mut Local,
    toplevel: &xdg_toplevel::XdgToplevel,
    output: Option<&wl_output::WlOutput>,
) -> (i32, i32, Vec<u32>) {
    local.roundtrip(run);
    local.client.configure = None;
    toplevel.set_fullscreen(output);
    let answer = local.redraw(run);
    paint(run, local, answer.0, answer.1, PAINT);
    answer
}

/// The second output, and where it lies.
fn second_output(run: &Running) -> (smithay::output::Output, Rectangle<i32, Logical>) {
    let second = run.state.space.outputs().nth(1).cloned();
    let second = second.expect("a second output");
    let area = run.state.space.output_geometry(&second).expect("placed");
    (second, area)
}

/// Runs `change` on the client's window and answers the configure it brings.
fn answer(
    run: &mut Running,
    local: &mut Local,
    change: impl FnOnce(&mut Running),
) -> (i32, i32, Vec<u32>) {
    local.roundtrip(run);
    local.client.configure = None;
    change(run);
    local.redraw(run)
}

fn geometry(run: &Running, window: &Window) -> smithay::utils::Rectangle<i32, Logical> {
    run.state
        .space
        .element_geometry(window)
        .expect("the window is mapped")
}

/// The client of the surface under `(x, y)`.
fn client_under(run: &Running, x: f64, y: f64) -> Option<ClientId> {
    let (surface, _) = run.state.surface_under((x, y).into())?;
    Some(surface.client()?.id())
}

/// A surface of `OVERLAY` on the overlay layer of the first output, in the middle of it.
fn overlay(run: &mut Running) -> (Local, wl_surface::WlSurface) {
    use zwlr_layer_shell_v1::Layer;
    let mut local = Local::connect(run, None);
    let (kept, layer, surface) = local.layer_kept(run, Layer::Overlay);
    assert!(kept, "the overlay is configured");
    let (serial, _, _) = local.client.layer_configure.expect("a configure");
    layer.ack_configure(serial);
    let shm: wl_shm::WlShm = local.bind(1);
    let (buffer, _) = local.buffer(&shm, 100, 100, OVERLAY);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(run);
    (local, surface)
}

/// The window takes the output, over the bar, and the client hears the state and the size.
#[test]
fn a_request_gives_the_window_its_output_whole() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (_bar, _surface) = bar(&mut run);
    let (mut local, toplevel, window) = window(&mut run);

    let (width, height, states) = enter(&mut run, &mut local, &toplevel, None);

    assert_eq!((width, height), OUTPUT, "the output's size, bar included");
    assert!(states.contains(&FULLSCREEN));
    let shown = geometry(&run, &window);
    assert_eq!((shown.loc.x, shown.loc.y), (0, 0));
    let picture = frame(&mut run);
    assert_eq!(at(&picture, 5, 5), PAINT, "over the bar");
    assert_eq!(at(&picture, OUTPUT.0 - 1, OUTPUT.1 - 1), PAINT);
    run.stop();
}

/// `unset_fullscreen` gives back the place and the size.
#[test]
fn leaving_restores_the_geometry() {
    let mut run = Running::start(None);
    let (mut local, toplevel, window) = window(&mut run);
    let before = geometry(&run, &window);
    enter(&mut run, &mut local, &toplevel, None);

    let (width, height, states) = answer(&mut run, &mut local, |_| toplevel.unset_fullscreen());

    assert!(!states.contains(&FULLSCREEN));
    assert_eq!((width, height), (before.size.w, before.size.h));
    assert_eq!(geometry(&run, &window), before);
    run.stop();
}

/// A window maximised before the request returns to that, in the area of that moment, and to
/// its first place after it.
#[test]
fn leaving_returns_a_maximised_window_to_its_maximised_size() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    let before = geometry(&run, &window);
    let (width, height, _) = answer(&mut run, &mut local, |_| toplevel.set_maximized());
    assert_eq!((width, height), OUTPUT);
    enter(&mut run, &mut local, &toplevel, None);
    // The area shrinks meanwhile, and the full-screen window does not hear of it.
    let (_bar, _surface) = bar(&mut run);

    let (width, height, states) = answer(&mut run, &mut local, |_| toplevel.unset_fullscreen());

    assert!(states.contains(&MAXIMIZED) && !states.contains(&FULLSCREEN));
    assert_eq!((width, height), (OUTPUT.0, OUTPUT.1 - BAR));
    assert_eq!(geometry(&run, &window).loc.y, BAR);
    let (.., states) = answer(&mut run, &mut local, |_| toplevel.unset_maximized());
    assert!(!states.contains(&MAXIMIZED));
    assert_eq!(geometry(&run, &window), before);
    run.stop();
}

/// `unset_maximized` while full-screen counts: the window leaves for where it was before.
#[test]
fn unmaximising_while_full_screen_counts_when_it_leaves() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    let before = geometry(&run, &window);
    answer(&mut run, &mut local, |_| toplevel.set_maximized());
    enter(&mut run, &mut local, &toplevel, None);

    answer(&mut run, &mut local, |_| toplevel.unset_maximized());
    let (width, height, states) = answer(&mut run, &mut local, |_| toplevel.unset_fullscreen());

    assert!(!states.contains(&MAXIMIZED) && !states.contains(&FULLSCREEN));
    assert_eq!((width, height), (before.size.w, before.size.h));
    assert_eq!(geometry(&run, &window), before);
    run.stop();
}

/// The lock covers a focused full-screen window, confined or not.
#[test]
fn the_lock_covers_a_full_screen_window() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel) = confined(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    let middle = (OUTPUT.0 / 2, OUTPUT.1 / 2);
    assert_ne!(at(&frame(&mut run), middle.0, middle.1), RED, "before");

    let _locker = lock_the_session(&mut run);

    assert_eq!(at(&frame(&mut run), middle.0, middle.1), RED, "the lock");
    run.stop();
}

/// The output the client names, not the window's own, and its workspace.
#[test]
fn a_request_naming_an_output_goes_there() {
    let mut run = Running::start(None);
    let (mut local, toplevel, window) = window(&mut run);
    let (second, area) = second_output(&run);
    let named = local.output(1);

    let (width, height, states) = enter(&mut run, &mut local, &toplevel, Some(&named));

    assert!(states.contains(&FULLSCREEN));
    assert_eq!((width, height), (area.size.w, area.size.h));
    assert_eq!(geometry(&run, &window), area);
    assert_eq!(
        run.state.workspace_of(&window),
        run.state.active_workspace(&second)
    );
    run.stop();
}

/// Above the top layer while it has the keyboard, and below it otherwise.
#[test]
fn the_focused_window_stands_above_the_top_layer() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (_bar, _surface) = bar(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    assert_eq!(at(&frame(&mut run), 5, 5), PAINT, "focused");

    run.state.focus(&fixture);
    assert_eq!(at(&frame(&mut run), 5, 5), BLACK, "without the keyboard");

    run.state.focus(&window);
    assert_eq!(at(&frame(&mut run), 5, 5), PAINT, "focused again");
    run.stop();
}

/// The overlay layer stays above a focused full-screen window.
#[test]
fn the_overlay_layer_stays_above() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel, _) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    let (_overlay, _surface) = overlay(&mut run);

    let picture = frame(&mut run);

    assert_eq!(at(&picture, OUTPUT.0 / 2, OUTPUT.1 / 2), OVERLAY);
    assert_eq!(at(&picture, 5, 5), PAINT);
    run.stop();
}

/// The pointer meets what the frame shows, in the same order.
#[test]
fn the_pointer_follows_the_stack() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (bar, _surface) = bar(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    let (overlay, _overlaid) = overlay(&mut run);
    let middle = (f64::from(OUTPUT.0 / 2), f64::from(OUTPUT.1 / 2));

    assert_eq!(client_under(&run, 5.0, 5.0), local.id, "over the bar");
    assert_eq!(client_under(&run, middle.0, middle.1), overlay.id);

    run.state.focus(&fixture);
    assert_eq!(client_under(&run, 5.0, 5.0), bar.id, "unfocused");
    assert_eq!(client_under(&run, 5.0, 60.0), local.id, "below the bar");

    run.state.focus(&window);
    assert_eq!(client_under(&run, 5.0, 5.0), local.id, "focused again");
    run.stop();
}

/// Workspaces come and go, the output changes size, and the window goes on filling it.
#[test]
fn it_keeps_the_output_through_a_workspace_and_a_size_change() {
    let mut run = Running::start(None);
    let (mut local, toplevel, window) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    let whole = geometry(&run, &window);

    run.state.switch_to(2);
    assert!(run.state.is_parked(&window), "the other workspace hides it");
    run.state.switch_to(1);
    assert_eq!(geometry(&run, &window), whole, "back on its workspace");

    let first = run
        .state
        .space
        .outputs()
        .next()
        .cloned()
        .expect("an output");
    let small = smithay::output::Mode {
        size: (640, 480).into(),
        refresh: 60_000,
    };
    let (width, height, states) = answer(&mut run, &mut local, |run| {
        first.change_current_state(Some(small), None, None, None);
        run.state.arrange_outputs();
    });
    assert!(states.contains(&FULLSCREEN), "the state stays");
    assert_eq!((width, height), (640, 480));
    assert_eq!(geometry(&run, &window).size, (640, 480).into());

    let (second, area) = second_output(&run);
    let there = run.state.active_workspace(&second).expect("a workspace");
    let (width, height, _) = answer(&mut run, &mut local, |run| {
        assert!(run.state.move_window_to(&window, there));
    });
    assert_eq!((width, height), (area.size.w, area.size.h), "as a move");
    assert_eq!(geometry(&run, &window), area);
    run.stop();
}

/// A client that answers with less stands centred, on black, with nothing below it to reach.
#[test]
fn nothing_shows_below_a_smaller_client() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (_bar, _surface) = bar(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);

    let (width, height) = (400, 300);
    paint(&mut run, &mut local, width, height, PAINT);

    let (x, y) = ((OUTPUT.0 - width) / 2, (OUTPUT.1 - height) / 2);
    let shown = geometry(&run, &window);
    assert_eq!((shown.loc.x, shown.loc.y), (x, y));
    let picture = frame(&mut run);
    assert_eq!(at(&picture, x + 1, y + 1), PAINT);
    assert_eq!(at(&picture, x - 1, y), BLACK, "beside it");
    assert_eq!(at(&picture, 60, 60), BLACK, "over the fixture's window");
    assert_eq!(
        at(&picture, OUTPUT.0 - 1, OUTPUT.1 - 1),
        BLACK,
        "the corner"
    );
    assert_eq!(client_under(&run, 5.0, 5.0), None, "the bar hidden");
    assert_eq!(
        client_under(&run, f64::from(x + 1), f64::from(y + 1)),
        local.id
    );
    run.stop();
}

/// A client larger than its output reaches the next one, and stands above nothing there.
#[test]
fn a_larger_client_lifts_nothing_on_the_next_output() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    let (width, height) = (OUTPUT.0 + 40, OUTPUT.1);

    paint(&mut run, &mut local, width, height, PAINT);

    assert_eq!(geometry(&run, &window).size, (width, height).into());
    let picture = run.state.capture(1).expect("a frame");
    let black = picture
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel == BLACK);
    assert!(black.count() < 16, "no black over the next output");
    run.stop();
}

/// Leaving and asking again before the first answer lands keeps the place to return to.
#[test]
fn asking_again_at_once_keeps_the_first_geometry() {
    let mut run = Running::start(None);
    let (mut local, toplevel, window) = window(&mut run);
    let before = geometry(&run, &window);
    enter(&mut run, &mut local, &toplevel, None);

    toplevel.unset_fullscreen();
    toplevel.set_fullscreen(None);
    local.roundtrip(&mut run);
    local.redraw(&mut run);
    local.client.configure = None;
    let (width, height, states) = answer(&mut run, &mut local, |_| toplevel.unset_fullscreen());

    assert!(!states.contains(&FULLSCREEN));
    assert_eq!((width, height), (before.size.w, before.size.h));
    assert_eq!(geometry(&run, &window), before);
    run.stop();
}

/// A window on an inactive workspace is served, and comes back as it was asked.
#[test]
fn leaving_from_another_workspace_is_served() {
    let mut run = Running::start(None);
    let (mut local, toplevel, window) = window(&mut run);
    let before = geometry(&run, &window);
    enter(&mut run, &mut local, &toplevel, None);
    run.state.switch_to(2);
    assert!(run.state.is_parked(&window));

    let (width, height, states) = answer(&mut run, &mut local, |_| toplevel.unset_fullscreen());

    assert!(!states.contains(&FULLSCREEN));
    assert_eq!((width, height), (before.size.w, before.size.h));
    run.state.switch_to(1);
    assert_eq!(geometry(&run, &window), before);
    run.stop();
}

/// The window that takes the keyboard takes the pointer from the bar it now hides.
#[test]
fn a_focus_change_moves_the_pointer() {
    let mut run = Running::start(None);
    let fixture = clear_the_fixture(&mut run);
    let (bar, _surface) = bar(&mut run);
    let (mut local, toplevel, window) = window(&mut run);
    enter(&mut run, &mut local, &toplevel, None);
    run.state.focus(&fixture);
    run.state
        .pointer_to((5.0, 5.0).into(), smithay::backend::input::InputTime::now());
    local.roundtrip(&mut run);
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    let focused = |pointer: &smithay::input::pointer::PointerHandle<State>| {
        pointer
            .current_focus()
            .and_then(|s| s.client())
            .map(|c| c.id())
    };
    assert_eq!(focused(&pointer), bar.id, "the bar shows");

    run.state.focus(&window);
    local.roundtrip(&mut run);

    assert_eq!(focused(&pointer), local.id, "the window shows");
    run.stop();
}

/// A request made before the surface's first commit is in its initial configure.
#[test]
fn a_request_before_the_first_commit_is_served() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let named = local.output(1);
    let (_, area) = second_output(&run);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_fullscreen(Some(&named));
    surface.commit();
    local.window = Some((surface, xdg));

    let (width, height, states) = local.redraw(&mut run);

    assert!(states.contains(&FULLSCREEN));
    assert_eq!((width, height), (area.size.w, area.size.h));
    let window = window_of(&run, local.id.as_ref().expect("an id"));
    assert_eq!(geometry(&run, &window), area);
    run.stop();
}

/// A confined client smaller than its output is bordered there, in front of the black.
#[test]
fn a_smaller_confined_client_keeps_its_border() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel) = confined(&mut run);
    enter(&mut run, &mut local, &toplevel, None);

    let (width, height) = (400, 300);
    paint(&mut run, &mut local, width, height, PAINT);

    let picture = frame(&mut run);
    let (x, y) = ((OUTPUT.0 - width) / 2, OUTPUT.1 / 2);
    assert_eq!(
        at(&picture, x - 2, y),
        [255, 255, 255, 255],
        "the white line"
    );
    assert_eq!(at(&picture, x - 1, y), BLACK);
    assert_eq!(at(&picture, x, y), PAINT);
    run.stop();
}

/// The request reaches a confined window too, and its border stays inside the output's edge.
#[test]
fn a_confined_window_keeps_its_border_inside_the_edge() {
    let mut run = Running::start(None);
    clear_the_fixture(&mut run);
    let (mut local, toplevel) = confined(&mut run);

    enter(&mut run, &mut local, &toplevel, None);

    let picture = frame(&mut run);
    let middle = OUTPUT.1 / 2;
    assert_eq!(at(&picture, 0, middle), BLACK, "the outer line");
    assert_eq!(
        at(&picture, 1, middle),
        [255, 255, 255, 255],
        "the white line"
    );
    assert_eq!(at(&picture, OUTPUT.0 - 2, middle), [255, 255, 255, 255]);
    assert_eq!(at(&picture, 3, middle), PAINT, "the client inside it");
    run.stop();
}
