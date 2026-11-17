//! The RemoteDesktop portal over libei (doc_portal.md PT15, amended on 2026-11-17), with its
//! Request and Session objects (PT4) and the `InputSessions` and `StopCapture` part of
//! `os.athanor.Portal1` (PT10).
//!
//! Input reaches the compositor only through EIS: `Start` asks the user through the dialog of
//! [`Host::consent`], after the owner of `os.athanor.Eis` is checked to be the compositor, and
//! the impl's `ConnectToEIS` then obtains one file descriptor, with the granted types only. Every
//! `Notify*` method is refused. When the session ends, from `Close`, from the bar's Stop or from
//! the backend, the compositor is asked to close its context; a Stop whose `CloseSession` fails
//! is answered with the error and leaves the session open and listed. A frontend that vanishes
//! without `Close` leaves the session open and listed until Stop or the backend's own end.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::watch;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::{ObjectPath, OwnedFd};
use zbus::{OwnedVariant as OwnedValue, Variant as Value};

use crate::eis::{Eis, Identity};
use crate::remote_desktop::{self, display_name, holds_context, Answer, Sessions, Stage, Ticket};

/// The private interface's name and object (PT10).
pub const PORTAL1_NAME: &str = "os.athanor.Portal";
pub const PORTAL1_PATH: &str = "/os/athanor/Portal1";

/// The impl interface's version: 2, the version that has `ConnectToEIS`.
const VERSION: u32 = 2;

/// What the consent dialog shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub app_id: String,
    /// The name of PT4: the desktop file's, the id, or "An application".
    pub name: String,
    /// `wayland:<handle>`, or anything else for an unparented dialog (PT4).
    pub parent_window: String,
    /// The device types the session asked for, never 0.
    pub types: u32,
}

/// What the service needs of the process that serves it.
pub trait Host: Send + Sync + 'static {
    type Identity: Identity + Send + Sync + 'static;
    /// The frontend-only rule of PT3: `Ok` only for a call from the owner of
    /// `org.freedesktop.portal.Desktop`.
    fn frontend(&self, header: &Header<'_>) -> impl Future<Output = Result<(), String>> + Send;
    /// The admission of PT10 for `InputSessions` and `StopCapture`: `Ok` only for the bar.
    fn bar(&self, header: &Header<'_>) -> impl Future<Output = Result<(), String>> + Send;
    /// The `Name` of `<app_id>.desktop`, if it has one.
    fn app_name(&self, app_id: &str) -> Option<String>;
    /// The consent dialog. Dropping the future dismisses it.
    fn consent(&self, ask: Ask) -> impl Future<Output = Answer> + Send;
}

struct Shared<H: Host> {
    host: H,
    eis: Eis<H::Identity>,
    sessions: Mutex<Sessions>,
    /// Set to `true` to dismiss the session's dialog: by the request's `Close`, or by the end
    /// of the session.
    dismiss: Mutex<HashMap<String, watch::Sender<bool>>>,
}

/// A lock held for a few statements and never across an await. A panic aborts (`panic =
/// "abort"`), so no guard is ever poisoned.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<H: Host> Shared<H> {
    fn sessions(&self) -> MutexGuard<'_, Sessions> {
        lock(&self.sessions)
    }

    async fn frontend(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.host.frontend(header).await.map_err(|why| {
            tracing::warn!(%why, "a call that is not the frontend's is refused");
            fdo::Error::AccessDenied(why)
        })
    }

    async fn bar(&self, header: &Header<'_>) -> fdo::Result<()> {
        self.host.bar(header).await.map_err(|why| {
            tracing::warn!(%why, "a caller of os.athanor.Portal1 that is not the bar is refused");
            fdo::Error::AccessDenied(why)
        })
    }

    /// Ends the session `handle`, whatever ends it: dismisses its dialog, removes its object
    /// and updates `InputSessions`. The bar's Stop tells the frontend. A Stop that has closed
    /// the compositor's context already ([`End::StoppedClosed`]) ends only the connected
    /// session of its ticket, the one whose context it closed. Any other end decides from the
    /// stage it removes, under the lock, and asks the compositor to close a context that
    /// stage holds; when that fails, the owner check included, one journal line says so and
    /// the compositor's revocation at the backend's change of owner covers it (PT15). `false`
    /// for no such session.
    async fn end(&self, server: &ObjectServer, handle: &ObjectPath<'_>, how: End) -> bool {
        let closed = {
            let mut sessions = self.sessions();
            match (how, sessions.current(handle)) {
                (End::StoppedClosed(ticket), Some((Stage::Connected(_), now))) if now == ticket => {
                    sessions.close(handle)
                }
                (End::StoppedClosed(_), _) => None,
                _ => sessions.close(handle),
            }
        };
        let Some(stage) = closed else {
            return false;
        };
        if let Some(dismiss) = lock(&self.dismiss).remove(handle.as_str()) {
            dismiss.send_replace(true);
        }
        if how != End::Closed {
            match server.interface::<_, Session<H>>(handle).await {
                Ok(session) => {
                    if let Err(error) = Session::<H>::closed(session.signal_emitter()).await {
                        tracing::error!(%error, %handle, "the frontend was not told the session closed");
                    }
                }
                Err(error) => tracing::error!(%error, %handle, "the session has no object"),
            }
        }
        if let Err(error) = server.remove::<Session<H>, _>(handle).await {
            tracing::error!(%error, %handle, "the session's object was not removed");
        }
        if holds_context(stage) {
            if !matches!(how, End::StoppedClosed(_)) {
                if let Err(why) = self.eis.close(handle).await {
                    tracing::error!(%why, %handle, "the compositor was not asked to close the session's EIS context");
                }
            }
            input_sessions_changed::<H>(server).await;
        }
        true
    }
}

/// What ends a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// The frontend's `Close`, or the backend's own end.
    Closed,
    /// The bar's Stop of a session with no open context.
    Stopped,
    /// The bar's Stop, once `CloseSession` closed the open context of the session of this
    /// ticket.
    StoppedClosed(Ticket),
}

async fn input_sessions_changed<H: Host>(server: &ObjectServer) {
    match server.interface::<_, Portal1<H>>(PORTAL1_PATH).await {
        Ok(portal) => {
            if let Err(error) = portal
                .get()
                .await
                .input_sessions_invalidate(portal.signal_emitter())
                .await
            {
                tracing::error!(%error, "InputSessions changed and no signal said so");
            }
        }
        Err(error) => tracing::error!(%error, "os.athanor.Portal1 is not served"),
    }
}

type Results = HashMap<String, OwnedValue>;

fn answer(response: u32) -> (u32, Results) {
    (response, Results::new())
}

/// The service: one per backend. Serve [`Backend::remote_desktop`] at the portal's object and
/// [`Backend::portal1`] at [`PORTAL1_PATH`] on the same connection.
pub struct Backend<H: Host>(Arc<Shared<H>>);

impl<H: Host> Backend<H> {
    pub fn new(host: H, eis: Eis<H::Identity>) -> Backend<H> {
        Backend(Arc::new(Shared {
            host,
            eis,
            sessions: Mutex::new(Sessions::new()),
            dismiss: Mutex::new(HashMap::new()),
        }))
    }

    pub fn remote_desktop(&self) -> RemoteDesktop<H> {
        RemoteDesktop(self.0.clone())
    }

    pub fn portal1(&self) -> Portal1<H> {
        Portal1(self.0.clone())
    }
}

/// `org.freedesktop.impl.portal.RemoteDesktop`.
pub struct RemoteDesktop<H: Host>(Arc<Shared<H>>);

const NOTIFY: &str = "input reaches the compositor only through EIS: call ConnectToEIS";

#[zbus::interface(name = "org.freedesktop.impl.portal.RemoteDesktop")]
impl<H: Host> RemoteDesktop<H> {
    async fn create_session(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        _handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: String,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, Results)> {
        self.0.frontend(&header).await?;
        let name = display_name(&app_id, self.0.host.app_name(&app_id).as_deref());
        if let Err(why) = self.0.sessions().create(&session_handle, &app_id, &name) {
            tracing::warn!(%why, "CreateSession refused");
            return Ok(answer(2));
        }
        let session = Session {
            shared: self.0.clone(),
        };
        match server.at(&session_handle, session).await {
            Ok(None) => {
                let (dismiss, _) = watch::channel(false);
                lock(&self.0.dismiss).insert(session_handle.to_string(), dismiss);
                Ok(answer(0))
            }
            Ok(Some(_)) | Err(_) => {
                tracing::warn!(%session_handle, "CreateSession refused: the object path is taken");
                self.0.sessions().close(&session_handle);
                Ok(answer(2))
            }
        }
    }

    async fn select_devices(
        &self,
        #[zbus(header)] header: Header<'_>,
        _handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: String,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, Results)> {
        self.0.frontend(&header).await?;
        // `persist_mode` and `restore_data` are not honoured: no consent is kept (PT15).
        let types = match options
            .get("types")
            .map(|types| types.downcast_ref::<u32>())
        {
            None => None,
            Some(Ok(types)) => Some(types),
            Some(Err(error)) => {
                tracing::warn!(%error, "SelectDevices refused: types is not a u32");
                return Ok(answer(2));
            }
        };
        let selected = remote_desktop::requested(types)
            .and_then(|types| self.0.sessions().select(&session_handle, &app_id, types));
        match selected {
            Ok(()) => Ok(answer(0)),
            Err(why) => {
                tracing::warn!(%why, "SelectDevices refused");
                Ok(answer(2))
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // the specification's signature
    async fn start(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: String,
        parent_window: String,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<(u32, Results)> {
        self.0.frontend(&header).await?;
        let Some(mut dismissed) = lock(&self.0.dismiss)
            .get(session_handle.as_str())
            .map(watch::Sender::subscribe)
        else {
            tracing::warn!(%session_handle, "Start refused: no such session");
            return Ok(answer(2));
        };
        let (types, ticket) = match self.0.sessions().ask(&session_handle, &app_id) {
            Ok(asked) => asked,
            Err(why) => {
                tracing::warn!(%why, "Start refused");
                return Ok(answer(2));
            }
        };
        if let Err(why) = self.0.eis.check().await {
            tracing::warn!(%why, "Start refused without a dialog");
            self.0
                .sessions()
                .answer(&session_handle, ticket, Answer::Refused);
            return Ok(answer(2));
        }
        let request = Request {
            shared: self.0.clone(),
            session: session_handle.to_string(),
        };
        if !matches!(server.at(&handle, request).await, Ok(None)) {
            tracing::warn!(%handle, "Start refused: the request's object path is taken");
            self.0
                .sessions()
                .answer(&session_handle, ticket, Answer::Refused);
            return Ok(answer(2));
        }
        let name = display_name(&app_id, self.0.host.app_name(&app_id).as_deref());
        let ask = Ask {
            app_id,
            name,
            parent_window,
            types,
        };
        let given = tokio::select! {
            given = self.0.host.consent(ask) => given,
            // A sender dropped with its session also dismisses the dialog.
            _ = dismissed.wait_for(|dismissed| *dismissed) => Answer::Refused,
        };
        if let Err(error) = server.remove::<Request<H>, _>(&handle).await {
            tracing::error!(%error, %handle, "the request's object was not removed");
        }
        match given {
            Answer::Granted(types) if self.0.sessions().answer(&session_handle, ticket, given) => {
                let mut results = Results::new();
                results.insert("devices".into(), OwnedValue::from(types));
                Ok((0, results))
            }
            _ => {
                self.0
                    .sessions()
                    .answer(&session_handle, ticket, Answer::Refused);
                Ok(answer(1))
            }
        }
    }

    #[zbus(name = "ConnectToEIS")]
    async fn connect_to_eis(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        session_handle: ObjectPath<'_>,
        app_id: String,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<OwnedFd> {
        self.0.frontend(&header).await?;
        let (types, ticket) = self
            .0
            .sessions()
            .connect(&session_handle, &app_id)
            .map_err(fdo::Error::Failed)?;
        let given = self.0.eis.connect(&session_handle, types).await;
        let opened = given.is_ok();
        let held = self.0.sessions().connected(&session_handle, ticket, opened);
        match given {
            Ok(fd) if held => {
                input_sessions_changed::<H>(server).await;
                Ok(fd.into())
            }
            Ok(fd) => {
                // The session closed while the compositor answered: the descriptor is dropped.
                // Its close already asked the compositor to close the context; asking again is
                // safe only while no new session holds the handle, which a close would revoke.
                drop(fd);
                if !self.0.sessions().exists(&session_handle) {
                    if let Err(why) = self.0.eis.close(&session_handle).await {
                        tracing::error!(%why, %session_handle, "the compositor was not asked to close the context of a closed session");
                    }
                }
                Err(fdo::Error::Failed(format!("{session_handle} closed")))
            }
            Err(why) => {
                tracing::warn!(%why, %session_handle, "no EIS file descriptor");
                Err(fdo::Error::Failed(why.to_string()))
            }
        }
    }

    async fn notify_pointer_motion(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _dx: f64,
        _dy: f64,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_pointer_motion_absolute(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _stream: u32,
        _x: f64,
        _y: f64,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_pointer_button(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _button: i32,
        _state: u32,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_pointer_axis(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _dx: f64,
        _dy: f64,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_pointer_axis_discrete(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _axis: u32,
        _steps: i32,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_keyboard_keycode(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _keycode: i32,
        _state: u32,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_keyboard_keysym(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _keysym: i32,
        _state: u32,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_touch_down(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _stream: u32,
        _slot: u32,
        _x: f64,
        _y: f64,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_touch_motion(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _stream: u32,
        _slot: u32,
        _x: f64,
        _y: f64,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    async fn notify_touch_up(
        &self,
        _session_handle: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        _slot: u32,
    ) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(NOTIFY.into()))
    }

    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        remote_desktop::AVAILABLE
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        VERSION
    }
}

/// `org.freedesktop.impl.portal.Request` on a `Start`'s handle while its dialog shows.
pub struct Request<H: Host> {
    shared: Arc<Shared<H>>,
    session: String,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl<H: Host> Request<H> {
    /// Dismisses the dialog: `Start` answers 1, and the session obtains no file descriptor.
    async fn close(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.shared.frontend(&header).await?;
        if let Some(dismiss) = lock(&self.shared.dismiss).get(&self.session) {
            dismiss.send_replace(true);
        }
        Ok(())
    }
}

/// `org.freedesktop.impl.portal.Session` on a session's handle, from `CreateSession` to its
/// end.
pub struct Session<H: Host> {
    shared: Arc<Shared<H>>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl<H: Host> Session<H> {
    async fn close(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        self.shared.frontend(&header).await?;
        let handle = header
            .path()
            .ok_or_else(|| fdo::Error::InvalidArgs("a call with no object path".into()))?;
        self.shared.end(server, handle, End::Closed).await;
        Ok(())
    }

    #[zbus(signal)]
    async fn closed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

/// The RemoteDesktop part of `os.athanor.Portal1` (PT10).
pub struct Portal1<H: Host>(Arc<Shared<H>>);

#[zbus::interface(name = "os.athanor.Portal1")]
impl<H: Host> Portal1<H> {
    /// Closes the session `id`, and so revokes its file descriptor. When the compositor's
    /// `CloseSession` fails, the error is the answer and the session stays open and listed, so
    /// the indicator and its Stop remain (PT15).
    async fn stop_capture(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        id: &str,
    ) -> fdo::Result<()> {
        self.0.bar(&header).await?;
        let handle = ObjectPath::try_from(id)
            .map_err(|_| fdo::Error::InvalidArgs(format!("no session {id}")))?;
        let (stage, ticket) = self
            .0
            .sessions()
            .current(&handle)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no session {id}")))?;
        let Stage::Connected(_) = stage else {
            // No open context to keep listed: the session ends at once, and `end` closes a
            // context from the stage it removes under the lock, so a `ConnectToEIS` answered
            // meanwhile finds no session and closes its late context itself.
            self.0.end(server, &handle, End::Stopped).await;
            return Ok(());
        };
        // A connected session leaves that stage only by its end, so this `CloseSession` closes
        // the context that exists now.
        if let Err(why) = self.0.eis.close(&handle).await {
            tracing::error!(%why, %handle, "StopCapture: the compositor did not close the session's EIS context, which stays listed");
            return Err(fdo::Error::Failed(format!("{handle} stays open: {why}")));
        }
        self.0
            .end(server, &handle, End::StoppedClosed(ticket))
            .await;
        Ok(())
    }

    /// Session id, application id and name of each session whose EIS context is open. The
    /// change signal carries no value, so only the admitted bar reads the list.
    #[zbus(property(emits_changed_signal = "invalidates"))]
    async fn input_sessions(
        &self,
        #[zbus(header)] header: Option<Header<'_>>,
    ) -> fdo::Result<Vec<(String, String, String)>> {
        let header =
            header.ok_or_else(|| fdo::Error::AccessDenied("a read with no caller".into()))?;
        self.0.bar(&header).await?;
        Ok(self.0.sessions().input_sessions())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use zbus::Connection;

    use super::*;
    use crate::eis::tests::{Call, Compositor, Fixture};
    use crate::remote_desktop::{KEYBOARD, POINTER, TOUCHSCREEN};
    use crate::testing::Bus;

    const DESKTOP: &str = "/org/freedesktop/portal/desktop";
    const RD: &str = "org.freedesktop.impl.portal.RemoteDesktop";
    const APP: &str = "org.example.Remote";

    /// The frontend and the bar are each one connection of the test, admitted by its unique
    /// name; every other caller is refused. The dialog records what it is asked and gives the
    /// scripted answer, or never answers.
    struct TestHost {
        frontend: String,
        bar: String,
        asks: Arc<Mutex<Vec<Ask>>>,
        answer: Option<Answer>,
    }

    fn only(header: &Header<'_>, who: &str, what: &str) -> Result<(), String> {
        match header.sender() {
            Some(sender) if sender.as_str() == who => Ok(()),
            sender => Err(format!("{sender:?} is not the {what}")),
        }
    }

    impl Host for TestHost {
        type Identity = Fixture;

        async fn frontend(&self, header: &Header<'_>) -> Result<(), String> {
            only(header, &self.frontend, "frontend")
        }

        async fn bar(&self, header: &Header<'_>) -> Result<(), String> {
            only(header, &self.bar, "bar")
        }

        fn app_name(&self, app_id: &str) -> Option<String> {
            (app_id == APP).then(|| "Remote".to_owned())
        }

        async fn consent(&self, ask: Ask) -> Answer {
            lock(&self.asks).push(ask);
            match self.answer {
                Some(answer) => answer,
                None => std::future::pending().await,
            }
        }
    }

    struct Rd {
        _bus: Bus,
        compositor: Connection,
        calls: Arc<Mutex<Vec<Call>>>,
        refuse: Arc<AtomicBool>,
        hold: Arc<AtomicBool>,
        close_releases: Arc<AtomicBool>,
        release: Arc<tokio::sync::Notify>,
        held: Arc<Mutex<Vec<(String, UnixStream)>>>,
        frontend: Connection,
        bar: Connection,
        stranger: Connection,
        /// The serving connection, kept open for the test.
        _serving: Connection,
        backend: String,
        asks: Arc<Mutex<Vec<Ask>>>,
    }

    async fn rd(name: &str, answer: Option<Answer>, change: impl FnOnce(&mut Fixture)) -> Rd {
        let bus = Bus::start(name);
        let mock = Compositor::default();
        let calls = mock.calls.clone();
        let refuse = mock.refuse.clone();
        let hold = mock.hold.clone();
        let close_releases = mock.close_releases.clone();
        let release = mock.release.clone();
        let held = mock.held.clone();
        let compositor = bus
            .builder()
            .name(crate::eis::NAME)
            .serve_at(crate::eis::PATH, mock)
            .build()
            .await
            .expect("compositor");
        let frontend = bus.builder().build().await.expect("frontend");
        let bar = bus.builder().build().await.expect("bar");
        let stranger = bus.builder().build().await.expect("stranger");
        let query = bus.builder().build().await.expect("query");
        let mut identity = Fixture::new(&query, &compositor).await;
        change(&mut identity);
        let asks = Arc::new(Mutex::new(Vec::new()));
        let host = TestHost {
            frontend: frontend.unique_name().expect("name").to_string(),
            bar: bar.unique_name().expect("name").to_string(),
            asks: asks.clone(),
            answer,
        };
        let service = Backend::new(
            host,
            Eis {
                conn: query,
                identity,
            },
        );
        let serving = bus
            .builder()
            .serve_at(DESKTOP, service.remote_desktop())
            .serve_at(PORTAL1_PATH, service.portal1())
            .build()
            .await
            .expect("backend");
        Rd {
            _bus: bus,
            compositor,
            calls,
            refuse,
            hold,
            close_releases,
            release,
            held,
            frontend,
            bar,
            stranger,
            backend: serving.unique_name().expect("name").to_string(),
            _serving: serving,
            asks,
        }
    }

    fn session(n: u32) -> ObjectPath<'static> {
        ObjectPath::try_from(format!("{DESKTOP}/session/1_1/s{n}")).expect("path")
    }

    fn request(n: u32) -> ObjectPath<'static> {
        ObjectPath::try_from(format!("{DESKTOP}/request/1_1/r{n}")).expect("path")
    }

    type Options<'a> = HashMap<&'a str, Value<'a>>;

    impl Rd {
        async fn call<B>(
            &self,
            from: &Connection,
            path: &ObjectPath<'_>,
            interface: &str,
            member: &str,
            body: &B,
        ) -> zbus::Result<zbus::Message>
        where
            B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
        {
            from.call_method(
                Some(self.backend.as_str()),
                path,
                Some(interface),
                member,
                body,
            )
            .await
        }

        async fn respond<B>(&self, member: &str, body: &B) -> (u32, Results)
        where
            B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
        {
            let desktop = ObjectPath::from_static_str(DESKTOP).expect("path");
            self.call(&self.frontend, &desktop, RD, member, body)
                .await
                .expect(member)
                .body()
                .deserialize()
                .expect("response")
        }

        async fn create(&self, n: u32) {
            let created = self
                .respond(
                    "CreateSession",
                    &(request(n), session(n), APP, Options::new()),
                )
                .await;
            assert_eq!(created.0, 0, "CreateSession");
        }

        async fn select(&self, n: u32, options: Options<'_>) -> u32 {
            self.respond("SelectDevices", &(request(n), session(n), APP, options))
                .await
                .0
        }

        async fn start(&self, n: u32) -> (u32, Results) {
            self.respond("Start", &(request(n), session(n), APP, "", Options::new()))
                .await
        }

        async fn connect(&self, n: u32) -> zbus::Result<UnixStream> {
            let desktop = ObjectPath::from_static_str(DESKTOP).expect("path");
            let reply = self
                .call(
                    &self.frontend,
                    &desktop,
                    RD,
                    "ConnectToEIS",
                    &(session(n), APP, Options::new()),
                )
                .await?;
            let fd: OwnedFd = reply.body().deserialize()?;
            Ok(UnixStream::from(std::os::fd::OwnedFd::from(fd)))
        }

        async fn close(&self, n: u32) -> zbus::Result<zbus::Message> {
            self.call(
                &self.frontend,
                &session(n),
                "org.freedesktop.impl.portal.Session",
                "Close",
                &(),
            )
            .await
        }

        async fn listed_by(&self, who: &Connection) -> zbus::Result<Vec<(String, String, String)>> {
            let portal = ObjectPath::from_static_str(PORTAL1_PATH).expect("path");
            let reply = self
                .call(
                    who,
                    &portal,
                    "org.freedesktop.DBus.Properties",
                    "Get",
                    &("os.athanor.Portal1", "InputSessions"),
                )
                .await?;
            let value: OwnedValue = reply.body().deserialize()?;
            Vec::try_from(value)
        }

        async fn listed(&self) -> Vec<(String, String, String)> {
            self.listed_by(&self.bar).await.expect("InputSessions")
        }

        async fn stop(&self, who: &Connection, n: u32) -> zbus::Result<zbus::Message> {
            let portal = ObjectPath::from_static_str(PORTAL1_PATH).expect("path");
            self.call(
                who,
                &portal,
                "os.athanor.Portal1",
                "StopCapture",
                &(session(n).as_str(),),
            )
            .await
        }

        fn calls(&self) -> Vec<Call> {
            lock(&self.calls).clone()
        }

        /// Waits until the compositor is asked for session `n`'s file descriptor.
        async fn connecting(&self, n: u32) {
            let call = Call::Connect(session(n).to_string(), KEYBOARD);
            for _ in 0..200 {
                if self.calls().contains(&call) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the compositor was not asked for {call:?}");
        }

        fn asked(&self) -> usize {
            lock(&self.asks).len()
        }
    }

    fn devices(results: &Results) -> Option<u32> {
        results
            .get("devices")
            .map(|devices| devices.downcast_ref::<u32>().expect("devices is a u32"))
    }

    fn fdo_error<T>(answer: &zbus::Result<T>) -> Option<&fdo::Error> {
        match answer {
            Err(zbus::Error::FDO(error)) => Some(error),
            _ => None,
        }
    }

    fn ended(mut stream: UnixStream) -> bool {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        let mut buf = [0u8; 1];
        matches!(stream.read(&mut buf), Ok(0))
    }

    #[tokio::test]
    async fn input_a_granted_session_connects_with_its_types_and_is_listed_until_closed() {
        let rd = rd("granted", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        rd.create(1).await;
        let types = Options::from([("types", Value::from(KEYBOARD | POINTER))]);
        assert_eq!(rd.select(1, types).await, 0);
        let (response, results) = rd.start(1).await;
        assert_eq!((response, devices(&results)), (0, Some(KEYBOARD)));
        assert_eq!(
            lock(&rd.asks)[0],
            Ask {
                app_id: APP.into(),
                name: "Remote".into(),
                parent_window: String::new(),
                types: KEYBOARD | POINTER,
            }
        );
        assert!(
            rd.calls().is_empty(),
            "nothing asked of the compositor before ConnectToEIS"
        );
        assert!(rd.listed().await.is_empty());

        let stream = rd.connect(1).await.expect("fd");
        assert_eq!(
            rd.calls(),
            [Call::Connect(session(1).to_string(), KEYBOARD)]
        );
        assert_eq!(
            rd.listed().await,
            [(session(1).to_string(), APP.to_owned(), "Remote".to_owned())]
        );
        assert!(rd.connect(1).await.is_err(), "a second ConnectToEIS");
        assert_eq!(
            rd.calls().len(),
            1,
            "the second call never reached the compositor"
        );

        rd.close(1).await.expect("Close");
        assert_eq!(rd.calls()[1], Call::Close(session(1).to_string()));
        assert!(rd.listed().await.is_empty());
        assert!(
            ended(stream),
            "the application's descriptor reads end of stream"
        );
    }

    #[tokio::test]
    async fn input_a_session_not_granted_or_refused_obtains_no_fd() {
        let rd = rd("refused", Some(Answer::Refused), |_| ()).await;
        rd.create(1).await;
        assert!(rd.connect(1).await.is_err(), "before Start");
        let (response, results) = rd.start(1).await;
        assert_eq!((response, devices(&results)), (1, None));
        assert!(rd.connect(1).await.is_err(), "refused");
        rd.close(1).await.expect("Close");
        assert!(rd.connect(1).await.is_err(), "closed");
        assert!(rd.calls().is_empty(), "the compositor was never asked");
    }

    #[tokio::test]
    async fn input_a_grant_of_nothing_or_of_types_not_asked_for_is_the_refusal() {
        for (n, given) in [(1, Answer::Granted(0)), (2, Answer::Granted(TOUCHSCREEN))] {
            let rd = rd(&format!("widened{n}"), Some(given), |_| ()).await;
            rd.create(n).await;
            let types = Options::from([("types", Value::from(KEYBOARD))]);
            assert_eq!(rd.select(n, types).await, 0);
            assert_eq!(rd.start(n).await.0, 1, "{given:?}");
            assert!(rd.connect(n).await.is_err());
            assert!(rd.calls().is_empty());
        }
    }

    #[tokio::test]
    async fn input_start_is_refused_without_a_dialog_when_the_owner_is_not_the_compositor() {
        type Change = Box<dyn FnOnce(&mut Fixture)>;
        let checks: [(&str, Change); 3] = [
            ("another unit", Box::new(|f| f.unit = "evil.service".into())),
            (
                "another executable",
                Box::new(|f| f.exec = "/usr/bin/sh".into()),
            ),
            ("no pidfd", Box::new(|f| f.hide_pidfd = true)),
        ];
        for (n, (what, change)) in (1..).zip(checks) {
            let rd = rd(
                &format!("owner{n}"),
                Some(Answer::Granted(KEYBOARD)),
                change,
            )
            .await;
            rd.create(n).await;
            assert_eq!(rd.start(n).await.0, 2, "{what}");
            assert_eq!(rd.asked(), 0, "{what}: no dialog");
            assert!(rd.connect(n).await.is_err(), "{what}");
            assert!(rd.calls().is_empty(), "{what}");
        }

        let no_owner = self::rd("no-owner", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        no_owner
            .compositor
            .release_name(crate::eis::NAME)
            .await
            .expect("release");
        no_owner.create(1).await;
        assert_eq!(no_owner.start(1).await.0, 2, "no owner");
        assert_eq!(no_owner.asked(), 0, "no owner: no dialog");
    }

    #[tokio::test]
    async fn input_a_compositor_refusal_is_the_error_and_leaves_no_fd() {
        let rd = rd("compositor", Some(Answer::Granted(POINTER)), |_| ()).await;
        rd.refuse.store(true, Ordering::SeqCst);
        rd.create(1).await;
        assert_eq!(
            rd.start(1).await.0,
            0,
            "Start is answered before ConnectToEIS"
        );
        let refused = rd.connect(1).await;
        assert!(
            matches!(fdo_error(&refused), Some(fdo::Error::Failed(why)) if why.contains("no context")),
            "{refused:?}"
        );
        assert!(rd.connect(1).await.is_err(), "no second attempt");
        assert!(rd.listed().await.is_empty());
        rd.close(1).await.expect("Close");
        assert_eq!(
            rd.calls(),
            [Call::Connect(session(1).to_string(), POINTER)],
            "nothing to close"
        );
    }

    #[tokio::test]
    async fn input_a_vanished_frontend_leaves_the_session_listed_until_stop_capture() {
        let rd = rd(
            "vanished",
            Some(Answer::Granted(KEYBOARD | POINTER)),
            |_| (),
        )
        .await;
        rd.create(1).await;
        rd.start(1).await;
        let stream = rd.connect(1).await.expect("fd");
        let Rd { frontend, .. } = &rd;
        frontend.clone().close().await.expect("the frontend goes");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(rd.listed().await.len(), 1, "still listed");

        assert!(
            rd.stop(&rd.stranger, 1).await.is_err(),
            "only the bar stops a session"
        );
        assert!(
            rd.listed_by(&rd.stranger).await.is_err(),
            "only the bar reads the list"
        );
        assert_eq!(rd.listed().await.len(), 1);

        rd.stop(&rd.bar, 1).await.expect("StopCapture");
        assert!(rd.listed().await.is_empty());
        assert_eq!(
            rd.calls(),
            [
                Call::Connect(session(1).to_string(), KEYBOARD | POINTER),
                Call::Close(session(1).to_string())
            ]
        );
        assert!(ended(stream));
        assert!(
            rd.stop(&rd.bar, 1).await.is_err(),
            "no such session any more"
        );
    }

    #[tokio::test]
    async fn input_closing_the_request_dismisses_the_dialog_and_the_session_gets_nothing() {
        let rd = Arc::new(rd("dismissed", None, |_| ()).await);
        rd.create(1).await;
        let start = tokio::spawn({
            let rd = rd.clone();
            async move { rd.start(1).await }
        });
        while rd.asked() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rd.call(
            &rd.stranger,
            &request(1),
            "org.freedesktop.impl.portal.Request",
            "Close",
            &(),
        )
        .await
        .expect_err("only the frontend closes a request");
        rd.call(
            &rd.frontend,
            &request(1),
            "org.freedesktop.impl.portal.Request",
            "Close",
            &(),
        )
        .await
        .expect("Close");
        let (response, results) = start.await.expect("Start");
        assert_eq!((response, devices(&results)), (1, None));
        assert!(rd.connect(1).await.is_err());

        rd.create(2).await;
        let start = tokio::spawn({
            let rd = rd.clone();
            async move { rd.start(2).await }
        });
        while rd.asked() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        rd.close(2).await.expect("Close");
        assert_eq!(
            start.await.expect("Start").0,
            1,
            "the session's close dismisses its dialog"
        );
        assert!(rd.calls().is_empty());
    }

    #[tokio::test]
    async fn input_persistence_is_not_honoured_and_every_session_asks_again() {
        let rd = rd("persist", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        for n in [1, 2] {
            rd.create(n).await;
            let options = Options::from([("persist_mode", Value::from(2u32))]);
            assert_eq!(rd.select(n, options).await, 0);
            let (response, results) = rd.start(n).await;
            assert_eq!(response, 0);
            assert!(!results.contains_key("restore_data"), "{results:?}");
            assert!(!results.contains_key("persist_mode"), "{results:?}");
            rd.close(n).await.expect("Close");
        }
        assert_eq!(rd.asked(), 2, "the second session showed the dialog again");
    }

    #[tokio::test]
    async fn input_every_notify_method_is_refused() {
        let rd = rd(
            "notify",
            Some(Answer::Granted(KEYBOARD | POINTER | TOUCHSCREEN)),
            |_| (),
        )
        .await;
        rd.create(1).await;
        rd.start(1).await;
        let desktop = ObjectPath::from_static_str(DESKTOP).expect("path");
        let s = session(1);
        let o = Options::new;
        let calls = [
            (
                "NotifyPointerMotion",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyPointerMotion",
                    &(&s, o(), 1.0, 1.0),
                )
                .await,
            ),
            (
                "NotifyPointerMotionAbsolute",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyPointerMotionAbsolute",
                    &(&s, o(), 0u32, 1.0, 1.0),
                )
                .await,
            ),
            (
                "NotifyPointerButton",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyPointerButton",
                    &(&s, o(), 272i32, 1u32),
                )
                .await,
            ),
            (
                "NotifyPointerAxis",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyPointerAxis",
                    &(&s, o(), 0.0, 1.0),
                )
                .await,
            ),
            (
                "NotifyPointerAxisDiscrete",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyPointerAxisDiscrete",
                    &(&s, o(), 0u32, 1i32),
                )
                .await,
            ),
            (
                "NotifyKeyboardKeycode",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyKeyboardKeycode",
                    &(&s, o(), 30i32, 1u32),
                )
                .await,
            ),
            (
                "NotifyKeyboardKeysym",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyKeyboardKeysym",
                    &(&s, o(), 0x61i32, 1u32),
                )
                .await,
            ),
            (
                "NotifyTouchDown",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyTouchDown",
                    &(&s, o(), 0u32, 0u32, 1.0, 1.0),
                )
                .await,
            ),
            (
                "NotifyTouchMotion",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyTouchMotion",
                    &(&s, o(), 0u32, 0u32, 1.0, 1.0),
                )
                .await,
            ),
            (
                "NotifyTouchUp",
                rd.call(
                    &rd.frontend,
                    &desktop,
                    RD,
                    "NotifyTouchUp",
                    &(&s, o(), 0u32),
                )
                .await,
            ),
        ];
        for (member, answer) in calls {
            assert!(
                matches!(fdo_error(&answer), Some(fdo::Error::NotSupported(_))),
                "{member}: {answer:?}"
            );
        }
        assert!(rd.calls().is_empty());
    }

    #[tokio::test]
    async fn input_stop_capture_keeps_the_session_listed_until_close_session_succeeds() {
        let rd = rd("stopfails", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        for n in [1, 2] {
            rd.create(n).await;
            rd.start(n).await;
        }
        let one = rd.connect(1).await.expect("fd 1");
        let _two = rd.connect(2).await.expect("fd 2");
        assert!(rd
            .compositor
            .release_name(crate::eis::NAME)
            .await
            .expect("release"));

        let stopped = rd.stop(&rd.bar, 1).await;
        assert!(
            matches!(fdo_error(&stopped), Some(fdo::Error::Failed(_))),
            "{stopped:?}"
        );
        assert_eq!(rd.listed().await.len(), 2, "stopped, but still listed");

        rd.close(2)
            .await
            .expect("Close ends the session all the same");
        assert_eq!(rd.listed().await.len(), 1);

        rd.compositor
            .request_name(crate::eis::NAME)
            .await
            .expect("the compositor again");
        rd.stop(&rd.bar, 1).await.expect("StopCapture");
        assert!(rd.listed().await.is_empty());
        assert_eq!(
            rd.calls(),
            [
                Call::Connect(session(1).to_string(), KEYBOARD),
                Call::Connect(session(2).to_string(), KEYBOARD),
                Call::Close(session(1).to_string())
            ],
            "nothing is sent while the owner is not the compositor"
        );
        assert!(ended(one));
    }

    /// The compositor answers `ConnectToEIS` while the bar's Stop waits for `CloseSession`,
    /// as a compositor that handles the two calls at once may: the application is left no live
    /// descriptor and the compositor no context.
    #[tokio::test]
    async fn input_stop_capture_during_connect_to_eis_leaves_no_live_context() {
        let rd = rd("stopconnecting", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        rd.hold.store(true, Ordering::SeqCst);
        rd.close_releases.store(true, Ordering::SeqCst);
        rd.create(1).await;
        rd.start(1).await;
        let (late, stopped) = tokio::join!(rd.connect(1), async {
            rd.connecting(1).await;
            rd.stop(&rd.bar, 1).await
        });
        stopped.expect("StopCapture");
        assert!(
            !matches!(late.map(ended), Ok(false)),
            "the application holds a live descriptor"
        );
        assert!(
            lock(&rd.held)
                .iter()
                .all(|(handle, _)| *handle != session(1).as_str()),
            "the compositor holds a context for the stopped session"
        );
        assert!(rd.listed().await.is_empty());
    }

    #[tokio::test]
    async fn input_the_input_sessions_signal_carries_no_value() {
        use futures_util::StreamExt;

        let rd = rd("invalidated", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        let properties = zbus::fdo::PropertiesProxy::builder(&rd.stranger)
            .destination(rd.backend.as_str())
            .path(PORTAL1_PATH)
            .build()
            .await
            .expect("proxy");
        let mut changes = properties
            .receive_properties_changed()
            .await
            .expect("subscribe");
        rd.create(1).await;
        rd.start(1).await;
        let _stream = rd.connect(1).await.expect("fd");
        let change = tokio::time::timeout(Duration::from_secs(2), changes.next())
            .await
            .expect("in time")
            .expect("a signal");
        let args = change.args().expect("args");
        assert_eq!(args.interface_name().as_str(), "os.athanor.Portal1");
        assert!(
            args.changed_properties().is_empty(),
            "{:?}",
            args.changed_properties()
        );
        assert_eq!(args.invalidated_properties(), &vec!["InputSessions"]);
    }

    #[tokio::test]
    async fn input_a_late_eis_answer_is_dropped_and_closed_only_when_no_session_holds_the_handle() {
        let rd = rd("late", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        rd.hold.store(true, Ordering::SeqCst);
        for n in [1, 2] {
            rd.create(n).await;
            rd.start(n).await;
        }

        let (late, ()) = tokio::join!(rd.connect(1), async {
            rd.connecting(1).await;
            rd.close(1).await.expect("Close");
            rd.create(1).await;
            rd.release.notify_one();
        });
        assert!(late.is_err(), "the late descriptor reaches no session");
        assert_eq!(
            rd.calls(),
            [
                Call::Connect(session(1).to_string(), KEYBOARD),
                Call::Close(session(1).to_string())
            ],
            "the handle a new session holds is not closed"
        );
        let given = {
            let mut held = lock(&rd.held);
            let at = held
                .iter()
                .position(|(handle, _)| *handle == session(1).as_str())
                .expect("the compositor's end of the late descriptor");
            held.remove(at).1
        };
        assert!(ended(given), "the backend dropped the late descriptor");
        assert!(rd.listed().await.is_empty());

        let (late, ()) = tokio::join!(rd.connect(2), async {
            rd.connecting(2).await;
            rd.close(2).await.expect("Close");
            rd.release.notify_one();
        });
        assert!(late.is_err());
        assert_eq!(
            rd.calls()[2..],
            [
                Call::Connect(session(2).to_string(), KEYBOARD),
                Call::Close(session(2).to_string()),
                Call::Close(session(2).to_string())
            ],
            "no session holds the handle: its late context is closed"
        );
        assert!(rd.listed().await.is_empty());
    }

    #[tokio::test]
    async fn input_calls_not_from_the_frontend_are_refused() {
        let rd = rd("stranger", Some(Answer::Granted(KEYBOARD)), |_| ()).await;
        let desktop = ObjectPath::from_static_str(DESKTOP).expect("path");
        let created = rd
            .call(
                &rd.stranger,
                &desktop,
                RD,
                "CreateSession",
                &(request(1), session(1), APP, Options::new()),
            )
            .await;
        assert!(
            matches!(fdo_error(&created), Some(fdo::Error::AccessDenied(_))),
            "{created:?}"
        );
        rd.create(1).await;
        let selected = rd
            .call(
                &rd.stranger,
                &desktop,
                RD,
                "SelectDevices",
                &(
                    request(1),
                    session(1),
                    APP,
                    Options::from([("types", Value::from(KEYBOARD))]),
                ),
            )
            .await;
        assert!(
            matches!(fdo_error(&selected), Some(fdo::Error::AccessDenied(_))),
            "{selected:?}"
        );
        let started = rd
            .call(
                &rd.stranger,
                &desktop,
                RD,
                "Start",
                &(request(1), session(1), APP, "", Options::new()),
            )
            .await;
        assert!(
            matches!(fdo_error(&started), Some(fdo::Error::AccessDenied(_))),
            "{started:?}"
        );
        assert_eq!(rd.asked(), 0, "a stranger's Start opens no dialog");
        rd.start(1).await;
        let connected = rd
            .call(
                &rd.stranger,
                &desktop,
                RD,
                "ConnectToEIS",
                &(session(1), APP, Options::new()),
            )
            .await;
        assert!(connected.is_err());
        rd.call(
            &rd.stranger,
            &session(1),
            "org.freedesktop.impl.portal.Session",
            "Close",
            &(),
        )
        .await
        .expect_err("only the frontend closes a session");
        assert!(rd.calls().is_empty());
        rd.connect(1)
            .await
            .expect("the frontend's call still works");
    }

    /// The interfaces are written by hand (PT2 asks for zbus-xmlgen output); this holds them to
    /// xdg-desktop-portal's own definitions, vendored unmodified from 1.20.4: every method,
    /// signal and property, with the type and direction of each argument.
    #[tokio::test]
    async fn input_the_interfaces_match_the_upstream_definitions() {
        use crate::testing::members;
        use zbus::object_server::Interface;
        let bus = Bus::start("introspect");
        let query = bus.builder().build().await.expect("query");
        let identity = Fixture::new(&query, &query).await;
        let host = TestHost {
            frontend: String::new(),
            bar: String::new(),
            asks: Arc::default(),
            answer: None,
        };
        let service = Backend::new(
            host,
            Eis {
                conn: query,
                identity,
            },
        );
        let ours = |interface: &dyn Fn(&mut String)| {
            let mut xml = String::new();
            interface(&mut xml);
            members(&xml)
        };
        let remote_desktop = ours(&|xml| service.remote_desktop().introspect_to_writer(xml, 0));
        let request = ours(&|xml| {
            Request {
                shared: service.0.clone(),
                session: String::new(),
            }
            .introspect_to_writer(xml, 0)
        });
        let session = ours(&|xml| {
            Session {
                shared: service.0.clone(),
            }
            .introspect_to_writer(xml, 0)
        });
        let upstream = |xml: &str| members(xml);
        assert_eq!(
            remote_desktop,
            upstream(include_str!(
                "../interfaces/org.freedesktop.impl.portal.RemoteDesktop.xml"
            ))
        );
        assert_eq!(
            remote_desktop.len(),
            16,
            "4 calls, 10 Notify*, 2 properties"
        );
        assert_eq!(
            request,
            upstream(include_str!(
                "../interfaces/org.freedesktop.impl.portal.Request.xml"
            ))
        );
        assert_eq!(
            session,
            upstream(include_str!(
                "../interfaces/org.freedesktop.impl.portal.Session.xml"
            ))
        );
    }
}
