//! `athanor-update serve`: the D-Bus service `os.athanor.Update1` (UT6). Two methods, no
//! arguments, no property. The subject polkit judges is the bus sender, never a PID.
use crate::check::Context;
use crate::requests::{self, Power, RebootError, Refusal};
use crate::tools::System;
use athanor_bus_api::polkit::check_polkit_auth_zbus;
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
