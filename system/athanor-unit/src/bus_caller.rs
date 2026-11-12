//! Who a D-Bus peer is: the unit of its process, asked of the bus and of systemd, never of
//! `/proc` (doc_session_daemons.md SD5, SD17). Every question goes through [`Questions`], which
//! the caller implements on connections that serve nothing (see [`OnBus`]) and bounds with a
//! timeout of its own: nothing here waits for long, and nothing here retries.
//!
//! [`unit_of`] names a peer's unit, as the session bus's `org.freedesktop.systemd1` tells it: a
//! record, not a check. [`admit`] admits a peer only when it is the main process of one service
//! of the image, in this order, and refuses at the first step that fails:
//!
//! 1. the bus's credentials of the peer: its user is ours and it carries a pidfd;
//! 2. the user's systemd is verified: the owner of `org.freedesktop.systemd1` on the session
//!    bus is a unique name whose credentials give our user and a pidfd, and the system's
//!    systemd, on the system bus, places that pidfd in `user@<uid>.service` with that owner's
//!    `ProcessID` as its `MainPID`, so a process of the user that took the name is refused;
//! 3. that manager, asked by its unique name and never by the well-known one, places the
//!    peer's pidfd in exactly the service's unit;
//! 4. that unit's `MainPID` is the peer's `ProcessID` from the same credentials, its one
//!    `ExecStart` runs exactly the service's executable, and its `FragmentPath` and every
//!    entry of `DropInPaths` lie beneath [`IMAGE_USER_UNITS`], so a transient unit and an
//!    override under `/etc`, `/run` or the user's configuration are refused;
//! 5. after those reads both pidfds, the peer's and the manager's, still refer to running
//!    processes, so neither pid compared above was reused.

use std::future::Future;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path};

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::Connection;
use zbus::{Fd, OwnedObjectPath, OwnedVariant};

/// The user units of the image, read-only under `/usr`.
pub const IMAGE_USER_UNITS: &str = "/usr/lib/systemd/user";

/// The well-known name of systemd on either bus.
pub const SYSTEMD: &str = "org.freedesktop.systemd1";

/// The system bus socket, fixed: `Builder::system()` would honour `DBUS_SYSTEM_BUS_ADDRESS`,
/// which a process of the user can set for a service through the user manager's environment.
const SYSTEM_BUS: &str = "unix:path=/run/dbus/system_bus_socket";

/// A builder for the connection to the system bus that [`OnBus`] asks, at [`SYSTEM_BUS`]
/// whatever the environment says. Every `system` connection given to [`OnBus`] comes from here.
pub fn system_bus() -> zbus::connection::Builder<'static> {
    zbus::connection::Builder::address(SYSTEM_BUS)
}

/// The service whose main process [`admit`] admits.
#[derive(Clone, Copy, Debug)]
pub struct Service<'a> {
    /// The unit id, as `GetUnitByPIDFD` returns it.
    pub unit: &'a str,
    /// The path of its one `ExecStart`.
    pub exec: &'a str,
}

/// Which systemd a question goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager<'a> {
    /// The user's, on the session bus, at this bus name.
    User(&'a str),
    /// The system's, on the system bus.
    System,
}

/// What the bus says of a peer (`GetConnectionCredentials`).
#[derive(Debug)]
pub struct Credentials {
    pub uid: Option<u32>,
    pub pid: Option<u32>,
    pub pidfd: Option<OwnedFd>,
}

/// The properties of a unit [`admit`] reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unit {
    pub main_pid: u32,
    /// The path of each `ExecStart` entry.
    pub exec_start: Vec<String>,
    pub fragment_path: String,
    pub drop_in_paths: Vec<String>,
}

/// The questions, asked of the buses and of systemd.
pub trait Questions {
    /// `GetNameOwner` of [`SYSTEMD`] on the session bus.
    fn user_manager(&self) -> impl Future<Output = Result<String, String>> + Send;
    /// `GetConnectionCredentials` of `peer` on the session bus.
    fn credentials(&self, peer: &str) -> impl Future<Output = Result<Credentials, String>> + Send;
    /// `GetUnitByPIDFD`: the unit's object path and its id.
    fn unit_by_pidfd(
        &self,
        manager: Manager<'_>,
        pidfd: BorrowedFd<'_>,
    ) -> impl Future<Output = Result<(OwnedObjectPath, String), String>> + Send;
    /// The `MainPID` of the service at `path`.
    fn main_pid(
        &self,
        manager: Manager<'_>,
        path: &OwnedObjectPath,
    ) -> impl Future<Output = Result<u32, String>> + Send;
    /// The properties of the unit at `path`.
    fn unit(
        &self,
        manager: Manager<'_>,
        path: &OwnedObjectPath,
    ) -> impl Future<Output = Result<Unit, String>> + Send;
}

/// The unit that holds `peer`'s process: its pidfd from the bus, then `GetUnitByPIDFD` of the
/// session bus's systemd, which a reused pid cannot mislead. The manager is not verified, so
/// the answer is only fit to be recorded.
pub async fn unit_of(questions: &impl Questions, peer: &str) -> Result<String, String> {
    let credentials = questions.credentials(peer).await?;
    let pidfd = pidfd_of(&credentials)?;
    Ok(questions
        .unit_by_pidfd(Manager::User(SYSTEMD), pidfd)
        .await?
        .1)
}

/// `Ok` only when `peer` is the main process of `service`, checked as the module says; the
/// error says which check refused it.
pub async fn admit(
    questions: &impl Questions,
    peer: &str,
    service: Service<'_>,
) -> Result<(), String> {
    let ours = nix::unistd::getuid().as_raw();
    let credentials = questions.credentials(peer).await?;
    if credentials.uid != Some(ours) {
        return Err(format!("user {:?} is not {ours}", credentials.uid));
    }
    let pidfd = pidfd_of(&credentials)?;
    let manager = user_manager(questions, ours).await?;
    let systemd = Manager::User(&manager.name);
    let (path, unit) = questions.unit_by_pidfd(systemd, pidfd).await?;
    if unit != service.unit {
        return Err(format!("unit {unit} is not {}", service.unit));
    }
    let properties = questions.unit(systemd, &path).await?;
    if properties.main_pid == 0 || credentials.pid != Some(properties.main_pid) {
        return Err(format!(
            "process {:?} is not the main process {} of {unit}",
            credentials.pid, properties.main_pid
        ));
    }
    if properties.exec_start != [service.exec] {
        return Err(format!(
            "{unit} runs {:?}, not {}",
            properties.exec_start, service.exec
        ));
    }
    if !in_image(&properties.fragment_path) {
        return Err(format!(
            "{unit} is defined by {:?}, outside {IMAGE_USER_UNITS}",
            properties.fragment_path
        ));
    }
    if let Some(drop_in) = properties.drop_in_paths.iter().find(|p| !in_image(p)) {
        return Err(format!(
            "{unit} has the drop-in {drop_in}, outside {IMAGE_USER_UNITS}"
        ));
    }
    if !alive(pidfd)? {
        return Err("the process ended while it was checked".into());
    }
    if !alive(pidfd_of(&manager.credentials)?)? {
        return Err("the user's systemd ended while it was checked".into());
    }
    Ok(())
}

/// The user's systemd as [`admit`] found it: its unique name on the session bus, and what the
/// bus says of it.
struct UserManager {
    name: String,
    credentials: Credentials,
}

/// Step 2 of the module: the owner of [`SYSTEMD`] on the session bus is the main process of
/// `user@<ours>.service`, as the system's systemd tells it.
async fn user_manager(questions: &impl Questions, ours: u32) -> Result<UserManager, String> {
    let name = questions.user_manager().await?;
    let credentials = questions.credentials(&name).await?;
    if credentials.uid != Some(ours) {
        return Err(format!(
            "the owner {name} of {SYSTEMD} is user {:?}, not {ours}",
            credentials.uid
        ));
    }
    let pidfd =
        pidfd_of(&credentials).map_err(|e| format!("the owner {name} of {SYSTEMD}: {e}"))?;
    let expected = format!("user@{ours}.service");
    let (path, unit) = questions.unit_by_pidfd(Manager::System, pidfd).await?;
    if unit != expected {
        return Err(format!(
            "the owner {name} of {SYSTEMD} runs in {unit}, not {expected}"
        ));
    }
    let main_pid = questions.main_pid(Manager::System, &path).await?;
    if main_pid == 0 || credentials.pid != Some(main_pid) {
        return Err(format!(
            "the owner {name} of {SYSTEMD} is process {:?}, not the main process {main_pid} of {expected}",
            credentials.pid
        ));
    }
    Ok(UserManager { name, credentials })
}

fn pidfd_of(credentials: &Credentials) -> Result<BorrowedFd<'_>, String> {
    credentials
        .pidfd
        .as_ref()
        .map(AsFd::as_fd)
        .ok_or_else(|| "the bus gives no ProcessFD".to_owned())
}

/// A file strictly beneath [`IMAGE_USER_UNITS`], with no `..` on the way.
fn in_image(path: &str) -> bool {
    let path = Path::new(path);
    path != Path::new(IMAGE_USER_UNITS)
        && path.starts_with(IMAGE_USER_UNITS)
        && path.components().all(|c| c != Component::ParentDir)
}

/// Whether the pidfd's process still runs: a pidfd turns readable when its process exits.
/// Any event at all, or one `poll` cannot name, counts as ended.
pub fn alive(pidfd: BorrowedFd<'_>) -> Result<bool, String> {
    let mut fds = [PollFd::new(pidfd, PollFlags::POLLIN)];
    poll(&mut fds, PollTimeout::ZERO).map_err(|e| format!("poll of the pidfd: {e}"))?;
    Ok(fds[0].revents().is_some_and(|events| events.is_empty()))
}

/// [`Questions`] on the buses: `bus` is the session bus's proxy on `session`, and `system` a
/// connection to the system bus. Each serves nothing, subscribes to nothing and owns no name,
/// so a handler can await them (issue #1035).
pub struct OnBus<'a> {
    pub bus: &'a DBusProxy<'static>,
    pub session: &'a Connection,
    pub system: &'a Connection,
}

impl OnBus<'_> {
    fn at<'m>(&self, manager: Manager<'m>) -> (&Connection, &'m str) {
        match manager {
            Manager::User(name) => (self.session, name),
            Manager::System => (self.system, SYSTEMD),
        }
    }

    async fn property(
        &self,
        manager: Manager<'_>,
        path: &OwnedObjectPath,
        interface: &str,
        name: &str,
    ) -> Result<OwnedVariant, String> {
        let (connection, destination) = self.at(manager);
        let reply = connection
            .call_method(
                Some(destination),
                path,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(interface, name),
            )
            .await
            .map_err(|e| format!("{name}: {e}"))?;
        reply
            .body()
            .deserialize()
            .map_err(|e| format!("{name}: {e}"))
    }
}

/// One `ExecStart` entry, `(sasbttttuii)`.
type Exec = (String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32);

const SERVICE: &str = "org.freedesktop.systemd1.Service";
const UNIT: &str = "org.freedesktop.systemd1.Unit";

fn convert(name: &str, e: zbus::Error) -> String {
    format!("{name}: {e}")
}

impl Questions for OnBus<'_> {
    async fn user_manager(&self) -> Result<String, String> {
        let name = BusName::try_from(SYSTEMD).map_err(|e| e.to_string())?;
        let owner = self
            .bus
            .get_name_owner(name)
            .await
            .map_err(|e| format!("the owner of {SYSTEMD}: {e}"))?;
        Ok(owner.to_string())
    }

    async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
        let name = BusName::try_from(peer).map_err(|e| e.to_string())?;
        let found = self
            .bus
            .get_connection_credentials(name)
            .await
            .map_err(|e| e.to_string())?;
        let pidfd = match found.process_fd() {
            Some(fd) => Some(fd.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(Credentials {
            uid: found.unix_user_id(),
            pid: found.process_id(),
            pidfd,
        })
    }

    async fn unit_by_pidfd(
        &self,
        manager: Manager<'_>,
        pidfd: BorrowedFd<'_>,
    ) -> Result<(OwnedObjectPath, String), String> {
        let (connection, destination) = self.at(manager);
        let reply = connection
            .call_method(
                Some(destination),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "GetUnitByPIDFD",
                &(Fd::from(pidfd),),
            )
            .await
            .map_err(|e| e.to_string())?;
        let (path, unit, _invocation): (OwnedObjectPath, String, Vec<u8>) =
            reply.body().deserialize().map_err(|e| e.to_string())?;
        Ok((path, unit))
    }

    async fn main_pid(&self, manager: Manager<'_>, path: &OwnedObjectPath) -> Result<u32, String> {
        u32::try_from(self.property(manager, path, SERVICE, "MainPID").await?)
            .map_err(|e| convert("MainPID", e))
    }

    async fn unit(&self, manager: Manager<'_>, path: &OwnedObjectPath) -> Result<Unit, String> {
        let main_pid = self.main_pid(manager, path).await?;
        let exec_start =
            Vec::<Exec>::try_from(self.property(manager, path, SERVICE, "ExecStart").await?)
                .map_err(|e| convert("ExecStart", e))?
                .into_iter()
                .map(|exec| exec.0)
                .collect();
        let fragment_path =
            String::try_from(self.property(manager, path, UNIT, "FragmentPath").await?)
                .map_err(|e| convert("FragmentPath", e))?;
        let drop_in_paths =
            Vec::<String>::try_from(self.property(manager, path, UNIT, "DropInPaths").await?)
                .map_err(|e| convert("DropInPaths", e))?;
        Ok(Unit {
            main_pid,
            exec_start,
            fragment_path,
            drop_in_paths,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    const PORTAL: Service = Service {
        unit: "xdg-desktop-portal-athanor.service",
        exec: "/usr/libexec/xdg-desktop-portal-athanor",
    };
    #[test]
    fn the_system_bus_is_the_fixed_socket_whatever_the_environment_says() {
        // Edition 2021: `set_var` is safe. Nothing else in this crate's tests reads this variable.
        std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent/bogus_bus");
        let builder = format!("{:?}", system_bus());
        assert!(builder.contains("/run/dbus/system_bus_socket"), "{builder}");
        assert!(!builder.contains("bogus_bus"), "{builder}");
    }

    const PID: u32 = 4242;
    const CALLER: &str = ":1.7";
    /// The unique name of the user's systemd on the fake session bus.
    const MANAGER: &str = ":1.2";
    const MANAGER_PID: u32 = 1717;

    /// A peer of the fake bus. Its "pidfd" is the read end of a pipe, which polls readable, as
    /// a pidfd of an ended process does, once its write end is closed.
    struct Process {
        uid: Option<u32>,
        pid: Option<u32>,
        pidfd: bool,
        /// Keeps the pipe's write end open: the "process" runs.
        _writer: Option<OwnedFd>,
        reader: OwnedFd,
    }

    impl Process {
        fn new(pid: u32) -> Process {
            let (reader, writer) = nix::unistd::pipe().expect("pipe");
            Process {
                uid: Some(nix::unistd::getuid().as_raw()),
                pid: Some(pid),
                pidfd: true,
                _writer: Some(writer),
                reader,
            }
        }

        fn answer(&self) -> Result<Credentials, String> {
            let pidfd = match self.pidfd {
                true => Some(self.reader.try_clone().map_err(|e| e.to_string())?),
                false => None,
            };
            Ok(Credentials {
                uid: self.uid,
                pid: self.pid,
                pidfd,
            })
        }
    }

    /// What the fake buses and the two systemds answer. Asked at its well-known name, the
    /// session bus's systemd answers as a process that took the name would.
    struct Fake {
        caller: Process,
        unit: &'static str,
        properties: Unit,
        manager: Process,
        /// What the system's systemd says of the manager's pidfd.
        manager_unit: String,
        manager_main_pid: u32,
    }

    impl Fake {
        fn portal() -> Fake {
            Fake {
                caller: Process::new(PID),
                unit: PORTAL.unit,
                properties: Unit {
                    main_pid: PID,
                    exec_start: vec![PORTAL.exec.into()],
                    fragment_path: "/usr/lib/systemd/user/xdg-desktop-portal-athanor.service"
                        .into(),
                    drop_in_paths: vec![
                        "/usr/lib/systemd/user/service.d/10-timeout-abort.conf".into()
                    ],
                },
                manager: Process::new(MANAGER_PID),
                manager_unit: format!("user@{}.service", nix::unistd::getuid().as_raw()),
                manager_main_pid: MANAGER_PID,
            }
        }
    }

    const IMPOSTOR: &str = "impostor.service";

    fn path() -> Result<OwnedObjectPath, String> {
        OwnedObjectPath::try_from("/org/freedesktop/systemd1/unit/x").map_err(|e| e.to_string())
    }

    impl Questions for Fake {
        async fn user_manager(&self) -> Result<String, String> {
            Ok(MANAGER.into())
        }

        async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
            match peer {
                CALLER => self.caller.answer(),
                MANAGER => self.manager.answer(),
                _ => Err(format!("no peer {peer}")),
            }
        }

        async fn unit_by_pidfd(
            &self,
            manager: Manager<'_>,
            _pidfd: BorrowedFd<'_>,
        ) -> Result<(OwnedObjectPath, String), String> {
            match manager {
                Manager::User(MANAGER) => Ok((path()?, self.unit.into())),
                Manager::User(_) => Ok((path()?, IMPOSTOR.into())),
                Manager::System => Ok((path()?, self.manager_unit.clone())),
            }
        }

        async fn main_pid(
            &self,
            manager: Manager<'_>,
            _path: &OwnedObjectPath,
        ) -> Result<u32, String> {
            match manager {
                Manager::System => Ok(self.manager_main_pid),
                Manager::User(_) => Err("asked the user's systemd".into()),
            }
        }

        async fn unit(
            &self,
            manager: Manager<'_>,
            _path: &OwnedObjectPath,
        ) -> Result<Unit, String> {
            match manager {
                Manager::User(MANAGER) => Ok(self.properties.clone()),
                other => Err(format!("asked {other:?}")),
            }
        }
    }

    /// The fake never waits, so one poll completes the check.
    fn now<T>(future: impl Future<Output = T>) -> T {
        match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("the fake never waits"),
        }
    }

    fn refused(fake: &Fake, why: &str) {
        let error = now(admit(fake, CALLER, PORTAL)).expect_err("refused");
        assert!(error.contains(why), "{error}");
    }

    #[test]
    fn the_portal_with_the_image_s_drop_in_is_admitted() {
        // The fake answers the portal's unit only at the manager's unique name.
        let fake = Fake::portal();
        assert_eq!(now(admit(&fake, CALLER, PORTAL)), Ok(()));
    }

    #[test]
    fn unit_of_asks_the_well_known_name_and_is_only_a_record() {
        let fake = Fake::portal();
        assert_eq!(now(unit_of(&fake, CALLER)).as_deref(), Ok(IMPOSTOR));
    }

    #[test]
    fn another_user_is_refused() {
        let mut fake = Fake::portal();
        fake.caller.uid = fake.caller.uid.map(|u| u + 1);
        refused(&fake, "is not");
        fake.caller.uid = None;
        refused(&fake, "user None");
    }

    #[test]
    fn no_pidfd_is_refused() {
        let mut fake = Fake::portal();
        fake.caller.pidfd = false;
        refused(&fake, "no ProcessFD");
        assert!(now(unit_of(&fake, CALLER)).is_err());
    }

    #[test]
    fn a_manager_of_another_user_is_refused() {
        let mut fake = Fake::portal();
        fake.manager.uid = fake.manager.uid.map(|u| u + 1);
        refused(&fake, "of org.freedesktop.systemd1 is user");
        fake.manager.uid = None;
        refused(&fake, "of org.freedesktop.systemd1 is user None");
    }

    #[test]
    fn a_manager_without_a_pidfd_is_refused() {
        let mut fake = Fake::portal();
        fake.manager.pidfd = false;
        refused(
            &fake,
            "of org.freedesktop.systemd1: the bus gives no ProcessFD",
        );
    }

    #[test]
    fn a_manager_outside_the_user_s_service_is_refused() {
        // Another user's manager: the tests may run as root, whose is user@0.service.
        let another = format!("user@{}.service", nix::unistd::getuid().as_raw() + 1);
        for unit in ["app-athanor-org.example.App@1.service", &another, ""] {
            let mut fake = Fake::portal();
            fake.manager_unit = unit.into();
            refused(&fake, "runs in");
        }
    }

    #[test]
    fn a_manager_that_is_not_the_service_s_main_process_is_refused() {
        let mut fake = Fake::portal();
        fake.manager_main_pid = MANAGER_PID + 1;
        refused(&fake, "not the main process 1718 of user@");
        fake.manager_main_pid = 0;
        fake.manager.pid = Some(0);
        refused(&fake, "of user@");
        fake.manager.pid = None;
        refused(&fake, "of user@");
    }

    #[test]
    fn a_manager_that_ended_is_refused() {
        let mut fake = Fake::portal();
        fake.manager._writer = None;
        refused(&fake, "the user's systemd ended");
    }

    #[test]
    fn another_unit_is_refused() {
        let mut fake = Fake::portal();
        fake.unit = "app-athanor-org.example.App@1.service";
        refused(&fake, "is not xdg-desktop-portal-athanor.service");
    }

    #[test]
    fn a_process_that_is_not_the_main_one_is_refused() {
        let mut fake = Fake::portal();
        fake.caller.pid = Some(PID + 1);
        refused(&fake, "is not the main process");
        fake.caller.pid = None;
        refused(&fake, "is not the main process");
        fake.caller.pid = Some(0);
        fake.properties.main_pid = 0;
        refused(&fake, "is not the main process");
    }

    #[test]
    fn another_executable_is_refused() {
        let mut fake = Fake::portal();
        fake.properties.exec_start = vec!["/usr/bin/python3".into()];
        refused(&fake, "runs");
        fake.properties.exec_start = vec![PORTAL.exec.into(), PORTAL.exec.into()];
        refused(&fake, "runs");
        fake.properties.exec_start.clear();
        refused(&fake, "runs");
    }

    #[test]
    fn a_unit_file_outside_the_image_is_refused() {
        for fragment in [
            "/etc/systemd/user/xdg-desktop-portal-athanor.service",
            "/home/u/.config/systemd/user/xdg-desktop-portal-athanor.service",
            "/run/user/1000/systemd/transient/xdg-desktop-portal-athanor.service",
            "/usr/lib/systemd/user/../../../etc/x.service",
            "/usr/lib/systemd/user",
            "",
        ] {
            let mut fake = Fake::portal();
            fake.properties.fragment_path = fragment.into();
            refused(&fake, "is defined by");
        }
    }

    #[test]
    fn a_drop_in_outside_the_image_is_refused() {
        for drop_in in [
            "/etc/systemd/user/xdg-desktop-portal-athanor.service.d/override.conf",
            "/home/u/.config/systemd/user/service.d/x.conf",
            "/run/systemd/user/xdg-desktop-portal-athanor.service.d/x.conf",
        ] {
            let mut fake = Fake::portal();
            fake.properties.drop_in_paths.push(drop_in.into());
            refused(&fake, "has the drop-in");
        }
    }

    #[test]
    fn a_process_that_ended_is_refused() {
        let mut fake = Fake::portal();
        fake.caller._writer = None;
        refused(&fake, "the process ended");
    }
}
