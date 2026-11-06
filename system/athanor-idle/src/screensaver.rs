//! `org.freedesktop.ScreenSaver` on the session bus (SD5), a thin layer over the inhibitor
//! table: the logic stays in [`Inhibitors`], and `inhibitors.json` in [`crate::store`].
//! `os.athanor.Idle1.InhibitSuspend` is not here (pull request I2b).

use crate::inhibitors::{Inhibitor, Inhibitors, Kind, RemoveError};
use crate::store;
use crate::timers::Hold;
use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch;
use zbus::fdo::{self, DBusProxy, RequestNameFlags};
use zbus::message::Header;
use zbus::names::BusName;
use zbus::{Connection, Result};

pub const NAME: &str = "org.freedesktop.ScreenSaver";
/// The two object paths the name is served on, as cosmic-idle does.
pub const PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

/// The table, the file it is written to and the channel that tells the timers what it holds.
struct State {
    table: Mutex<Inhibitors>,
    file: PathBuf,
    hold: watch::Sender<Hold>,
}

impl State {
    fn lock(&self) -> MutexGuard<'_, Inhibitors> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// After a change: tell the timers and rewrite the file. A write that fails is logged;
    /// the inhibitor still holds for as long as this process lives.
    fn changed(&self, table: &Inhibitors) {
        self.hold.send_replace(table.hold());
        if let Err(e) = store::save(&self.file, table) {
            tracing::warn!("cannot write {}: {e}", self.file.display());
        }
    }
}

struct ScreenSaver(Arc<State>);

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    async fn inhibit(
        &self,
        application_name: &str,
        reason: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<u32> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("no sender".into()))?;
        let app_id = app_id_of(conn, sender.clone().into()).await;
        let mut table = self.0.lock();
        let cookie = table
            .add(Inhibitor {
                kind: Kind::Screen,
                owner: sender.to_string(),
                app_id: app_id.clone(),
                reason: reason.to_owned(),
            })
            .ok_or_else(|| fdo::Error::Failed("no cookie left".into()))?;
        tracing::info!(cookie, app_id, application_name, reason, "inhibit");
        self.0.changed(&table);
        Ok(cookie)
    }

    async fn un_inhibit(&self, cookie: u32, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("no sender".into()))?;
        let mut table = self.0.lock();
        match table.remove(cookie, sender.as_str()) {
            Ok(i) => {
                tracing::info!(cookie, app_id = i.app_id, reason = i.reason, "uninhibit");
                self.0.changed(&table);
                Ok(())
            }
            Err(RemoveError::UnknownCookie) => {
                Err(fdo::Error::InvalidArgs(format!("no inhibitor {cookie}")))
            }
            Err(RemoveError::NotOwner) => Err(fdo::Error::AccessDenied(
                "the inhibitor belongs to another peer".into(),
            )),
        }
    }
}

/// The application id of the caller's unit, empty outside an application unit.
async fn app_id_of(conn: &Connection, name: BusName<'_>) -> String {
    let Ok(proxy) = DBusProxy::new(conn).await else {
        return String::new();
    };
    let Ok(pid) = proxy.get_connection_unix_process_id(name).await else {
        return String::new();
    };
    std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .map(|c| app_id_from_cgroup(&c))
        .unwrap_or_default()
}

/// `app-athanor-<escaped id>@<random>.service` (SD8) gives `<id>`; anything else, empty.
pub fn app_id_from_cgroup(cgroup: &str) -> String {
    let unit = cgroup
        .lines()
        .filter_map(|l| l.rsplit('/').next())
        .find(|u| u.starts_with("app-athanor-"))
        .unwrap_or_default();
    let Some(rest) = unit.strip_prefix("app-athanor-") else {
        return String::new();
    };
    let Some((escaped, _)) = rest.split_once('@') else {
        return String::new();
    };
    unescape(escaped)
}

/// systemd's `\xNN` escapes.
fn unescape(s: &str) -> String {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let byte = (b[i] == b'\\' && b.get(i + 1) == Some(&b'x'))
            .then(|| s.get(i + 2..i + 4))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match byte {
            Some(v) => {
                out.push(v);
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Restores the saved inhibitors whose owner is still on the bus, serves the interface on
/// both paths and owns the name (no `AllowReplacement`, no queue). Returns the channel that
/// carries what the table holds.
pub async fn serve(conn: &Connection, file: PathBuf) -> Result<watch::Receiver<Hold>> {
    let bus = DBusProxy::new(conn).await?;
    // Subscribed before the restore, so an owner that leaves in between is not missed.
    let mut left = bus.receive_name_owner_changed().await?;

    let mut table = Inhibitors::new();
    match store::load(&file) {
        Ok(entries) => {
            for (cookie, i) in entries {
                let present = match BusName::try_from(i.owner.as_str()) {
                    Ok(n) => bus.name_has_owner(n).await.unwrap_or(false),
                    Err(_) => false,
                };
                if present && table.restore(cookie, i.clone()) {
                    tracing::info!(cookie, app_id = i.app_id, "inhibitor restored");
                } else {
                    tracing::info!(cookie, owner = i.owner, "inhibitor dropped at start");
                }
            }
        }
        Err(e) => tracing::warn!("{} unusable, starting empty: {e}", file.display()),
    }
    let (hold, rx) = watch::channel(table.hold());
    let state = Arc::new(State {
        table: Mutex::new(table),
        file,
        hold,
    });
    state.changed(&state.lock());

    for path in PATHS {
        conn.object_server()
            .at(path, ScreenSaver(state.clone()))
            .await?;
    }
    conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
        .await?;

    tokio::spawn(async move {
        while let Some(signal) = left.next().await {
            let Ok(args) = signal.args() else { continue };
            if args.new_owner().is_none() && matches!(args.name(), BusName::Unique(_)) {
                let mut table = state.lock();
                for i in table.remove_owner(args.name().as_str()) {
                    tracing::info!(
                        app_id = i.app_id,
                        reason = i.reason,
                        "owner left, uninhibit"
                    );
                }
                state.changed(&table);
            }
        }
    });
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;
    use std::{env, fs};
    use zbus::connection::Builder;

    struct Bus {
        daemon: Child,
        address: String,
        dir: PathBuf,
    }

    impl Bus {
        fn start(tag: &str) -> Bus {
            let dir = env::temp_dir().join(format!("athanor-idle-{tag}-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
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
            .unwrap();
            let mut daemon = Command::new("dbus-daemon")
                .args(["--nofork", "--nopidfile", "--print-address"])
                .arg(format!("--config-file={}", config.display()))
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            Bus {
                daemon,
                address: address.trim().to_owned(),
                dir,
            }
        }

        async fn connect(&self) -> Connection {
            Builder::address(self.address.as_str())
                .unwrap()
                .build()
                .await
                .unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    async fn call(
        c: &Connection,
        path: &str,
        member: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> zbus::Result<zbus::Message> {
        c.call_method(Some(NAME), path, Some(NAME), member, body)
            .await
    }

    async fn settle(rx: &mut watch::Receiver<Hold>, want: Hold) {
        tokio::time::timeout(Duration::from_secs(10), rx.wait_for(|h| *h == want))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn inhibit_uninhibit_owner_rules_and_restart() {
        let bus = Bus::start("ss");
        let file = bus.dir.join("inhibitors.json");
        let server = bus.connect().await;
        let mut rx = serve(&server, file.clone()).await.unwrap();
        assert_eq!(*rx.borrow(), Hold::None);

        // A second instance cannot take the name.
        let other = bus.connect().await;
        assert!(serve(&other, bus.dir.join("other.json")).await.is_err());

        let a = bus.connect().await;
        let b = bus.connect().await;
        let reply = call(&a, PATHS[1], "Inhibit", &("app", "playing"))
            .await
            .unwrap();
        let cookie: u32 = reply.body().deserialize().unwrap();
        assert_eq!(cookie, 1);
        settle(&mut rx, Hold::All).await;
        assert!(fs::read_to_string(&file).unwrap().contains("playing"));

        // Another peer cannot end it; an unknown method is UnknownMethod.
        assert!(call(&b, PATHS[0], "UnInhibit", &(cookie,)).await.is_err());
        assert_eq!(*rx.borrow(), Hold::All);
        let err = call(&a, PATHS[0], "Throttle", &("x", "y"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("UnknownMethod"), "{err}");

        // The creator can, on either path; cookies are shared across both.
        let c2: u32 = call(&a, PATHS[0], "Inhibit", &("app", "again"))
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(c2, 2);
        call(&a, PATHS[1], "UnInhibit", &(cookie,)).await.unwrap();

        // A restart restores the live owner's inhibitor and continues above its cookie.
        server.close().await.unwrap();
        let server2 = bus.connect().await;
        let mut rx2 = serve(&server2, file.clone()).await.unwrap();
        assert_eq!(*rx2.borrow(), Hold::All);
        let c3: u32 = call(&b, PATHS[0], "Inhibit", &("app", "third"))
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(c3, 3);
        call(&b, PATHS[0], "UnInhibit", &(c3,)).await.unwrap();

        // The owner leaving the bus drops its inhibitors.
        a.close().await.unwrap();
        settle(&mut rx2, Hold::None).await;
    }

    #[test]
    fn the_application_id_comes_from_the_unit_name() {
        let cg = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-athanor-org.gnome.Totem@1a2b.service\n";
        assert_eq!(app_id_from_cgroup(cg), "org.gnome.Totem");
        let cg = "0::/app.slice/app-athanor-org.x.My\\x2dApp@9.service";
        assert_eq!(app_id_from_cgroup(cg), "org.x.My-App");
        assert_eq!(app_id_from_cgroup("0::/user.slice/session-3.scope"), "");
    }
}
