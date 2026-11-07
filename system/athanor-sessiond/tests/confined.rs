//! The daemon's Landlock (SD17) on a real bus with real dconf: under the grants it computes, a
//! write outside `$XDG_RUNTIME_DIR/dconf` and a read outside its trees are refused, and the reset of
//! COSMIC's appearance values (SD16) still reads and writes through dconf without the critical
//! line dconf's client logs when its shared-memory flag is refused.
//!
//! Landlock binds the thread that applies it, so the test runs itself again as a child with the
//! environment of a session of its own and confines that child's test thread.

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use athanor_sessiond::{appearance, confine};
use gio::prelude::*;

const CHILD: &str = "ATHANOR_SESSIOND_CONFINED_CHILD";
const MODE: &str = "ATHANOR_SESSIOND_CONFINED_MODE";
/// dconf's client says so when its shared-memory flag in `$XDG_RUNTIME_DIR/dconf` is refused.
const REFUSED_FLAG: &str = "dconf will not work properly";
/// The three keys under the GNOME schema's id, so the test does not depend on the machine's.
const SCHEMA: &str = r#"<schemalist>
  <schema id="org.gnome.desktop.interface" path="/org/gnome/desktop/interface/">
    <key name="icon-theme" type="s"><default>'Adwaita'</default></key>
    <key name="font-name" type="s"><default>'Adwaita Sans 11'</default></key>
    <key name="monospace-font-name" type="s"><default>'Adwaita Mono 11'</default></key>
  </schema>
</schemalist>"#;

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
            std::env::temp_dir().join(format!("athanor-sessiond-confined-{}", std::process::id()));
        fs::remove_dir_all(&base).ok();
        for dir in ["home/.config", "runtime", "schemas"] {
            fs::create_dir_all(base.join(dir)).expect("mkdir");
        }
        fs::set_permissions(
            base.join("runtime"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .expect("chmod");
        fs::write(base.join("schemas/test.gschema.xml"), SCHEMA).expect("schema");
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

    /// Runs the confined half, `child`, in `mode`; returns its stderr.
    fn child(&self, mode: &str) -> String {
        let out = self
            .command(std::env::current_exe().expect("exe"))
            .args(["child", "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, &self.base)
            .env(MODE, mode)
            .output()
            .expect("child");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            out.status.success(),
            "child {mode}: {}{stderr}",
            String::from_utf8_lossy(&out.stdout)
        );
        stderr
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
fn the_confined_daemon_writes_only_its_grants_and_resets_through_dconf() {
    let session = Session::start();
    let base = &session.base;
    let interface = "org.gnome.desktop.interface";
    session.gsettings(&["set", interface, "icon-theme", "Cosmic"]);
    session.gsettings(&["set", interface, "font-name", "Inter 11"]);

    // The control: without dconf's grant, the client logs its critical line.
    let without = session.child("without-dconf");
    assert!(without.contains(REFUSED_FLAG), "{without}");

    let with = session.child("granted");
    assert!(!with.contains("CRITICAL"), "{with}");
    assert_eq!(
        session.gsettings(&["get", interface, "icon-theme"]),
        "'Adwaita'"
    );
    assert_eq!(
        session.gsettings(&["get", interface, "font-name"]),
        "'Inter 11'"
    );
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
    athanor_unit::journal::init();
    let tcp = TcpListener::bind("127.0.0.1:0").expect("tcp");

    let mut grants = confine::daemon(|name| std::env::var_os(name), Some(&base.join("schemas")))
        .expect("grants");
    let dconf = fs::metadata(runtime.join("dconf")).expect("dconf");
    let [checked] = grants.write.as_slice() else {
        panic!("{grants:?}")
    };
    let checked = fs::File::from(checked.try_clone().expect("dup"))
        .metadata()
        .expect("fstat");
    assert_eq!((checked.dev(), checked.ino()), (dconf.dev(), dconf.ino()));
    let granted = std::env::var(MODE).as_deref() == Ok("granted");
    if !granted {
        grants.write.clear();
    }
    grants.apply().expect("confine");

    let refused = |error: std::io::Error, path: &Path| {
        assert_eq!(
            error.kind(),
            ErrorKind::PermissionDenied,
            "{}",
            path.display()
        );
    };
    for path in [
        base.join("outside"),
        runtime.join("stray"),
        // The user's database is read, and written only by dconf's service.
        base.join("home/.config/dconf/user"),
    ] {
        refused(
            fs::write(&path, b"x").expect_err("a write outside the grants"),
            &path,
        );
    }
    // Nothing is read outside the grants: the home, /proc.
    for path in [base.join("home"), PathBuf::from("/proc/self")] {
        refused(
            fs::read_dir(&path).expect_err("a read outside the grants"),
            &path,
        );
    }
    let environ = Path::new("/proc/self/environ");
    refused(
        fs::read(environ).expect_err("/proc is not granted"),
        environ,
    );
    let error = TcpStream::connect(tcp.local_addr().expect("addr")).expect_err("TCP");
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);

    let settings = appearance::open().expect("dconf's backend and the interface schema");
    if granted {
        assert_eq!(appearance::reset(&settings), Ok(vec!["icon-theme"]));
    } else {
        // A read alone opens dconf's flag.
        assert_eq!(settings.string("icon-theme"), "Cosmic");
    }
}
