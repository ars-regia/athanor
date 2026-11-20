//! The lock confines itself only while it has one thread (LP15, plan decision M3). A binary
//! without libtest's harness, whose `main` is the process's only thread: with a second thread
//! alive the confinement is refused and nothing is applied; once that thread has gone it is
//! applied, writes leave only the runtime directory, and the process is not dumpable.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use athanor_lock::confine::confine;

fn threads() -> usize {
    fs::read_dir("/proc/self/task").expect("tasks").count()
}

fn main() {
    let base =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("confine-{}", std::process::id()));
    let (runtime, outside) = (base.join("runtime"), base.join("outside"));
    fs::create_dir_all(&runtime).expect("mkdir runtime");
    fs::create_dir_all(&outside).expect("mkdir outside");

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
    assert!(
        !nix::sys::prctl::get_dumpable().expect("get_dumpable"),
        "the confined process is not dumpable"
    );
    println!("confine: ok");
}
