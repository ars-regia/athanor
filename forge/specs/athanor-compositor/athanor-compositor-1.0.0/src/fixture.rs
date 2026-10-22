//! The in-process fixture of doc_own_stack.md OS6: the compositor on its headless backend and a
//! Wayland client in one process, joined by a socket pair. The client lists the globals, maps a
//! window from a wl_shm buffer and waits for its frame callback; the compositor's frames are then
//! read back and their pixels compared.

use crate::handlers::MAX_HELD_COMMITS;
use crate::headless::{self, VirtualOutput};
use crate::identity::{AllowList, FileId, Process};
use crate::state::{State, Under, BACKGROUND};
use crate::Error;
use smithay::backend::allocator::dmabuf::{Dmabuf, DmabufFlags};
use smithay::backend::allocator::{Fourcc, Modifier};
use smithay::desktop::layer_map_for_output;
use smithay::reexports::calloop::{EventLoop, Interest};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, Resource};
use smithay::utils::{Point, SERIAL_COUNTER};
use smithay::wayland::security_context::SecurityContext;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::backend::{ObjectId, WaylandError};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{
    delegate_noop, event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1, ext_foreign_toplevel_list_v1,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1, ext_image_capture_source_v1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1, ext_image_copy_capture_manager_v1,
    ext_image_copy_capture_session_v1,
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1,
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1, ext_workspace_manager_v1,
};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1, wp_security_context_v1,
};
use wayland_protocols::xdg::shell::client::{
    xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

/// The window: its top half red, its bottom half blue, so that a frame read upside down fails.
const WIDTH: i32 = 200;
const HEIGHT: i32 = 100;

const OUTPUTS: [VirtualOutput; 2] = [
    VirtualOutput {
        width: 1280,
        height: 800,
        scale: 1.0,
    },
    VirtualOutput {
        width: 800,
        height: 600,
        scale: 1.5,
    },
];

/// Pixels as `headless::FRAME_FORMAT` reads them back: R, G, B, A.
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
/// The background, `state::BACKGROUND` in bytes: whole multiples of 1/255, which Pixman and
/// GLES both read back exactly.
fn background(px: &[u8; 4]) -> bool {
    px == &BACKGROUND.map(|c| (c * 255.0).round() as u8)
}

/// How long the compositor waits for the client.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Every global the compositor offers on every backend.
const GLOBALS: [&str; 13] = [
    "wl_compositor",
    "wl_subcompositor",
    "wl_shm",
    "wl_seat",
    "xdg_wm_base",
    "zwlr_layer_shell_v1",
    "zxdg_output_manager_v1",
    "wp_viewporter",
    "wp_fractional_scale_manager_v1",
    "wp_presentation",
    "wp_cursor_shape_manager_v1",
    "xdg_activation_v1",
    "wl_data_device_manager",
];

#[derive(Default)]
struct Client {
    /// Name, interface and version of each global.
    globals: Vec<(u32, String, u32)>,
    configure: Option<u32>,
    /// The serial, width and height of the last configure of a layer surface.
    layer_configure: Option<(u32, u32, u32)>,
    /// A layer surface was closed by the compositor.
    layer_closed: bool,
    frame_done: bool,
    /// Roundtrips answered (`Local::roundtrip`).
    synced: u32,
    /// The windows of the foreign toplevel list.
    listed: Vec<Listed>,
    /// The buffer size of the last capture session, once its constraints are done.
    capture_size: Option<(u32, u32)>,
    /// The capture sessions stopped.
    stopped: Vec<ObjectId>,
    /// The last capture frame: ready, or failed with its reason.
    captured: Option<Result<(), u32>>,
    locked: bool,
    /// The serial, width and height of the last configure of a lock surface.
    lock_configure: Option<(u32, u32, u32)>,
    /// `idled` and `resumed`, in order.
    idle: Vec<&'static str>,
    /// Each workspace group: the outputs and the workspaces that entered it.
    groups: Vec<(ObjectId, Vec<ObjectId>, Vec<ObjectId>)>,
    /// Each workspace and its last state.
    workspaces: Vec<(ObjectId, Option<ext_workspace_handle_v1::State>)>,
    workspaces_done: bool,
    /// The evdev code of each key pressed while the client had the keyboard.
    keys: Vec<u32>,
}

/// A window of the foreign toplevel list as the client has seen it.
struct Listed {
    handle: ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    title: String,
    app_id: String,
    done: bool,
    closed: bool,
}

/// The user data of a `wl_display.sync` that counts as a roundtrip.
struct Synced;

impl Client {
    fn bind<I>(
        &self,
        registry: &wl_registry::WlRegistry,
        qh: &QueueHandle<Self>,
        version: u32,
    ) -> Result<I, Error>
    where
        I: wayland_client::Proxy + 'static,
        Self: Dispatch<I, ()>,
    {
        let interface = I::interface().name;
        let (name, _, offered) = self
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .ok_or_else(|| format!("no {interface}"))?;
        Ok(registry.bind(*name, version.min(*offered), qh, ()))
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            client.globals.push((name, interface, version));
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for Client {
    fn event(
        client: &mut Self,
        _: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            client.configure = Some(serial);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            client.frame_done = true;
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => client.layer_configure = Some((serial, width, height)),
            zwlr_layer_surface_v1::Event::Closed => client.layer_closed = true,
            _ => {}
        }
    }
}

delegate_noop!(Client: wl_compositor::WlCompositor);
delegate_noop!(Client: zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(Client: wl_shm_pool::WlShmPool);
delegate_noop!(Client: ignore wl_surface::WlSurface);
delegate_noop!(Client: ignore wl_shm::WlShm);
delegate_noop!(Client: ignore wl_buffer::WlBuffer);
delegate_noop!(Client: ignore xdg_toplevel::XdgToplevel);
delegate_noop!(Client: xdg_positioner::XdgPositioner);
delegate_noop!(Client: ignore xdg_popup::XdgPopup);
delegate_noop!(Client: ignore wl_seat::WlSeat);
delegate_noop!(Client: ignore wl_output::WlOutput);
delegate_noop!(Client: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(Client: ext_image_capture_source_v1::ExtImageCaptureSourceV1);
delegate_noop!(Client: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1);
delegate_noop!(Client: ext_session_lock_manager_v1::ExtSessionLockManagerV1);
delegate_noop!(Client: ext_idle_notifier_v1::ExtIdleNotifierV1);
delegate_noop!(Client: wp_security_context_manager_v1::WpSecurityContextManagerV1);
delegate_noop!(Client: wp_security_context_v1::WpSecurityContextV1);

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            client.keys.push(key);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, Synced> for Client {
    fn event(
        client: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &Synced,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        client.synced += 1;
    }
}

impl Dispatch<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            client.listed.push(Listed {
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
                done: false,
                closed: false,
            });
        }
    }

    event_created_child!(Client, ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()> for Client {
    fn event(
        client: &mut Self,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_foreign_toplevel_handle_v1::Event;
        let Some(listed) = client.listed.iter_mut().find(|l| &l.handle == handle) else {
            return;
        };
        match event {
            Event::Title { title } => listed.title = title,
            Event::AppId { app_id } => listed.app_id = app_id,
            Event::Done => listed.done = true,
            Event::Closed => listed.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1, ()> for Client {
    fn event(
        client: &mut Self,
        session: &ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => client.capture_size = Some((width, height)),
            Event::Stopped => client.stopped.push(session.id()),
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Ready => client.captured = Some(Ok(())),
            Event::Failed { reason } => {
                let reason = match reason {
                    WEnum::Value(reason) => u32::from(reason),
                    WEnum::Unknown(reason) => reason,
                };
                client.captured = Some(Err(reason));
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_session_lock_v1::ExtSessionLockV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_session_lock_v1::ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_v1::Event::Locked = event {
            client.locked = true;
        }
    }
}

impl Dispatch<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            client.lock_configure = Some((serial, width, height));
        }
    }
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => client.idle.push("idled"),
            ext_idle_notification_v1::Event::Resumed => client.idle.push("resumed"),
            _ => {}
        }
    }
}

impl Dispatch<ext_workspace_manager_v1::ExtWorkspaceManagerV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_manager_v1::Event;
        match event {
            Event::WorkspaceGroup { workspace_group } => {
                client
                    .groups
                    .push((workspace_group.id(), Vec::new(), Vec::new()));
            }
            Event::Workspace { workspace } => client.workspaces.push((workspace.id(), None)),
            Event::Done => client.workspaces_done = true,
            _ => {}
        }
    }

    event_created_child!(Client, ext_workspace_manager_v1::ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()> for Client {
    fn event(
        client: &mut Self,
        group: &ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_workspace_group_handle_v1::Event;
        let Some((_, outputs, workspaces)) = client
            .groups
            .iter_mut()
            .find(|(id, _, _)| *id == group.id())
        else {
            return;
        };
        match event {
            Event::OutputEnter { output } => outputs.push(output.id()),
            Event::WorkspaceEnter { workspace } => workspaces.push(workspace.id()),
            _ => {}
        }
    }
}

impl Dispatch<ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()> for Client {
    fn event(
        client: &mut Self,
        workspace: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_workspace_handle_v1::Event::State {
            state: WEnum::Value(state),
        } = event
        {
            if let Some((_, last)) = client
                .workspaces
                .iter_mut()
                .find(|(id, _)| *id == workspace.id())
            {
                *last = Some(state);
            }
        }
    }
}

/// A new file, already unlinked, for a client's buffers.
fn unlinked() -> Result<std::fs::File, Error> {
    let path = std::env::temp_dir().join(format!(
        "athanor-compositor-fixture-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    Ok(file)
}

/// An unlinked file holding the window's pixels, XRGB8888 in memory order B, G, R, X.
fn pixels() -> Result<std::fs::File, Error> {
    let mut file = unlinked()?;
    let row = |bgrx: [u8; 4]| bgrx.repeat(WIDTH as usize);
    for y in 0..HEIGHT {
        let colour = if y < HEIGHT / 2 {
            [0, 0, 255, 255]
        } else {
            [255, 0, 0, 255]
        };
        file.write_all(&row(colour))?;
    }
    Ok(file)
}

/// The client's side: lists the globals, maps the window, waits for its first frame callback,
/// reports the globals' interfaces and stays connected until `quit` is dropped, sending a
/// request at each message on it.
fn client(
    stream: UnixStream,
    report: &mpsc::Sender<Result<Vec<String>, String>>,
    quit: &mpsc::Receiver<()>,
) -> Result<(), Error> {
    let connection = Connection::from_socket(stream)?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let mut state = Client::default();
    queue.roundtrip(&mut state)?;

    let compositor: wl_compositor::WlCompositor = state.bind(&registry, &qh, 4)?;
    let shm: wl_shm::WlShm = state.bind(&registry, &qh, 1)?;
    let wm_base: xdg_wm_base::XdgWmBase = state.bind(&registry, &qh, 1)?;
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("fixture".into());
    surface.commit();
    while state.configure.is_none() {
        queue.blocking_dispatch(&mut state)?;
    }
    xdg.ack_configure(state.configure.ok_or("no configure")?);

    let file = pixels()?;
    let pool = shm.create_pool(file.as_fd(), WIDTH * HEIGHT * 4, &qh, ());
    let buffer = pool.create_buffer(
        0,
        WIDTH,
        HEIGHT,
        WIDTH * 4,
        wl_shm::Format::Xrgb8888,
        &qh,
        (),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, WIDTH, HEIGHT);
    surface.frame(&qh, ());
    surface.commit();
    while !state.frame_done {
        queue.blocking_dispatch(&mut state)?;
    }

    report.send(Ok(state
        .globals
        .iter()
        .map(|(_, i, _)| i.clone())
        .collect()))?;
    // The window lives while the connection does; the compositor reads its frames meanwhile.
    // A message asks for a request, which wakes the compositor to read from its clients; the
    // compositor drops its sender to say it is done, which ends the wait.
    while quit.recv().is_ok() {
        connection.display().sync(&qh, ());
        connection.flush()?;
    }
    Ok(())
}

/// The pixel at `(x, y)` of a frame `width` pixels wide.
fn pixel(frame: &[u8], width: i32, x: i32, y: i32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    let mut px = [0; 4];
    px.copy_from_slice(&frame[at..at + 4]);
    px
}

/// The compositor on two outputs, and a client that has mapped its window and had its frame.
struct Running {
    event_loop: EventLoop<'static, State>,
    state: State,
    /// The interfaces of the globals the client saw.
    globals: Vec<String>,
    /// A message makes the client send a request; dropped, it lets the client go.
    quit: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
    /// The executable and cgroup of the next client to connect: the bar's at the start.
    identity: Arc<Mutex<(&'static str, &'static str)>>,
    /// The file its executable opens: the bar's at the start.
    executable_file: Arc<Mutex<FileId>>,
    /// How many times the compositor has read the file a client's executable opens.
    executable_stats: Arc<AtomicUsize>,
}

/// The bar, as the fixture's clients connect by default: its executable and its cgroup.
const BAR: (&str, &str) = (
    "/usr/bin/athanor-bar",
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service\n",
);

/// What the fixture grants the bar: every privileged global, so that the tests of the earlier
/// tasks run as the bar.
const FIXTURE_ALLOW_LIST: &str = "/usr/bin/athanor-bar athanor-bar.service zwlr_layer_shell_v1 \
    ext_foreign_toplevel_list_v1 ext_foreign_toplevel_image_capture_source_manager_v1 \
    ext_image_copy_capture_manager_v1 ext_session_lock_manager_v1\n";

/// The bar's executable file, as the compositor sees it.
const BAR_FILE: FileId = FileId { dev: 1, ino: 1 };

/// The process at the other end of the fixture's sockets, which is the test's own, as whatever
/// executable, file and cgroup the test sets: the pidfd is read as the compositor reads it. The
/// compositor's file system holds the bar's executable only.
struct Fake(
    Arc<Mutex<(&'static str, &'static str)>>,
    Arc<Mutex<FileId>>,
    Arc<AtomicUsize>,
);

impl Process for Fake {
    fn executable(&self, _pid: i32) -> std::io::Result<PathBuf> {
        Ok(self.0.lock().expect("identity").0.into())
    }
    fn executable_file(&self, _pid: i32) -> std::io::Result<FileId> {
        self.2.fetch_add(1, Ordering::SeqCst);
        Ok(*self.1.lock().expect("file"))
    }
    fn cgroup(&self, _pid: i32) -> std::io::Result<String> {
        Ok(self.0.lock().expect("identity").1.into())
    }
    fn file(&self, path: &Path) -> std::io::Result<FileId> {
        if path == Path::new(BAR.0) {
            Ok(BAR_FILE)
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }
}

impl Running {
    /// Starts the compositor on `node`, or on Pixman, and runs it until the client has had
    /// its frame.
    fn start(node: Option<&Path>) -> Self {
        let mut event_loop: EventLoop<State> = EventLoop::try_new().expect("event loop");
        let display: Display<State> = Display::new().expect("display");
        let mut state =
            headless::start(&mut event_loop, display, node, &OUTPUTS).expect("headless backend");

        let (server_end, client_end) = UnixStream::pair().expect("socket pair");
        let identity = Arc::new(Mutex::new(BAR));
        let executable_file = Arc::new(Mutex::new(BAR_FILE));
        let executable_stats = Arc::new(AtomicUsize::new(0));
        state.allow_list = AllowList::parse(FIXTURE_ALLOW_LIST).expect("allow-list");
        state.process = Box::new(Fake(
            identity.clone(),
            executable_file.clone(),
            executable_stats.clone(),
        ));
        let client_state = state.client_state(&server_end, None);
        state
            .display_handle
            .insert_client(server_end, client_state)
            .expect("client");
        let (globals_tx, globals_rx) = mpsc::channel();
        let (quit, quit_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            if let Err(error) = client(client_end, &globals_tx, &quit_rx) {
                // The compositor's loop below stops on this.
                drop(globals_tx.send(Err(error.to_string())));
            }
        });

        let started = Instant::now();
        let globals = loop {
            event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut state)
                .expect("dispatch");
            match globals_rx.try_recv() {
                Ok(result) => break result.expect("the client failed"),
                Err(mpsc::TryRecvError::Empty) => {
                    assert!(
                        started.elapsed() < TIMEOUT,
                        "the client had no frame in time"
                    )
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("the client ended without a result")
                }
            }
        };
        Self {
            event_loop,
            state,
            globals,
            quit,
            thread,
            identity,
            executable_file,
            executable_stats,
        }
    }

    /// Runs the compositor until `done` holds of its state.
    fn until(&mut self, what: &str, done: impl Fn(&State) -> bool) {
        let started = Instant::now();
        while !done(&self.state) {
            assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
            self.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut self.state)
                .expect("dispatch");
        }
    }

    fn stop(self) {
        drop(self.quit);
        self.thread.join().expect("client thread");
    }
}

/// Starts the compositor on `node`, or on Pixman, with two outputs and one client, and checks
/// the globals, the window and the frames of both outputs.
fn fixture(node: Option<&Path>) {
    let mut run = Running::start(node);
    let (globals, state) = (&run.globals, &mut run.state);

    for global in GLOBALS {
        assert!(globals.iter().any(|g| g == global), "{global} missing");
    }
    let count = |name: &str| globals.iter().filter(|g| *g == name).count();
    assert_eq!(count("wl_output"), OUTPUTS.len());
    // linux-dmabuf and explicit sync come with a render node, and syncobj only when it can wait.
    assert_eq!(count("zwp_linux_dmabuf_v1"), usize::from(node.is_some()));
    assert_eq!(
        count("wp_linux_drm_syncobj_manager_v1"),
        usize::from(state.syncobj_state.is_some())
    );
    eprintln!(
        "explicit sync on this backend: {}",
        state.syncobj_state.is_some()
    );

    assert_eq!(state.space.elements().count(), 1, "one window");
    let window = state.space.elements().next().expect("the window").clone();
    let area = state.space.element_geometry(&window).expect("mapped");
    assert_eq!((area.size.w, area.size.h), (WIDTH, HEIGHT));

    // Output 0 at scale 1: the window centred, red above blue, the background around it.
    let first = state.capture(0).expect("frame of output 0");
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    assert_eq!(first.len(), (w * h * 4) as usize);
    let (cx, cy) = (w / 2, h / 2);
    assert_eq!(pixel(&first, w, cx, cy - HEIGHT / 4), RED, "top half");
    assert_eq!(pixel(&first, w, cx, cy + HEIGHT / 4), BLUE, "bottom half");
    for (x, y, place) in [(0, 0, "corner"), (cx, cy - HEIGHT, "above the window")] {
        let px = pixel(&first, w, x, y);
        assert!(background(&px), "{place}: {px:?}");
    }

    // Output 1, at scale 1.5 to the right of output 0, shows no window.
    let second = state.capture(1).expect("frame of output 1");
    let (w, h) = (OUTPUTS[1].width, OUTPUTS[1].height);
    assert_eq!(second.len(), (w * h * 4) as usize);
    assert!(
        second.as_chunks::<4>().0.iter().all(background),
        "output 1 is background only"
    );

    run.stop();
}

/// A commit held on a fence that has not signalled: a one-plane dmabuf on a pipe with nothing
/// to read, which polls as a fence still pending until a byte is written.
fn hold(state: &mut State, surface: &WlSurface) -> std::io::PipeWriter {
    let (reader, writer) = std::io::pipe().expect("pipe");
    let mut builder = Dmabuf::builder(
        (1, 1),
        Fourcc::Argb8888,
        Modifier::Linear,
        DmabufFlags::empty(),
    );
    assert!(builder.add_plane(OwnedFd::from(reader), 0, 4));
    let dmabuf = builder.build().expect("dmabuf");
    let (blocker, source) = dmabuf
        .generate_blocker(Interest::READ)
        .expect("an empty pipe is a pending fence");
    state.hold_commit(surface, blocker, source);
    writer
}

/// A client holds commits on fences up to the bound; a fence that signals frees its place; the
/// commit past the bound is not held but disconnects the client, and its held commits go with it.
#[test]
fn a_client_holding_too_many_commits_is_disconnected() {
    let mut run = Running::start(None);
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let surface = window.toplevel().expect("a toplevel").wl_surface().clone();

    let mut fences: Vec<_> = (0..MAX_HELD_COMMITS)
        .map(|_| hold(&mut run.state, &surface))
        .collect();
    assert_eq!(run.state.held.len(), MAX_HELD_COMMITS);

    fences[0].write_all(&[0]).expect("signal a fence");
    run.until("a signalled fence frees its commit", |state| {
        state.held.len() == MAX_HELD_COMMITS - 1
    });
    fences.push(hold(&mut run.state, &surface));
    assert_eq!(run.state.held.len(), MAX_HELD_COMMITS);
    assert!(surface.is_alive(), "within the bound the client stays");

    fences.push(hold(&mut run.state, &surface));
    assert_eq!(run.state.held.len(), MAX_HELD_COMMITS, "past the bound");
    // The client is killed, not gone: the compositor drops it the next time it reads from its
    // clients, which a request from the client, still connected, makes it do.
    run.quit.send(()).expect("the client is listening");
    run.until("the client past the bound is disconnected", |state| {
        !surface.is_alive() && state.held.is_empty()
    });
    run.stop();
}

#[test]
fn headless_on_pixman() {
    fixture(None);
}

/// The same on a GPU: `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -- --ignored`.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn headless_on_a_render_node() {
    let node = PathBuf::from(
        std::env::var_os("ATHANOR_RENDER_NODE").expect("ATHANOR_RENDER_NODE names a render node"),
    );
    fixture(Some(&node));
}

/// The background is Calmo's dark `bg1` as the generator derives it from tokens.toml; the
/// generator's own check keeps the sheet in step with the tokens.
#[test]
fn the_background_is_calmo_bg1() {
    let sheet = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../system/athanor-style/calmo/generated/css/calmo-dark.css");
    let sheet = std::fs::read_to_string(sheet).expect("calmo-dark.css");
    let hex = sheet
        .lines()
        .find_map(|line| {
            line.strip_prefix("@define-color ath_bg1 #")?
                .strip_suffix(';')
        })
        .expect("ath_bg1 in calmo-dark.css");
    let bytes = BACKGROUND.map(|c| (c * 255.0).round() as u8);
    assert_eq!(
        format!("{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2]),
        hex
    );
}
/// What the test asks its second client, the shell's stand-in, to do next.
enum Step {
    /// A top-layer surface along the output's top edge, 50 high, with an exclusive zone of 50
    /// and exclusive keyboard interactivity.
    Exclusive,
    /// A window, placed in what the zone leaves.
    Window,
    /// A top-layer surface along the output's bottom edge, 40 high, that takes the keyboard on
    /// demand.
    OnDemand,
    /// The exclusive surface commits a null buffer.
    UnmapExclusive,
    /// The exclusive surface opens a popup that grabs the seat with this serial.
    Popup(u32),
    /// A top-layer bar along the output's top edge, 30 high, with no exclusive zone, that never
    /// takes the keyboard.
    Bar,
    /// The bar opens a popup that grabs the seat with this serial.
    BarPopup(u32),
    /// The exclusive surface commits again, unchanged; the sender is told once the compositor
    /// has handled it.
    Recommit(mpsc::Sender<()>),
    /// The exclusive surface, unmapped, is mapped again as it was first; the sender is told
    /// once the compositor has handled it.
    RemapExclusive(mpsc::Sender<()>),
    /// The sender is told, once the compositor has handled every step before, the keys the
    /// shell's client was sent.
    Keys(mpsc::Sender<Vec<u32>>),
}

/// The shell's stand-in: its connection's queue and state, and the globals it binds.
struct Shell {
    queue: wayland_client::EventQueue<Client>,
    client: Client,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    wm_base: xdg_wm_base::XdgWmBase,
    seat: wl_seat::WlSeat,
    /// The buffers' files, open while their buffers are.
    files: Vec<std::fs::File>,
}

impl Shell {
    /// A buffer of `width` by `height` pixels, black.
    fn buffer(&mut self, width: i32, height: i32) -> Result<wl_buffer::WlBuffer, Error> {
        let qh = &self.queue.handle();
        let file = unlinked()?;
        file.set_len(u64::try_from(width * height * 4)?)?;
        let pool = self
            .shm
            .create_pool(file.as_fd(), width * height * 4, qh, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width * 4,
            wl_shm::Format::Xrgb8888,
            qh,
            (),
        );
        pool.destroy();
        self.files.push(file);
        Ok(buffer)
    }

    /// Maps a layer surface `height` high along the `anchor` edge of the first output.
    fn layer(
        &mut self,
        namespace: &str,
        anchor: zwlr_layer_surface_v1::Anchor,
        height: u32,
        zone: i32,
        keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> Result<
        (
            wl_surface::WlSurface,
            zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        ),
        Error,
    > {
        let qh = self.queue.handle();
        let surface = self.compositor.create_surface(&qh, ());
        let layer = self.layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            namespace.into(),
            &qh,
            (),
        );
        self.map_layer(&surface, &layer, anchor, height, zone, keyboard)?;
        Ok((surface, layer))
    }

    /// Maps the layer surface `layer` of `surface`, new or unmapped, which forgets its state:
    /// its state, a commit, the configure acknowledged and a black buffer of its size.
    fn map_layer(
        &mut self,
        surface: &wl_surface::WlSurface,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        anchor: zwlr_layer_surface_v1::Anchor,
        height: u32,
        zone: i32,
        keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> Result<(), Error> {
        layer.set_layer(zwlr_layer_shell_v1::Layer::Top);
        layer.set_anchor(anchor);
        layer.set_size(0, height);
        layer.set_exclusive_zone(zone);
        layer.set_keyboard_interactivity(keyboard);
        self.client.layer_configure = None;
        surface.commit();
        while self.client.layer_configure.is_none() {
            self.queue.blocking_dispatch(&mut self.client)?;
        }
        let (serial, width, height) = self.client.layer_configure.ok_or("no configure")?;
        layer.ack_configure(serial);
        let buffer = self.buffer(i32::try_from(width)?, i32::try_from(height)?)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        Ok(())
    }

    /// Maps a popup of the fixture's size on the layer surface `parent`, grabbing the seat with
    /// `serial`.
    fn popup(
        &mut self,
        parent: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        serial: u32,
    ) -> Result<(), Error> {
        let qh = self.queue.handle();
        let positioner = self.wm_base.create_positioner(&qh, ());
        positioner.set_size(WIDTH, HEIGHT);
        positioner.set_anchor_rect(0, 0, 1, 1);
        let surface = self.compositor.create_surface(&qh, ());
        let xdg = self.wm_base.get_xdg_surface(&surface, &qh, ());
        let popup = xdg.get_popup(None, &positioner, &qh, ());
        parent.get_popup(&popup);
        popup.grab(&self.seat, serial);
        self.client.configure = None;
        surface.commit();
        while self.client.configure.is_none() {
            self.queue.blocking_dispatch(&mut self.client)?;
        }
        xdg.ack_configure(self.client.configure.ok_or("no configure")?);
        let buffer = self.buffer(WIDTH, HEIGHT)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        Ok(())
    }

    /// Maps a window of the fixture's size.
    fn window(&mut self) -> Result<(), Error> {
        let qh = self.queue.handle();
        let surface = self.compositor.create_surface(&qh, ());
        let xdg = self.wm_base.get_xdg_surface(&surface, &qh, ());
        xdg.get_toplevel(&qh, ());
        self.client.configure = None;
        surface.commit();
        while self.client.configure.is_none() {
            self.queue.blocking_dispatch(&mut self.client)?;
        }
        xdg.ack_configure(self.client.configure.ok_or("no configure")?);
        let buffer = self.buffer(WIDTH, HEIGHT)?;
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        Ok(())
    }
}

/// The shell's stand-in: does each step it is sent, until the sender is dropped.
fn shell_client(stream: UnixStream, steps: &mpsc::Receiver<Step>) -> Result<(), Error> {
    use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity};
    let connection = Connection::from_socket(stream)?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let mut client = Client::default();
    queue.roundtrip(&mut client)?;
    let mut shell = Shell {
        compositor: client.bind(&registry, &qh, 4)?,
        shm: client.bind(&registry, &qh, 1)?,
        layer_shell: client.bind(&registry, &qh, 4)?,
        wm_base: client.bind(&registry, &qh, 1)?,
        seat: client.bind(&registry, &qh, 1)?,
        files: Vec::new(),
        queue,
        client,
    };
    let _keyboard = shell.seat.get_keyboard(&qh, ());
    let mut exclusive = None;
    let mut bar = None;
    while let Ok(step) = steps.recv() {
        match step {
            Step::Exclusive => {
                exclusive = Some(shell.layer(
                    "exclusive",
                    Anchor::Top | Anchor::Left | Anchor::Right,
                    50,
                    50,
                    KeyboardInteractivity::Exclusive,
                )?);
            }
            Step::Window => shell.window()?,
            Step::OnDemand => {
                shell.layer(
                    "on-demand",
                    Anchor::Bottom | Anchor::Left | Anchor::Right,
                    40,
                    0,
                    KeyboardInteractivity::OnDemand,
                )?;
            }
            Step::UnmapExclusive => {
                let (surface, _) = exclusive.as_ref().ok_or("no exclusive surface")?;
                surface.attach(None, 0, 0);
                surface.commit();
            }
            Step::Popup(serial) => {
                let (_, layer) = exclusive.clone().ok_or("no exclusive surface")?;
                shell.popup(&layer, serial)?;
            }
            Step::Bar => {
                bar = Some(shell.layer(
                    "bar",
                    Anchor::Top | Anchor::Left | Anchor::Right,
                    30,
                    0,
                    KeyboardInteractivity::None,
                )?);
            }
            Step::BarPopup(serial) => {
                let (_, layer) = bar.clone().ok_or("no bar")?;
                shell.popup(&layer, serial)?;
            }
            Step::Recommit(done) => {
                let (surface, _) = exclusive.as_ref().ok_or("no exclusive surface")?;
                surface.commit();
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(()).map_err(|_| "the test is gone")?;
            }
            Step::RemapExclusive(done) => {
                let (surface, layer) = exclusive.clone().ok_or("no exclusive surface")?;
                shell.map_layer(
                    &surface,
                    &layer,
                    Anchor::Top | Anchor::Left | Anchor::Right,
                    50,
                    50,
                    KeyboardInteractivity::Exclusive,
                )?;
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(()).map_err(|_| "the test is gone")?;
            }
            Step::Keys(done) => {
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(shell.client.keys.clone())
                    .map_err(|_| "the test is gone")?;
            }
        }
        connection.flush()?;
    }
    Ok(())
}

/// The keyboard follows the layer surfaces' interactivity (register row 1 of CO3): an
/// exclusive top-layer surface takes it while mapped, and a window mapped meanwhile neither
/// takes it nor overlaps the surface's exclusive zone; a click on an on-demand surface does
/// not take it from the exclusive one; unmapping the exclusive surface hands it back to the
/// top window; a click then gives it to the on-demand surface.
#[test]
fn layer_surfaces_take_and_give_back_the_keyboard() {
    let mut run = Running::start(None);
    let first = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    run.state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client");
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    let layer = |state: &State, namespace: &str| {
        let output = state.space.outputs().next()?;
        let map = layer_map_for_output(output);
        let layer = map.layers().find(|layer| layer.namespace() == namespace)?;
        Some(layer.wl_surface().clone())
    };

    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        let exclusive = layer(state, "exclusive");
        exclusive.is_some() && focus(state) == exclusive
    });
    let exclusive = layer(&run.state, "exclusive");

    // Centred in what the zone leaves: 50 lower by half the zone, at y 375 rather than 350.
    steps.send(Step::Window).expect("the shell's client");
    let (x, y) = ((OUTPUTS[0].width - WIDTH) / 2, 50 + (750 - HEIGHT) / 2);
    run.until("the window is placed below the zone", |state| {
        state
            .space
            .elements()
            .any(|w| *w != first && state.space.element_location(w) == Some((x, y).into()))
    });
    assert_eq!(focus(&run.state), exclusive, "the window does not take it");
    let second = run
        .state
        .space
        .elements()
        .find(|w| **w != first)
        .expect("the second window")
        .clone();
    let second = second.toplevel().expect("a toplevel").wl_surface().clone();

    let bottom = Point::from((
        f64::from(OUTPUTS[0].width) / 2.0,
        f64::from(OUTPUTS[0].height) - 20.0,
    ));
    steps.send(Step::OnDemand).expect("the shell's client");
    run.until("the on-demand surface is under the pointer", |state| {
        matches!(state.under(bottom), Some((Under::Layer(l), _)) if l.namespace() == "on-demand")
    });
    run.state.click_focus(bottom);
    assert_eq!(
        focus(&run.state),
        exclusive,
        "a click does not take it from the exclusive surface"
    );

    steps
        .send(Step::UnmapExclusive)
        .expect("the shell's client");
    run.until("unmapping hands the keyboard back", |state| {
        focus(state).as_ref() == Some(&second)
    });

    run.state.click_focus(bottom);
    assert_eq!(
        focus(&run.state),
        layer(&run.state, "on-demand"),
        "a click on demand takes it"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// An exclusive surface's own popup keeps the keyboard grab it was given: the surface
/// committing again, as a bar does each frame, does not take the keyboard back from the popup
/// and end the grab.
#[test]
fn an_exclusive_surface_keeps_the_keyboard_in_its_own_popup() {
    let mut run = Running::start(None);
    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", |state| {
        focus(state)
            .is_some_and(|surface| surface.client().map(|c| c.id()) == Some(shell_id.clone()))
    });
    let exclusive = focus(&run.state).expect("the exclusive surface");

    // A key pressed while the exclusive surface holds the keyboard, as the bar's popover opens.
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, shell_id.clone()));
    steps
        .send(Step::Popup(serial.into()))
        .expect("the shell's client");
    let in_popup = |state: &State| {
        focus(state).is_some_and(|surface| {
            surface != exclusive && state.popups.find_popup(&surface).is_some()
        })
    };
    run.until("the popup's grab takes the keyboard", in_popup);
    let popup = focus(&run.state);

    let (done, done_rx) = mpsc::channel();
    steps
        .send(Step::Recommit(done))
        .expect("the shell's client");
    run.until("the recommit is handled", |_| done_rx.try_recv().is_ok());
    assert_eq!(focus(&run.state), popup, "the popup keeps the keyboard");
    assert!(
        run.state
            .seat
            .get_keyboard()
            .is_some_and(|k| k.is_grabbed()),
        "the popup's grab goes on"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// A click on a bar, whose interactivity is `none`, opens a popup that grabs the pointer but
/// not the keyboard: the grab's end would hand the keyboard to the bar, which refuses it, so
/// the window the user types in keeps it.
#[test]
fn a_bar_popup_leaves_the_keyboard_to_the_window() {
    use smithay::input::pointer::MotionEvent;
    let mut run = Running::start(None);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).is_some()
    });
    let window = focus(&run.state);

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });

    steps.send(Step::Bar).expect("the shell's client");
    let top = Point::from((f64::from(OUTPUTS[0].width) / 2.0, 15.0));
    run.until(
        "the bar is under the pointer",
        |state| matches!(state.under(top), Some((Under::Layer(l), _)) if l.namespace() == "bar"),
    );
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    let under = run.state.surface_under(top);
    let motion = MotionEvent {
        location: top,
        serial: SERIAL_COUNTER.next_serial(),
        time: smithay::backend::input::InputTime::now(),
    };
    pointer.motion(&mut run.state, under, &motion);
    pointer.frame(&mut run.state);

    // A button pressed on the bar, while the window holds the keyboard.
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.button = Some((serial, shell_id));
    steps
        .send(Step::BarPopup(serial.into()))
        .expect("the shell's client");
    run.until("the popup's grab takes the pointer", |_| {
        pointer.is_grabbed()
    });
    assert_eq!(focus(&run.state), window, "the window keeps the keyboard");
    assert!(
        run.state
            .seat
            .get_keyboard()
            .is_some_and(|k| !k.is_grabbed()),
        "the grab leaves the keyboard alone"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// Presses the keys of evdev codes `codes` in order and releases them in reverse, as the
/// backend hands them to the seat.
fn chord(state: &mut State, codes: &[u32]) {
    use smithay::backend::input::{InputTime, KeyState};
    // An xkb keycode is the evdev code plus 8.
    for code in codes {
        state.key_input((code + 8).into(), KeyState::Pressed, InputTime::now());
    }
    for code in codes.iter().rev() {
        state.key_input((code + 8).into(), KeyState::Released, InputTime::now());
    }
}

/// Left Super and Escape, as evdev numbers them.
const ESCAPE_CHORD: [u32; 2] = [125, 1];

/// The keys the shell's client was sent, once the compositor has handled every step before.
fn shell_keys(run: &mut Running, steps: &mpsc::Sender<Step>) -> Vec<u32> {
    let (sent, keys) = mpsc::channel();
    steps.send(Step::Keys(sent)).expect("the shell's client");
    let got = std::cell::RefCell::new(None);
    run.until("the shell's client tells its keys", |_| {
        if let Ok(keys) = keys.try_recv() {
            got.replace(Some(keys));
        }
        got.borrow().is_some()
    });
    got.into_inner().expect("the keys")
}

/// The key the compositor keeps for itself (C1 task 8): Super+Escape takes the keyboard back at
/// the press from a granted client's exclusive surface, and ends its popup's grab, with no lock
/// client to take the lock, and asks for the lock; the client never sees the Escape. The escape
/// holds for the identity the grant went to: its surface committing again, as a bar does each
/// frame, mapped again, a new one, or one of a second connection does not take the keyboard
/// back. A click on a layer surface of the client that takes the keyboard gives it back; one on
/// its window, or on its bar, which takes none, does not. With no exclusive surface the key asks
/// for the lock all the same and is not the window's; while the session is locked it is the lock
/// surface's, and asks for nothing. Super+Shift+Escape is the window's. A lock alone escapes
/// nobody.
#[test]
fn task8_super_escape_takes_the_keyboard_back_and_asks_for_the_lock() {
    let mut run = Running::start(None);
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).is_some()
    });
    let window = focus(&run.state);
    let window_client = window.as_ref().and_then(|s| s.client()).map(|c| c.id());

    let (server_end, client_end) = UnixStream::pair().expect("socket pair");
    let client_state = run.state.client_state(&server_end, None);
    let shell_id = run
        .state
        .display_handle
        .insert_client(server_end, client_state)
        .expect("client")
        .id();
    let (steps, steps_rx) = mpsc::channel();
    let shell = std::thread::spawn(move || {
        shell_client(client_end, &steps_rx).map_err(|error| error.to_string())
    });
    let shell_has_it = {
        let id = shell_id.clone();
        move |state: &State| {
            focus(state).is_some_and(|surface| surface.client().map(|c| c.id()) == Some(id.clone()))
        }
    };

    // A popup of the exclusive surface, opened with the keyboard's grab: the key ends the grab.
    steps.send(Step::Exclusive).expect("the shell's client");
    run.until("the exclusive surface takes the keyboard", &shell_has_it);
    let serial = SERIAL_COUNTER.next_serial();
    run.state.grab_serials.key_press = Some((serial, shell_id.clone()));
    steps
        .send(Step::Popup(serial.into()))
        .expect("the shell's client");
    run.until("the popup's grab takes the keyboard", |state| {
        state.seat.get_keyboard().is_some_and(|k| k.is_grabbed())
    });
    let keyboard = run.state.seat.get_keyboard().expect("a keyboard");
    chord(&mut run.state, &ESCAPE_CHORD);
    assert_eq!(run.state.lock_requests, 1, "Super+Escape asks for the lock");
    // No lock client runs, as in a session with none, or logind failed: the keyboard is back at
    // the press, and the popup's grab is over.
    assert_eq!(
        focus(&run.state),
        window,
        "the press takes the keyboard back"
    );
    assert!(!keyboard.is_grabbed(), "the press ends the popup's grab");
    assert_eq!(
        run.state.escaped.len(),
        1,
        "the identity whose surface held the keyboard is escaped"
    );
    // A second connection of the same identity maps an exclusive surface: it does not take the
    // keyboard either.
    {
        let mut second = Local::connect(&mut run, None);
        let compositor: wl_compositor::WlCompositor = second.bind(4);
        let layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = second.bind(4);
        let qh = second.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let layer = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            "second".into(),
            &qh,
            (),
        );
        layer.set_size(100, 100);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
        assert!(second.answer(&mut run, &surface), "the surface is mapped");
        assert!(
            run.state.exclusive_layer().is_none(),
            "a second connection of an escaped identity holds no exclusive surface"
        );
        assert_eq!(focus(&run.state), window, "and does not take the keyboard");
    }
    run.until("the second connection is gone", |state| {
        state.escaped.len() == 1
    });
    // A lock client takes the lock that follows: it changes nothing for the escape.
    let mut local = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let lock = locks.lock(&local.queue.handle(), ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    lock.unlock_and_destroy();
    local.pump(
        &mut run,
        "the unlock gives the window the keyboard",
        |_, state| focus(state) == window,
    );
    assert!(
        run.state.kept_keys.is_empty(),
        "each key the compositor kept is released"
    );
    assert_eq!(
        shell_keys(&mut run, &steps),
        [ESCAPE_CHORD[0]],
        "the client saw Super go down, never the Escape"
    );

    let (done, done_rx) = mpsc::channel();
    steps
        .send(Step::Recommit(done))
        .expect("the shell's client");
    run.until("the recommit is handled", |_| done_rx.try_recv().is_ok());
    assert_eq!(focus(&run.state), window, "a commit does not take it back");
    steps
        .send(Step::UnmapExclusive)
        .expect("the shell's client");
    let (done, done_rx) = mpsc::channel();
    steps
        .send(Step::RemapExclusive(done))
        .expect("the shell's client");
    run.until("the remap is handled", |_| done_rx.try_recv().is_ok());
    assert_eq!(
        focus(&run.state),
        window,
        "mapped again, it does not take it back"
    );
    steps.send(Step::Exclusive).expect("the shell's client");
    shell_keys(&mut run, &steps);
    assert_eq!(focus(&run.state), window, "a new surface does not either");

    // No exclusive surface holds the keyboard: Super+Escape asks for the lock all the same, and
    // is not the window's; Super+Shift+Escape is.
    use smithay::backend::input::{InputTime, KeyState};
    use smithay::input::keyboard::Keycode;
    let [logo, escape] = ESCAPE_CHORD.map(|code| Keycode::from(code + 8));
    let now = InputTime::now;
    let requests = run.state.lock_requests;
    run.state.key_input(logo, KeyState::Pressed, now());
    let logo_sent = run.state.grab_serials.key_press.clone();
    run.state.key_input(escape, KeyState::Pressed, now());
    let escape_sent = run.state.grab_serials.key_press.clone();
    run.state.key_input(escape, KeyState::Released, now());
    run.state.key_input(logo, KeyState::Released, now());
    assert_eq!(escape_sent, logo_sent, "the Escape is not sent on");
    assert_eq!(
        run.state.lock_requests,
        requests + 1,
        "it asks for the lock"
    );
    let shift = Keycode::from(42u32 + 8);
    run.state.key_input(logo, KeyState::Pressed, now());
    run.state.key_input(shift, KeyState::Pressed, now());
    run.state.key_input(escape, KeyState::Pressed, now());
    let escape_sent = run.state.grab_serials.key_press.clone();
    for key in [escape, shift, logo] {
        run.state.key_input(key, KeyState::Released, now());
    }
    assert_eq!(
        escape_sent.map(|(_, to)| to),
        window_client,
        "Super+Shift+Escape is the window's"
    );
    assert_eq!(
        run.state.lock_requests,
        requests + 1,
        "and asks for nothing"
    );

    // A click on a window of the client, or on its surface that takes no keyboard, does not
    // end the escape.
    steps.send(Step::Window).expect("the shell's client");
    steps.send(Step::Bar).expect("the shell's client");
    shell_keys(&mut run, &steps);
    let shell_window = run
        .state
        .space
        .elements()
        .find(|window| {
            window
                .toplevel()
                .and_then(|toplevel| toplevel.wl_surface().client())
                .map(|client| client.id())
                == Some(shell_id.clone())
        })
        .cloned()
        .expect("the shell's window");
    let window_area = run
        .state
        .space
        .element_geometry(&shell_window)
        .expect("mapped");
    let output = run.state.space.outputs().next().expect("an output").clone();
    let bar_area = {
        let map = layer_map_for_output(&output);
        let bar = map
            .layers()
            .find(|layer| layer.namespace() == "bar")
            .expect("the bar");
        let mut area = map.layer_geometry(bar).expect("placed");
        area.loc += run
            .state
            .space
            .output_geometry(&output)
            .expect("placed")
            .loc;
        area
    };
    for (area, what) in [(window_area, "its window"), (bar_area, "its bar")] {
        let centre = area.to_f64().loc
            + Point::from((f64::from(area.size.w) / 2.0, f64::from(area.size.h) / 2.0));
        run.state.click_focus(centre);
        assert_eq!(
            run.state.escaped.len(),
            1,
            "a click on {what} keeps the escape"
        );
    }

    // A click on the client's surface that takes the keyboard gives it back.
    run.state
        .click_focus(Point::from((f64::from(OUTPUTS[0].width) / 2.0, 25.0)));
    assert!(run.state.escaped.is_empty(), "a click ends the escape");
    assert!(
        focus(&run.state).is_some() && focus(&run.state) == run.state.exclusive_layer(),
        "a click gives it back"
    );

    // Locked, the key is the lock surface's and asks for nothing. The exclusive surface held the
    // keyboard when the lock was taken and nobody pressed the key: it has it again at the
    // unlock.
    let mut local = Local::connect(&mut run, None);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let lock = locks.lock(&local.queue.handle(), ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    let requests = run.state.lock_requests;
    chord(&mut run.state, &ESCAPE_CHORD);
    assert_eq!(
        run.state.lock_requests, requests,
        "the key asks for nothing while locked"
    );
    assert!(run.state.escaped.is_empty(), "a lock alone escapes nobody");
    lock.unlock_and_destroy();
    local.pump(
        &mut run,
        "the unlock gives the exclusive surface the keyboard",
        |_, state| shell_has_it(state),
    );
    assert!(
        !shell_keys(&mut run, &steps).contains(&ESCAPE_CHORD[1]),
        "the client never saw the Escape"
    );

    drop(steps);
    run.until("the shell's client is done", |_| shell.is_finished());
    shell
        .join()
        .expect("the shell's client thread")
        .expect("the shell's client");
    run.stop();
}

/// A virtual-terminal switch kept a key press, and the release comes on the other terminal: the
/// session pausing forgets the record, so the next release of that key reaches the client.
#[test]
fn task8_a_pause_forgets_the_keys_the_compositor_kept() {
    use smithay::backend::input::{InputTime, KeyState};
    use smithay::input::keyboard::Keycode;
    let mut run = Running::start(None);
    let [ctrl, alt, f1] = [29u32, 56, 59].map(|code| Keycode::from(code + 8));
    let now = InputTime::now;
    for key in [ctrl, alt, f1] {
        run.state.key_input(key, KeyState::Pressed, now());
    }
    assert_eq!(run.state.kept_keys, [f1], "Ctrl+Alt+F1 is kept");
    run.state.pause();
    assert!(run.state.kept_keys.is_empty(), "the pause forgets it");
    run.state.grab_serials.key_release = None;
    run.state.key_input(f1, KeyState::Released, now());
    assert!(
        run.state.grab_serials.key_release.is_some(),
        "a later release of F1 reaches the client"
    );
    run.stop();
}

/// A failure the backend cannot go on after ends the loop with the reason, which `main` turns
/// into the exit status.
#[test]
fn task8_a_fatal_failure_is_kept_for_main() {
    let mut run = Running::start(None);
    run.state.fail("the GPU was removed".into());
    run.state.fail("a second reason".into());
    assert_eq!(run.state.fatal.as_deref(), Some("the GPU was removed"));
    run.stop();
}

/// An output that goes takes no window with it (C1 task 8): a window only it showed is centred
/// on the first output, where it was placed at first.
#[test]
fn task8_a_window_on_an_output_that_goes_moves_onto_the_first() {
    let mut run = Running::start(None);
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let placed = run.state.space.element_location(&window);
    let second = run
        .state
        .space
        .outputs()
        .nth(1)
        .expect("the second output")
        .clone();
    let area = run
        .state
        .space
        .output_geometry(&second)
        .expect("the second output's area");
    run.state
        .space
        .relocate_element(&window, area.loc + Point::from((10, 10)));
    run.state.remove_output(&second);
    assert_eq!(
        run.state.space.element_location(&window),
        placed,
        "centred on the first output"
    );
    run.stop();
}

/// A client in the test's own thread, for the protocols of C1 task 4: the test runs the
/// compositor's loop and the client's queue in turn (`pump`).
struct Local {
    connection: Connection,
    queue: wayland_client::EventQueue<Client>,
    registry: wl_registry::WlRegistry,
    client: Client,
    /// Its id in the compositor, when the fixture inserted it.
    id: Option<ClientId>,
    /// The buffers' files, open while their buffers are.
    files: Vec<std::fs::File>,
}

impl Local {
    /// Connects to the compositor, inside `context` if any, and lists the globals.
    fn connect(run: &mut Running, context: Option<SecurityContext>) -> Self {
        let (server_end, client_end) = UnixStream::pair().expect("socket pair");
        let data = run.state.client_state(&server_end, context);
        let id = run
            .state
            .display_handle
            .insert_client(server_end, data)
            .expect("client")
            .id();
        Self::over(run, client_end, Some(id))
    }

    /// A client on `stream`, connected already, and its id in the compositor if the fixture
    /// inserted it; lists the globals.
    fn over(run: &mut Running, stream: UnixStream, id: Option<ClientId>) -> Self {
        let connection = Connection::from_socket(stream).expect("connection");
        let queue = connection.new_event_queue();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let mut local = Self {
            connection,
            queue,
            registry,
            client: Client::default(),
            id,
            files: Vec::new(),
        };
        local.roundtrip(run);
        local
    }

    /// Runs the compositor and the client in turn until `done` holds.
    fn pump(&mut self, run: &mut Running, what: &str, done: impl Fn(&Client, &State) -> bool) {
        let started = Instant::now();
        loop {
            self.connection.flush().expect("flush");
            if let Some(guard) = self.queue.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("{what}: {error}"),
                }
            }
            self.queue
                .dispatch_pending(&mut self.client)
                .expect("client dispatch");
            if done(&self.client, &run.state) {
                return;
            }
            assert!(started.elapsed() < TIMEOUT, "{what}: not in time");
            run.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut run.state)
                .expect("dispatch");
        }
    }

    /// Until the compositor has handled every request sent so far.
    fn roundtrip(&mut self, run: &mut Running) {
        let synced = self.client.synced;
        self.connection.display().sync(&self.queue.handle(), Synced);
        self.pump(run, "a roundtrip", |client, _| client.synced > synced);
    }

    fn bind<I>(&self, version: u32) -> I
    where
        I: Proxy + 'static,
        Client: Dispatch<I, ()>,
    {
        self.client
            .bind(&self.registry, &self.queue.handle(), version)
            .expect("bind")
    }

    /// Whether the client sees the global `interface`.
    fn sees(&self, interface: &str) -> bool {
        self.client.globals.iter().any(|(_, i, _)| i == interface)
    }

    /// A buffer of `width` by `height` pixels in XRGB8888, each `bgrx`, and its file.
    fn buffer(
        &mut self,
        shm: &wl_shm::WlShm,
        width: i32,
        height: i32,
        bgrx: [u8; 4],
    ) -> (wl_buffer::WlBuffer, std::fs::File) {
        let mut file = unlinked().expect("buffer file");
        let pixels = usize::try_from(width * height).expect("size");
        file.write_all(&bgrx.repeat(pixels)).expect("pixels");
        let qh = self.queue.handle();
        let pool = shm.create_pool(file.as_fd(), width * height * 4, &qh, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width * 4,
            wl_shm::Format::Xrgb8888,
            &qh,
            (),
        );
        pool.destroy();
        let kept = file.try_clone().expect("buffer file");
        self.files.push(file);
        (buffer, kept)
    }
}

/// The globals of C1 task 4, each offered once.
const TASK4_GLOBALS: [&str; 7] = [
    "ext_foreign_toplevel_list_v1",
    "ext_workspace_manager_v1",
    "ext_image_copy_capture_manager_v1",
    "ext_foreign_toplevel_image_capture_source_manager_v1",
    "wp_security_context_manager_v1",
    "ext_session_lock_manager_v1",
    "ext_idle_notifier_v1",
];

/// The fixture's window and the id of its client.
fn fixture_window(run: &Running) -> (smithay::desktop::Window, ClientId) {
    let window = run
        .state
        .space
        .elements()
        .next()
        .expect("the window")
        .clone();
    let id = window
        .toplevel()
        .and_then(|t| t.wl_surface().client())
        .expect("the fixture's client")
        .id();
    (window, id)
}

/// The globals of task 4 are offered once each; a client inside a security context sees only
/// the idle notifier among them (spike Q4: the privileged ones are refused to sandboxes).
#[test]
fn task4_standard_globals_are_offered() {
    let mut run = Running::start(None);
    for global in TASK4_GLOBALS {
        let count = run.globals.iter().filter(|g| *g == global).count();
        assert_eq!(count, 1, "{global}");
    }
    let (_, creator) = fixture_window(&run);
    let context = SecurityContext {
        sandbox_engine: Some("org.flatpak".into()),
        app_id: Some("org.example.App".into()),
        instance_id: None,
        creator_client_id: creator,
    };
    let sandboxed = Local::connect(&mut run, Some(context));
    for global in TASK4_GLOBALS {
        let expected = global == "ext_idle_notifier_v1";
        assert_eq!(
            sandboxed.sees(global),
            expected,
            "{global} inside a context"
        );
    }
    assert!(sandboxed.sees("wl_compositor"), "the core stays");
    run.stop();
}

/// A window appears in the foreign toplevel list with its title and app id, follows a new
/// title, and is closed when the window is destroyed; the others stay.
#[test]
fn task4_a_window_is_listed_and_unlisted() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    local.pump(&mut run, "the fixture's window is listed", |client, _| {
        client.listed.iter().any(|l| l.done && l.title == "fixture")
    });

    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("Probe".into());
    toplevel.set_app_id("org.athanor.Probe".into());
    surface.commit();
    local.pump(&mut run, "the window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let named = |title: &'static str| {
        move |client: &Client, _: &State| {
            client
                .listed
                .iter()
                .any(|l| l.done && l.title == title && l.app_id == "org.athanor.Probe")
        }
    };
    local.pump(&mut run, "the window is listed", named("Probe"));

    toplevel.set_title("Probe 2".into());
    local.pump(&mut run, "the new title is listed", named("Probe 2"));

    toplevel.destroy();
    xdg.destroy();
    surface.destroy();
    local.pump(&mut run, "the window is unlisted", |client, _| {
        client
            .listed
            .iter()
            .any(|l| l.title == "Probe 2" && l.closed)
    });
    let fixture = local.client.listed.iter().find(|l| l.title == "fixture");
    assert!(
        fixture.is_some_and(|l| !l.closed),
        "the fixture's window stays"
    );
    run.stop();
}

/// A window is captured into the client's wl_shm buffer, red above blue as it draws itself;
/// locking the session stops the capture, and a new session is refused while locked (OS4).
#[test]
fn task4_a_window_is_captured_and_refused_while_locked() {
    use ext_image_copy_capture_manager_v1::Options;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let shm: wl_shm::WlShm = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let sources: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 =
        local.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    local.pump(&mut run, "the fixture's window is listed", |client, _| {
        client.listed.iter().any(|l| l.done && l.title == "fixture")
    });
    let qh = local.queue.handle();
    let handle = local
        .client
        .listed
        .iter()
        .find(|l| l.title == "fixture")
        .map(|l| l.handle.clone())
        .expect("the fixture's handle");
    let source = sources.create_source(&handle, &qh, ());
    let session = copy.create_session(&source, Options::empty(), &qh, ());
    local.pump(&mut run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });
    assert_eq!(
        local.client.capture_size,
        Some((WIDTH as u32, HEIGHT as u32))
    );

    let (buffer, file) = local.buffer(&shm, WIDTH, HEIGHT, [0x55; 4]);
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, WIDTH, HEIGHT);
    frame.capture();
    local.pump(&mut run, "the frame is captured", |client, _| {
        client.captured.is_some()
    });
    assert_eq!(local.client.captured, Some(Ok(())), "the frame is ready");
    let at = |x: i32, y: i32| {
        let mut px = [0; 4];
        let offset = u64::try_from((y * WIDTH + x) * 4).expect("offset");
        file.read_exact_at(&mut px, offset).expect("the buffer");
        px
    };
    // XRGB8888 in memory: B, G, R, X.
    assert_eq!(at(WIDTH / 2, HEIGHT / 4)[..3], [0, 0, 255], "top half red");
    assert_eq!(
        at(WIDTH / 2, HEIGHT * 3 / 4)[..3],
        [255, 0, 0],
        "bottom half blue"
    );
    frame.destroy();

    let _lock = locks.lock(&qh, ());
    let stopped = session.id();
    local.pump(&mut run, "locking stops the session", |client, _| {
        client.stopped.contains(&stopped)
    });
    let refused = copy.create_session(&source, Options::empty(), &qh, ());
    let refused = refused.id();
    local.pump(
        &mut run,
        "a session is refused while locked",
        |client, _| client.stopped.contains(&refused),
    );
    run.stop();
}

/// The lock surface of output 0, as the compositor holds it.
fn surface_on_server(state: &State) -> WlSurface {
    let output = state.space.outputs().next().expect("an output");
    state
        .lock
        .as_ref()
        .and_then(|lock| lock.surface_on(output))
        .expect("a lock surface")
        .clone()
}

/// A window that closes while it is captured stops the session: the next frame fails with
/// `stopped` and the session receives `stopped`, as the failure reason promises.
#[test]
fn task4_a_closed_window_stops_its_capture() {
    use ext_image_copy_capture_manager_v1::Options;
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let wm_base: xdg_wm_base::XdgWmBase = local.bind(1);
    let _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1 = local.bind(1);
    let sources: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 =
        local.bind(1);
    let copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("Probe".into());
    surface.commit();
    local.pump(&mut run, "the window is configured", |client, _| {
        client.configure.is_some()
    });
    xdg.ack_configure(local.client.configure.expect("a configure"));
    let (buffer, _) = local.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.pump(&mut run, "the window is listed", |client, _| {
        client.listed.iter().any(|l| l.done && l.title == "Probe")
    });
    let handle = local
        .client
        .listed
        .iter()
        .find(|l| l.title == "Probe")
        .map(|l| l.handle.clone())
        .expect("the window's handle");
    let source = sources.create_source(&handle, &qh, ());
    let session = copy.create_session(&source, Options::empty(), &qh, ());
    local.pump(&mut run, "the session has its constraints", |client, _| {
        client.capture_size.is_some()
    });

    toplevel.destroy();
    xdg.destroy();
    surface.destroy();
    local.roundtrip(&mut run);
    let (target, _) = local.buffer(&shm, WIDTH, HEIGHT, [0x55; 4]);
    let frame = session.create_frame(&qh, ());
    frame.attach_buffer(&target);
    frame.capture();
    let stopped = session.id();
    local.pump(&mut run, "the session is stopped", |client, _| {
        client.stopped.contains(&stopped)
    });
    // ext_image_copy_capture_frame_v1.failure_reason.stopped
    assert_eq!(
        local.client.captured,
        Some(Err(2)),
        "the frame fails stopped"
    );
    run.stop();
}

/// While locked only the lock surface is drawn, on its output, and only it has the keyboard and
/// the pointer, whatever is clicked or focused and though the pointer has not moved; unlocking
/// gives both back to the window.
#[test]
fn task4_the_lock_hides_and_takes_the_keyboard() {
    use smithay::input::pointer::MotionEvent;
    let mut run = Running::start(None);
    let (window, _) = fixture_window(&run);
    let window_surface = window.toplevel().expect("a toplevel").wl_surface().clone();
    let focus = |state: &State| state.seat.get_keyboard().and_then(|k| k.current_focus());
    run.until("the window has the keyboard", |state| {
        focus(state).as_ref() == Some(&window_surface)
    });
    let (w, h) = (OUTPUTS[0].width, OUTPUTS[0].height);
    let centre = Point::from((
        f64::from(w) / 2.0,
        f64::from(h) / 2.0 - f64::from(HEIGHT) / 4.0,
    ));
    // The pointer rests on the window, and does not move again.
    let pointer = run.state.seat.get_pointer().expect("a pointer");
    let under = run.state.surface_under(centre);
    let motion = MotionEvent {
        location: centre,
        serial: SERIAL_COUNTER.next_serial(),
        time: smithay::backend::input::InputTime::now(),
    };
    pointer.motion(&mut run.state, under, &motion);
    pointer.frame(&mut run.state);
    assert_eq!(pointer.current_focus().as_ref(), Some(&window_surface));

    let mut local = Local::connect(&mut run, None);
    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let output: wl_output::WlOutput = local.bind(4);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let qh = local.queue.handle();
    let lock = locks.lock(&qh, ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    assert_eq!(focus(&run.state), None, "no surface has the keyboard");

    let surface = compositor.create_surface(&qh, ());
    let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    local.pump(&mut run, "the lock surface is configured", |client, _| {
        client.lock_configure.is_some()
    });
    let (serial, width, height) = local.client.lock_configure.expect("a configure");
    assert_eq!(
        (width, height),
        (OUTPUTS[0].width as u32, OUTPUTS[0].height as u32)
    );
    lock_surface.ack_configure(serial);
    let (buffer, _) = local.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    let id = local.id.clone().expect("a client the fixture inserted");
    let locker_has_it = move |state: &State| {
        focus(state).and_then(|s| s.client()).map(|c| c.id()) == Some(id.clone())
    };
    local.pump(&mut run, "the lock surface has the keyboard", |_, state| {
        locker_has_it(state)
    });
    // Its buffer committed.
    local.roundtrip(&mut run);
    assert_eq!(
        pointer.current_focus().as_ref(),
        Some(&surface_on_server(&run.state)),
        "a click goes to the lock surface, not the window under it"
    );

    let first = run.state.capture(0).expect("frame of output 0");
    assert_eq!(
        pixel(&first, w, w / 2, h / 2 - HEIGHT / 4),
        [0, 0, 0, 255],
        "the window is hidden"
    );
    let second = run.state.capture(1).expect("frame of output 1");
    assert!(
        second.as_chunks::<4>().0.iter().all(background),
        "an output without a lock surface shows the background only"
    );
    assert!(
        run.state.under(centre).is_none(),
        "nothing under the pointer"
    );
    run.state.click_focus(centre);
    run.state.focus(&window);
    run.state.focus_top();
    assert!(
        locker_has_it(&run.state),
        "the keyboard stays with the lock surface"
    );

    lock.unlock_and_destroy();
    local.pump(&mut run, "unlocking gives the keyboard back", |_, state| {
        focus(state).as_ref() == Some(&window_surface)
    });
    assert_eq!(
        pointer.current_focus().as_ref(),
        Some(&window_surface),
        "and the pointer"
    );
    let first = run.state.capture(0).expect("frame of output 0");
    assert_eq!(
        pixel(&first, w, w / 2, h / 2 - HEIGHT / 4),
        RED,
        "the window is back"
    );
    run.stop();
}

/// The idle notification fires after its timeout, and input resumes it.
#[test]
fn task4_idle_fires_and_input_resets_it() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let seat: wl_seat::WlSeat = local.bind(1);
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = local.bind(1);
    let started = Instant::now();
    let _notification = notifier.get_idle_notification(100, &seat, &local.queue.handle(), ());
    local.pump(&mut run, "the user is idle", |client, _| {
        client.idle == ["idled"]
    });
    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "not before the timeout"
    );
    run.state.activity();
    local.pump(&mut run, "input resumes", |client, _| {
        client.idle == ["idled", "resumed"]
    });
    run.stop();
}

/// Each output is a workspace group with one workspace, active, that the client's own
/// `wl_output` of that output has entered.
#[test]
fn task4_each_output_has_an_active_workspace() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let qh = local.queue.handle();
    let outputs: Vec<wl_output::WlOutput> = local
        .client
        .globals
        .iter()
        .filter(|(_, interface, _)| interface == "wl_output")
        .map(|(name, _, version)| local.registry.bind(*name, (*version).min(4), &qh, ()))
        .collect();
    assert_eq!(outputs.len(), OUTPUTS.len());
    local.roundtrip(&mut run);
    let _manager: ext_workspace_manager_v1::ExtWorkspaceManagerV1 = local.bind(1);
    local.pump(&mut run, "the workspaces are sent", |client, _| {
        client.workspaces_done
    });

    let client = &local.client;
    assert_eq!(client.groups.len(), OUTPUTS.len(), "one group per output");
    let mut entered: Vec<ObjectId> = Vec::new();
    for (_, outputs_in, workspaces_in) in &client.groups {
        assert_eq!(outputs_in.len(), 1, "a group stands for one output");
        entered.extend(outputs_in.iter().cloned());
        assert_eq!(workspaces_in.len(), 1, "one workspace per group");
        let state = client
            .workspaces
            .iter()
            .find(|(id, _)| *id == workspaces_in[0])
            .and_then(|(_, state)| *state);
        assert_eq!(state, Some(ext_workspace_handle_v1::State::Active));
    }
    let bound: Vec<ObjectId> = outputs.iter().map(Proxy::id).collect();
    assert!(
        bound.iter().all(|id| entered.contains(id)),
        "each output has its group"
    );
    run.stop();
}

/// The privileged globals of OS4 that the compositor offers (C1 task 5).
const PRIVILEGED: [&str; 4] = [
    "ext_foreign_toplevel_list_v1",
    "ext_foreign_toplevel_image_capture_source_manager_v1",
    "ext_image_copy_capture_manager_v1",
    "ext_session_lock_manager_v1",
];

/// Another executable, and another unit: a shell started in an application scope.
const SH: &str = "/usr/bin/sh";
const OTHER_UNIT: &str =
    "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-sh-1.scope\n";

impl Local {
    /// Whether the client sees every privileged global (true) or none (false); a mix fails.
    fn privileged(&self) -> bool {
        let seen: Vec<bool> = PRIVILEGED.iter().map(|g| self.sees(g)).collect();
        assert!(
            seen.iter().all(|s| *s == seen[0]),
            "some privileged globals but not all: {seen:?}"
        );
        assert!(
            [
                "wl_compositor",
                "zwlr_layer_shell_v1",
                "ext_idle_notifier_v1"
            ]
            .iter()
            .all(|g| self.sees(g)),
            "the standard globals stay"
        );
        seen[0]
    }

    /// Lists the globals again, on a new registry.
    fn relist(&mut self, run: &mut Running) {
        self.client.globals.clear();
        self.registry = self
            .connection
            .display()
            .get_registry(&self.queue.handle(), ());
        self.roundtrip(run);
    }

    /// A layer surface on `layer`, committed: whether the compositor configures it (true) or
    /// closes it (false).
    fn layer_kept(
        &mut self,
        run: &mut Running,
        layer: zwlr_layer_shell_v1::Layer,
    ) -> (
        bool,
        zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        wl_surface::WlSurface,
    ) {
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = self.bind(4);
        let qh = self.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(&surface, None, layer, "task5".into(), &qh, ());
        layer.set_size(100, 100);
        (self.answer(run, &surface), layer, surface)
    }

    /// Commits `surface`, a layer surface: whether it is configured (true) or closed (false).
    fn answer(&mut self, run: &mut Running, surface: &wl_surface::WlSurface) -> bool {
        self.client.layer_configure = None;
        self.client.layer_closed = false;
        surface.commit();
        self.pump(run, "a configure or a close", |client, _| {
            client.layer_configure.is_some() || client.layer_closed
        });
        !self.client.layer_closed
    }
}

/// The bar's executable in the bar's unit is granted the privileged globals of the allow-list.
#[test]
fn task5_bar_unit_and_executable_get_privileged_globals() {
    let mut run = Running::start(None);
    let bar = Local::connect(&mut run, None);
    assert!(bar.privileged(), "the bar sees the privileged globals");
    run.stop();
}

/// Another executable in the bar's unit is granted nothing.
#[test]
fn task5_other_executable_in_the_bar_unit_gets_standard_globals() {
    let mut run = Running::start(None);
    *run.identity.lock().expect("identity") = (SH, BAR.1);
    let other = Local::connect(&mut run, None);
    assert!(!other.privileged(), "another executable in the bar's unit");
    run.stop();
}

/// The bar's executable in another unit is granted nothing.
#[test]
fn task5_other_unit_gets_standard_globals() {
    let mut run = Running::start(None);
    *run.identity.lock().expect("identity") = (BAR.0, OTHER_UNIT);
    let other = Local::connect(&mut run, None);
    assert!(!other.privileged(), "the bar's executable in another unit");
    run.stop();
}

/// A file at the bar's path that is not the bar's file, as a sandbox with its own `/usr` has
/// it, is granted nothing.
#[test]
fn task5_another_file_at_the_bar_s_path_gets_standard_globals() {
    let mut run = Running::start(None);
    *run.executable_file.lock().expect("file") = FileId { dev: 1, ino: 2 };
    let other = Local::connect(&mut run, None);
    assert!(!other.privileged(), "another file at the bar's path");
    run.stop();
}

/// Only a client that names an allow-listed executable and unit has its executable's file read:
/// that read can block on the client's own file system, a stalled FUSE mount, and the
/// compositor with it.
#[test]
fn task5_only_an_allow_listed_name_has_its_file_read() {
    let mut run = Running::start(None);
    let stats = Arc::clone(&run.executable_stats);
    let before = stats.load(Ordering::SeqCst);
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    Local::connect(&mut run, None);
    assert_eq!(
        stats.load(Ordering::SeqCst),
        before,
        "another executable, another unit"
    );
    *run.identity.lock().expect("identity") = (SH, BAR.1);
    Local::connect(&mut run, None);
    assert_eq!(
        stats.load(Ordering::SeqCst),
        before,
        "another executable, the bar's unit"
    );
    *run.identity.lock().expect("identity") = (BAR.0, OTHER_UNIT);
    Local::connect(&mut run, None);
    assert_eq!(
        stats.load(Ordering::SeqCst),
        before,
        "the bar's executable, another unit"
    );
    *run.identity.lock().expect("identity") = BAR;
    Local::connect(&mut run, None);
    assert_eq!(stats.load(Ordering::SeqCst), before + 1, "the bar");
    run.stop();
}

/// The bar itself, connecting through a security context, is granted nothing.
#[test]
fn task5_security_context_gets_standard_globals() {
    let mut run = Running::start(None);
    let (_, creator) = fixture_window(&run);
    let context = SecurityContext {
        sandbox_engine: Some("org.flatpak".into()),
        app_id: Some("org.example.App".into()),
        instance_id: None,
        creator_client_id: creator,
    };
    let sandboxed = Local::connect(&mut run, Some(context));
    for global in PRIVILEGED {
        assert!(!sandboxed.sees(global), "{global} inside a context");
    }
    run.stop();
}

/// The bar's own process, connecting through a security context's listener as a sandboxed
/// application does, is granted nothing: the path from `wp_security_context_manager_v1` to the
/// listener's clients.
#[test]
fn task5_a_security_context_listener_gets_standard_globals() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    let path = std::env::temp_dir().join(format!("athanor-context-{}.sock", std::process::id()));
    let listener = std::os::unix::net::UnixListener::bind(&path).expect("listen");
    // The compositor listens until the write end closes.
    let (close, _open) = std::io::pipe().expect("pipe");
    let manager: wp_security_context_manager_v1::WpSecurityContextManagerV1 = bar.bind(1);
    let qh = bar.queue.handle();
    let context = manager.create_listener(listener.as_fd(), close.as_fd(), &qh, ());
    context.set_sandbox_engine("org.flatpak".into());
    context.set_app_id("org.example.App".into());
    context.commit();
    bar.roundtrip(&mut run);

    let stream = UnixStream::connect(&path).expect("connect");
    std::fs::remove_file(&path).expect("remove");
    let sandboxed = Local::over(&mut run, stream, None);
    assert!(sandboxed.sees("wl_compositor"), "the core");
    for global in PRIVILEGED {
        assert!(!sandboxed.sees(global), "{global} through the listener");
    }
    run.stop();
}

/// The identity is the one read at connect: a client that connected as the bar keeps the
/// bar's globals when its process becomes another executable, and one that connected as
/// another gains nothing when it becomes the bar.
#[test]
fn task5_exec_after_connect_keeps_the_connect_identity() {
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    *run.identity.lock().expect("identity") = (SH, OTHER_UNIT);
    bar.relist(&mut run);
    assert!(bar.privileged(), "connected as the bar");

    let mut other = Local::connect(&mut run, None);
    *run.identity.lock().expect("identity") = BAR;
    other.relist(&mut run);
    assert!(!other.privileged(), "connected as another");
    run.stop();
}

/// The top and overlay layers are the granted client's: another one's surface there is closed,
/// whether it asked for the layer at creation or moves there with `set_layer`; the bottom
/// layer stays every unconfined client's.
#[test]
fn task5_top_layers_are_closed_to_a_client_not_granted_them() {
    use zwlr_layer_shell_v1::Layer;
    let mut run = Running::start(None);
    let mut bar = Local::connect(&mut run, None);
    assert!(
        bar.layer_kept(&mut run, Layer::Top).0,
        "the bar's top layer"
    );
    assert!(
        bar.layer_kept(&mut run, Layer::Overlay).0,
        "the bar's overlay"
    );

    *run.identity.lock().expect("identity") = (SH, BAR.1);
    let mut other = Local::connect(&mut run, None);
    assert!(
        !other.layer_kept(&mut run, Layer::Top).0,
        "another's top layer"
    );
    assert!(
        !other.layer_kept(&mut run, Layer::Overlay).0,
        "another's overlay"
    );
    let (kept, layer, surface) = other.layer_kept(&mut run, Layer::Bottom);
    assert!(kept, "another's bottom layer");
    layer.set_layer(Layer::Top);
    assert!(!other.answer(&mut run, &surface), "moved to the top layer");
    run.stop();
}
