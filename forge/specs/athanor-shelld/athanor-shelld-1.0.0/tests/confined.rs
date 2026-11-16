//! The daemon's Landlock (BR1) under the grants of `athanor_shelld::confine`: nothing under
//! `/proc` is read, not even this process's environment, a write outside the state directory is
//! refused, and the bar is still admitted, through the bus and systemd alone (issue #1047).
//!
//! Landlock binds the thread that applies it, so the test runs itself again as a child and
//! confines that child's test thread. The buses and the daemon start first, since dbus-daemon
//! opens `/dev/null`; the runtime runs every task on that one thread, so the call and its
//! admission run confined.

mod common;

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::process::Command;

use athanor_shelld::server::PRIVATE_PATH;
use athanor_shelld::wire::WireNotification;
use common::{Bus, BAR};
use zbus::Proxy;

const CHILD: &str = "ATHANOR_SHELLD_CONFINED_CHILD";
const RAN: &str = "the confined child ran";

#[test]
fn the_confined_daemon_reads_nothing_under_proc_and_still_admits_the_bar() {
    let mut child = Command::new(env::current_exe().expect("exe"));
    child.args(["child", "--exact", "--nocapture", "--test-threads=1"]);
    let out = child.env(CHILD, "1").output().expect("child");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "child: {}{stderr}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(stderr.contains(RAN), "{stderr}");
}

/// The confined half; it does nothing when the test above did not start it.
#[test]
fn child() {
    if env::var_os(CHILD).is_none() {
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let bus = Bus::start("confined");
    let _daemon = runtime.block_on(bus.daemon(BAR));
    athanor_shelld::confine(&bus.dir.join("state"))
        .expect("Landlock must be enforced, not skipped");

    // /proc is not granted: nothing of another process, or of this one, is read there. A read
    // that succeeds is not printed: the environment can hold tokens.
    for path in ["/proc/self/environ", "/proc/self/cgroup"] {
        match fs::read(path) {
            Ok(_) => panic!("{path} is readable: /proc is granted"),
            Err(error) => assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{path}: {error}"),
        }
    }
    let error = fs::write(bus.dir.join("outside"), "x").expect_err("outside the state directory");
    assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{error}");

    runtime.block_on(async {
        let proxy = Proxy::new(
            &bus.client().await,
            "org.freedesktop.Notifications",
            PRIVATE_PATH,
            "os.athanor.Notifications1",
        )
        .await
        .expect("proxy");
        let (_dnd, listed) = proxy
            .call::<_, _, (bool, Vec<WireNotification>)>("List", &())
            .await
            .expect("the bar is admitted without /proc");
        assert!(listed.is_empty());
    });
    eprintln!("{RAN}");
}
