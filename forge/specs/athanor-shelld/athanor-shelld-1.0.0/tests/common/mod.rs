//! A private dbus-daemon per test, and a fake /proc that puts the test process in a unit.

#![allow(dead_code)] // each test file uses part of it

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::{env, fs, process};

use athanor_shelld::sender::{BarUnit, BAR_UNIT};
use athanor_shelld::server::{self, Config};
use zbus::connection::Builder;
use zbus::Connection;

pub const BAR_CGROUP: &str =
    "/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service";
pub const APP_CGROUP: &str =
    "/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-foo@0123.service";

pub struct Bus {
    daemon: Child,
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

    /// A daemon that sees this test process in `cgroup`, with its state under the bus directory.
    pub async fn daemon(&self, cgroup: &str) -> Connection {
        let proc_root = fake_proc(&self.dir, cgroup);
        let state_dir = self.dir.join("state");
        fs::create_dir_all(&state_dir).expect("state dir");
        server::start(
            self.builder(),
            Config {
                state_dir,
                bar: BarUnit::with_proc_root(BAR_UNIT, proc_root),
            },
        )
        .await
        .expect("daemon")
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // Test teardown: a daemon that already exited, or a directory already gone, is fine.
        self.daemon.kill().ok();
        self.daemon.wait().ok();
        fs::remove_dir_all(&self.dir).ok();
    }
}

pub fn fake_proc(dir: &Path, cgroup: &str) -> PathBuf {
    let root = dir.join("proc");
    let own = root.join(process::id().to_string());
    fs::create_dir_all(&own).expect("mkdir");
    fs::write(own.join("cgroup"), format!("0::{cgroup}\n")).expect("cgroup");
    root
}
