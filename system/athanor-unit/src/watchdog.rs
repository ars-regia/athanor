//! The manager's watchdog for a daemon on the session bus (doc_session.md SN6): `WATCHDOG=1`
//! only after a call the daemon made to the bus on its own connection has been answered through
//! its own event loop, so that one answer proves that the loop runs and that the connection
//! serves. A timer alone proves only the first.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use tokio::time::{self, MissedTickBehavior};

use crate::notify;

/// Pings the manager's watchdog from a task of the current Tokio runtime, at half of
/// `WATCHDOG_USEC`, each ping after `org.freedesktop.DBus.GetId` on `connection` has been
/// answered. Spawns nothing without `WatchdogSec=` in the unit or outside systemd. The task runs
/// only while the runtime polls it, so a loop that blocks stops the pings as a wedged
/// connection does.
pub fn spawn(connection: &zbus::Connection) {
    let Some(period) = notify::watchdog_timeout().and_then(|timeout| timeout.checked_div(2)) else {
        return;
    };
    let Some(socket) = env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    let connection = connection.clone();
    tokio::spawn(serve(period, socket, move || roundtrip(connection.clone())));
    tracing::info!(
        ?period,
        "the manager's watchdog is pinged after each roundtrip on the bus"
    );
}

/// One call to the bus itself, not to another service, so that a peer that is down cannot make
/// a healthy daemon fail (SN6).
async fn roundtrip(connection: zbus::Connection) -> zbus::Result<()> {
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetId",
            &(),
        )
        .await
        .map(drop)
}

/// Every `period`, one roundtrip, and `WATCHDOG=1` to `socket` once it has succeeded. A roundtrip
/// that fails, or that has not returned when the next ping is due, sends no ping for that period.
async fn serve<F, R, E>(period: Duration, socket: OsString, mut roundtrip: F)
where
    F: FnMut() -> R,
    R: Future<Output = Result<(), E>>,
    E: Display,
{
    let mut ticks = time::interval(period);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticks.tick().await;
        match time::timeout(period, roundtrip()).await {
            Ok(Ok(())) => ping(&socket),
            Ok(Err(error)) => {
                tracing::warn!(%error, "the roundtrip on the bus failed: no WATCHDOG=1")
            }
            Err(_) => tracing::warn!(
                ?period,
                "the roundtrip on the bus did not return: no WATCHDOG=1"
            ),
        }
    }
}

fn ping(socket: &OsStr) {
    if let Err(error) = notify::notify_to(socket, b"WATCHDOG=1") {
        tracing::warn!(%error, "cannot send WATCHDOG=1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::{fs, process};
    use tokio::net::UnixDatagram;

    const PERIOD: Duration = Duration::from_millis(100);

    /// A test `NOTIFY_SOCKET`: a datagram socket of its own in a fresh directory.
    fn notify_socket(name: &str) -> (PathBuf, UnixDatagram) {
        let dir = env::temp_dir().join(format!("athanor-unit-watchdog-{}-{name}", process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("notify");
        let _ = fs::remove_file(&path);
        let listener = UnixDatagram::bind(&path).expect("bind");
        (path, listener)
    }

    /// The next datagram within `within`, or `None`.
    async fn next(listener: &UnixDatagram, within: Duration) -> Option<Vec<u8>> {
        let mut buffer = [0u8; 32];
        let read = time::timeout(within, listener.recv(&mut buffer))
            .await
            .ok()?;
        Some(buffer[..read.expect("recv")].to_vec())
    }

    /// Drops every datagram already queued.
    async fn drain(listener: &UnixDatagram) {
        while next(listener, Duration::ZERO).await.is_some() {}
    }

    /// A private `dbus-daemon`, stopped and its directory removed on drop.
    struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start(dir: &Path) -> Bus {
            // A configuration of its own: `--session` reads the host's, which on a system
            // running dbus-broker has no <listen> element.
            let config = dir.join("bus.conf");
            fs::write(
                &config,
                format!(
                    "<busconfig><type>session</type><listen>unix:path={}</listen>\
                     <policy context=\"default\"><allow send_destination=\"*\"/>\
                     <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>",
                    dir.join("bus").display()
                ),
            )
            .expect("bus.conf");
            let mut daemon = Command::new("dbus-daemon")
                .args(["--nofork", "--nopidfile", "--print-address"])
                .arg(format!("--config-file={}", config.display()))
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon on PATH");
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().expect("stdout"))
                .read_line(&mut address)
                .expect("address");
            Bus {
                daemon,
                address: address.trim().to_owned(),
                dir: dir.to_owned(),
            }
        }

        fn signal(&self, signal: &str) {
            let status = Command::new("kill")
                .args([signal, &self.daemon.id().to_string()])
                .status()
                .expect("kill");
            assert!(status.success(), "kill {signal}");
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[tokio::test]
    async fn a_ping_follows_an_answered_roundtrip_and_none_a_stalled_bus() {
        let (path, listener) = notify_socket("bus");
        let bus = Bus::start(path.parent().expect("dir"));
        let connection = zbus::connection::Builder::address(bus.address.as_str())
            .build()
            .await
            .expect("connect");
        tokio::spawn(serve(PERIOD, path.clone().into(), move || {
            roundtrip(connection.clone())
        }));
        assert_eq!(
            next(&listener, PERIOD * 5).await.as_deref(),
            Some(&b"WATCHDOG=1"[..])
        );

        // The bus stops reading: the connection is wedged while the loop runs.
        bus.signal("-STOP");
        time::sleep(PERIOD * 2).await;
        drain(&listener).await;
        assert_eq!(
            next(&listener, PERIOD * 5).await,
            None,
            "a ping while the bus is stopped"
        );

        bus.signal("-CONT");
        assert_eq!(
            next(&listener, PERIOD * 5).await.as_deref(),
            Some(&b"WATCHDOG=1"[..])
        );
    }

    #[tokio::test]
    async fn a_failed_or_unanswered_roundtrip_sends_no_ping() {
        let (failed, listener) = notify_socket("failed");
        tokio::spawn(serve(PERIOD, failed.clone().into(), || async {
            Err::<(), _>("refused")
        }));
        assert_eq!(
            next(&listener, PERIOD * 5).await,
            None,
            "a ping after a failed roundtrip"
        );

        let (path, listener) = notify_socket("pending");
        tokio::spawn(serve(
            PERIOD,
            path.clone().into(),
            std::future::pending::<Result<(), &str>>,
        ));
        assert_eq!(
            next(&listener, PERIOD * 5).await,
            None,
            "a ping without an answer"
        );
        for socket in [failed, path] {
            let _ = fs::remove_dir_all(socket.parent().expect("dir"));
        }
    }
}
