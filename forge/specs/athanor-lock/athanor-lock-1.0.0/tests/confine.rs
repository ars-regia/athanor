//! The lock confines itself only while it has one thread (LP15, plan decision M3). A binary
//! without libtest's harness, whose `main` is the process's only thread: with a second thread
//! alive the confinement is refused and nothing is applied; once that thread has gone it is
//! applied, writes leave only the runtime directory, TCP and the abstract Unix sockets of the
//! session are refused, path sockets stay open, and no core file can be written (D43). The
//! process stays dumpable, for Sulphur to identify it at connect; `main.rs` tests the change
//! after.

use std::fs;
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use athanor_lock::confine::confine;
use nix::sys::resource::{getrlimit, setrlimit, Resource};

fn threads() -> usize {
    fs::read_dir("/proc/self/task").expect("tasks").count()
}

fn main() {
    let base =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("confine-{}", std::process::id()));
    let (runtime, outside) = (base.join("runtime"), base.join("outside"));
    // The rig keeps its target and a container numbers its processes from 1 again, so a run
    // can meet the directory, and the bound socket, of an earlier run with the same pid.
    match fs::remove_dir_all(&base) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("remove {}: {e}", base.display()),
    }
    fs::create_dir_all(&runtime).expect("mkdir runtime");
    fs::create_dir_all(&outside).expect("mkdir outside");

    // A core limit the confinement must lower: the soft limit raised to the hard one.
    let (_, hard) = getrlimit(Resource::RLIMIT_CORE).expect("getrlimit");
    setrlimit(Resource::RLIMIT_CORE, hard, hard).expect("setrlimit");
    if hard == 0 {
        println!("confine: the hard core limit is 0 already, so its lowering is not observed");
    }

    // What the session offers before the confinement: a TCP listener, an abstract Unix socket
    // and a path socket, as the buses and the Wayland socket are.
    let tcp = TcpListener::bind("127.0.0.1:0").expect("tcp bind");
    let port = tcp.local_addr().expect("tcp address").port();
    let abstract_address =
        SocketAddr::from_abstract_name(format!("athanor-lock-confine-{}", std::process::id()))
            .expect("abstract name");
    let abstract_socket = UnixListener::bind_addr(&abstract_address).expect("abstract bind");
    let path_socket = UnixListener::bind(base.join("path.sock")).expect("path bind");

    // A second thread, as the logind thread started before the confinement would be.
    let (release, wait) = channel::<()>();
    let other = std::thread::spawn(move || wait.recv());
    assert_eq!(threads(), 2);
    assert!(
        confine(Some(runtime.as_os_str())).is_err(),
        "a process with two threads must not confine itself"
    );
    assert!(
        fs::write(outside.join("before"), b"x").is_ok(),
        "a refused confinement applies no ruleset"
    );
    assert!(
        nix::sys::prctl::get_dumpable().expect("get_dumpable"),
        "a refused confinement leaves the process as it was"
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_ok()
            && UnixStream::connect_addr(&abstract_address).is_ok(),
        "a refused confinement leaves the network as it was"
    );
    assert!(
        fs::write("/proc/self/comm", b"confine").is_ok(),
        "a refused confinement leaves procfs writable"
    );
    assert_eq!(
        getrlimit(Resource::RLIMIT_CORE).expect("getrlimit"),
        (hard, hard),
        "a refused confinement leaves the core limit as it was"
    );
    release.send(()).expect("release");
    other.join().expect("join").expect("released");
    // A joined thread can show in /proc a moment longer.
    let deadline = Instant::now() + Duration::from_secs(5);
    while threads() != 1 {
        assert!(Instant::now() < deadline, "the second thread never left");
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        confine(None).is_err(),
        "no runtime directory, no confinement"
    );
    assert!(
        confine(Some("relative".as_ref())).is_err(),
        "a relative runtime directory is refused"
    );
    assert!(
        confine(Some(base.join("missing").as_os_str())).is_err(),
        "a missing runtime directory is refused"
    );

    confine(Some(runtime.as_os_str())).expect("Landlock must be enforced, not skipped");
    assert_eq!(
        fs::write(outside.join("after"), b"x")
            .expect_err("a write outside the runtime directory")
            .kind(),
        ErrorKind::PermissionDenied
    );
    assert!(
        fs::write(runtime.join("after"), b"x").is_ok(),
        "writes beneath the runtime directory stay open"
    );
    assert!(fs::read(outside.join("before")).is_ok(), "reads stay open");
    // procfs is a filesystem Landlock mediates: the writes `ProtectKernelTunables=` and
    // `ProtectControlGroups=` refused are refused here too, shown on a file the process owns.
    assert_eq!(
        fs::write("/proc/self/comm", b"confine")
            .expect_err("a write to procfs")
            .kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        TcpStream::connect(("127.0.0.1", port))
            .expect_err("TCP is refused")
            .kind(),
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        UnixStream::connect_addr(&abstract_address)
            .expect_err("an abstract socket of the session is refused")
            .kind(),
        ErrorKind::PermissionDenied
    );
    assert!(
        UnixStream::connect(base.join("path.sock")).is_ok(),
        "path sockets, the buses' and Wayland's, stay open"
    );
    drop((tcp, abstract_socket, path_socket));
    assert_eq!(
        getrlimit(Resource::RLIMIT_CORE).expect("getrlimit"),
        (0, 0),
        "no core file may be written, by any path"
    );
    assert!(
        nix::sys::prctl::get_dumpable().expect("get_dumpable"),
        "the confined process stays dumpable until Sal has connected"
    );
    println!("confine: ok");
}
