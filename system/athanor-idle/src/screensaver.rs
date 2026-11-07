//! `org.freedesktop.ScreenSaver` and `os.athanor.Idle1` on the session bus (SD5), a thin layer
//! over the inhibitor table: the logic stays in [`Inhibitors`], and `inhibitors.json` in
//! [`crate::store`]. `os.athanor.Idle1.InhibitSuspend` is admitted only from the main process
//! of the portal backend's unit, as [`athanor_unit::bus_caller::admit`] checks it.
//!
//! Rule: no task that consumes the main connection's streams (the object server, the
//! `NameOwnerChanged` drain) ever awaits a reply on that connection, since zbus has one socket
//! reader that stops when a queue is full. Every outgoing call goes through the query
//! connection, which serves nothing, subscribes to nothing and owns no name, under a timeout.

use crate::inhibitors::{
    truncate, Inhibitor, Inhibitors, Kind, RemoveError, MAX_ENTRIES, MAX_PER_OWNER, MAX_TEXT,
};
use crate::store;
use crate::timers::Hold;
use athanor_unit::bus_caller::{self, OnBus, Questions, Service};
use futures_util::StreamExt;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;
use zbus::fdo::{self, DBusProxy, RequestNameFlags};
use zbus::message::Header;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::{Connection, Result};

pub const NAME: &str = "org.freedesktop.ScreenSaver";
/// The two object paths the name is served on, as cosmic-idle does.
pub const PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];
/// The name, path and interface of the suspend-only inhibitors.
pub const IDLE_NAME: &str = "os.athanor.Idle1";
pub const IDLE_PATH: &str = "/os/athanor/Idle1";
/// The one service whose main process may call `InhibitSuspend` (SD5, `doc_portal.md` PT11).
pub const PORTAL: Service<'static> = Service {
    unit: "xdg-desktop-portal-athanor.service",
    exec: "/usr/libexec/xdg-desktop-portal-athanor",
};
/// How long a question to the bus may take.
const ASK: Duration = Duration::from_secs(2);

/// The table, the file it is written to and the channel that tells the timers what it holds.
struct State {
    table: Mutex<Inhibitors>,
    file: PathBuf,
    hold: watch::Sender<Hold>,
    /// The query connection, and the bus as it sees it.
    query: Connection,
    bus: DBusProxy<'static>,
    /// A query connection to the system bus, where the user's systemd is verified.
    system: Connection,
    /// Told when the query connection is found closed: the daemon can no longer follow the bus.
    query_gone: Notify,
    /// Set, under the table's lock, when the bus is gone: nothing is added any more.
    closed: AtomicBool,
    /// Set once the name is owned: only that instance writes the file.
    owns_name: AtomicBool,
}

impl State {
    fn lock(&self) -> MutexGuard<'_, Inhibitors> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// After a change: tell the timers and rewrite the file. A write that fails is logged;
    /// the inhibitor still holds for as long as this process lives.
    fn changed(&self, table: &Inhibitors) {
        self.hold.send_replace(table.hold());
        // After the end the file keeps the table for the restart (SN7).
        if !self.owns_name.load(Ordering::SeqCst) || self.closed.load(Ordering::SeqCst) {
            return;
        }
        if let Err(e) = store::save(&self.file, table) {
            tracing::warn!("cannot write {}: {e}", self.file.display());
        }
    }

    /// `NameHasOwner` on the query connection.
    async fn has_owner(&self, owner: &str) -> std::result::Result<bool, String> {
        let name = BusName::try_from(owner).map_err(|e| e.to_string())?;
        self.ask(self.bus.name_has_owner(name)).await
    }

    /// A question to the bus, [`bounded`]; a failure with the query connection closed ends the
    /// daemon's view of the bus (fail closed).
    async fn ask<T, E: Display>(
        &self,
        call: impl Future<Output = std::result::Result<T, E>>,
    ) -> std::result::Result<T, String> {
        let answer = bounded(call).await;
        if answer.is_err() {
            self.check_query();
        }
        answer
    }

    /// The questions of [`athanor_unit::bus_caller`], on the query connection.
    fn questions(&self) -> OnBus<'_> {
        OnBus {
            bus: &self.bus,
            session: &self.query,
            system: &self.system,
        }
    }

    /// Adds `inhibitor` within the bounds and returns its cookie; the caller logs it.
    fn add(&self, inhibitor: Inhibitor) -> fdo::Result<u32> {
        let mut table = self.lock();
        if self.closed.load(Ordering::SeqCst) {
            return Err(fdo::Error::Failed("the bus connection ended".into()));
        }
        if table.count_owner(&inhibitor.owner) >= MAX_PER_OWNER || table.len() >= MAX_ENTRIES {
            return Err(fdo::Error::LimitsExceeded("too many inhibitors".into()));
        }
        let cookie = table
            .add(inhibitor)
            .ok_or_else(|| fdo::Error::Failed("no cookie left".into()))?;
        self.changed(&table);
        Ok(cookie)
    }

    /// Ends the inhibitor `cookie` of this kind, for its owner only.
    fn remove(&self, cookie: u32, kind: Kind, caller: &str) -> fdo::Result<()> {
        let mut table = self.lock();
        match table.remove(cookie, kind, caller) {
            Ok(i) => {
                tracing::info!(cookie, app_id = i.app_id, reason = i.reason, "uninhibit");
                self.changed(&table);
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

    fn check_query(&self) {
        if self.query.is_closed() {
            self.query_gone.notify_one();
        }
    }

    /// The bus is gone: nothing is added any more and the timers are free. In memory only.
    fn close(&self) {
        let mut table = self.lock();
        self.closed.store(true, Ordering::SeqCst);
        table.clear();
        self.hold.send_replace(table.hold());
    }
}

/// `call` under the 2 s bound of a question to the bus: an error and a timeout both come back
/// as text.
async fn bounded<T, E: Display>(
    call: impl Future<Output = std::result::Result<T, E>>,
) -> std::result::Result<T, String> {
    match tokio::time::timeout(ASK, call).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timed out".into()),
    }
}

/// SD5's admission of `InhibitSuspend`: `peer` is the main process of [`PORTAL`], every step
/// within one bound.
async fn admit_portal(questions: &impl Questions, peer: &str) -> std::result::Result<(), String> {
    bounded(bus_caller::admit(questions, peer, PORTAL)).await
}

struct ScreenSaver(Arc<State>);

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    async fn inhibit(
        &self,
        application_name: &str,
        reason: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<u32> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("no sender".into()))?;
        let owner = sender.to_string();
        let application_name = truncate(application_name, MAX_TEXT);
        let app_id = truncate(&app_id_of(&self.0, sender.as_str()).await, MAX_TEXT);
        let reason = truncate(reason, MAX_TEXT);
        let cookie = self.0.add(Inhibitor {
            kind: Kind::Screen,
            owner: owner.clone(),
            app_id: app_id.clone(),
            reason: reason.clone(),
        })?;
        tracing::info!(cookie, app_id, application_name, reason, "inhibit");
        // The caller may have left while the application id was read: its signal came
        // before the entry existed, so look again, at this owner only, now that it does.
        reconcile(&self.0, BTreeSet::from([owner])).await;
        Ok(cookie)
    }

    async fn un_inhibit(&self, cookie: u32, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("no sender".into()))?;
        self.0.remove(cookie, Kind::Screen, sender.as_str())
    }
}

struct Idle1(Arc<State>);

#[zbus::interface(name = "os.athanor.Idle1")]
impl Idle1 {
    /// A suspend-only inhibitor, admitted only from the portal backend (SD5); `app_id` is the
    /// portal's word for the application, recorded and never checked.
    async fn inhibit_suspend(
        &self,
        app_id: &str,
        reason: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<u32> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("no sender".into()))?;
        let owner = sender.to_string();
        if let Err(e) = admit_portal(&self.0.questions(), &owner).await {
            self.0.check_query();
            tracing::warn!(owner, "InhibitSuspend refused: {e}");
            return Err(fdo::Error::AccessDenied(
                "only the portal backend may inhibit suspend".into(),
            ));
        }
        let app_id = truncate(app_id, MAX_TEXT);
        let reason = truncate(reason, MAX_TEXT);
        let cookie = self.0.add(Inhibitor {
            kind: Kind::SuspendOnly,
            owner: owner.clone(),
            app_id: app_id.clone(),
            reason: reason.clone(),
        })?;
        tracing::info!(cookie, app_id, reason, "inhibit suspend");
        // As for `Inhibit`: the caller may have left while it was admitted.
        reconcile(&self.0, BTreeSet::from([owner])).await;
        Ok(cookie)
    }

    async fn uninhibit(&self, cookie: u32, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::Failed("no sender".into()))?;
        self.0.remove(cookie, Kind::SuspendOnly, sender.as_str())
    }
}

/// Drops the inhibitors of these owners when they are no longer on the bus. A failed question
/// is logged and keeps the inhibitor; the periodic reconciliation asks again.
async fn reconcile(state: &State, owners: BTreeSet<String>) {
    for owner in owners {
        match state.has_owner(&owner).await {
            Ok(false) => {
                let mut table = state.lock();
                for i in table.remove_owner(&owner) {
                    tracing::info!(
                        app_id = i.app_id,
                        reason = i.reason,
                        "owner gone, uninhibit"
                    );
                }
                state.changed(&table);
            }
            Ok(true) => {}
            Err(e) => tracing::warn!(owner, "NameHasOwner failed, inhibitor kept: {e}"),
        }
    }
}

fn all_owners(state: &State) -> BTreeSet<String> {
    state.lock().iter().map(|(_, i)| i.owner.clone()).collect()
}

/// A unique name as the bus hands them out, `:N.M`; a well-known name is never an owner.
fn is_unique_form(name: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    name.strip_prefix(':')
        .and_then(|r| r.split_once('.'))
        .is_some_and(|(a, b)| digits(a) && digits(b))
}

/// The application id of the caller's unit (SD8), empty outside an application unit. A failure
/// or a timeout of the questions is logged and records an empty id. The daemon reads nothing
/// under `/proc`, which its Landlock does not grant (SD17).
async fn app_id_of(state: &State, name: &str) -> String {
    match state
        .ask(bus_caller::unit_of(&state.questions(), name))
        .await
    {
        Ok(unit) => app_id_from_unit(&unit),
        Err(e) => {
            tracing::warn!("cannot tell the caller's unit; its application id is empty: {e}");
            String::new()
        }
    }
}

/// `app-athanor-<escaped id>@<random>.service` (SD8) gives `<id>`; any other unit, empty.
pub fn app_id_from_unit(unit: &str) -> String {
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

/// Restores the saved inhibitors whose owner is still on the bus, serves the ScreenSaver
/// interface on both paths and `os.athanor.Idle1` on its own, and owns both names (no
/// `AllowReplacement`, no queue). `query` is a second
/// connection to the same bus, opened apart (never a clone of `conn`, which would share its
/// socket reader), with no objects, no signal subscriptions and no names, used only for
/// questions (see the module rule); `serve` refuses a clone. The daemon ends, as for the main
/// stream, when `query` is closed. Returns the
/// channel that carries what the table holds, and a handle that completes when the daemon can
/// no longer follow the bus (the `NameOwnerChanged` stream ended): the table is then cleared,
/// so the timers are free, and the caller is expected to exit and let the manager restart it
/// (ST5).
pub async fn serve(
    conn: &Connection,
    query: &Connection,
    system: &Connection,
    file: PathBuf,
) -> Result<(watch::Receiver<Hold>, JoinHandle<()>)> {
    if query.unique_name() == conn.unique_name() {
        return Err(zbus::Error::Failure(
            "the query connection must be opened apart from the main one".into(),
        ));
    }
    let bus = DBusProxy::builder(query)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let (hold, rx) = watch::channel(Hold::None);
    let state = Arc::new(State {
        table: Mutex::new(Inhibitors::new()),
        file,
        hold,
        query: query.clone(),
        bus,
        system: system.clone(),
        query_gone: Notify::new(),
        closed: AtomicBool::new(false),
        owns_name: AtomicBool::new(false),
    });

    // The drain starts before anything else, and does nothing but remove owners.
    let mut left = DBusProxy::new(conn)
        .await?
        .receive_name_owner_changed()
        .await?;
    let again = Arc::new(Notify::new());
    // The reconciler: every 60 s and when the drain asks.
    let reconciler = {
        let (state, again) = (state.clone(), again.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = again.notified() => {}
                }
                state.check_query();
                reconcile(&state, all_owners(&state)).await;
            }
        })
    };
    let stop_reconciler = reconciler.abort_handle();
    let ended = {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                let signal = tokio::select! {
                    s = left.next() => match s {
                        Some(s) => s,
                        None => break,
                    },
                    _ = state.query_gone.notified() => {
                        tracing::error!("the query connection ended");
                        break;
                    }
                };
                match signal.args() {
                    Ok(args) => {
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
                    Err(e) => {
                        tracing::warn!("bad NameOwnerChanged: {e}");
                        again.notify_one();
                    }
                }
            }
            reconciler.abort();
            tracing::error!(
                "the bus is no longer followed: inhibitors cleared, the daemon must exit"
            );
            state.close();
        })
    };
    let stop_drain = ended.abort_handle();

    let started = async {
        match store::load(&state.file) {
            Ok(entries) => {
                for (cookie, i) in entries {
                    if !is_unique_form(&i.owner) {
                        tracing::warn!(
                            cookie,
                            owner = i.owner,
                            "owner is not a unique name, dropped"
                        );
                        continue;
                    }
                    // An answer that does not come drops the entry: toward locking.
                    let present = state.has_owner(&i.owner).await == Ok(true);
                    // A suspend inhibitor is admitted again, as InhibitSuspend admits it.
                    if present && i.kind == Kind::SuspendOnly {
                        if let Err(e) = admit_portal(&state.questions(), &i.owner).await {
                            state.check_query();
                            tracing::warn!(
                                cookie,
                                owner = i.owner,
                                "suspend inhibitor refused at start: {e}"
                            );
                            continue;
                        }
                    }
                    if present && state.lock().restore(cookie, i.clone()) {
                        tracing::info!(cookie, app_id = i.app_id, "inhibitor restored");
                    } else {
                        tracing::info!(cookie, owner = i.owner, "inhibitor dropped at start");
                    }
                }
            }
            Err(e) => tracing::warn!("{} unusable, starting empty: {e}", state.file.display()),
        }
        for path in PATHS {
            conn.object_server()
                .at(path, ScreenSaver(state.clone()))
                .await?;
        }
        conn.object_server()
            .at(IDLE_PATH, Idle1(state.clone()))
            .await?;
        conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
            .await?;
        // Without its name only InhibitSuspend is missing: the rest of the daemon goes on.
        if let Err(e) = conn
            .request_name_with_flags(IDLE_NAME, RequestNameFlags::DoNotQueue.into())
            .await
        {
            tracing::error!("{IDLE_NAME} not owned, InhibitSuspend is unavailable: {e}");
            conn.object_server().remove::<Idle1, _>(IDLE_PATH).await?;
        }
        Ok::<(), zbus::Error>(())
    }
    .await;
    if let Err(e) = started {
        stop_drain.abort();
        stop_reconciler.abort();
        return Err(e);
    }
    // Only the instance that owns the name writes the file. An owner that left while the
    // restore ran was not yet in the table when its signal came: look at all of them now.
    state.owns_name.store(true, Ordering::SeqCst);
    reconcile(&state, all_owners(&state)).await;
    state.changed(&state.lock());
    Ok((rx, ended))
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
        let (mut rx, _end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
        assert_eq!(*rx.borrow(), Hold::None);

        // A second instance cannot take the name.
        let other = bus.connect().await;
        assert!(serve(
            &other,
            &bus.connect().await,
            &bus.connect().await,
            bus.dir.join("other.json")
        )
        .await
        .is_err());

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
        let (mut rx2, _end2) = serve(
            &server2,
            &bus.connect().await,
            &bus.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
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

    #[tokio::test]
    async fn a_caller_that_leaves_at_once_leaves_no_orphan() {
        let bus = Bus::start("orphan");
        let file = bus.dir.join("inhibitors.json");
        let server = bus.connect().await;
        let (rx, _end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
        for _ in 0..5 {
            let c = bus.connect().await;
            let msg = zbus::Message::method_call(PATHS[0], "Inhibit")
                .unwrap()
                .destination(NAME)
                .unwrap()
                .interface(NAME)
                .unwrap()
                .with_flags(zbus::message::Flags::NoReplyExpected)
                .unwrap()
                .build(&("app", "fire and forget"))
                .unwrap();
            c.send(&msg).await.unwrap();
            c.close().await.unwrap();
        }
        // Handlers run in tasks: give them time, then nothing may hold.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(*rx.borrow(), Hold::None);
        assert!(!fs::read_to_string(&file)
            .unwrap()
            .contains("fire and forget"));
    }

    #[tokio::test]
    async fn a_flood_of_owner_changes_does_not_stall_the_daemon() {
        let bus = Bus::start("flood");
        let server = bus.connect().await;
        let (rx, _end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            bus.dir.join("inhibitors.json"),
        )
        .await
        .unwrap();
        let a = bus.connect().await;
        call(&a, PATHS[0], "Inhibit", &("app", "r")).await.unwrap();
        // Well over the 64 signals a stream queues; the table is reconciled meanwhile.
        for _ in 0..150 {
            bus.connect().await.close().await.unwrap();
        }
        let reply = tokio::time::timeout(
            Duration::from_secs(10),
            call(&a, PATHS[1], "Inhibit", &("app", "after")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(reply.body().deserialize::<u32>().unwrap(), 2);
        assert_eq!(*rx.borrow(), Hold::All);
    }

    #[tokio::test]
    async fn the_end_of_the_bus_clears_the_table_and_is_reported() {
        let bus = Bus::start("end");
        let file = bus.dir.join("inhibitors.json");
        let server = bus.connect().await;
        let (mut rx, end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
        let a = bus.connect().await;
        call(&a, PATHS[0], "Inhibit", &("app", "r")).await.unwrap();
        settle(&mut rx, Hold::All).await;
        server.close().await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), end)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*rx.borrow(), Hold::None);
        // The file still holds the table for the restart.
        assert!(fs::read_to_string(&file).unwrap().contains("\"r\""));
    }

    fn inhibit_message() -> zbus::Message {
        zbus::Message::method_call(PATHS[0], "Inhibit")
            .unwrap()
            .destination(NAME)
            .unwrap()
            .interface(NAME)
            .unwrap()
            .with_flags(zbus::message::Flags::NoReplyExpected)
            .unwrap()
            .build(&("app", "burst"))
            .unwrap()
    }

    #[tokio::test]
    async fn a_burst_of_calls_does_not_stall_the_daemon() {
        let bus = Bus::start("burst");
        let server = bus.connect().await;
        let (mut rx, _end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            bus.dir.join("i.json"),
        )
        .await
        .unwrap();
        let a = bus.connect().await;
        for _ in 0..100 {
            a.send(&inhibit_message()).await.unwrap();
        }
        let b = bus.connect().await;
        tokio::time::timeout(
            Duration::from_secs(10),
            call(&b, PATHS[1], "Inhibit", &("app", "after")),
        )
        .await
        .unwrap()
        .unwrap();
        // The drain still runs: the burst's owner leaving frees what it held.
        a.close().await.unwrap();
        b.close().await.unwrap();
        settle(&mut rx, Hold::None).await;
    }

    #[tokio::test]
    async fn a_closed_query_connection_ends_the_daemon_view_of_the_bus() {
        let bus = Bus::start("qend");
        let server = bus.connect().await;
        let q = bus.connect().await;
        let (mut rx, end) = serve(&server, &q, &bus.connect().await, bus.dir.join("i.json"))
            .await
            .unwrap();
        let a = bus.connect().await;
        call(&a, PATHS[0], "Inhibit", &("app", "r")).await.unwrap();
        settle(&mut rx, Hold::All).await;
        q.close().await.unwrap();
        // The next question fails and finds the connection closed.
        let _ = call(&a, PATHS[0], "Inhibit", &("app", "again")).await;
        tokio::time::timeout(Duration::from_secs(10), end)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*rx.borrow(), Hold::None);
        // Nothing is added after the end.
        assert!(call(&a, PATHS[0], "Inhibit", &("app", "late"))
            .await
            .is_err());
        assert_eq!(*rx.borrow(), Hold::None);
    }

    #[tokio::test]
    async fn a_clone_of_the_main_connection_is_refused_as_the_query() {
        let bus = Bus::start("clone");
        let server = bus.connect().await;
        let err = serve(
            &server,
            &server.clone(),
            &bus.connect().await,
            bus.dir.join("i.json"),
        )
        .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn bounds_and_a_forged_well_known_owner() {
        let bus = Bus::start("bounds");
        let file = bus.dir.join("inhibitors.json");
        fs::write(
            &file,
            br#"{"version":1,"inhibitors":[{"cookie":7,"owner":"org.freedesktop.DBus","app_id":"x","kind":"idle","reason":"forged"}]}"#,
        )
        .unwrap();
        let server = bus.connect().await;
        let (rx, _end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
        assert_eq!(*rx.borrow(), Hold::None);
        assert!(!fs::read_to_string(&file).unwrap().contains("forged"));

        let a = bus.connect().await;
        for _ in 0..MAX_PER_OWNER {
            call(&a, PATHS[0], "Inhibit", &("app", "r")).await.unwrap();
        }
        let err = call(&a, PATHS[0], "Inhibit", &("app", "r"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("LimitsExceeded"), "{err}");
    }

    /// A bus that never answers: the admission ends at the 2 s bound, refused.
    struct Silent;

    impl Questions for Silent {
        async fn user_manager(&self) -> std::result::Result<String, String> {
            Err("not asked".into())
        }

        async fn main_pid(
            &self,
            _manager: bus_caller::Manager<'_>,
            _path: &zbus::zvariant::OwnedObjectPath,
        ) -> std::result::Result<u32, String> {
            Err("not asked".into())
        }

        async fn credentials(
            &self,
            _peer: &str,
        ) -> std::result::Result<bus_caller::Credentials, String> {
            std::future::pending().await
        }

        async fn unit_by_pidfd(
            &self,
            _manager: bus_caller::Manager<'_>,
            _pidfd: std::os::fd::BorrowedFd<'_>,
        ) -> std::result::Result<(zbus::zvariant::OwnedObjectPath, String), String> {
            Err("not asked".into())
        }

        async fn unit(
            &self,
            _manager: bus_caller::Manager<'_>,
            _path: &zbus::zvariant::OwnedObjectPath,
        ) -> std::result::Result<bus_caller::Unit, String> {
            Err("not asked".into())
        }
    }

    #[tokio::test]
    async fn an_admission_that_does_not_answer_is_refused_at_the_bound() {
        let answer = tokio::time::timeout(ASK * 2, admit_portal(&Silent, ":1.9"))
            .await
            .expect("the admission is bounded");
        assert_eq!(answer, Err("timed out".into()));
    }

    /// A systemd's manager that places every pidfd in `unit`, at `path`.
    struct FakeSystemd {
        unit: String,
        path: &'static str,
    }

    #[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
    impl FakeSystemd {
        #[zbus(name = "GetUnitByPIDFD")]
        fn get_unit_by_pidfd(
            &self,
            _pidfd: zbus::zvariant::Fd<'_>,
        ) -> (zbus::zvariant::OwnedObjectPath, String, Vec<u8>) {
            (
                zbus::zvariant::OwnedObjectPath::try_from(self.path).unwrap(),
                self.unit.clone(),
                Vec::new(),
            )
        }
    }

    const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/portal";
    const USER_PATH: &str = "/org/freedesktop/systemd1/unit/user";

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
        fn exec_start(
            &self,
        ) -> Vec<(String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32)> {
            vec![(
                PORTAL.exec.into(),
                vec![PORTAL.exec.into()],
                false,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            )]
        }
    }

    struct FakeUnit;

    #[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
    impl FakeUnit {
        #[zbus(property, name = "FragmentPath")]
        fn fragment_path(&self) -> String {
            "/usr/lib/systemd/user/xdg-desktop-portal-athanor.service".into()
        }

        #[zbus(property, name = "DropInPaths")]
        fn drop_in_paths(&self) -> Vec<String> {
            vec!["/usr/lib/systemd/user/service.d/10-timeout-abort.conf".into()]
        }
    }

    /// The user's systemd on `bus`: it places every caller in the portal's unit, whose main
    /// process is `pid`. This process owns it, so this process is its main process too.
    async fn fake_systemd(bus: &Bus, pid: u32) -> Connection {
        let c = bus.connect().await;
        let manager = FakeSystemd {
            unit: PORTAL.unit.into(),
            path: UNIT_PATH,
        };
        c.object_server()
            .at("/org/freedesktop/systemd1", manager)
            .await
            .unwrap();
        c.object_server()
            .at(UNIT_PATH, FakeService { pid })
            .await
            .unwrap();
        c.object_server().at(UNIT_PATH, FakeUnit).await.unwrap();
        c.request_name("org.freedesktop.systemd1").await.unwrap();
        c
    }

    /// The system's systemd on `bus`, a stand-in for the system bus: it places every pidfd in
    /// `unit`, whose main process is `pid`.
    async fn fake_system(bus: &Bus, unit: &str, pid: u32) -> Connection {
        let c = bus.connect().await;
        let manager = FakeSystemd {
            unit: unit.into(),
            path: USER_PATH,
        };
        c.object_server()
            .at("/org/freedesktop/systemd1", manager)
            .await
            .unwrap();
        c.object_server()
            .at(USER_PATH, FakeService { pid })
            .await
            .unwrap();
        c.request_name("org.freedesktop.systemd1").await.unwrap();
        c
    }

    /// `user@<uid>.service` of this process's user, who owns `bus`'s directory.
    fn user_service(bus: &Bus) -> String {
        format!("user@{}.service", uid(bus))
    }

    fn uid(bus: &Bus) -> u32 {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(&bus.dir).unwrap().uid()
    }

    async fn idle(
        c: &Connection,
        member: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> zbus::Result<zbus::Message> {
        c.call_method(Some(IDLE_NAME), IDLE_PATH, Some(IDLE_NAME), member, body)
            .await
    }

    fn denied(answer: zbus::Result<zbus::Message>) {
        let err = answer.unwrap_err();
        assert!(err.to_string().contains("AccessDenied"), "{err}");
    }

    #[tokio::test]
    async fn inhibit_suspend_is_refused_to_a_peer_that_is_not_the_portal() {
        let bus = Bus::start("refused");
        let system = Bus::start("refused-system");
        let file = bus.dir.join("inhibitors.json");
        let server = bus.connect().await;
        let (rx, _end) = serve(
            &server,
            &bus.connect().await,
            &system.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
        let a = bus.connect().await;
        // No systemd on either bus: the unit cannot be told, and the call is refused.
        denied(idle(&a, "InhibitSuspend", &("org.example.App", "r")).await);
        // A verified user's systemd that names the portal's unit with another main process.
        let _system = fake_system(&system, &user_service(&bus), std::process::id()).await;
        let _systemd = fake_systemd(&bus, std::process::id() + 1).await;
        denied(idle(&a, "InhibitSuspend", &("org.example.App", "r")).await);
        assert_eq!(*rx.borrow(), Hold::None);
        // Idle1's Uninhibit does not end a ScreenSaver inhibitor.
        let c: u32 = call(&a, PATHS[0], "Inhibit", &("app", "r"))
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        let err = idle(&a, "Uninhibit", &(c,)).await.unwrap_err();
        assert!(err.to_string().contains("InvalidArgs"), "{err}");
        assert_eq!(*rx.borrow(), Hold::All);
    }

    /// A process of the user that owns `org.freedesktop.systemd1` on the session bus, and
    /// names the portal's unit, is refused unless the system's systemd vouches for it as the
    /// main process of `user@<uid>.service`.
    #[tokio::test]
    async fn a_user_systemd_the_system_does_not_vouch_for_is_refused() {
        let pid = std::process::id();
        for n in 0..3 {
            let bus = Bus::start(&format!("vouch{n}"));
            let system = Bus::start(&format!("vouch{n}-system"));
            let (unit, main_pid) = match n {
                0 => ("app-athanor-org.example.Fake@1.service".to_owned(), pid),
                1 => (user_service(&bus), pid + 1),
                _ => (format!("user@{}.service", uid(&bus) + 1), pid),
            };
            let _systemd = fake_systemd(&bus, pid).await;
            let _system = fake_system(&system, &unit, main_pid).await;
            let server = bus.connect().await;
            let (rx, _end) = serve(
                &server,
                &bus.connect().await,
                &system.connect().await,
                bus.dir.join("inhibitors.json"),
            )
            .await
            .unwrap();
            let portal = bus.connect().await;
            denied(idle(&portal, "InhibitSuspend", &("org.example.Player", "x")).await);
            assert_eq!(*rx.borrow(), Hold::None, "{unit} {main_pid}");
        }
    }

    #[tokio::test]
    async fn the_portal_s_suspend_inhibitor_keeps_the_blank_and_is_kept() {
        let bus = Bus::start("suspend");
        let system = Bus::start("suspend-system");
        let file = bus.dir.join("inhibitors.json");
        // This process is the portal's main process, as the fake systemd tells it, and the
        // user's systemd's, as the fake system's tells it.
        let _systemd = fake_systemd(&bus, std::process::id()).await;
        let _system = fake_system(&system, &user_service(&bus), std::process::id()).await;
        let server = bus.connect().await;
        let (mut rx, _end) = serve(
            &server,
            &bus.connect().await,
            &system.connect().await,
            file.clone(),
        )
        .await
        .unwrap();
        let portal = bus.connect().await;
        let cookie: u32 = idle(
            &portal,
            "InhibitSuspend",
            &("org.example.Player", "playing"),
        )
        .await
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
        settle(&mut rx, Hold::SuspendOnly).await;
        let saved = fs::read_to_string(&file).unwrap();
        assert!(
            saved.contains(r#""kind":"suspend""#) && saved.contains("org.example.Player"),
            "{saved}"
        );
        // Only its owner ends it, and not through the ScreenSaver interface.
        let other = bus.connect().await;
        assert!(idle(&other, "Uninhibit", &(cookie,)).await.is_err());
        assert!(call(&portal, PATHS[0], "UnInhibit", &(cookie,))
            .await
            .is_err());
        assert_eq!(*rx.borrow(), Hold::SuspendOnly);
        idle(&portal, "Uninhibit", &(cookie,)).await.unwrap();
        settle(&mut rx, Hold::None).await;
        // Its owner leaving the bus ends it.
        idle(&portal, "InhibitSuspend", &("org.example.Player", "again"))
            .await
            .unwrap();
        settle(&mut rx, Hold::SuspendOnly).await;
        portal.close().await.unwrap();
        settle(&mut rx, Hold::None).await;
    }

    /// A suspend inhibitor in the file is admitted again at start: refused, it is dropped.
    #[tokio::test]
    async fn a_restored_suspend_inhibitor_is_admitted_again() {
        for admitted in [false, true] {
            let tag = format!("restore-{admitted}");
            let bus = Bus::start(&tag);
            let system = Bus::start(&format!("{tag}-system"));
            let pid = std::process::id();
            let _systemd = fake_systemd(&bus, pid).await;
            // Without the system's word the user's systemd is not believed.
            let _system = match admitted {
                true => Some(fake_system(&system, &user_service(&bus), pid).await),
                false => None,
            };
            let owner = bus.connect().await;
            let file = bus.dir.join("inhibitors.json");
            fs::write(
                &file,
                format!(
                    r#"{{"version":1,"inhibitors":[{{"cookie":7,"owner":"{}","app_id":"org.example.Player","kind":"suspend","reason":"playing"}}]}}"#,
                    owner.unique_name().unwrap()
                ),
            )
            .unwrap();
            let server = bus.connect().await;
            let (rx, _end) = serve(
                &server,
                &bus.connect().await,
                &system.connect().await,
                file.clone(),
            )
            .await
            .unwrap();
            let want = match admitted {
                true => Hold::SuspendOnly,
                false => Hold::None,
            };
            assert_eq!(*rx.borrow(), want);
            let saved = fs::read_to_string(&file).unwrap();
            assert_eq!(saved.contains("playing"), admitted, "{saved}");
        }
    }

    /// Another process holds `os.athanor.Idle1`: the daemon serves the rest, without it.
    #[tokio::test]
    async fn without_its_name_idle1_is_missing_and_the_rest_runs() {
        let bus = Bus::start("idle-name");
        let squatter = bus.connect().await;
        squatter.request_name(IDLE_NAME).await.unwrap();
        let server = bus.connect().await;
        let (mut rx, _end) = serve(
            &server,
            &bus.connect().await,
            &bus.connect().await,
            bus.dir.join("inhibitors.json"),
        )
        .await
        .unwrap();
        let a = bus.connect().await;
        call(&a, PATHS[0], "Inhibit", &("app", "r")).await.unwrap();
        settle(&mut rx, Hold::All).await;
        // Its object is gone from the daemon's connection too.
        let err = a
            .call_method(
                server.unique_name().map(|n| n.as_str()),
                IDLE_PATH,
                Some(IDLE_NAME),
                "InhibitSuspend",
                &("org.example.App", "r"),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("UnknownObject"), "{err}");
    }

    #[test]
    fn only_dotted_numeric_unique_names_are_owners() {
        assert!(is_unique_form(":1.5"));
        for bad in ["org.freedesktop.DBus", ":1", ":a.b", ":1.", "", ":.1"] {
            assert!(!is_unique_form(bad), "{bad}");
        }
    }

    #[test]
    fn the_application_id_comes_from_the_unit_name() {
        assert_eq!(
            app_id_from_unit("app-athanor-org.gnome.Totem@1a2b.service"),
            "org.gnome.Totem"
        );
        assert_eq!(
            app_id_from_unit("app-athanor-org.x.My\\x2dApp@9.service"),
            "org.x.My-App"
        );
        assert_eq!(app_id_from_unit("session-3.scope"), "");
        assert_eq!(app_id_from_unit("app-athanor-no-instance.service"), "");
    }
}
