//! Decision 33, wave 2b: `wl_drm` (`wl_drm.rs`), on a render node only and naming the one
//! linux-dmabuf's feedback names. Without a render node the fixture runs on Pixman, where the
//! global must be absent; on a GPU, `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test --
//! --ignored` binds it, reads `device` and creates a prime buffer.
//!
//! wp_tearing_control_manager_v1 (`tearing.rs`): the hint is applied with the commit, a second
//! object for a surface is the protocol's error, and each condition of decision 33 refuses the
//! tear on its own, read from the frame as the DRM backend reads it.

use super::*;
use smithay::backend::allocator::gbm::{GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Buffer;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer as ServerBuffer;
use smithay::wayland::dmabuf::get_dmabuf;
use std::os::unix::fs::MetadataExt;
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};
use wl_drm_client::wl_drm;
use super::marks::{at, clear_the_fixture, confined, frame, lock_the_session, paint};
use crate::tearing::Conditions;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::utils::Physical;
use wayland_protocols::wp::tearing_control::v1::client::{
    wp_tearing_control_manager_v1::WpTearingControlManagerV1,
    wp_tearing_control_v1::{PresentationHint, WpTearingControlV1},
};

/// The client side of Mesa's wayland-drm.xml, which the compositor serves.
#[allow(
    dead_code,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    clippy::all
)]
mod wl_drm_client {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-client re-exports.
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/wayland-drm.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/wayland-drm.xml");
}

/// What a client of wl_drm and of linux-dmabuf's default feedback was told, on a queue of its
/// own beside the fixture client's.
#[derive(Default)]
struct Heard {
    device: Option<String>,
    formats: Vec<u32>,
    capabilities: Option<u32>,
    authenticated: bool,
    /// The feedback's main device, a `dev_t` in native byte order.
    main_device: Option<Vec<u8>>,
}

impl Dispatch<wl_drm::WlDrm, ()> for Heard {
    fn event(
        heard: &mut Self,
        _: &wl_drm::WlDrm,
        event: wl_drm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_drm::Event::Device { name } => heard.device = Some(name),
            wl_drm::Event::Format { format } => heard.formats.push(format),
            wl_drm::Event::Capabilities { value } => heard.capabilities = Some(value),
            wl_drm::Event::Authenticated => heard.authenticated = true,
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, ()> for Heard {
    fn event(
        heard: &mut Self,
        _: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } = event {
            heard.main_device = Some(device);
        }
    }
}

delegate_noop!(Heard: ignore zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1);
delegate_noop!(Heard: ignore wl_buffer::WlBuffer);

/// Binds the global `interface` of `local` at `version` on `queue`.
fn bind_on<I>(local: &Local, queue: &wayland_client::EventQueue<Heard>, version: u32) -> I
where
    I: Proxy + 'static,
    Heard: Dispatch<I, ()>,
{
    let (name, _, _) = local
        .client
        .globals
        .iter()
        .find(|(_, interface, _)| interface == I::interface().name)
        .unwrap_or_else(|| panic!("{} is listed", I::interface().name));
    local
        .registry
        .bind::<I, (), Heard>(*name, version, &queue.handle(), ())
}

/// Without a render node, wl_drm is not offered: it would name no device.
#[test]
fn wave2b_wl_drm_is_not_offered_without_a_render_node() {
    let mut run = Running::start(None);
    let local = Local::connect(&mut run, None);
    assert!(local.sees("wl_compositor"), "the client lists the globals");
    assert!(!local.sees("wl_drm"), "wl_drm without a render node");
    assert!(!local.sees("zwp_linux_dmabuf_v1"));
    run.stop();
}

/// On a render node a client binds wl_drm at version 2 and is sent the node linux-dmabuf's
/// default feedback names, the formats and prime; a buffer it allocates there through GBM and
/// passes to `create_prime_buffer` is a dmabuf in the compositor, which the renderer imported.
/// `authenticate` is answered; a flink name is the `invalid_name` error.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn wave2b_wl_drm_names_the_feedback_node_and_takes_a_prime_buffer() {
    let node = PathBuf::from(
        std::env::var_os("ATHANOR_RENDER_NODE").expect("ATHANOR_RENDER_NODE names a render node"),
    );
    let mut run = Running::start(Some(&node));
    let mut local = Local::connect(&mut run, None);
    let mut queue = local.connection.new_event_queue::<Heard>();
    let mut heard = Heard::default();
    let drm: wl_drm::WlDrm = bind_on(&local, &queue, 2);
    let dmabuf: zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1 = bind_on(&local, &queue, 4);
    let _feedback = dmabuf.get_default_feedback(&queue.handle(), ());
    drm.authenticate(0);
    local.roundtrip(&mut run);
    queue.dispatch_pending(&mut heard).expect("dispatch");

    let device = heard.device.clone().expect("wl_drm sent its device");
    let main = heard
        .main_device
        .clone()
        .expect("the feedback sent its main device");
    let main = u64::from_ne_bytes(main.try_into().expect("a dev_t"));
    let rdev = std::fs::metadata(&device)
        .expect("the device exists")
        .rdev();
    assert_eq!(rdev, main, "{device} is the node the feedback names");
    assert!(
        device.contains("renderD"),
        "{device} is a render node, which needs no authentication"
    );
    assert_eq!(heard.capabilities, Some(1), "prime");
    assert!(heard.authenticated);
    let argb = Fourcc::Argb8888 as u32;
    assert!(heard.formats.contains(&argb), "{:x?}", heard.formats);

    let gbm = GbmDevice::new(std::fs::File::open(&device).expect("the render node"))
        .expect("a GBM device");
    let bo = gbm
        .create_buffer_object::<()>(64, 64, Fourcc::Argb8888, GbmBufferFlags::RENDERING)
        .expect("a buffer object");
    let fd = bo.fd().expect("its dmabuf");
    let (offset, stride) = (bo.offset(0), bo.stride());
    let buffer = drm.create_prime_buffer(
        fd.as_fd(),
        64,
        64,
        argb,
        i32::try_from(offset).expect("an offset"),
        i32::try_from(stride).expect("a stride"),
        0,
        0,
        0,
        0,
        &queue.handle(),
        (),
    );
    local.roundtrip(&mut run);
    assert!(local.connection.protocol_error().is_none());
    let id = local.id.clone().expect("an id");
    let object = run
        .state
        .display_handle
        .backend_handle()
        .object_for_protocol_id(id, ServerBuffer::interface(), buffer.id().protocol_id())
        .expect("the buffer is the compositor's");
    let server = ServerBuffer::from_id(&run.state.display_handle, object).expect("a wl_buffer");
    let imported = get_dmabuf(&server).expect("a dmabuf buffer");
    assert_eq!((imported.size().w, imported.size().h), (64, 64));
    assert_eq!(imported.format().code, Fourcc::Argb8888);
    assert_eq!(imported.format().modifier, Modifier::Invalid);
    assert_eq!(imported.strides().collect::<Vec<_>>(), [stride]);

    let mut flink = Local::connect(&mut run, None);
    let queue = flink.connection.new_event_queue::<Heard>();
    let drm: wl_drm::WlDrm = bind_on(&flink, &queue, 2);
    let _buffer = drm.create_buffer(1, 64, 64, stride, argb, &queue.handle(), ());
    let error = super::devices::protocol_error(&mut run, &mut flink);
    assert_eq!(error.object_interface, "wl_drm");
    assert_eq!(error.code, 2, "invalid_name");
    run.stop();
}

delegate_noop!(Client: ignore WpTearingControlManagerV1);
delegate_noop!(Client: ignore WpTearingControlV1);

const PAINT: [u8; 4] = [0x60, 0x20, 0x60, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// Decision 33's conditions for the first output's next frame, read from the elements the DRM
/// backend draws, at a magnifier `zoom` if any.
fn conditions(run: &mut Running, zoom: Option<(Point<i32, Physical>, f64)>) -> Conditions {
    let output = run.state.space.outputs().next().expect("an output").clone();
    let mut renderer = PixmanRenderer::new().expect("a renderer");
    let marks = run.state.marks();
    let elements = crate::marks::stack(&mut renderer, &run.state.space, &output, &marks);
    let elements = crate::look::zoomed(elements, zoom);
    let popups = run.state.composing.popups_on(&run.state.space, &output);
    let locked = run.state.lock.is_some();
    crate::tearing::conditions(&run.state.space, &output, locked, &popups, &elements)
}

/// A window of a client, confined or not, alone on the first output: the fixture's window goes
/// to the second. With `full_screen`, it takes the output.
fn alone(run: &mut Running, confine: bool, full_screen: bool) -> (Local, xdg_toplevel::XdgToplevel) {
    let fixture = clear_the_fixture(run);
    let (mut local, toplevel) = if confine {
        confined(run)
    } else {
        let mut local = Local::connect(run, None);
        let toplevel = local.toplevel(run);
        (local, toplevel)
    };
    let second = run.state.space.outputs().nth(1).expect("a second output").clone();
    let area = run.state.space.output_geometry(&second).expect("placed");
    run.state.space.map_element(fixture, area.loc, false);
    if full_screen {
        local.roundtrip(run);
        local.client.configure = None;
        toplevel.set_fullscreen(None);
        let (width, height, _) = local.redraw(run);
        paint(run, &mut local, width, height, PAINT);
    }
    (local, toplevel)
}

/// Asks for an async presentation, which the next commit applies.
fn ask(run: &mut Running, local: &mut Local) -> WpTearingControlV1 {
    let manager: WpTearingControlManagerV1 = local.bind(1);
    let (surface, _) = local.window.clone().expect("a window");
    let control = manager.get_tearing_control(&surface, &local.queue.handle(), ());
    control.set_presentation_hint(PresentationHint::Async);
    surface.commit();
    local.roundtrip(run);
    control
}

/// Every condition of decision 33 met: a confined client's full-screen window, alone, with its
/// border in the frame.
const MET: Conditions = Conditions {
    asked: true,
    alone: true,
    layered: false,
    locked: false,
    confined: true,
    bordered: true,
};

/// Every client sees the manager; the hint is double-buffered, and destroying the object takes
/// it back to vsync with the next commit.
#[test]
fn a_presentation_hint_is_applied_with_the_commit() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, true);
    assert!(local.sees("wp_tearing_control_manager_v1"));
    let manager: WpTearingControlManagerV1 = local.bind(1);
    let (surface, _) = local.window.clone().expect("a window");
    let control = manager.get_tearing_control(&surface, &local.queue.handle(), ());
    control.set_presentation_hint(PresentationHint::Async);
    local.roundtrip(&mut run);
    assert!(!conditions(&mut run, None).asked, "before the commit");
    surface.commit();
    local.roundtrip(&mut run);
    assert!(conditions(&mut run, None).asked, "after the commit");

    control.destroy();
    local.roundtrip(&mut run);
    assert!(conditions(&mut run, None).asked, "destroyed, before the commit");
    surface.commit();
    local.roundtrip(&mut run);
    assert!(!conditions(&mut run, None).asked, "destroyed, after the commit");
    // A destroyed object leaves room for a new one.
    let again = manager.get_tearing_control(&surface, &local.queue.handle(), ());
    again.set_presentation_hint(PresentationHint::Vsync);
    surface.commit();
    local.roundtrip(&mut run);
    assert!(local.connection.protocol_error().is_none());
    run.stop();
}

/// A second wp_tearing_control_v1 for a surface is the manager's tearing_control_exists error.
#[test]
fn a_second_tearing_control_is_the_protocol_error() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    let manager: WpTearingControlManagerV1 = local.bind(1);
    let (surface, _) = local.window.clone().expect("a window");
    let qh = local.queue.handle();
    let _first = manager.get_tearing_control(&surface, &qh, ());
    let _second = manager.get_tearing_control(&surface, &qh, ());
    let error = super::devices::protocol_error(&mut run, &mut local);
    assert_eq!(error.code, 0, "tearing_control_exists");
    assert_eq!(error.object_interface, "wp_tearing_control_manager_v1");
    run.stop();
}

/// A confined client's full-screen window alone on its output, asking for async, meets every
/// condition, and OS4's border is drawn, at full opacity, in that frame.
#[test]
fn a_frame_that_may_tear_draws_the_border() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, true);
    let _control = ask(&mut run, &mut local);
    let met = conditions(&mut run, None);
    assert_eq!(met, MET);
    assert!(crate::tearing::tears(met));
    let shown = frame(&mut run);
    assert_eq!(at(&shown, 1, 400), WHITE, "the border's white line");
    assert_eq!(at(&shown, 0, 400), BLACK, "the border's outer line");
    run.stop();
}

/// A window outside a security context has no border, and needs none.
#[test]
fn an_unconfined_window_may_tear_without_a_border() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, false, true);
    let _control = ask(&mut run, &mut local);
    let met = conditions(&mut run, None);
    assert_eq!(
        met,
        Conditions {
            confined: false,
            bordered: false,
            ..MET
        }
    );
    assert!(crate::tearing::tears(met));
    run.stop();
}

/// A surface that has not asked for async is never torn.
#[test]
fn vsync_refuses_the_tear() {
    let mut run = Running::start(None);
    alone(&mut run, true, true);
    let refused = conditions(&mut run, None);
    assert_eq!(refused, Conditions { asked: false, ..MET });
    assert!(!crate::tearing::tears(refused));
    run.stop();
}

/// A window that is alone but not full-screen is not torn.
#[test]
fn a_window_not_full_screen_refuses_the_tear() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, false);
    let _control = ask(&mut run, &mut local);
    let refused = conditions(&mut run, None);
    assert!(refused.asked && !refused.alone, "{refused:?}");
    assert!(!crate::tearing::tears(refused));
    run.stop();
}

/// A full-screen window with another window on its output is not torn.
#[test]
fn another_window_on_the_output_refuses_the_tear() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, true);
    let _control = ask(&mut run, &mut local);
    let mut other = Local::connect(&mut run, None);
    other.toplevel(&mut run);
    let window = window_of(&run, other.id.as_ref().expect("an id"));
    run.state.space.map_element(window, (40, 40), false);
    let refused = conditions(&mut run, None);
    assert_eq!(refused, Conditions { alone: false, ..MET });
    assert!(!crate::tearing::tears(refused));
    run.stop();
}

/// A full-screen window with a layer surface mapped on its output is not torn, even below it.
#[test]
fn a_mapped_layer_surface_refuses_the_tear() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, true);
    let _control = ask(&mut run, &mut local);
    let mut shell = Local::connect(&mut run, None);
    shell.bar(&mut run, 40);
    let refused = conditions(&mut run, None);
    assert_eq!(refused, Conditions { layered: true, ..MET });
    assert!(!crate::tearing::tears(refused));
    run.stop();
}

/// No frame tears while the session is locked.
#[test]
fn the_lock_refuses_the_tear() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, true);
    let _control = ask(&mut run, &mut local);
    let _locker = lock_the_session(&mut run);
    let refused = conditions(&mut run, None);
    assert_eq!(refused, Conditions { locked: true, ..MET });
    assert!(!crate::tearing::tears(refused));
    run.stop();
}

/// A confined client's window whose border is not wholly in the frame is not torn: the
/// magnifier pushes its edges off the output, or the frame leaves the lines out.
#[test]
fn a_confined_window_without_its_border_is_not_torn() {
    let mut run = Running::start(None);
    let (mut local, _toplevel) = alone(&mut run, true, true);
    let _control = ask(&mut run, &mut local);
    let middle = Point::from((OUTPUTS[0].width / 2, OUTPUTS[0].height / 2));
    let refused = conditions(&mut run, Some((middle, 2.0)));
    assert_eq!(refused, Conditions { bordered: false, ..MET });
    assert!(!crate::tearing::tears(refused));

    let output = run.state.space.outputs().next().expect("an output").clone();
    let mut renderer = PixmanRenderer::new().expect("a renderer");
    let marks = run.state.marks();
    let elements = crate::marks::stack(&mut renderer, &run.state.space, &output, &marks);
    let unlined: Vec<_> = elements
        .into_iter()
        .filter(|element| !matches!(element, crate::marks::Stacked::Line(_)))
        .collect();
    let refused = crate::tearing::conditions(&run.state.space, &output, false, &[], &unlined);
    assert_eq!(refused, Conditions { bordered: false, ..MET });
    run.stop();
}
