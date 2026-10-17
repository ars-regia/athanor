//! The in-process fixture of doc_own_stack.md OS6: the compositor on its headless backend and a
//! Wayland client in one process, joined by a socket pair. The client lists the globals, maps a
//! window from a wl_shm buffer and waits for its frame callback; the compositor's frames are then
//! read back and their pixels compared.

use crate::handlers::MAX_HELD_COMMITS;
use crate::headless::{self, VirtualOutput};
use crate::state::{State, Under, BACKGROUND};
use crate::Error;
use smithay::backend::allocator::dmabuf::{Dmabuf, DmabufFlags};
use smithay::backend::allocator::{Fourcc, Modifier};
use smithay::desktop::layer_map_for_output;
use smithay::reexports::calloop::{EventLoop, Interest};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, Resource};
use smithay::utils::{Point, SERIAL_COUNTER};
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
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
    frame_done: bool,
}

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
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            client.layer_configure = Some((serial, width, height));
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
        let client_state = state.client_state();
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
    /// The exclusive surface commits again, unchanged; the sender is told once the compositor
    /// has handled it.
    Recommit(mpsc::Sender<()>),
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
        Ok((surface, layer))
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
    let mut exclusive = None;
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
            Step::Recommit(done) => {
                let (surface, _) = exclusive.as_ref().ok_or("no exclusive surface")?;
                surface.commit();
                shell.queue.roundtrip(&mut shell.client)?;
                done.send(()).map_err(|_| "the test is gone")?;
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
    let client_state = run.state.client_state();
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
    let client_state = run.state.client_state();
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
