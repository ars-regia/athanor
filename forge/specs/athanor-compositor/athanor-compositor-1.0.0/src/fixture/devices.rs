//! Fixtures of the tablet, pointer-gestures, xdg-decoration and xdg-foreign globals
//! (doc_own_stack.md decision 30). libinput cannot be driven here, so the tests call the
//! `State` methods the input events end in.

use super::*;
use smithay::backend::input::{
    ButtonState, ProximityState, TabletToolCapabilities, TabletToolTipState, TabletToolType,
};
use smithay::input::tablet::tool::AxisFrame;
use smithay::input::tablet::TabletDescriptor;
use wayland_client::event_created_child;
use wayland_protocols::wp::pointer_gestures::zv1::client::{
    zwp_pointer_gesture_pinch_v1, zwp_pointer_gestures_v1,
};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2, zwp_tablet_pad_v2, zwp_tablet_seat_v2, zwp_tablet_tool_v2, zwp_tablet_v2,
};
use wayland_protocols::xdg::decoration::zv1::client::{
    zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use wayland_protocols::xdg::foreign::zv2::client::{
    zxdg_exported_v2, zxdg_exporter_v2, zxdg_imported_v2, zxdg_importer_v2,
};

/// What the client of these globals was told.
#[derive(Default)]
pub(super) struct Seen {
    /// The mode of each decoration configure.
    modes: Vec<WEnum<zxdg_toplevel_decoration_v1::Mode>>,
    /// The handle an exported object announced.
    handle: Option<String>,
    /// An imported object was told its handle is gone.
    import_destroyed: bool,
    /// The scale of each pinch update; `begun` and `ended` count the pinch's other events.
    pinch_scales: Vec<f64>,
    pinch_begun: usize,
    pinch_ended: usize,
    /// Whether the last pinch end said the pinch was cancelled.
    pinch_cancelled: Option<bool>,
    /// The tablets and tools announced.
    tablets: usize,
    tools: usize,
    /// The surface a tool entered, and the positions it moved to.
    tool_entered: Option<ObjectId>,
    tool_motion: Vec<(f64, f64)>,
    /// The tip downs, button presses, pressure axes and proximity outs of a tool.
    tool_downs: usize,
    tool_buttons: usize,
    tool_releases: usize,
    tool_pressures: usize,
    tool_left: usize,
}

delegate_noop!(Client: zwp_pointer_gestures_v1::ZwpPointerGesturesV1);
delegate_noop!(Client: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);
delegate_noop!(Client: zxdg_exporter_v2::ZxdgExporterV2);
delegate_noop!(Client: zxdg_importer_v2::ZxdgImporterV2);
delegate_noop!(Client: zwp_tablet_manager_v2::ZwpTabletManagerV2);
delegate_noop!(Client: ignore zwp_tablet_pad_v2::ZwpTabletPadV2);
delegate_noop!(Client: ignore zwp_tablet_v2::ZwpTabletV2);

impl Dispatch<zwp_pointer_gesture_pinch_v1::ZwpPointerGesturePinchV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwp_pointer_gesture_pinch_v1::ZwpPointerGesturePinchV1,
        event: zwp_pointer_gesture_pinch_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_pointer_gesture_pinch_v1::Event::Begin { .. } => client.devices.pinch_begun += 1,
            zwp_pointer_gesture_pinch_v1::Event::Update { scale, .. } => {
                client.devices.pinch_scales.push(scale)
            }
            zwp_pointer_gesture_pinch_v1::Event::End { cancelled, .. } => {
                client.devices.pinch_ended += 1;
                client.devices.pinch_cancelled = Some(cancelled != 0);
            }
            _ => {}
        }
    }
}

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            client.devices.modes.push(mode);
        }
    }
}

impl Dispatch<zxdg_exported_v2::ZxdgExportedV2, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zxdg_exported_v2::ZxdgExportedV2,
        event: zxdg_exported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_exported_v2::Event::Handle { handle } = event {
            client.devices.handle = Some(handle);
        }
    }
}

impl Dispatch<zxdg_imported_v2::ZxdgImportedV2, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zxdg_imported_v2::ZxdgImportedV2,
        event: zxdg_imported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_imported_v2::Event::Destroyed = event {
            client.devices.import_destroyed = true;
        }
    }
}

impl Dispatch<zwp_tablet_seat_v2::ZwpTabletSeatV2, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwp_tablet_seat_v2::ZwpTabletSeatV2,
        event: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_seat_v2::Event::TabletAdded { .. } => client.devices.tablets += 1,
            zwp_tablet_seat_v2::Event::ToolAdded { .. } => client.devices.tools += 1,
            _ => {}
        }
    }

    event_created_child!(Client, zwp_tablet_seat_v2::ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (zwp_tablet_v2::ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (zwp_tablet_tool_v2::ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (zwp_tablet_pad_v2::ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<zwp_tablet_tool_v2::ZwpTabletToolV2, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwp_tablet_tool_v2::ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_tool_v2::Event::ProximityIn { surface, .. } => {
                client.devices.tool_entered = Some(surface.id())
            }
            zwp_tablet_tool_v2::Event::Motion { x, y } => client.devices.tool_motion.push((x, y)),
            zwp_tablet_tool_v2::Event::Down { .. } => client.devices.tool_downs += 1,
            zwp_tablet_tool_v2::Event::Button {
                state: WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed),
                ..
            } => client.devices.tool_buttons += 1,
            zwp_tablet_tool_v2::Event::Button {
                state: WEnum::Value(zwp_tablet_tool_v2::ButtonState::Released),
                ..
            } => client.devices.tool_releases += 1,
            zwp_tablet_tool_v2::Event::Pressure { .. } => client.devices.tool_pressures += 1,
            zwp_tablet_tool_v2::Event::ProximityOut => client.devices.tool_left += 1,
            _ => {}
        }
    }
}

/// Whether every mode the client was configured with is client-side.
fn all_client_side(client: &Client) -> bool {
    use zxdg_toplevel_decoration_v1::Mode;
    client
        .devices
        .modes
        .iter()
        .all(|mode| *mode == WEnum::Value(Mode::ClientSide))
}

/// Every request is answered with a configure that says client-side: a server-side request, an
/// unset, a repeated request, and a decoration object made again each get one.
#[test]
fn a_server_side_decoration_request_is_configured_as_client_side() {
    use zxdg_toplevel_decoration_v1::Mode;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let manager: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let decoration = manager.get_toplevel_decoration(&toplevel, &qh, ());
    let mut answers = 1;
    local.pump(&mut run, "the mode of a new decoration", |client, _| {
        client.devices.modes.len() == answers
    });
    for (what, ask) in [
        ("a server-side request", Some(Mode::ServerSide)),
        ("an unset", None),
        ("the same request again", Some(Mode::ServerSide)),
        ("the same request a third time", Some(Mode::ServerSide)),
    ] {
        match ask {
            Some(mode) => decoration.set_mode(mode),
            None => decoration.unset_mode(),
        }
        answers += 1;
        local.pump(&mut run, what, |client, _| {
            client.devices.modes.len() == answers
        });
    }
    decoration.destroy();
    let _again = manager.get_toplevel_decoration(&toplevel, &qh, ());
    answers += 1;
    local.pump(&mut run, "a decoration made again", |client, _| {
        client.devices.modes.len() == answers
    });
    assert!(
        all_client_side(&local.client),
        "{:?}",
        local.client.devices.modes
    );
    run.stop();
}

/// A client that asks and never acknowledges is answered a bounded number of times.
#[test]
fn a_decoration_request_without_an_ack_is_answered_a_bounded_number_of_times() {
    use zxdg_toplevel_decoration_v1::Mode;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let manager: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 = local.bind(1);
    let decoration = manager.get_toplevel_decoration(&toplevel, &local.queue.handle(), ());
    for _ in 0..30 {
        decoration.set_mode(Mode::ServerSide);
    }
    local.roundtrip(&mut run);
    let answered = local.client.devices.modes.len();
    assert!(
        (2..=9).contains(&answered),
        "{answered} answers to 31 requests"
    );
    run.stop();
}

/// A request left unanswered by the cap is answered when the client acknowledges, with no new
/// request.
#[test]
fn a_decoration_request_left_unanswered_is_answered_at_the_ack() {
    use zxdg_toplevel_decoration_v1::Mode;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let toplevel = local.toplevel(&mut run);
    let manager: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 = local.bind(1);
    let decoration = manager.get_toplevel_decoration(&toplevel, &local.queue.handle(), ());
    for _ in 0..30 {
        decoration.set_mode(Mode::ServerSide);
    }
    local.roundtrip(&mut run);
    let answered = local.client.devices.modes.len();
    let xdg = local.window.as_ref().expect("the window").1.clone();
    xdg.ack_configure(local.client.configure.expect("a configure"));
    local.pump(&mut run, "the owed answer", |client, _| {
        client.devices.modes.len() == answered + 1
    });
    run.stop();
}

/// A decoration object made before the first configure gets client-side with it, whatever it asks.
#[test]
fn a_decoration_made_before_the_first_configure_is_client_side() {
    use zxdg_toplevel_decoration_v1::Mode;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let manager: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    let decoration = manager.get_toplevel_decoration(&toplevel, &qh, ());
    decoration.set_mode(Mode::ServerSide);
    surface.commit();
    local.pump(&mut run, "the first configure", |client, _| {
        client.configure.is_some() && !client.devices.modes.is_empty()
    });
    assert!(
        all_client_side(&local.client),
        "{:?}",
        local.client.devices.modes
    );
    run.stop();
}

/// A client with a window and the pointer over it.
fn under_the_pointer(run: &mut Running, local: &mut Local) -> Point<f64, Logical> {
    local.toplevel(run);
    let id = local.id.clone().expect("an id");
    let at = location_of(run, &id).to_f64() + Point::from((20.0, 10.0));
    run.state.pointer_to(at, InputTime::now());
    local.pump(run, "the pointer enters the window", |client, _| {
        client.pointer_enter.is_some()
    });
    at
}

/// A pinch reaches the surface the pointer is on, with its scale.
#[test]
fn a_pinch_reaches_the_focused_surface() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    under_the_pointer(&mut run, &mut local);
    let gestures: zwp_pointer_gestures_v1::ZwpPointerGesturesV1 = local.bind(1);
    let pointer = local.pointer.clone().expect("a pointer");
    let _pinch = gestures.get_pinch_gesture(&pointer, &local.queue.handle(), ());
    local.roundtrip(&mut run);
    run.state.pinch_begin(InputTime::now(), 2);
    run.state
        .pinch_update(InputTime::now(), Point::from((1.0, 2.0)), 1.5, 10.0);
    run.state.pinch_end(InputTime::now(), false);
    local.pump(&mut run, "the pinch arrives", |client, _| {
        client.devices.pinch_ended == 1
    });
    assert_eq!(local.client.devices.pinch_begun, 1);
    assert_eq!(local.client.devices.pinch_scales, [1.5]);
    run.stop();
}

/// A tool's proximity and motion reach the surface under it.
#[test]
fn a_tablet_tools_proximity_and_motion_reach_the_surface_under_it() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let at = under_the_pointer(&mut run, &mut local);
    let seat = local.seat.clone().expect("a seat");
    let manager: zwp_tablet_manager_v2::ZwpTabletManagerV2 = local.bind(1);
    let _tablets = manager.get_tablet_seat(&seat, &local.queue.handle(), ());
    local.roundtrip(&mut run);
    let tablet = TabletDescriptor {
        name: "fixture tablet".into(),
        usb_id: None,
        syspath: None,
    };
    let tool = smithay::backend::input::TabletToolDescriptor {
        tool_type: TabletToolType::Pen,
        hardware_serial: 1,
        hardware_id_wacom: 0,
        capabilities: TabletToolCapabilities::PRESSURE,
    };
    run.state.tablet_added(&tablet);
    run.state.tool_proximity(
        &tablet,
        &tool,
        at,
        ProximityState::In,
        AxisFrame::default(),
        InputTime::now(),
    );
    run.state.tool_motion(
        &tool,
        at + Point::from((5.0, 5.0)),
        AxisFrame::default(),
        InputTime::now(),
    );
    local.pump(&mut run, "the tool moves", |client, _| {
        client.devices.tool_motion.len() == 2
    });
    let (surface, _) = local.window.as_ref().expect("the window");
    assert_eq!(local.client.devices.tablets, 1);
    assert_eq!(local.client.devices.tool_entered, Some(surface.id()));
    // Entering sends the position too; the last is the move.
    assert_eq!(
        local.client.devices.tool_motion,
        [(20.0, 10.0), (25.0, 15.0)]
    );
    run.stop();
}

/// The server-side parent of the window `local` mapped.
fn parent_of(run: &Running, local: &Local) -> Option<WlSurface> {
    let id = local.id.clone().expect("an id");
    window_of(run, &id).toplevel().expect("a toplevel").parent()
}

/// An imported surface is the importing toplevel's parent, and the handle stops working when
/// the exported surface is destroyed.
#[test]
fn an_imported_surface_is_the_parent_until_the_exported_one_is_destroyed() {
    let mut run = Running::start(None);
    let mut owner = Local::connect(&mut run, None);
    let owner_toplevel = owner.toplevel(&mut run);
    let mut dialog = Local::connect(&mut run, None);
    dialog.toplevel(&mut run);
    let (owner_surface, owner_xdg) = owner.window.clone().expect("the owner's window");
    let (dialog_surface, _) = dialog.window.clone().expect("the dialog's window");
    let exporter: zxdg_exporter_v2::ZxdgExporterV2 = owner.bind(1);
    let _exported = exporter.export_toplevel(&owner_surface, &owner.queue.handle(), ());
    owner.pump(&mut run, "a handle", |client, _| {
        client.devices.handle.is_some()
    });
    let handle = owner.client.devices.handle.clone().expect("a handle");

    let importer: zxdg_importer_v2::ZxdgImporterV2 = dialog.bind(1);
    let imported = importer.import_toplevel(handle.clone(), &dialog.queue.handle(), ());
    imported.set_parent_of(&dialog_surface);
    dialog.roundtrip(&mut run);
    let server_owner = {
        let id = owner.id.clone().expect("an id");
        window_of(&run, &id)
            .toplevel()
            .expect("a toplevel")
            .wl_surface()
            .clone()
    };
    assert_eq!(parent_of(&run, &dialog), Some(server_owner));
    assert!(!dialog.client.devices.import_destroyed);

    // The exported toplevel goes, its exported object stays.
    owner_toplevel.destroy();
    owner_xdg.destroy();
    owner_surface.destroy();
    owner.roundtrip(&mut run);
    dialog.pump(&mut run, "the import is told", |client, _| {
        client.devices.import_destroyed
    });
    assert_eq!(parent_of(&run, &dialog), None, "the parent is gone");

    dialog.client.devices.import_destroyed = false;
    let _again = importer.import_toplevel(handle, &dialog.queue.handle(), ());
    dialog.pump(&mut run, "the handle no longer works", |client, _| {
        client.devices.import_destroyed
    });
    run.stop();
}

/// A pen in range over the window of `local`: its tablet, tool and position, once the client has
/// the tool entered.
fn pen_over_window(
    run: &mut Running,
    local: &mut Local,
) -> (
    TabletDescriptor,
    smithay::backend::input::TabletToolDescriptor,
    Point<f64, Logical>,
) {
    let at = under_the_pointer(run, local);
    let seat = local.seat.clone().expect("a seat");
    let manager: zwp_tablet_manager_v2::ZwpTabletManagerV2 = local.bind(1);
    let _tablets = manager.get_tablet_seat(&seat, &local.queue.handle(), ());
    local.roundtrip(run);
    let tablet = TabletDescriptor {
        name: "fixture tablet".into(),
        usb_id: None,
        syspath: None,
    };
    let tool = smithay::backend::input::TabletToolDescriptor {
        tool_type: TabletToolType::Pen,
        hardware_serial: 1,
        hardware_id_wacom: 0,
        capabilities: TabletToolCapabilities::PRESSURE,
    };
    run.state.tablet_added(&tablet);
    run.state.tool_proximity(
        &tablet,
        &tool,
        at,
        ProximityState::In,
        AxisFrame::default(),
        InputTime::now(),
    );
    local.pump(run, "the tool enters", |client, _| {
        client.devices.tool_entered.is_some()
    });
    (tablet, tool, at)
}

fn pressure() -> AxisFrame {
    AxisFrame {
        pressure: Some(0.5),
        ..AxisFrame::default()
    }
}

/// A tool that moves off a surface leaves it before its axes are sent, so the surface is not
/// sent the axes of a position it does not have.
#[test]
fn a_tool_moving_off_a_surface_sends_it_no_axes() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    run.state.tool_motion(
        &tool,
        Point::from((-4000.0, -4000.0)),
        pressure(),
        InputTime::now(),
    );
    local.roundtrip(&mut run);
    assert_eq!(
        local.client.devices.tool_left, 1,
        "the tool left the window"
    );
    assert_eq!(
        local.client.devices.tool_pressures, 0,
        "and sent it no axes"
    );
    run.stop();
}

/// A tip down focuses the window under the tool, as a button press does.
#[test]
fn a_tip_down_focuses_the_window_under_the_tool() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.state.set_keyboard_focus(None);
    assert!(focus(&run.state).is_none());
    run.state
        .tool_tip(&tool, TabletToolTipState::Down, InputTime::now());
    let focused = focus(&run.state).expect("a focus");
    assert_eq!(focused.client().map(|c| c.id()), local.id);
    run.stop();
}

/// A pen in range over a window when the session locks reaches the window with no press, no
/// tip, and no axis, as the pointer reaches it with no click; before the lock it does.
#[test]
fn a_lock_takes_a_tool_in_range_from_the_window_under_it() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, at) = pen_over_window(&mut run, &mut local);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    local.pump(&mut run, "a press before the lock", |client, _| {
        client.devices.tool_buttons == 1
    });
    let (mut locker, _lock) = lock_session(&mut run);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    run.state
        .tool_tip(&tool, TabletToolTipState::Down, InputTime::now());
    run.state
        .tool_motion(&tool, at, pressure(), InputTime::now());
    locker.roundtrip(&mut run);
    local.roundtrip(&mut run);
    let seen = &local.client.devices;
    assert_eq!(seen.tool_buttons, 1, "no press");
    assert_eq!(seen.tool_downs, 0, "no tip");
    assert_eq!(seen.tool_pressures, 0, "no axis");
    assert_eq!(seen.tool_left, 1, "the tool left the window at the lock");
    run.stop();
}

/// A tip held down when the session locks stops reaching the window it was drawing in.
#[test]
fn a_lock_ends_the_stroke_in_a_window() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, at) = pen_over_window(&mut run, &mut local);
    run.state
        .tool_tip(&tool, TabletToolTipState::Down, InputTime::now());
    local.pump(&mut run, "the tip down", |client, _| {
        client.devices.tool_downs == 1
    });
    run.state.tool_motion(
        &tool,
        at + Point::from((2.0, 2.0)),
        pressure(),
        InputTime::now(),
    );
    local.pump(&mut run, "a stroke", |client, _| {
        client.devices.tool_pressures == 1
    });
    let (mut locker, _lock) = lock_session(&mut run);
    // The lock itself ends the grab; the tool may be moved once at that instant, then it is out.
    local.roundtrip(&mut run);
    assert_eq!(
        local.client.devices.tool_left, 1,
        "the tool left at the lock"
    );
    let moves = local.client.devices.tool_motion.len();
    let pressures = local.client.devices.tool_pressures;
    run.state.tool_motion(
        &tool,
        at + Point::from((9.0, 9.0)),
        pressure(),
        InputTime::now(),
    );
    locker.roundtrip(&mut run);
    local.roundtrip(&mut run);
    assert_eq!(local.client.devices.tool_motion.len(), moves, "no motion");
    assert_eq!(local.client.devices.tool_pressures, pressures, "no axis");
    run.stop();
}

/// A pinch in progress when the session locks is ended, as cancelled, on the surface it began
/// on, and what follows reaches nobody. Smithay does this when the focus leaves
/// (`pointer_gestures::leave`), so the test pins the behaviour rather than code of ours; it has
/// no negative control in this crate.
#[test]
fn a_lock_cancels_a_pinch_in_progress() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    under_the_pointer(&mut run, &mut local);
    let gestures: zwp_pointer_gestures_v1::ZwpPointerGesturesV1 = local.bind(1);
    let pointer = local.pointer.clone().expect("a pointer");
    let _pinch = gestures.get_pinch_gesture(&pointer, &local.queue.handle(), ());
    local.roundtrip(&mut run);
    run.state.pinch_begin(InputTime::now(), 2);
    let (_locker, _lock) = lock_session(&mut run);
    local.pump(&mut run, "the pinch is ended", |client, _| {
        client.devices.pinch_ended == 1
    });
    assert_eq!(local.client.devices.pinch_cancelled, Some(true));
    // The end libinput sends later reaches nobody.
    run.state.pinch_end(InputTime::now(), false);
    local.roundtrip(&mut run);
    assert_eq!(local.client.devices.pinch_ended, 1);
    run.stop();
}

/// Waits for the protocol error of `local`.
fn protocol_error(
    run: &mut Running,
    local: &mut Local,
) -> wayland_client::backend::protocol::ProtocolError {
    local.connection.flush().expect("flush");
    let started = Instant::now();
    loop {
        if let Some(error) = local.connection.protocol_error() {
            return error;
        }
        assert!(started.elapsed() < TIMEOUT, "the error: not in time");
        if let Some(guard) = local.queue.prepare_read() {
            drop(guard.read());
        }
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }
}

/// A client with a window, exported, and a second client's import of it.
fn exported_window(
    run: &mut Running,
) -> (
    Local,
    zxdg_exported_v2::ZxdgExportedV2,
    Local,
    zxdg_importer_v2::ZxdgImporterV2,
    zxdg_imported_v2::ZxdgImportedV2,
    String,
) {
    let (owner, exported, dialog, importer, imported, handle, _) = exported_toplevel(run);
    (owner, exported, dialog, importer, imported, handle)
}

/// `exported_window`, and the importing client's toplevel.
fn exported_toplevel(
    run: &mut Running,
) -> (
    Local,
    zxdg_exported_v2::ZxdgExportedV2,
    Local,
    zxdg_importer_v2::ZxdgImporterV2,
    zxdg_imported_v2::ZxdgImportedV2,
    String,
    xdg_toplevel::XdgToplevel,
) {
    let mut owner = Local::connect(run, None);
    owner.toplevel(run);
    let mut dialog = Local::connect(run, None);
    let dialog_toplevel = dialog.toplevel(run);
    let surface = owner.window.as_ref().expect("the window").0.clone();
    let exporter: zxdg_exporter_v2::ZxdgExporterV2 = owner.bind(1);
    let exported = exporter.export_toplevel(&surface, &owner.queue.handle(), ());
    owner.pump(run, "a handle", |client, _| client.devices.handle.is_some());
    let handle = owner.client.devices.handle.clone().expect("a handle");
    let importer: zxdg_importer_v2::ZxdgImporterV2 = dialog.bind(1);
    let imported = importer.import_toplevel(handle.clone(), &dialog.queue.handle(), ());
    let child = dialog.window.as_ref().expect("the window").0.clone();
    imported.set_parent_of(&child);
    dialog.roundtrip(run);
    (
        owner,
        exported,
        dialog,
        importer,
        imported,
        handle,
        dialog_toplevel,
    )
}

/// Destroying the exported object revokes the handle: the import is told and the child is
/// no longer one.
#[test]
fn destroying_the_exported_object_revokes_the_handle() {
    let mut run = Running::start(None);
    let (mut owner, exported, mut dialog, _importer, _imported, _) = exported_window(&mut run);
    assert!(parent_of(&run, &dialog).is_some());
    exported.destroy();
    owner.roundtrip(&mut run);
    dialog.pump(&mut run, "the import is told", |client, _| {
        client.devices.import_destroyed
    });
    assert_eq!(parent_of(&run, &dialog), None);
    run.stop();
}

/// Destroying the imported object ends the parent it set.
#[test]
fn destroying_the_imported_object_ends_the_parent() {
    let mut run = Running::start(None);
    let (_owner, _exported, mut dialog, _importer, imported, _) = exported_window(&mut run);
    assert!(parent_of(&run, &dialog).is_some());
    imported.destroy();
    dialog.roundtrip(&mut run);
    assert_eq!(parent_of(&run, &dialog), None);
    run.stop();
}

/// Destroying the importer leaves an imported object it made working, as the protocol says.
#[test]
fn destroying_the_importer_leaves_the_parent() {
    let mut run = Running::start(None);
    let (_owner, _exported, mut dialog, importer, _imported, _) = exported_window(&mut run);
    importer.destroy();
    dialog.roundtrip(&mut run);
    assert!(parent_of(&run, &dialog).is_some());
    run.stop();
}

/// A window that is the parent of its own parent would make a loop: a protocol error.
#[test]
fn a_parent_loop_is_a_protocol_error() {
    let mut run = Running::start(None);
    let (mut owner, _exported, dialog, _importer, _imported, _) = exported_window(&mut run);
    // The dialog is the owner's child; the dialog exported, the owner makes it its parent.
    let child = dialog.window.as_ref().expect("the window").0.clone();
    let mut dialog = dialog;
    let exporter: zxdg_exporter_v2::ZxdgExporterV2 = dialog.bind(1);
    let _exported = exporter.export_toplevel(&child, &dialog.queue.handle(), ());
    dialog.pump(&mut run, "a handle", |client, _| {
        client.devices.handle.is_some()
    });
    let handle = dialog.client.devices.handle.clone().expect("a handle");
    let importer: zxdg_importer_v2::ZxdgImporterV2 = owner.bind(1);
    let imported = importer.import_toplevel(handle, &owner.queue.handle(), ());
    let own = owner.window.as_ref().expect("the window").0.clone();
    imported.set_parent_of(&own);
    let error = protocol_error(&mut run, &mut owner);
    assert_eq!(error.object_interface, "zxdg_imported_v2");
    assert_eq!(error.code, zxdg_imported_v2::Error::InvalidSurface as u32);
    run.stop();
}

/// A surface that is no toplevel cannot be a child: a protocol error.
#[test]
fn a_child_that_is_no_toplevel_is_a_protocol_error() {
    let mut run = Running::start(None);
    let (_owner, _exported, mut dialog, _importer, imported, _) = exported_window(&mut run);
    let compositor: wl_compositor::WlCompositor = dialog.bind(4);
    let plain = compositor.create_surface(&dialog.queue.handle(), ());
    imported.set_parent_of(&plain);
    let error = protocol_error(&mut run, &mut dialog);
    assert_eq!(error.object_interface, "zxdg_imported_v2");
    assert_eq!(error.code, zxdg_imported_v2::Error::InvalidSurface as u32);
    run.stop();
}

/// A child that is destroyed is no longer kept by the handle it was a child of, so a client
/// cannot grow the handle's table by making children.
#[test]
fn a_destroyed_child_is_forgotten() {
    let mut run = Running::start(None);
    let (_owner, _exported, mut dialog, _importer, imported, handle) = exported_window(&mut run);
    let compositor: wl_compositor::WlCompositor = dialog.bind(4);
    let wm_base: xdg_wm_base::XdgWmBase = dialog.bind(1);
    let qh = dialog.queue.handle();
    for _ in 0..3 {
        let surface = compositor.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        imported.set_parent_of(&surface);
        dialog.roundtrip(&mut run);
        toplevel.destroy();
        xdg.destroy();
        surface.destroy();
        dialog.roundtrip(&mut run);
    }
    let kept = run.state.foreign.exports[&handle].children.len();
    assert_eq!(kept, 1, "only the dialog window, which is alive");
    run.stop();
}

/// A client sets a parent itself: the toplevel is no child of the handle any more, so destroying
/// the imported object leaves the parent the client chose.
#[test]
fn a_parent_the_client_sets_natively_outlives_the_imported_object() {
    let mut run = Running::start(None);
    let (_owner, _exported, mut dialog, _importer, imported, handle, toplevel) =
        exported_toplevel(&mut run);
    let own = parent_of(&run, &dialog).expect("the imported parent");
    let compositor: wl_compositor::WlCompositor = dialog.bind(4);
    let wm_base: xdg_wm_base::XdgWmBase = dialog.bind(1);
    let qh = dialog.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let other = xdg.get_toplevel(&qh, ());
    surface.commit();
    dialog.roundtrip(&mut run);
    toplevel.set_parent(Some(&other));
    dialog.roundtrip(&mut run);
    dialog.roundtrip(&mut run);
    // The client has two windows now; the parents of all of them.
    let parents = |run: &Running, dialog: &Local| -> Vec<WlSurface> {
        let id = dialog.id.clone().expect("an id");
        run.state
            .space
            .elements()
            .filter(|window| owned_by(window, &id))
            .filter_map(|window| window.toplevel().and_then(|t| t.parent()))
            .collect()
    };
    let native = parents(&run, &dialog);
    assert_eq!(native.len(), 1, "the native parent is set");
    assert_ne!(native[0], own, "and is not the imported one");
    assert!(run.state.foreign.exports[&handle].children.is_empty());
    imported.destroy();
    dialog.roundtrip(&mut run);
    assert_eq!(parents(&run, &dialog), native, "the native parent stays");
    run.stop();
}

/// A toplevel is the child of one handle at a time: the second import takes it from the first.
#[test]
fn a_child_of_a_second_handle_leaves_the_first() {
    let mut run = Running::start(None);
    let (_owner, _exported, mut dialog, importer, _imported, first) = exported_window(&mut run);
    let mut second = Local::connect(&mut run, None);
    second.toplevel(&mut run);
    let surface = second.window.as_ref().expect("the window").0.clone();
    let exporter: zxdg_exporter_v2::ZxdgExporterV2 = second.bind(1);
    let _exported = exporter.export_toplevel(&surface, &second.queue.handle(), ());
    second.pump(&mut run, "a handle", |client, _| {
        client.devices.handle.is_some()
    });
    let handle = second.client.devices.handle.clone().expect("a handle");
    let imported = importer.import_toplevel(handle.clone(), &dialog.queue.handle(), ());
    let child = dialog.window.as_ref().expect("the window").0.clone();
    imported.set_parent_of(&child);
    dialog.roundtrip(&mut run);
    assert!(run.state.foreign.exports[&first].children.is_empty());
    assert_eq!(run.state.foreign.exports[&handle].children.len(), 1);
    run.stop();
}

/// A locker with a tablet seat, and the lock it asked for; no lock surface yet.
fn locker_without_surface(
    run: &mut Running,
) -> (
    Local,
    ext_session_lock_v1::ExtSessionLockV1,
    wl_surface::WlSurface,
    wl_output::WlOutput,
) {
    let mut locker = Local::connect(run, None);
    let compositor: wl_compositor::WlCompositor = locker.bind(4);
    let output: wl_output::WlOutput = locker.bind(4);
    let seat: wl_seat::WlSeat = locker.bind(1);
    let tablets: zwp_tablet_manager_v2::ZwpTabletManagerV2 = locker.bind(1);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = locker.bind(1);
    let qh = locker.queue.handle();
    let _tablet_seat = tablets.get_tablet_seat(&seat, &qh, ());
    let lock = locks.lock(&qh, ());
    let surface = compositor.create_surface(&qh, ());
    locker.roundtrip(run);
    (locker, lock, surface, output)
}

/// Gives the lock its surface on `output`, mapped.
fn show_lock_surface(
    run: &mut Running,
    locker: &mut Local,
    lock: &ext_session_lock_v1::ExtSessionLockV1,
    surface: &wl_surface::WlSurface,
    output: &wl_output::WlOutput,
) {
    let shm: wl_shm::WlShm = locker.bind(1);
    let qh = locker.queue.handle();
    let lock_surface = lock.get_lock_surface(surface, output, &qh, ());
    locker.pump(run, "the lock surface is configured", |client, _| {
        client.lock_configure.is_some()
    });
    let (serial, width, height) = locker.client.lock_configure.expect("a configure");
    lock_surface.ack_configure(serial);
    let (buffer, _) = locker.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    locker.pump(run, "the session is locked", |client, _| client.locked);
}

/// A tip and a button held on `tool`.
fn hold(run: &mut Running, tool: &smithay::backend::input::TabletToolDescriptor) {
    run.state
        .tool_button(tool, 0x14b, ButtonState::Pressed, InputTime::now());
    run.state
        .tool_tip(tool, TabletToolTipState::Down, InputTime::now());
}

/// A tip and a button held on a window when the session locks are not replayed to the lock
/// surface that takes the tool: it sees the tool, and no press.
#[test]
fn a_pen_held_at_lock_presses_nothing_on_the_lock_surface() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    hold(&mut run, &tool);
    local.pump(&mut run, "the hold", |client, _| {
        client.devices.tool_downs == 1 && client.devices.tool_buttons == 1
    });
    let (mut locker, lock, surface, output) = locker_without_surface(&mut run);
    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    local.roundtrip(&mut run);
    assert!(
        locker.client.devices.tool_entered.is_some(),
        "the lock has the tool"
    );
    assert_eq!(locker.client.devices.tool_downs, 0, "no tip on the lock");
    assert_eq!(
        locker.client.devices.tool_buttons, 0,
        "no button on the lock"
    );
    assert_eq!(local.client.devices.tool_left, 1);
    run.stop();
}

/// While the lock has no surface under the pen, a press and a tip reach nothing and leave
/// nothing to replay: the lock surface that appears later sees the tool and no press.
#[test]
fn a_press_before_the_lock_surface_is_not_kept_for_it() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    let (mut locker, lock, surface, output) = locker_without_surface(&mut run);
    local.roundtrip(&mut run);
    assert_eq!(local.client.devices.tool_left, 1, "the lock took the tool");
    hold(&mut run, &tool);
    local.roundtrip(&mut run);
    assert_eq!(local.client.devices.tool_downs, 0, "no tip on the window");
    assert_eq!(
        local.client.devices.tool_buttons, 0,
        "no button on the window"
    );
    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    assert!(
        locker.client.devices.tool_entered.is_some(),
        "the lock has the tool"
    );
    assert_eq!(
        locker.client.devices.tool_downs, 0,
        "no tip kept for the lock"
    );
    assert_eq!(
        locker.client.devices.tool_buttons, 0,
        "no button kept for the lock"
    );
    run.stop();
}

/// A tip and a button held on the lock surface when the session unlocks are not replayed to the
/// window that takes the tool; a new press reaches it.
#[test]
fn a_pen_held_at_unlock_presses_nothing_on_the_window() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    let (mut locker, lock, surface, output) = locker_without_surface(&mut run);
    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    hold(&mut run, &tool);
    locker.roundtrip(&mut run);
    assert_eq!(locker.client.devices.tool_downs, 1, "the lock was pressed");
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    local.roundtrip(&mut run);
    assert_eq!(local.client.devices.tool_downs, 0, "no tip replayed");
    assert_eq!(local.client.devices.tool_buttons, 0, "no button replayed");
    run.state
        .tool_tip(&tool, TabletToolTipState::Up, InputTime::now());
    run.state
        .tool_tip(&tool, TabletToolTipState::Down, InputTime::now());
    local.pump(&mut run, "a new tip", |client, _| {
        client.devices.tool_downs == 1
    });
    run.stop();
}

/// A barrel button held on a window when the session locks, and let go over the lock surface,
/// reaches the lock surface as no event: it never saw the press.
#[test]
fn a_button_held_across_the_lock_is_released_unseen_by_the_lock_surface() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    local.pump(&mut run, "the press", |client, _| {
        client.devices.tool_buttons == 1
    });
    let (mut locker, lock, surface, output) = locker_without_surface(&mut run);
    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Released, InputTime::now());
    locker.roundtrip(&mut run);
    assert_eq!(locker.client.devices.tool_releases, 0, "a release unseen");
    run.stop();
}

/// The same across the unlock: the window sees no release of a press it never saw.
#[test]
fn a_button_held_across_the_unlock_is_released_unseen_by_the_window() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, _) = pen_over_window(&mut run, &mut local);
    let (mut locker, lock, surface, output) = locker_without_surface(&mut run);
    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    locker.pump(&mut run, "the press", |client, _| {
        client.devices.tool_buttons == 1
    });
    lock.unlock_and_destroy();
    locker.roundtrip(&mut run);
    local.roundtrip(&mut run);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Released, InputTime::now());
    local.roundtrip(&mut run);
    assert_eq!(local.client.devices.tool_releases, 0, "a release unseen");
    run.stop();
}

/// A pen left over an output the lock has no surface on holds a tip and a button, which reach
/// nothing; moved back over the lock surface it presses nothing there. This is what
/// `tool_refused` guards: Smithay keeps a tip and buttons held with no focus and replays them
/// to the next one.
#[test]
fn a_press_over_an_output_without_a_lock_surface_is_not_kept() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (tablet, tool, at) = pen_over_window(&mut run, &mut local);
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .and_then(|output| run.state.space.output_geometry(output))
        .expect("the second output's area");
    let away = (second.loc + Point::from((10, 10))).to_f64();
    for (location, proximity) in [(at, ProximityState::Out), (away, ProximityState::In)] {
        run.state.tool_proximity(
            &tablet,
            &tool,
            location,
            proximity,
            AxisFrame::default(),
            InputTime::now(),
        );
    }
    let (mut locker, lock, surface, output) = locker_without_surface(&mut run);
    show_lock_surface(&mut run, &mut locker, &lock, &surface, &output);
    assert!(run.state.surface_under(away).is_none(), "no lock there");
    assert!(run.state.surface_under(at).is_some(), "a lock surface here");
    hold(&mut run, &tool);
    run.state
        .tool_motion(&tool, at, pressure(), InputTime::now());
    locker.roundtrip(&mut run);
    local.roundtrip(&mut run);
    assert!(
        locker.client.devices.tool_entered.is_some(),
        "the lock has the tool"
    );
    assert_eq!(locker.client.devices.tool_downs, 0, "no tip kept");
    assert_eq!(locker.client.devices.tool_buttons, 0, "no button kept");
    run.stop();
}

/// A pen that leaves the range with a barrel button held and comes back over another surface
/// sends that surface no release of a press it never saw.
#[test]
fn a_button_held_through_a_proximity_out_is_released_unseen() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (tablet, tool, at) = pen_over_window(&mut run, &mut local);
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    local.pump(&mut run, "the press", |client, _| {
        client.devices.tool_buttons == 1
    });
    for proximity in [ProximityState::Out, ProximityState::In] {
        run.state.tool_proximity(
            &tablet,
            &tool,
            at,
            proximity,
            AxisFrame::default(),
            InputTime::now(),
        );
    }
    // Smithay releases what was held to the surface the tool leaves: that one is expected.
    local.roundtrip(&mut run);
    let released = local.client.devices.tool_releases;
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Released, InputTime::now());
    local.roundtrip(&mut run);
    assert_eq!(
        local.client.devices.tool_releases, released,
        "a release unseen"
    );
    run.stop();
}

/// A pen that moves onto no surface with a barrel button held, and enters again by a proximity
/// in while Smithay still counts it in range, sends the surface no release of a press it never
/// saw.
#[test]
fn a_button_held_through_a_move_off_and_a_proximity_in_is_released_unseen() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (tablet, tool, at) = pen_over_window(&mut run, &mut local);
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .and_then(|output| run.state.space.output_geometry(output))
        .expect("the second output's area");
    let away = (second.loc + Point::from((10, 10))).to_f64();
    assert!(run.state.surface_under(away).is_none(), "no surface there");
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    local.pump(&mut run, "the press", |client, _| {
        client.devices.tool_buttons == 1
    });
    run.state
        .tool_motion(&tool, away, pressure(), InputTime::now());
    run.state.tool_proximity(
        &tablet,
        &tool,
        at,
        ProximityState::In,
        AxisFrame::default(),
        InputTime::now(),
    );
    // Smithay releases what was held to the surface the tool leaves: that one is expected.
    local.roundtrip(&mut run);
    let released = local.client.devices.tool_releases;
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Released, InputTime::now());
    local.roundtrip(&mut run);
    assert_eq!(
        local.client.devices.tool_releases, released,
        "a release unseen"
    );
    run.stop();
}

/// A pen that moves onto no surface with a barrel button held and back onto the window by
/// motion alone: the window sees the press replayed, so it gets the release.
#[test]
fn a_button_held_through_a_move_off_and_back_is_released_once() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let (_, tool, at) = pen_over_window(&mut run, &mut local);
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .and_then(|output| run.state.space.output_geometry(output))
        .expect("the second output's area");
    let away = (second.loc + Point::from((10, 10))).to_f64();
    assert!(run.state.surface_under(away).is_none(), "no surface there");
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Pressed, InputTime::now());
    local.pump(&mut run, "the press", |client, _| {
        client.devices.tool_buttons == 1
    });
    run.state
        .tool_motion(&tool, away, pressure(), InputTime::now());
    run.state
        .tool_motion(&tool, at, pressure(), InputTime::now());
    local.roundtrip(&mut run);
    let released = local.client.devices.tool_releases;
    assert_eq!(local.client.devices.tool_buttons, 2, "the press replayed");
    run.state
        .tool_button(&tool, 0x14b, ButtonState::Released, InputTime::now());
    local.roundtrip(&mut run);
    assert_eq!(
        local.client.devices.tool_releases,
        released + 1,
        "the release"
    );
    run.stop();
}
