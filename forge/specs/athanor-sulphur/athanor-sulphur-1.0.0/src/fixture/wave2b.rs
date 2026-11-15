//! Decision 33, wave 2b: `wl_drm` (`wl_drm.rs`), on a render node only and naming the one
//! linux-dmabuf's feedback names. Without a render node the fixture runs on Pixman, where the
//! global must be absent; on a GPU, `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test --
//! --ignored` binds it, reads `device` and creates a prime buffer.

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
