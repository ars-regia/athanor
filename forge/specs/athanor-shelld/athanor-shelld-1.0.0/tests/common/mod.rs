//! A private dbus-daemon per test, and fakes of the user's and the system's systemd that put the
//! test process in a unit as the main process of its service.

#![allow(dead_code)] // each test file uses part of it

use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::{env, fs, process};

use athanor_shelld::sender::BAR as BAR_SERVICE;
use athanor_shelld::server::{self, Config};
use zbus::connection::Builder;
use zbus::zvariant::{Fd, OwnedObjectPath};
use zbus::Connection;

pub const BAR: &str = BAR_SERVICE.unit;
pub const APP: &str = "app-athanor-foo@0123.service";

pub struct Bus {
    daemon: Child,
    name: String,
    pub address: String,
    pub dir: PathBuf,
}

impl Bus {
    pub fn start(name: &str) -> Bus {
        let dir = env::temp_dir().join(format!("athanor-shelld-{name}-{}", process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--address=unix:path={}", dir.join("bus").display()))
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("stdout"))
            .read_line(&mut address)
            .expect("address");
        Bus {
            daemon,
            name: name.to_owned(),
            address: address.trim().to_owned(),
            dir,
        }
    }

    pub fn builder(&self) -> Builder<'static> {
        Builder::address(self.address.as_str()).expect("address")
    }

    pub async fn client(&self) -> Connection {
        self.builder().build().await.expect("client")
    }

    /// A daemon whose user systemd places this test process in `unit`, with its state under the
    /// bus directory.
    pub async fn daemon(&self, unit: &str) -> Daemon {
        self.daemon_asking(unit, self).await
    }

    /// Stops the bus daemon: its socket stays open and nothing is answered.
    pub fn freeze(&self) {
        let pid = self.daemon.id().to_string();
        assert!(Command::new("kill")
            .args(["-STOP", &pid])
            .status()
            .expect("kill")
            .success());
    }

    /// As `daemon`, but its query connection is on `query`'s bus.
    pub async fn daemon_asking(&self, unit: &str, query: &Bus) -> Daemon {
        let pid = process::id();
        let systemd = fake_systemd(self, unit, pid).await;
        let system_bus = Bus::start(&format!("{}-system", self.name));
        let system = fake_systemd(&system_bus, &user_service(self), pid).await;
        let state_dir = self.dir.join("state");
        fs::create_dir_all(&state_dir).expect("state dir");
        let connection = server::start(
            self.builder(),
            query.builder(),
            Config {
                state_dir,
                system: Some(system_bus.client().await),
            },
        )
        .await
        .expect("daemon");
        Daemon {
            _connection: connection,
            _systemd: [systemd, system],
            _system_bus: system_bus,
        }
    }
}

/// A running daemon and the fakes it asks; dropped together.
pub struct Daemon {
    _connection: Connection,
    _systemd: [Connection; 2],
    _system_bus: Bus,
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        fs::remove_dir_all(&self.dir).ok();
    }
}

const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/x";

/// A systemd's manager that places every pidfd in `unit`.
struct FakeManager {
    unit: String,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl FakeManager {
    #[zbus(name = "GetUnitByPIDFD")]
    fn get_unit_by_pidfd(&self, _pidfd: Fd<'_>) -> (OwnedObjectPath, String, Vec<u8>) {
        let path = OwnedObjectPath::try_from(UNIT_PATH).expect("path");
        (path, self.unit.clone(), Vec::new())
    }
}

/// The unit's service, as the bar's unit file in the image defines it, whose main process is
/// `pid`.
struct FakeService {
    pid: u32,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Service")]
impl FakeService {
    #[zbus(property, name = "MainPID")]
    fn main_pid(&self) -> u32 {
        self.pid
    }

    #[zbus(property, name = "ExecStart")]
    #[allow(clippy::type_complexity)]
    fn exec_start(&self) -> Vec<(String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32)> {
        let exec = BAR_SERVICE.exec.to_owned();
        vec![(exec.clone(), vec![exec], false, 0, 0, 0, 0, 0, 0, 0)]
    }
}

struct FakeUnit;

#[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
impl FakeUnit {
    #[zbus(property, name = "FragmentPath")]
    fn fragment_path(&self) -> String {
        "/usr/lib/systemd/user/athanor-bar.service".into()
    }

    #[zbus(property, name = "DropInPaths")]
    fn drop_in_paths(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A systemd on `bus` that places every pidfd in `unit`, whose main process is `pid`. This
/// process owns its name, so it is that main process too: on the session bus the user's
/// systemd, on the stand-in for the system bus the system's.
async fn fake_systemd(bus: &Bus, unit: &str, pid: u32) -> Connection {
    let c = bus.client().await;
    let server = c.object_server();
    let manager = FakeManager { unit: unit.into() };
    server
        .at("/org/freedesktop/systemd1", manager)
        .await
        .expect("manager");
    server
        .at(UNIT_PATH, FakeService { pid })
        .await
        .expect("service");
    server.at(UNIT_PATH, FakeUnit).await.expect("unit");
    c.request_name("org.freedesktop.systemd1")
        .await
        .expect("name");
    c
}

/// `user@<uid>.service` of the user who owns `bus`'s directory: this process's.
fn user_service(bus: &Bus) -> String {
    let uid = fs::metadata(&bus.dir).expect("bus directory").uid();
    format!("user@{uid}.service")
}
