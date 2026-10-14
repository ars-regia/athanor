//! `athanor-update serve`: the D-Bus service `os.athanor.Update1` (UT6). Two acting methods
//! and one read method, no arguments, no property. The subject polkit judges is the bus
//! sender, never a PID; the caller's session, for the count of other sessions, comes from the
//! bus daemon's credentials for that sender, never from the caller. `State` asks no polkit: the state is not secret, and a reader in a
//! user namespace cannot check the file's owner itself, so the service checks it.
use crate::check::Context;
use crate::requests::{self, Caller, Power, RebootError, Refusal};
use crate::tools::System;
use athanor_bus_api::polkit::check_polkit_auth_zbus;
use athanor_trust_state::ReadError;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{interface, Connection};

pub const BUS_NAME: &str = "os.athanor.Update1";
pub const OBJECT_PATH: &str = "/os/athanor/Update1";
const ACTION_ROLLBACK: &str = "os.athanor.update.rollback";
/// The service is D-Bus activated: it leaves after this long without a request.
const IDLE: Duration = Duration::from_secs(60);

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "os.athanor.Update1.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotAuthorized(String),
    Busy(String),
    NothingDownloaded(String),
    NoPreviousVersion(String),
    Blocked(String),
    Failed(String),
    /// `State`: no state is published yet.
    NoState(String),
    /// `State`: the file is not a root-owned regular file others cannot write.
    Untrusted(String),
    /// `State`: the file could not be read or is not schema 1.
    Unreadable(String),
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::NotAuthorized => Self::NotAuthorized("not authorized".into()),
            Refusal::Busy => Self::Busy("a check or another request is running".into()),
            Refusal::NothingDownloaded => Self::NothingDownloaded("no update is downloaded; the timer downloads at its next run".into()),
            Refusal::NoPreviousVersion => Self::NoPreviousVersion("there is no previous version to go back to".into()),
            Refusal::Blocked => Self::Blocked("a restart is blocked by an inhibitor".into()),
            Refusal::Failed => Self::Failed("the request failed; the journal of athanor-update.service has the detail".into()),
        }
    }
}

const LOGIND: &str = "org.freedesktop.login1";

struct Logind<'a>(&'a Connection);

impl Logind<'_> {
    async fn call<B: serde::Serialize + zbus::zvariant::DynamicType>(&self, method: &str, body: &B) -> zbus::Result<zbus::Message> {
        self.0.call_method(Some(LOGIND), "/org/freedesktop/login1", Some("org.freedesktop.login1.Manager"), method, body).await
    }

    /// A property of one of logind's objects, read with `org.freedesktop.DBus.Properties.Get`.
    async fn property<T: TryFrom<OwnedValue, Error = zbus::zvariant::Error>>(&self, path: &ObjectPath<'_>, interface: &str, name: &str) -> zbus::Result<T> {
        let reply = self.0.call_method(Some(LOGIND), path, Some("org.freedesktop.DBus.Properties"), "Get", &(interface, name)).await?;
        Ok(T::try_from(reply.body().deserialize::<OwnedValue>()?)?)
    }
}

/// Session classes that hold nobody's work: they never count as another session (UT6,
/// ADR-0103 D22). Every other class counts, one this list does not name included.
const UNCOUNTED_CLASSES: [&str; 7] = ["greeter", "lock-screen", "background", "background-light", "manager", "manager-early", "none"];

/// One entry of `login1.Manager.ListSessions`: id, uid, user name, seat, object path.
type Session = (String, u32, String, String, OwnedObjectPath);

/// The caller's logind session: the session of the sender's PID as the bus daemon reports it
/// (`GetConnectionCredentials`), else the display session of the uid of the same credentials,
/// as polkit assigns one to a process of a systemd user unit. Never a value the caller supplies.
async fn caller_session(conn: &Connection, sender: &str) -> zbus::Result<OwnedObjectPath> {
    let name = zbus::names::BusName::try_from(sender)?;
    let credentials = zbus::fdo::DBusProxy::new(conn).await?.get_connection_credentials(name).await?;
    let logind = Logind(conn);
    // PID 0 would name the service's own process to logind.
    if let Some(pid) = credentials.process_id().filter(|pid| *pid != 0) {
        match logind.call("GetSessionByPID", &(pid,)).await.and_then(|reply| reply.body().deserialize::<OwnedObjectPath>()) {
            Ok(session) => return Ok(session),
            Err(err) => tracing::info!(%err, pid, "the caller's process has no session: its user's display session is used"),
        }
    }
    let uid = credentials.unix_user_id().ok_or_else(|| zbus::Error::Failure("the bus daemon reported no uid for the caller".into()))?;
    let user: OwnedObjectPath = logind.call("GetUser", &(uid,)).await?.body().deserialize()?;
    let (id, display): (String, OwnedObjectPath) = logind.property(&user, "org.freedesktop.login1.User", "Display").await?;
    if id.is_empty() {
        return Err(zbus::Error::Failure(format!("user {uid} has no display session")));
    }
    Ok(display)
}

/// Whether a counting session other than `caller` is open. Any error is the caller's to
/// read as "open": a class that cannot be read is never taken as one that does not count.
async fn others_open(conn: &Connection, caller: &ObjectPath<'_>) -> zbus::Result<bool> {
    let logind = Logind(conn);
    let sessions: Vec<Session> = logind.call("ListSessions", &()).await?.body().deserialize()?;
    for (_, _, _, _, path) in sessions.iter().filter(|session| session.4.as_str() != caller.as_str()) {
        let class: String = logind.property(path, "org.freedesktop.login1.Session", "Class").await?;
        if !UNCOUNTED_CLASSES.contains(&class.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The sender of `Apply()`, with its session found once when the request arrives.
struct Sender<'a> {
    conn: &'a Connection,
    name: String,
    session: Option<OwnedObjectPath>,
}

impl<'a> Sender<'a> {
    async fn new(conn: &'a Connection, name: String) -> Sender<'a> {
        let session = match caller_session(conn, &name).await {
            Ok(session) => Some(session),
            Err(err) => {
                tracing::warn!(%err, "the caller's session could not be determined: other sessions are assumed open");
                None
            }
        };
        Sender { conn, name, session }
    }
}

impl Caller for Sender<'_> {
    async fn others_open(&self) -> bool {
        let Some(session) = &self.session else {
            return true;
        };
        match others_open(self.conn, session).await {
            Ok(open) => open,
            Err(err) => {
                tracing::warn!(%err, "logind's sessions could not be counted: other sessions are assumed open");
                true
            }
        }
    }

    async fn authorize(&self, action: &str) -> Result<(), Refusal> {
        polkit(self.conn, &self.name, action).await
    }
}

/// One entry of `login1.Manager.ListInhibitors`: what, who, why, mode, uid, pid.
type Inhibitor = (String, String, String, String, u32, u32);

/// Whether the inhibitors logind listed block a shutdown. A list that could not be had
/// refuses the request as failed: it never goes ahead on a question logind did not answer,
/// and it never claims an inhibitor nobody saw (ADR-0103 D22).
fn blocks_shutdown(listed: zbus::Result<Vec<Inhibitor>>) -> Result<bool, Refusal> {
    match listed {
        Ok(inhibitors) => Ok(inhibitors.iter().any(|(what, _, _, mode, _, _)| mode == "block" && what.split(':').any(|kind| kind == "shutdown"))),
        Err(err) => {
            tracing::error!(%err, "logind's inhibitors could not be listed: the request is refused");
            Err(Refusal::Failed)
        }
    }
}

impl Power for Logind<'_> {
    async fn blocked(&self) -> Result<bool, Refusal> {
        blocks_shutdown(self.call("ListInhibitors", &()).await.and_then(|reply| reply.body().deserialize()))
    }

    async fn reboot(&self) -> Result<(), RebootError> {
        // Reboot(interactive = false). Never RebootWithFlags: flag 16 skips the inhibitors.
        match self.call("Reboot", &(false,)).await {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.freedesktop.login1.BlockedByInhibitorLock" => Err(RebootError::Blocked),
            Err(err) => {
                tracing::error!(%err, "logind refused the reboot");
                Err(RebootError::Failed)
            }
        }
    }
}

/// Counts the requests in flight, so the idle exit never cuts a polkit prompt short.
#[derive(Default)]
struct Activity {
    in_flight: AtomicUsize,
    last: Mutex<Option<Instant>>,
}

struct InFlight(Arc<Activity>);

impl Activity {
    fn enter(self: &Arc<Self>) -> InFlight {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        InFlight(Arc::clone(self))
    }

    fn idle_for(&self, started: Instant) -> Option<Duration> {
        let last = self.last.lock().map(|last| *last).unwrap_or(None);
        (self.in_flight.load(Ordering::SeqCst) == 0).then(|| last.unwrap_or(started).elapsed())
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Ok(mut last) = self.0.last.lock() {
            *last = Some(Instant::now());
        }
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Update1 {
    activity: Arc<Activity>,
}

/// The unique name the bus daemon stamped on the call: the subject polkit judges.
fn sender_of(header: &Header<'_>) -> Result<String, Error> {
    Ok(header.sender().ok_or_else(|| Error::NotAuthorized("the call has no sender".into()))?.to_string())
}

async fn polkit(conn: &Connection, sender: &str, action: &str) -> Result<(), Refusal> {
    match check_polkit_auth_zbus(conn, sender, action, true).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(Refusal::NotAuthorized),
        Err(err) => {
            tracing::error!(%err, action, "polkit could not be asked");
            Err(Refusal::NotAuthorized)
        }
    }
}

async fn uid_of(conn: &Connection, sender: &str) -> Option<u32> {
    let name = zbus::names::BusName::try_from(sender).ok()?;
    zbus::fdo::DBusProxy::new(conn).await.ok()?.get_connection_unix_user(name).await.ok()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}

/// The state file at `path`, checked as owned by `owner` (root in the service), returned as
/// the validated document serialised again: never bytes the reader has not parsed.
fn state_json(path: &Path, owner: u32) -> Result<String, Error> {
    let state = athanor_trust_state::read_owned_by(path, owner).map_err(|err| match err {
        ReadError::Missing => Error::NoState("no trust state is published yet".into()),
        ReadError::Untrusted => Error::Untrusted("the trust state file is not a root-owned regular file".into()),
        ReadError::Malformed | ReadError::Io(_) => {
            tracing::error!(?err, "the trust state file could not be read");
            Error::Unreadable("the trust state file could not be read".into())
        }
    })?;
    serde_json::to_string(&state).map_err(|err| Error::Unreadable(err.to_string()))
}

impl Update1 {
    /// `State()` on the file at `path` owned by `owner`: an activity, so the idle exit waits.
    fn state_at(&self, path: &Path, owner: u32) -> Result<String, Error> {
        let _in_flight = self.activity.enter();
        state_json(path, owner)
    }
}

#[interface(name = "os.athanor.Update1")]
impl Update1 {
    async fn apply(&self, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> Result<(), Error> {
        let _in_flight = self.activity.enter();
        let caller = Sender::new(conn, sender_of(&header)?).await;
        let (tools, store) = (System, crate::store::Store::system());
        let ctx = Context::system(&tools, &store, now());
        Ok(requests::apply(&ctx, &Logind(conn), &caller).await?)
    }

    async fn go_back(&self, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> Result<(), Error> {
        let _in_flight = self.activity.enter();
        let sender = sender_of(&header)?;
        polkit(conn, &sender, ACTION_ROLLBACK).await?;
        // UT6: the caller's uid at notice. It identifies who asked; polkit already decided.
        let uid = uid_of(conn, &sender).await;
        tracing::info!(?uid, "going back to the previous version was requested and authorized");
        let (tools, store) = (System, crate::store::Store::system());
        let ctx = Context::system(&tools, &store, now());
        Ok(requests::go_back(&ctx, &Logind(conn)).await?)
    }

    /// The published trust state (UT7) as JSON, for readers that cannot check its owner.
    async fn state(&self) -> Result<String, Error> {
        self.state_at(Path::new(athanor_trust_state::STATE_PATH), 0)
    }
}

/// Serves until idle, then releases the name and returns; the bus starts it again on demand.
///
/// # Errors
/// The name cannot be owned: the `system.d` policy file is missing, or another owner exists.
pub async fn run() -> zbus::Result<()> {
    let activity = Arc::new(Activity::default());
    let conn = zbus::connection::Builder::system()?.name(BUS_NAME)?.serve_at(OBJECT_PATH, Update1 { activity: Arc::clone(&activity) })?.build().await?;
    let started = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if activity.idle_for(started).is_some_and(|idle| idle >= IDLE) {
            conn.release_name(BUS_NAME).await?;
            // A call that raced the release is answered before leaving.
            if activity.idle_for(started).is_some() {
                return Ok(());
            }
            conn.request_name(BUS_NAME).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    use std::path::PathBuf;

    fn scratch(test: &str) -> (PathBuf, u32) {
        let dir = std::env::temp_dir().join(format!("athanor-update-serve-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let owner = std::fs::metadata(&dir).expect("meta").uid();
        (dir.join("state.json"), owner)
    }

    fn published(path: &Path) -> athanor_trust_state::State {
        let store = crate::store::Store { run: path.parent().expect("dir").into(), var: path.parent().expect("dir").join("var") };
        let state: athanor_trust_state::State = serde_json::from_str(include_str!("../tests/state-verified.json")).expect("fixture");
        store.publish(&state).expect("publish");
        state
    }

    #[test]
    fn a_missing_state_is_a_named_error() {
        let (path, owner) = scratch("missing");
        assert!(matches!(state_json(&path, owner), Err(Error::NoState(_))));
    }

    #[test]
    fn a_refused_state_is_a_named_error() {
        let (path, owner) = scratch("refused");
        published(&path);
        assert!(matches!(state_json(&path, owner.wrapping_add(1)), Err(Error::Untrusted(_))), "another owner");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).expect("chmod");
        assert!(matches!(state_json(&path, owner), Err(Error::Untrusted(_))), "writable by others");
    }

    #[test]
    fn an_unparsable_state_is_a_named_error() {
        let (path, owner) = scratch("malformed");
        std::fs::write(&path, "{\"schema\": 2}").expect("write");
        assert!(matches!(state_json(&path, owner), Err(Error::Unreadable(_))));
    }

    #[test]
    fn a_valid_state_is_returned_as_the_validated_document() {
        let (path, owner) = scratch("valid");
        let state = published(&path);
        let json = state_json(&path, owner).expect("state");
        assert_eq!(athanor_trust_state::parse(&json), Ok(state));
    }

    fn inhibitor(what: &str, mode: &str) -> Inhibitor {
        (what.into(), "who".into(), "why".into(), mode.into(), 1000, 4242)
    }

    #[test]
    fn a_failing_inhibitor_query_refuses_as_failed() {
        assert_eq!(blocks_shutdown(Err(zbus::Error::Failure("logind did not answer".into()))), Err(Refusal::Failed));
    }

    #[test]
    fn only_a_block_inhibitor_on_shutdown_blocks() {
        assert_eq!(blocks_shutdown(Ok(vec![])), Ok(false), "no inhibitor");
        assert_eq!(blocks_shutdown(Ok(vec![inhibitor("shutdown", "delay"), inhibitor("sleep:idle", "block")])), Ok(false), "delay, or not shutdown");
        assert_eq!(blocks_shutdown(Ok(vec![inhibitor("sleep:shutdown", "block")])), Ok(true), "shutdown among several kinds");
    }

    /// `login1.Manager.ListInhibitors` on a private bus, with a well-formed answer.
    struct Inhibits(Vec<Inhibitor>);

    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Inhibits {
        fn list_inhibitors(&self) -> Vec<Inhibitor> {
            self.0.clone()
        }
    }

    /// The same method answering with a body of the wrong signature.
    struct Garbled;

    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Garbled {
        fn list_inhibitors(&self) -> String {
            "not a list".into()
        }
    }

    async fn ask_logind<I: zbus::object_server::Interface>(name: &str, logind: Option<I>) -> Result<bool, Refusal> {
        let bus = crate::goback::tests::Bus::start(name);
        let mut service = None;
        if let Some(logind) = logind {
            let builder = bus.builder().name("org.freedesktop.login1").expect("name").serve_at("/org/freedesktop/login1", logind).expect("serve");
            service = Some(builder.build().await.expect("logind"));
        }
        let client = bus.builder().build().await.expect("client");
        let answer = Logind(&client).blocked().await;
        drop(service);
        answer
    }

    #[tokio::test]
    async fn logind_is_asked_and_its_answer_read() {
        assert_eq!(ask_logind("logind-block", Some(Inhibits(vec![inhibitor("shutdown", "block")]))).await, Ok(true));
        assert_eq!(ask_logind("logind-none", Some(Inhibits(vec![]))).await, Ok(false));
    }

    #[tokio::test]
    async fn an_unreadable_or_missing_logind_refuses_as_failed() {
        assert_eq!(ask_logind("logind-garbled", Some(Garbled)).await, Err(Refusal::Failed), "a reply of the wrong signature");
        assert_eq!(ask_logind::<Garbled>("logind-absent", None).await, Err(Refusal::Failed), "no logind on the bus");
    }

    /// logind on a private bus, as the count of other sessions reads it. Every connection of
    /// a test shares this process's PID, so `GetSessionByPID` answers for the caller.
    #[derive(Clone, Default)]
    struct Seats {
        /// Listed sessions: id and class; a class of `None` has no object to read it from.
        sessions: Vec<(&'static str, u32, Option<&'static str>)>,
        /// The session of the caller's process; `None` for a process of a systemd user unit.
        by_pid: Option<&'static str>,
        /// The display session of the caller's user.
        display: Option<&'static str>,
        list_fails: bool,
    }

    fn session_path(id: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(format!("/org/freedesktop/login1/session/s{id}")).expect("path")
    }

    const USER_PATH: &str = "/org/freedesktop/login1/user/u1000";

    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Seats {
        fn list_sessions(&self) -> zbus::fdo::Result<Vec<Session>> {
            if self.list_fails {
                return Err(zbus::fdo::Error::Failed("ListSessions failed".into()));
            }
            Ok(self.sessions.iter().map(|(id, uid, _)| ((*id).into(), *uid, format!("user{uid}"), "seat0".into(), session_path(id))).collect())
        }

        #[zbus(name = "GetSessionByPID")]
        fn get_session_by_pid(&self, pid: u32) -> zbus::fdo::Result<OwnedObjectPath> {
            match self.by_pid {
                Some(id) if pid == std::process::id() => Ok(session_path(id)),
                _ => Err(zbus::fdo::Error::Failed("no session for this PID".into())),
            }
        }

        fn get_user(&self, _uid: u32) -> OwnedObjectPath {
            OwnedObjectPath::try_from(USER_PATH).expect("path")
        }
    }

    struct SessionObject(&'static str);

    #[zbus::interface(name = "org.freedesktop.login1.Session")]
    impl SessionObject {
        #[zbus(property)]
        fn class(&self) -> String {
            self.0.into()
        }
    }

    struct UserObject(Option<&'static str>);

    #[zbus::interface(name = "org.freedesktop.login1.User")]
    impl UserObject {
        #[zbus(property)]
        fn display(&self) -> (String, OwnedObjectPath) {
            // logind's answer for a user without a display session: an empty id and "/".
            self.0.map_or_else(|| (String::new(), OwnedObjectPath::try_from("/").expect("path")), |id| (id.into(), session_path(id)))
        }
    }

    /// The first count of `Apply()` for a caller on `seats`, through the bus daemon's
    /// credentials of the caller's unique name, as the service makes it.
    async fn count(name: &str, seats: Seats) -> bool {
        let bus = crate::goback::tests::Bus::start(name);
        let mut builder = bus.builder().name(LOGIND).expect("name").serve_at("/org/freedesktop/login1", seats.clone()).expect("serve");
        for (id, _, class) in &seats.sessions {
            if let Some(class) = class {
                builder = builder.serve_at(session_path(id), SessionObject(class)).expect("session");
            }
        }
        let logind = builder.serve_at(USER_PATH, UserObject(seats.display)).expect("user").build().await.expect("logind");
        let client = bus.builder().build().await.expect("client");
        let sender = client.unique_name().expect("unique name").to_string();
        let open = Sender::new(&client, sender).await.others_open().await;
        drop(logind);
        open
    }

    fn alone() -> Seats {
        Seats { sessions: vec![("2", 1000, Some("user"))], by_pid: Some("2"), display: None, list_fails: false }
    }

    #[tokio::test]
    async fn the_caller_alone_counts_no_other_session() {
        assert!(!count("count-alone", alone()).await);
    }

    #[tokio::test]
    async fn a_user_unit_caller_resolves_to_its_display_session() {
        assert!(!count("count-user-unit", Seats { by_pid: None, display: Some("2"), ..alone() }).await);
    }

    #[tokio::test]
    async fn another_session_counts_whatever_its_uid() {
        let other_user = Seats { sessions: vec![("2", 1000, Some("user")), ("3", 1001, Some("user"))], ..alone() };
        assert!(count("count-other-user", other_user).await, "another user behind a fast user switch");
        let same_user = Seats { sessions: vec![("2", 1000, Some("user")), ("c1", 1000, Some("user"))], ..alone() };
        assert!(count("count-same-user", same_user).await, "a second session of the caller's own user");
    }

    #[tokio::test]
    async fn only_uncounted_classes_count_no_other_session() {
        let mut sessions = vec![("2", 1000, Some("user"))];
        sessions.extend(UNCOUNTED_CLASSES.iter().zip(["g", "l", "b", "bl", "m", "me", "n"]).map(|(class, id)| (id, 0, Some(*class))));
        assert!(!count("count-uncounted", Seats { sessions, ..alone() }).await);
    }

    #[tokio::test]
    async fn every_other_class_counts_an_unknown_one_included() {
        for class in ["user", "user-early", "user-incomplete", "user-light", "a-class-logind-adds-later"] {
            let seats = Seats { sessions: vec![("2", 1000, Some("user")), ("3", 1001, Some(class))], ..alone() };
            assert!(count(&format!("count-class-{class}"), seats).await, "{class}");
        }
    }

    #[tokio::test]
    async fn the_count_fails_closed() {
        assert!(count("count-list-fails", Seats { list_fails: true, ..alone() }).await, "ListSessions failing");
        assert!(count("count-unreadable", Seats { sessions: vec![("2", 1000, Some("user")), ("3", 1001, None)], ..alone() }).await, "a class that cannot be read");
        assert!(count("count-undetermined", Seats { by_pid: None, ..alone() }).await, "a caller without a session");
    }

    #[test]
    fn an_activity_is_busy_until_its_call_ends() {
        let activity = Arc::new(Activity::default());
        let started = Instant::now();
        {
            let _in_flight = activity.enter();
            assert_eq!(activity.idle_for(started), None, "busy while the call runs");
        }
        assert!(activity.idle_for(started).is_some_and(|idle| idle < IDLE), "idle again, counted from the call");
    }

    #[test]
    fn a_state_call_leaves_the_service_idle_afterwards() {
        let (path, owner) = scratch("activity");
        published(&path);
        let service = Update1 { activity: Arc::new(Activity::default()) };
        let long_ago = Instant::now().checked_sub(IDLE * 2).expect("the clock has run for two idle periods");
        assert!(service.activity.idle_for(long_ago).is_some_and(|idle| idle >= IDLE), "idle before the call");
        assert!(service.state_at(&path, owner).is_ok());
        assert!(service.activity.idle_for(long_ago).is_some_and(|idle| idle < IDLE), "idle again, counted from the call");
    }
}
