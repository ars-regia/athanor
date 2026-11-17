//! The compositor's EIS socket, as the backend reaches it (doc_portal.md PT15;
//! doc_own_stack.md decision 33, wave 2c): `os.athanor.Eis1` at `/os/athanor/Eis1`, under the
//! name `os.athanor.Eis` on the session bus.
//!
//! Before the dialog, and again before each call, the owner of `os.athanor.Eis` is read anew
//! and admitted by its unit, as [`athanor_unit::bus_caller::admit`] checks it: through the
//! pidfd the bus returns, the verified user systemd places the owner in exactly
//! `athanor-sulphur.service`, whose main process it is and whose one `ExecStart` runs
//! `/usr/bin/athanor-sulphur`. Nothing is read under `/proc` (issue #1047), and a bus that
//! gives no pidfd is refused. The call then goes to the unique name that was checked, never to
//! the well-known one, so the name cannot change hands between the check and the call. Every
//! failure refuses: no owner, a bus that does not answer in time, any question that fails.

use std::os::fd::{BorrowedFd, OwnedFd};
use std::time::Duration;

use athanor_unit::bus_caller::{self, Credentials, Manager, Questions, Service, Unit};
use tokio::sync::Mutex;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::Connection;
use zbus::ObjectPath;
use zbus::OwnedObjectPath;

/// The compositor's name and object.
pub const NAME: &str = "os.athanor.Eis";
pub const PATH: &str = "/os/athanor/Eis1";
pub const INTERFACE: &str = "os.athanor.Eis1";
/// The compositor's unit and the executable of its one `ExecStart`.
pub const COMPOSITOR: Service<'static> = Service {
    unit: "athanor-sulphur.service",
    exec: "/usr/bin/athanor-sulphur",
};

/// Bound on each check and each call to the bus.
pub const ASK: Duration = Duration::from_secs(2);

/// The seam of the owner check: who owns [`NAME`], and the questions of
/// [`athanor_unit::bus_caller`]. CI runs the check through it with fixture answers for systemd;
/// the backend through [`OnBus`].
pub trait Identity: Questions {
    /// The unique name that owns [`NAME`], `None` when nobody does.
    fn owner(&self) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send;
}

/// The unique name of the compositor, checked as the module says, or why not.
pub async fn compositor(identity: &impl Identity) -> Result<String, String> {
    let owner = identity
        .owner()
        .await?
        .ok_or_else(|| format!("no process owns {NAME}"))?;
    bus_caller::admit(identity, &owner, COMPOSITOR)
        .await
        .map_err(|why| format!("{owner}: {why}"))?;
    Ok(owner)
}

/// [`Identity`] on the buses: the session bus's proxy on a connection of the caller's, and a
/// system bus connection of its own, built at the first question and again whenever it is
/// closed, as [`crate::bar::BarUnit`]'s is. Each serves nothing, so a handler can await them
/// (issue #1035).
pub struct OnBus {
    bus: DBusProxy<'static>,
    session: Connection,
    /// Where the system bus is; `None` is the system's own, from [`bus_caller::system_bus`].
    address: Option<String>,
    system: Mutex<Option<Connection>>,
}

impl OnBus {
    /// Asks on `query`, a session bus connection that serves nothing.
    pub async fn new(query: &Connection, address: Option<String>) -> zbus::Result<OnBus> {
        let bus = DBusProxy::builder(query)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        Ok(OnBus {
            bus,
            session: query.clone(),
            address,
            system: Mutex::new(None),
        })
    }

    /// The system bus connection, connected again if it closed.
    async fn system(&self) -> Result<Connection, String> {
        crate::bar::system(&self.system, self.address.as_deref()).await
    }

    fn questions<'a>(&'a self, system: &'a Connection) -> bus_caller::OnBus<'a> {
        bus_caller::OnBus {
            bus: &self.bus,
            session: &self.session,
            system,
        }
    }
}

impl Questions for OnBus {
    async fn user_manager(&self) -> Result<String, String> {
        let system = self.system().await?;
        self.questions(&system).user_manager().await
    }

    async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
        let system = self.system().await?;
        self.questions(&system).credentials(peer).await
    }

    async fn unit_by_pidfd(
        &self,
        manager: Manager<'_>,
        pidfd: BorrowedFd<'_>,
    ) -> Result<(OwnedObjectPath, String), String> {
        let system = self.system().await?;
        self.questions(&system).unit_by_pidfd(manager, pidfd).await
    }

    async fn main_pid(&self, manager: Manager<'_>, path: &OwnedObjectPath) -> Result<u32, String> {
        let system = self.system().await?;
        self.questions(&system).main_pid(manager, path).await
    }

    async fn unit(&self, manager: Manager<'_>, path: &OwnedObjectPath) -> Result<Unit, String> {
        let system = self.system().await?;
        self.questions(&system).unit(manager, path).await
    }
}

impl Identity for OnBus {
    async fn owner(&self) -> Result<Option<String>, String> {
        let name = BusName::try_from(NAME).map_err(|e| e.to_string())?;
        match self.bus.get_name_owner(name).await {
            Ok(owner) => Ok(Some(owner.to_string())),
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
            Err(e) => Err(format!("the owner of {NAME}: {e}")),
        }
    }
}

/// Why the compositor gave no file descriptor, or did not close a session.
#[derive(Debug)]
pub enum Refused {
    /// The owner of [`NAME`] is not the compositor: nothing was sent.
    NotCompositor(String),
    /// The compositor answered with an error, or did not answer in time.
    Compositor(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NotCompositor(why) => write!(f, "{NAME} is not the compositor's: {why}"),
            Refused::Compositor(why) => write!(f, "the compositor refused: {why}"),
        }
    }
}

/// The compositor's EIS service, reached on `conn`, a connection that serves nothing, after
/// the owner check of `identity` at every call.
pub struct Eis<I> {
    pub conn: Connection,
    pub identity: I,
}

impl<I: Identity> Eis<I> {
    /// The owner check alone, bound by [`ASK`]: the compositor's unique name.
    pub async fn check(&self) -> Result<String, Refused> {
        match tokio::time::timeout(ASK, compositor(&self.identity)).await {
            Ok(Ok(owner)) => Ok(owner),
            Ok(Err(why)) => Err(Refused::NotCompositor(why)),
            Err(_) => Err(Refused::NotCompositor(format!(
                "the bus did not say who owns {NAME} in time"
            ))),
        }
    }

    async fn call<B>(&self, member: &str, body: &B) -> Result<zbus::Message, Refused>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
    {
        let owner = self.check().await?;
        let call = self
            .conn
            .call_method(Some(owner.as_str()), PATH, Some(INTERFACE), member, body);
        match tokio::time::timeout(ASK, call).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(e)) => Err(Refused::Compositor(e.to_string())),
            Err(_) => Err(Refused::Compositor(format!(
                "{member} was not answered in time"
            ))),
        }
    }

    /// `ConnectToEIS` for the portal session `session`, with the granted `types`.
    pub async fn connect(&self, session: &ObjectPath<'_>, types: u32) -> Result<OwnedFd, Refused> {
        let reply = self.call("ConnectToEIS", &(session, types)).await?;
        let fd: zbus::OwnedFd = reply
            .body()
            .deserialize()
            .map_err(|e| Refused::Compositor(format!("ConnectToEIS answered no fd: {e}")))?;
        Ok(fd.into())
    }

    /// `CloseSession` for the portal session `session`.
    pub async fn close(&self, session: &ObjectPath<'_>) -> Result<(), Refused> {
        self.call("CloseSession", &(session,)).await.map(drop)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use zbus::message::Header;

    use super::*;
    use crate::testing::Bus;

    /// What the mock compositor was asked, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Call {
        Connect(String, u32),
        Close(String),
    }

    /// `os.athanor.Eis1` as decision 33 defines it: 0 or an unknown bit is `InvalidArgs` and
    /// creates nothing, a second `ConnectToEIS` for a held handle is `Failed`, and a file
    /// descriptor is one end of a real socket pair, whose other end the mock keeps.
    #[derive(Default)]
    pub(crate) struct Compositor {
        pub(crate) calls: Arc<Mutex<Vec<Call>>>,
        pub(crate) held: Arc<Mutex<Vec<(String, UnixStream)>>>,
        /// Every `ConnectToEIS` is refused, as a compositor that cannot open a context does.
        pub(crate) refuse: Arc<AtomicBool>,
        /// Each `ConnectToEIS` waits for one `release` before it answers.
        pub(crate) hold: Arc<AtomicBool>,
        /// Each `CloseSession` gives one `release`, and answers 200 ms later.
        pub(crate) close_releases: Arc<AtomicBool>,
        pub(crate) release: Arc<tokio::sync::Notify>,
    }

    #[zbus::interface(name = "os.athanor.Eis1")]
    impl Compositor {
        #[zbus(name = "ConnectToEIS")]
        async fn connect_to_eis(
            &self,
            #[zbus(header)] _header: Header<'_>,
            session_handle: ObjectPath<'_>,
            device_types: u32,
        ) -> zbus::fdo::Result<zbus::OwnedFd> {
            self.calls
                .lock()
                .expect("calls")
                .push(Call::Connect(session_handle.to_string(), device_types));
            if self.hold.load(Ordering::SeqCst) {
                self.release.notified().await;
            }
            if self.refuse.load(Ordering::SeqCst) {
                return Err(zbus::fdo::Error::Failed("no context".into()));
            }
            if device_types == 0 || device_types & !7 != 0 {
                return Err(zbus::fdo::Error::InvalidArgs(format!("{device_types:#x}")));
            }
            let mut held = self.held.lock().expect("held");
            if held.iter().any(|(h, _)| h == session_handle.as_str()) {
                return Err(zbus::fdo::Error::Failed("held already".into()));
            }
            let (server, client) = UnixStream::pair().expect("pair");
            held.push((session_handle.to_string(), server));
            Ok(OwnedFd::from(client).into())
        }

        async fn close_session(&self, session_handle: ObjectPath<'_>) {
            self.calls
                .lock()
                .expect("calls")
                .push(Call::Close(session_handle.to_string()));
            self.held
                .lock()
                .expect("held")
                .retain(|(h, _)| h != session_handle.as_str());
            if self.close_releases.load(Ordering::SeqCst) {
                self.release.notify_one();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }

    /// The identity seam: the bus's real answer for the owner and its credentials, the pidfd
    /// hidden on demand, and fixture answers for both systemds. The user's systemd is the
    /// compositor's connection here, so its credentials are real; the system's places it in
    /// `user@<uid>.service` with its own pid as the main process.
    pub(crate) struct Fixture {
        bus: DBusProxy<'static>,
        manager: String,
        pub(crate) unit: String,
        pub(crate) exec: String,
        pub(crate) fragment: String,
        /// The compositor unit's `MainPID`, the owner's own pid when `None`.
        pub(crate) main_pid: Option<u32>,
        pub(crate) hide_pidfd: bool,
    }

    impl Fixture {
        pub(crate) async fn new(conn: &Connection, manager: &Connection) -> Fixture {
            Fixture {
                bus: DBusProxy::new(conn).await.expect("proxy"),
                manager: manager.unique_name().expect("name").to_string(),
                unit: COMPOSITOR.unit.into(),
                exec: COMPOSITOR.exec.into(),
                fragment: format!("{}/{}", bus_caller::IMAGE_USER_UNITS, COMPOSITOR.unit),
                main_pid: None,
                hide_pidfd: false,
            }
        }
    }

    const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/fixture";

    fn unit_path() -> OwnedObjectPath {
        OwnedObjectPath::try_from(UNIT_PATH).expect("path")
    }

    impl Questions for Fixture {
        async fn user_manager(&self) -> Result<String, String> {
            Ok(self.manager.clone())
        }

        async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
            let name = BusName::try_from(peer).map_err(|e| e.to_string())?;
            let credentials = self
                .bus
                .get_connection_credentials(name)
                .await
                .map_err(|e| e.to_string())?;
            let pidfd = match credentials.process_fd() {
                Some(fd) if !self.hide_pidfd => Some(
                    std::os::fd::AsFd::as_fd(fd)
                        .try_clone_to_owned()
                        .expect("dup"),
                ),
                _ => None,
            };
            Ok(Credentials {
                uid: credentials.unix_user_id(),
                pid: credentials.process_id(),
                pidfd,
            })
        }

        async fn unit_by_pidfd(
            &self,
            manager: Manager<'_>,
            _pidfd: BorrowedFd<'_>,
        ) -> Result<(OwnedObjectPath, String), String> {
            let unit = match manager {
                Manager::System => format!("user@{}.service", nix::unistd::getuid().as_raw()),
                Manager::User(_) => self.unit.clone(),
            };
            Ok((unit_path(), unit))
        }

        async fn main_pid(
            &self,
            _manager: Manager<'_>,
            _path: &OwnedObjectPath,
        ) -> Result<u32, String> {
            Ok(std::process::id())
        }

        async fn unit(
            &self,
            _manager: Manager<'_>,
            _path: &OwnedObjectPath,
        ) -> Result<Unit, String> {
            Ok(Unit {
                main_pid: self.main_pid.unwrap_or_else(std::process::id),
                exec_start: vec![self.exec.clone()],
                fragment_path: self.fragment.clone(),
                drop_in_paths: Vec::new(),
            })
        }
    }

    impl Identity for Fixture {
        async fn owner(&self) -> Result<Option<String>, String> {
            match self
                .bus
                .get_name_owner(BusName::try_from(NAME).expect("name"))
                .await
            {
                Ok(owner) => Ok(Some(owner.to_string())),
                Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    struct Rig {
        _bus: Bus,
        compositor: Connection,
        calls: Arc<Mutex<Vec<Call>>>,
        backend: Connection,
    }

    async fn rig(name: &str) -> Rig {
        let bus = Bus::start(name);
        let mock = Compositor::default();
        let calls = mock.calls.clone();
        let compositor = bus
            .builder()
            .name(NAME)
            .serve_at(PATH, mock)
            .build()
            .await
            .expect("compositor");
        let backend = bus.builder().build().await.expect("backend");
        Rig {
            _bus: bus,
            compositor,
            calls,
            backend,
        }
    }

    async fn eis(rig: &Rig, change: impl FnOnce(&mut Fixture)) -> Eis<Fixture> {
        let mut identity = Fixture::new(&rig.backend, &rig.compositor).await;
        change(&mut identity);
        Eis {
            conn: rig.backend.clone(),
            identity,
        }
    }

    const SESSION: &str = "/org/freedesktop/portal/desktop/session/1_1/a";

    fn session() -> ObjectPath<'static> {
        ObjectPath::from_static_str(SESSION).expect("path")
    }

    #[tokio::test]
    async fn eis_the_compositor_is_the_main_process_of_its_unit() {
        let rig = rig("owner").await;
        let owner = rig.compositor.unique_name().expect("name").to_string();
        assert_eq!(
            eis(&rig, |_| ()).await.check().await.expect("admitted"),
            owner
        );
    }

    #[tokio::test]
    async fn eis_no_owner_another_unit_or_no_pidfd_is_refused_and_nothing_sent() {
        let rig = rig("refused").await;
        type Change = Box<dyn FnOnce(&mut Fixture)>;
        let refusals: [(&str, Change); 5] = [
            ("another unit", Box::new(|f| f.unit = "evil.service".into())),
            (
                "another executable",
                Box::new(|f| f.exec = "/usr/bin/sh".into()),
            ),
            (
                "a unit outside the image",
                Box::new(|f| f.fragment = format!("/etc/systemd/user/{}", COMPOSITOR.unit)),
            ),
            (
                "not the main process",
                Box::new(|f| f.main_pid = Some(std::process::id() + 1)),
            ),
            ("no pidfd", Box::new(|f| f.hide_pidfd = true)),
        ];
        for (what, change) in refusals {
            let eis = eis(&rig, change).await;
            assert!(
                matches!(
                    eis.connect(&session(), 1).await,
                    Err(Refused::NotCompositor(_))
                ),
                "{what}"
            );
            assert!(
                matches!(eis.close(&session()).await, Err(Refused::NotCompositor(_))),
                "{what}"
            );
        }
        assert!(
            rig.calls.lock().expect("calls").is_empty(),
            "nothing reached the compositor"
        );

        rig.compositor.release_name(NAME).await.expect("release");
        let eis = eis(&rig, |_| ()).await;
        let refused = eis.connect(&session(), 1).await;
        assert!(
            matches!(&refused, Err(Refused::NotCompositor(why)) if why.contains("no process owns")),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn eis_connect_carries_the_types_and_returns_a_live_socket() {
        let rig = rig("connect").await;
        let eis = eis(&rig, |_| ()).await;
        let fd = eis.connect(&session(), 3).await.expect("fd");
        // The mock keeps the other end: closing the session ends the stream.
        eis.close(&session()).await.expect("close");
        let mut stream = UnixStream::from(fd);
        let mut buf = [0u8; 1];
        assert_eq!(
            stream.read(&mut buf).expect("read"),
            0,
            "end of stream once closed"
        );
        assert_eq!(
            *rig.calls.lock().expect("calls"),
            [
                Call::Connect(SESSION.into(), 3),
                Call::Close(SESSION.into())
            ]
        );
    }

    #[tokio::test]
    async fn eis_a_compositor_refusal_is_an_error_with_no_fd() {
        let rig = rig("invalid").await;
        let eis = eis(&rig, |_| ()).await;
        for types in [0, 8] {
            assert!(matches!(
                eis.connect(&session(), types).await,
                Err(Refused::Compositor(_))
            ));
        }
        eis.connect(&session(), 1).await.expect("first");
        assert!(
            matches!(
                eis.connect(&session(), 1).await,
                Err(Refused::Compositor(_))
            ),
            "a second ConnectToEIS for a held handle"
        );
    }

    #[tokio::test]
    async fn eis_the_system_bus_is_connected_again_once_closed() {
        let session = Bus::start("onbus-session");
        let system = Bus::start("onbus-system");
        let query = session.builder().build().await.expect("query");
        let on = OnBus::new(&query, Some(system.address().to_owned()))
            .await
            .expect("OnBus");
        let first = on.system().await.expect("first connection");
        let again = on.system().await.expect("the same connection");
        assert_eq!(first.unique_name(), again.unique_name(), "kept while open");
        first.clone().close().await.expect("close");
        assert!(first.is_closed());
        let second = on.system().await.expect("connected again");
        assert!(!second.is_closed());
        assert_ne!(first.unique_name(), second.unique_name());
        drop(system);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            on.user_manager().await.is_err(),
            "no system bus: the question fails"
        );
    }
}
