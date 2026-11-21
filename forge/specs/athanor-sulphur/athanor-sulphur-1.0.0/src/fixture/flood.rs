//! A client that writes requests as fast as it can does not starve the others: the backend reads
//! each client's socket once per pass and returns to the event loop after one pass over the
//! clients that have something to read (vendor/wayland-backend/PATCHES.md, change 4).

use super::null_arguments::{string, Wire, REGISTRY};
use super::*;
use std::io::ErrorKind;
use std::sync::atomic::AtomicBool;

/// The objects of the raw client after its registry (2) and its first sync's callback (3).
const COMPOSITOR: u32 = 4;
const SURFACE: u32 = 5;
const SYNC: u32 = 6;

/// How long the flood lasts at most. The other clients must be served before it ends.
const FLOOD: Duration = Duration::from_secs(10);

/// How much the flooding client has written before the others start: more than a Unix socket's
/// buffer holds, so the compositor has been reading it.
const UNDER_WAY: usize = 1 << 20;

/// Binds the compositor and creates a surface on `wire`, and waits until both are done.
fn surface(run: &mut Running, wire: &mut Wire) {
    let name = wire.global("wl_compositor");
    let mut args = vec![name];
    args.extend(string("wl_compositor"));
    args.extend([4, COMPOSITOR]);
    wire.send(REGISTRY, 0, &args);
    // wl_compositor.create_surface, then wl_display.sync.
    wire.send(COMPOSITOR, 0, &[SURFACE]);
    wire.send(1, 0, &[SYNC]);
    wire.pump(run, "the surface is created", |wire| answered(wire, SYNC));
}

/// Whether the callback `id` is done.
fn answered(wire: &Wire, id: u32) -> bool {
    wire.events
        .iter()
        .any(|(object, opcode, _)| (*object, *opcode) == (id, 0))
}

/// The bytes of one read's worth of requests on `SURFACE`: 169 `wl_surface.damage` and one
/// `wl_surface.commit`, which applies the damage, so that a flood keeps no state growing.
fn chunk() -> Vec<u8> {
    let damage = [SURFACE, 24 << 16 | 2, 0, 0, 1, 1];
    let commit = [SURFACE, 8 << 16 | 6];
    std::iter::repeat_n(damage.as_slice(), 169)
        .chain([commit.as_slice()])
        .flatten()
        .flat_map(|word| word.to_ne_bytes())
        .collect()
}

/// A flood of `chunk`s on `stream` from a thread of its own, until `FLOOD` has passed or `stop`
/// is set; `written` counts the bytes, `over` says that it has ended.
struct Flood {
    stop: Arc<AtomicBool>,
    over: Arc<AtomicBool>,
    written: Arc<AtomicUsize>,
    thread: std::thread::JoinHandle<()>,
}

impl Flood {
    fn start(stream: UnixStream) -> Self {
        let (stop, over, written) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(0)),
        );
        let (thread_stop, thread_over, thread_written) =
            (stop.clone(), over.clone(), written.clone());
        let mut stream = stream;
        stream.set_nonblocking(false).expect("blocking");
        // A write that waits rechecks the clock and `stop`.
        stream
            .set_write_timeout(Some(Duration::from_millis(50)))
            .expect("write timeout");
        let thread = std::thread::spawn(move || {
            let bytes = chunk();
            let until = Instant::now() + FLOOD;
            let mut at = 0;
            while !thread_stop.load(Ordering::Relaxed) && Instant::now() < until {
                match stream.write(&bytes[at..]) {
                    Ok(count) => {
                        thread_written.fetch_add(count, Ordering::Relaxed);
                        at = (at + count) % bytes.len();
                    }
                    Err(error)
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(error) => panic!("the flood: {error}"),
                }
            }
            thread_over.store(true, Ordering::Relaxed);
        });
        Self {
            stop,
            over,
            written,
            thread,
        }
    }

    fn end(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.join().expect("the flood's thread");
    }
}

/// While one client floods the compositor with requests, another maps a window, has its frame,
/// locks the session and commits its lock surface, all before the flood ends.
#[test]
fn a_flooding_client_does_not_starve_the_others() {
    let mut run = Running::start(None);
    let mut wire = Wire::connect(&mut run);
    surface(&mut run, &mut wire);
    let flood = Flood::start(wire.stream.try_clone().expect("the raw client's socket"));
    let started = Instant::now();
    while flood.written.load(Ordering::Relaxed) < UNDER_WAY {
        assert!(started.elapsed() < TIMEOUT, "the flood is not under way");
        run.event_loop
            .dispatch(Some(Duration::from_millis(5)), &mut run.state)
            .expect("dispatch");
    }

    let mut local = Local::connect(&mut run, None);
    local.toplevel(&mut run);
    let (window, _) = local.window.clone().expect("a window");
    let qh = local.queue.handle();
    window.frame(&qh, ());
    local.client.frame_done = false;
    window.commit();
    local.pump(&mut run, "the window's frame callback", |client, _| {
        client.frame_done
    });

    let compositor: wl_compositor::WlCompositor = local.bind(4);
    let shm: wl_shm::WlShm = local.bind(1);
    let output: wl_output::WlOutput = local.bind(4);
    let locks: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = local.bind(1);
    let lock = locks.lock(&qh, ());
    local.pump(&mut run, "the session is locked", |client, _| client.locked);
    let surface = compositor.create_surface(&qh, ());
    let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    local.pump(&mut run, "the lock surface is configured", |client, _| {
        client.lock_configure.is_some()
    });
    let (serial, width, height) = local.client.lock_configure.expect("a configure");
    lock_surface.ack_configure(serial);
    let (buffer, _) = local.buffer(&shm, width as i32, height as i32, [0, 0, 0, 255]);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    local.roundtrip(&mut run);

    assert!(
        !flood.over.load(Ordering::Relaxed),
        "the other client was served only once the flood had ended"
    );
    flood.end();
    run.stop();
}

/// A burst of many reads' worth of requests, written at once and followed by a sync, is served to
/// its end although the client then writes nothing more: no request waits in the backend's
/// buffer for a read that the socket will not wake.
#[test]
fn a_burst_longer_than_one_read_is_served_to_its_end() {
    let mut run = Running::start(None);
    let mut wire = Wire::connect(&mut run);
    surface(&mut run, &mut wire);
    let bytes = chunk().repeat(12);
    wire.stream.write_all(&bytes).expect("the burst");
    wire.send(1, 0, &[SYNC + 1]);
    wire.pump(&mut run, "the sync after the burst is answered", |wire| {
        answered(wire, SYNC + 1)
    });
    run.stop();
}
