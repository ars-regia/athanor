//! A private dbus-daemon per test, and a fake /proc that puts the test process in a unit.

#![allow(dead_code)] // each test file uses part of it

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::{env, fs, process};

use athanor_shelld::sender::{Admitted, Caller};
use athanor_shelld::server::{self, Config};
use zbus::connection::Builder;
use zbus::Connection;

pub const BAR_CGROUP: &str =
    "/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service";
/// A sender in no application unit: a login session scope.
pub const SESSION_CGROUP: &str = "/user.slice/user-1000.slice/session-2.scope";
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
        // A configuration of its own: `--session` reads the host's, which on a system running
        // dbus-broker has no <listen> element, and dbus-daemon refuses to start without one.
        let config = dir.join("bus.conf");
        fs::write(
            &config,
            format!(
                "<busconfig><type>session</type><listen>unix:path={}</listen>\
                 <policy context=\"default\"><allow send_destination=\"*\"/>\
                 <allow receive_sender=\"*\"/><allow own=\"*\"/>\
                 </policy></busconfig>",
                dir.join("bus").display()
            ),
        )
        .expect("bus.conf");
        let mut daemon = Command::new("dbus-daemon")
            .args(["--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--config-file={}", config.display()))
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

    /// A daemon that sees this test process in `cgroup`, with its state under the bus directory,
    /// and that admits the callers of the private interface by the pid's cgroup.
    pub async fn daemon(&self, cgroup: &str) -> Connection {
        let proc_root = fake_proc(&self.dir, cgroup);
        let (state_dir, config_dir) = (self.dir.join("state"), self.dir.join("config"));
        for dir in [&state_dir, &config_dir] {
            fs::create_dir_all(dir).expect("mkdir");
        }
        server::start(
            self.builder(),
            Config {
                state_dir,
                config_dir,
                admitted: Admitted::from_proc_root(&proc_root),
                proc_root,
            },
        )
        .await
        .expect("daemon")
        .connection
    }

    /// A daemon that admits `callers` by unique name and sees every sender in `cgroup`.
    pub async fn daemon_for(&self, cgroup: &str, callers: &[(&Connection, Caller)]) -> Connection {
        let proc_root = fake_proc(&self.dir, cgroup);
        let names: HashMap<String, Caller> = callers
            .iter()
            .map(|(conn, caller)| {
                (
                    conn.unique_name().expect("unique name").to_string(),
                    *caller,
                )
            })
            .collect();
        let (state_dir, config_dir) = (self.dir.join("state"), self.dir.join("config"));
        for dir in [&state_dir, &config_dir] {
            fs::create_dir_all(dir).expect("mkdir");
        }
        server::start(
            self.builder(),
            Config {
                state_dir,
                config_dir,
                proc_root,
                admitted: Admitted::from_fn(move |name, _pid| names.get(name).copied()),
            },
        )
        .await
        .expect("daemon")
        .connection
    }
}

/// Waits up to 5 s for `path` to contain `needle`.
pub async fn wait_for_file(path: &Path, needle: &str) {
    for _ in 0..100 {
        if fs::read_to_string(path).is_ok_and(|text| text.contains(needle)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("{} never contained {needle}", path.display());
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
