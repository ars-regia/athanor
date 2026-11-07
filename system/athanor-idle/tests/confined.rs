//! The daemon's Landlock (SD17) on a real bus with real dconf: under the grants it computes,
//! a write outside `$XDG_RUNTIME_DIR/athanor-idle` is refused, and what the daemon does still
//! works: `inhibitors.json`, the read of the Power page's store, GSettings reads and writes
//! through dconf, the Wayland socket, the session bus and the ScreenSaver interface, which
//! asks the bus and systemd for a caller's unit and reads nothing under `/proc`.
//!
//! Landlock binds the thread that applies it, so the test runs itself again as a child with
//! the environment of a session of its own and confines that child's test thread.

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use athanor_idle::confine;
use athanor_idle::screensaver;
use athanor_sulphur_client::idle::{read_in, user_dir, Key};
use gio::prelude::*;

const CHILD: &str = "ATHANOR_IDLE_CONFINED_CHILD";
const SCHEMA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/",
    "org.athanor.desktop.idle.gschema.xml"
);

/// A session of its own beneath `base`: a home, a runtime directory, compiled schemas and a
/// `dbus-daemon` that activates `dconf-service` with this session's environment.
struct Session {
    base: PathBuf,
    bus: Child,
    address: String,
}

impl Session {
    fn start() -> Session {
        let base =
            std::env::temp_dir().join(format!("athanor-idle-confined-{}", std::process::id()));
        fs::remove_dir_all(&base).ok();
        for dir in ["home/.config", "home/.local/share", "runtime", "schemas"] {
            fs::create_dir_all(base.join(dir)).expect("mkdir");
        }
        fs::set_permissions(
            base.join("runtime"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .expect("chmod");
        fs::copy(
            SCHEMA,
            base.join("schemas/org.athanor.desktop.idle.gschema.xml"),
        )
        .expect("schema");
        let compiled = Command::new("glib-compile-schemas")
            .arg(base.join("schemas"))
            .status()
            .expect("glib-compile-schemas on PATH");
        assert!(compiled.success());
        let config = base.join("bus.conf");
        fs::write(
            &config,
            format!(
                "<busconfig><type>session</type><listen>unix:path={}</listen>\
                 <servicedir>/usr/share/dbus-1/services</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/>\
                 </policy></busconfig>",
                base.join("bus").display()
            ),
        )
        .expect("bus.conf");
        // The environment before the bus starts: the dconf-service it activates inherits it, and
        // would otherwise write the database of the user running the test.
        let mut bus = Command::new("dbus-daemon");
        bus.args(["--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--config-file={}", config.display()))
            .stdout(Stdio::piped());
        let mut session = Session {
            bus: Session::env(&base, &mut bus, "")
                .spawn()
                .expect("dbus-daemon on PATH"),
            base,
            address: String::new(),
        };
        BufReader::new(session.bus.stdout.take().expect("stdout"))
            .read_line(&mut session.address)
            .expect("address");
        session.address = session.address.trim().to_owned();
        session
    }

    fn env<'a>(base: &Path, command: &'a mut Command, address: &str) -> &'a mut Command {
        let home = base.join("home");
        command
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", base.join("runtime"))
            .env("GSETTINGS_SCHEMA_DIR", base.join("schemas"))
            .env("DBUS_SESSION_BUS_ADDRESS", address)
            .env_remove("GSETTINGS_BACKEND")
            .env_remove("DCONF_PROFILE")
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        Session::env(&self.base, &mut command, &self.address);
        command
    }

    fn gsettings(&self, args: &[&str]) -> String {
        let out = self
            .command("gsettings")
            .args(args)
            .output()
            .expect("gsettings on PATH");
        assert!(
            out.status.success(),
            "gsettings {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Teardown: a bus that already exited, or a directory already gone, is fine.
        self.bus.kill().ok();
        self.bus.wait().ok();
        fs::remove_dir_all(&self.base).ok();
    }
}

#[test]
fn the_confined_daemon_writes_only_its_directory_and_still_works() {
    let session = Session::start();
    let base = &session.base;
    session.gsettings(&["set", "org.athanor.desktop.idle", "suspend-after-ac", "77"]);
    let _wayland =
        std::os::unix::net::UnixListener::bind(base.join("runtime/wayland-test")).expect("bind");

    let out = session
        .command(std::env::current_exe().expect("exe"))
        .args(["child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, base)
        .output()
        .expect("child");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "child: {}{stderr}",
        String::from_utf8_lossy(&out.stdout)
    );
    // dconf's client keeps a shared-memory flag in `$XDG_RUNTIME_DIR/dconf`; refused, it says so.
    assert!(!stderr.contains("dconf will not work properly"), "{stderr}");
    // The Inhibit of a caller outside any unit: one warning, with its reason, and an empty id.
    let warned: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("cannot tell the caller's unit"))
        .collect();
    assert_eq!(warned.len(), 1, "{stderr}");
    eprintln!("{}", warned[0]);
    assert_eq!(
        session.gsettings(&["get", "org.athanor.desktop.idle", "blank-after"]),
        "uint32 42"
    );
    assert!(base.join("runtime/athanor-idle/inhibitors.json").is_file());
    assert!(!base.join("outside").exists());
    assert!(!base.join("runtime/stray").exists());
}

/// The confined half; it does nothing when the test above did not start it.
#[test]
fn child() {
    let Some(base) = std::env::var_os(CHILD).map(PathBuf::from) else {
        return;
    };
    let runtime = base.join("runtime");
    // The Power page's store, beneath this session's XDG_CONFIG_HOME.
    athanor_unit::journal::init();
    let store = user_dir().expect("the Power page's store");
    fs::create_dir_all(&store).expect("mkdir");
    fs::write(store.join("screen_off_time"), "Some(60000)").expect("store");
    let tcp = TcpListener::bind("127.0.0.1:0").expect("tcp");

    let (dir, grants) =
        confine::daemon(&runtime, Some(&store), |name| std::env::var_os(name)).expect("grants");
    assert_eq!(
        grants.write.len(),
        2,
        "athanor-idle's and dconf's: {grants:?}"
    );
    grants.apply().expect("confine");
    eprintln!("grants: {grants:?}");

    let refused = |path: &Path| {
        let error = fs::write(path, b"x").expect_err("a write outside the grants");
        assert_eq!(
            error.kind(),
            ErrorKind::PermissionDenied,
            "{}",
            path.display()
        );
    };
    refused(&base.join("outside"));
    refused(&runtime.join("stray"));
    refused(&store.join("screen_off_time"));
    let error = TcpStream::connect(tcp.local_addr().expect("addr")).expect_err("TCP");
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);

    // /proc is not granted: nothing of another process, or of this one, is read there.
    for path in ["/proc/self/environ", "/proc/self/cgroup"] {
        let error = fs::read(path).expect_err("/proc is not granted");
        assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{path}");
    }
    assert_eq!(read_in(&store).screen_off_time, Key::Millis(60000));
    UnixStream::connect(runtime.join("wayland-test")).expect("the Wayland socket");

    let backend = gio::SettingsBackend::default();
    assert_eq!(backend.type_().name(), "DConfSettingsBackend");
    let idle = gio::Settings::new("org.athanor.desktop.idle");
    assert_eq!(idle.uint("suspend-after-ac"), 77);
    idle.set_uint("blank-after", 42).expect("a GSettings write");
    gio::Settings::sync();

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let bus = zbus::Connection::session().await.expect("the session bus");
            let dbus = zbus::fdo::DBusProxy::new(&bus).await.expect("proxy");
            dbus.get_id().await.expect("GetId");

            // The ScreenSaver interface keeps its table in inhibitors.json (SN7).
            let query = zbus::Connection::session()
                .await
                .expect("the query connection");
            let (_hold, _end) = screensaver::serve(&bus, &query, dir.join("inhibitors.json"))
                .await
                .expect("the ScreenSaver interface");
            let client = zbus::Connection::session().await.expect("a client");
            let reply = client
                .call_method(
                    Some(screensaver::NAME),
                    screensaver::PATHS[0],
                    Some(screensaver::NAME),
                    "Inhibit",
                    &("app", "confined"),
                )
                .await
                .expect("Inhibit");
            let _cookie: u32 = reply.body().deserialize().expect("a cookie");
            let saved = fs::read_to_string(dir.join("inhibitors.json")).expect("inhibitors.json");
            assert!(saved.contains("confined"), "{saved}");
            // The private bus has no systemd to name the caller's unit: the id is empty, and
            // the journal says why (the parent reads it).
            assert!(saved.contains(r#""app_id":"""#), "{saved}");
        });
}
