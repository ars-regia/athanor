//! logind, the lock's only D-Bus (doc_lock_and_prompts.md, LP2, LP3, LP4). A thread of its own
//! runs a current-thread Tokio runtime, as `athanor-idle` runs its own, and builds both of its
//! connections inside that runtime's `block_on`, so zbus runs on Tokio and starts no runtime of
//! its own (plan decision M3: the thread starts after Landlock).
//!
//! The thread finds the session through `Display` of `/org/freedesktop/login1/user/self` and
//! checks that its `Type` is `wayland` and its `User` this process's uid (LP2). It subscribes
//! to the session's `Lock` and `Unlock` and the Manager's `PrepareForSleep`, then reads the
//! session's `LockedHint` and reports it as [`Report::Ready`], for `Machine::start`. From then
//! on it hands each signal to the main loop as an [`Event`] and serves the main loop's
//! [`Call`]s: `SetLockedHint`, and the `sleep` inhibitor in `delay` mode of LP4.
//!
//! Each report is sent on a channel, then the main loop is woken by the `calloop::ping` of the
//! lock's event source. Neither channel carries any text. The thread ends on any error, and a
//! guard dropped as it ends sends `Fatal(LogindEnded)` whatever the cause, so the process exits
//! non-zero and the unit's restart brings the lock back (LP10).
//!
//! The signals are read on one connection and every call goes on another, which subscribes to
//! nothing: a reply awaited on a connection whose streams the same task drains can stall behind
//! them (issue #1035).

use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::time::Duration;

use calloop::ping::Ping;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use zbus::export::futures_core::Stream;
use zbus::export::ordered_stream::{self, OrderedStreamExt};
use zbus::proxy::CacheProperties;
use zbus::{Connection, Message, OwnedFd, OwnedObjectPath};

use crate::state::{Action, Event, Fatal};

/// The system bus socket, fixed: `Builder::system()` would honour `DBUS_SYSTEM_BUS_ADDRESS`,
/// which a process of the user can set for a unit through the user manager's environment, as
/// `athanor-unit`'s `bus_caller` says.
pub const SYSTEM_BUS: &str = "unix:path=/run/dbus/system_bus_socket";

/// How long a call to logind may take, as `athanor-idle` bounds its own.
pub const ASK: Duration = Duration::from_secs(2);

/// The inhibitor of LP4: what, who, why and mode.
pub const INHIBITOR: [&str; 4] = [
    "sleep",
    "athanor-lock",
    "Lock the screen before sleep",
    "delay",
];

/// What the logind thread tells the main loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    /// The session is ours and the subscriptions hold; its `LockedHint` as read after them.
    Ready { locked_hint: bool },
    /// A signal of logind, or the end of the thread.
    Event(Event),
}

/// What the main loop asks of logind: the machine's actions that need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    SetLockedHint(bool),
    TakeInhibitor,
    ReleaseInhibitor,
}

impl Call {
    /// The call that `action` asks for, if it is one of logind's.
    pub fn of(action: Action) -> Option<Call> {
        match action {
            Action::SetLockedHint(hint) => Some(Call::SetLockedHint(hint)),
            Action::TakeInhibitor => Some(Call::TakeInhibitor),
            Action::ReleaseInhibitor => Some(Call::ReleaseInhibitor),
            _ => None,
        }
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.User",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1/user/self"
)]
trait User {
    #[zbus(property)]
    fn display(&self) -> zbus::Result<(String, OwnedObjectPath)>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn user(&self) -> zbus::Result<(u32, OwnedObjectPath)>;
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
    fn set_locked_hint(&self, locked: bool) -> zbus::Result<()>;
}

/// Starts the logind thread on the bus at `address`, [`SYSTEM_BUS`] outside the tests. Every
/// report goes on `reports`, followed by `ping`. The main loop's calls go on the sender
/// returned; dropping it ends the thread.
pub fn spawn(
    address: &str,
    reports: Sender<Report>,
    ping: Ping,
) -> io::Result<UnboundedSender<Call>> {
    let (calls, called) = unbounded_channel();
    let address = address.to_owned();
    std::thread::Builder::new()
        .name("logind".into())
        .spawn(move || {
            let main = Main { reports, ping };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    return eprintln!("<3>athanor-lock: cannot start logind's runtime: {error}")
                }
            };
            match runtime.block_on(run(&address, &main, called)) {
                Ok(Some(fatal)) => {
                    main.send(Report::Event(Event::Fatal(fatal)));
                }
                Ok(None) => {}
                Err(error) => eprintln!("<3>athanor-lock: logind: {error}"),
            }
        })?;
    Ok(calls)
}

/// The main loop's side of the channel; dropped as the thread ends, it sends the fatal event.
struct Main {
    reports: Sender<Report>,
    ping: Ping,
}

impl Main {
    /// False once the main loop has gone.
    fn send(&self, report: Report) -> bool {
        let sent = self.reports.send(report).is_ok();
        self.ping.ping();
        sent
    }
}

impl Drop for Main {
    fn drop(&mut self) {
        // The main loop may have gone first, when nothing is left to tell.
        self.send(Report::Event(Event::Fatal(Fatal::LogindEnded)));
    }
}

/// The thread's work. `Some` is a fatal event of its own to send before the guard's; `None` is
/// the main loop gone.
async fn run(
    address: &str,
    main: &Main,
    mut called: UnboundedReceiver<Call>,
) -> zbus::Result<Option<Fatal>> {
    let Some(query) =
        connect(zbus::connection::Builder::address(address).method_timeout(ASK)).await
    else {
        return Ok(Some(Fatal::BusLost));
    };
    let Some(watch) = connect(zbus::connection::Builder::address(address)).await else {
        return Ok(Some(Fatal::BusLost));
    };

    let Some(path) = our_session(&query).await? else {
        return Ok(Some(Fatal::NotOurSession));
    };
    let manager = ManagerProxy::builder(&watch)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let watched = SessionProxy::builder(&watch)
        .path(path.clone())
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    // One stream in the order logind sent them: the machine's rules depend on it (a `Lock`
    // after an `Unlock` cancels it), and three streams polled side by side can swap two.
    let mut signals = ordered_stream::join(
        ordered_stream::join(
            watched.inner().receive_signal("Lock").await?,
            watched.inner().receive_signal("Unlock").await?,
        ),
        manager.inner().receive_signal("PrepareForSleep").await?,
    )
    .into_stream();

    let session = SessionProxy::builder(&query)
        .path(path)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let manager = ManagerProxy::builder(&query)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    if !main.send(Report::Ready {
        locked_hint: session.locked_hint().await?,
    }) {
        return Ok(None);
    }

    // Held for its descriptor: closing it releases the inhibitor.
    let mut inhibitor: Option<OwnedFd> = None;
    loop {
        let event = tokio::select! {
            signal = next(&mut signals) => match signal {
                Some(message) => Some(event(&message)?),
                None => None,
            },
            call = called.recv() => {
                match call {
                    Some(Call::SetLockedHint(hint)) => session.set_locked_hint(hint).await?,
                    Some(Call::TakeInhibitor) => {
                        let [what, who, why, mode] = INHIBITOR;
                        inhibitor.replace(manager.inhibit(what, who, why, mode).await?);
                    }
                    Some(Call::ReleaseInhibitor) => drop(inhibitor.take()),
                    None => return Ok(None),
                }
                continue;
            }
        };
        // A signal stream ends only with its connection.
        let Some(event) = event else {
            return Ok(Some(Fatal::BusLost));
        };
        if !main.send(Report::Event(event)) {
            return Ok(None);
        }
    }
}

/// A connection to the bus, or `None`, logged, when the bus cannot be reached.
async fn connect(builder: zbus::connection::Builder<'static>) -> Option<Connection> {
    match builder.build().await {
        Ok(connection) => Some(connection),
        Err(error) => {
            eprintln!("<3>athanor-lock: cannot connect to the system bus: {error}");
            None
        }
    }
}

/// The session of `Display`, if it is a Wayland session of this process's uid (LP2).
async fn our_session(query: &Connection) -> zbus::Result<Option<OwnedObjectPath>> {
    let user = UserProxy::builder(query)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let (id, path) = user.display().await?;
    if id.is_empty() {
        eprintln!("<3>athanor-lock: the user has no display session");
        return Ok(None);
    }
    let session = SessionProxy::builder(query)
        .path(path.clone())
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let kind = session.kind().await?;
    let (owner, _) = session.user().await?;
    // The uid the bus took from the socket: `/proc/self` belongs to root once the process is
    // not dumpable, as the lock makes itself before this thread starts.
    let me = query
        .unique_name()
        .ok_or_else(|| zbus::Error::Failure("the connection has no unique name".into()))?;
    let uid = zbus::fdo::DBusProxy::new(query)
        .await?
        .get_connection_unix_user(me.as_ref().into())
        .await?;
    if kind != "wayland" || owner != uid {
        eprintln!(
            "<3>athanor-lock: session {id} is of type {kind:?} and uid {owner}, not a Wayland session of uid {uid}"
        );
        return Ok(None);
    }
    Ok(Some(path))
}

/// The event of one of the three signals `run` subscribes to.
fn event(message: &Message) -> zbus::Result<Event> {
    let header = message.header();
    match header.member().map(|member| member.as_str()) {
        Some("Lock") => Ok(Event::Lock),
        Some("Unlock") => {
            eprintln!("<5>athanor-lock: logind's Unlock for the session");
            Ok(Event::Unlock)
        }
        Some("PrepareForSleep") => Ok(Event::PrepareForSleep(message.body().deserialize()?)),
        member => Err(zbus::Error::Failure(format!(
            "a signal not subscribed to: {member:?}"
        ))),
    }
}

async fn next<S: Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}
