//! `athanor-update serve`: the D-Bus service `os.athanor.Update1` (UT6). Two acting methods
//! and one read method, no arguments, no property. The subject polkit judges is the bus
//! sender, never a PID. `State` asks no polkit: the state is not secret, and a reader in a
//! user namespace cannot check the file's owner itself, so the service checks it.
use crate::check::Context;
use crate::requests::{self, Power, RebootError, Refusal};
use crate::tools::System;
use athanor_bus_api::polkit::check_polkit_auth_zbus;
use athanor_trust_state::ReadError;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::message::Header;
use zbus::{interface, Connection};

pub const BUS_NAME: &str = "os.athanor.Update1";
pub const OBJECT_PATH: &str = "/os/athanor/Update1";
const ACTION_APPLY: &str = "os.athanor.update.apply";
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
            Refusal::Busy => Self::Busy("a check or another request is running".into()),
            Refusal::NothingDownloaded => Self::NothingDownloaded("no update is downloaded; the timer downloads at its next run".into()),
            Refusal::NoPreviousVersion => Self::NoPreviousVersion("there is no previous version to go back to".into()),
            Refusal::Blocked => Self::Blocked("a restart is blocked by an inhibitor".into()),
            Refusal::Failed => Self::Failed("the request failed; the journal of athanor-update.service has the detail".into()),
        }
    }
}

struct Logind<'a>(&'a Connection);

impl Logind<'_> {
    async fn call<B: serde::Serialize + zbus::zvariant::DynamicType>(&self, method: &str, body: &B) -> zbus::Result<zbus::Message> {
        self.0.call_method(Some("org.freedesktop.login1"), "/org/freedesktop/login1", Some("org.freedesktop.login1.Manager"), method, body).await
    }
}

impl Power for Logind<'_> {
    async fn blocked(&self) -> bool {
        type Inhibitor = (String, String, String, String, u32, u32);
        let Ok(reply) = self.call("ListInhibitors", &()).await else { return false };
        let inhibitors: Vec<Inhibitor> = reply.body().deserialize().unwrap_or_default();
        inhibitors.iter().any(|(what, _, _, mode, _, _)| mode == "block" && what.split(':').any(|kind| kind == "shutdown"))
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

async fn authorize(conn: &Connection, header: &Header<'_>, action: &str) -> Result<String, Error> {
    let sender = header.sender().ok_or_else(|| Error::NotAuthorized("the call has no sender".into()))?.to_string();
    match check_polkit_auth_zbus(conn, &sender, action, true).await {
        Ok(true) => Ok(sender),
        Ok(false) => Err(Error::NotAuthorized("not authorized".into())),
        Err(err) => {
            tracing::error!(%err, action, "polkit could not be asked");
            Err(Error::NotAuthorized("polkit could not be asked".into()))
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
        authorize(conn, &header, ACTION_APPLY).await?;
        let (tools, store) = (System, crate::store::Store::system());
        let ctx = Context::system(&tools, &store, now());
        Ok(requests::apply(&ctx, &Logind(conn)).await?)
    }

    async fn go_back(&self, #[zbus(header)] header: Header<'_>, #[zbus(connection)] conn: &Connection) -> Result<(), Error> {
        let _in_flight = self.activity.enter();
        let sender = authorize(conn, &header, ACTION_ROLLBACK).await?;
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
