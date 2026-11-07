//! Who a D-Bus peer is: the unit of its process, asked of the bus and of the user's systemd,
//! never of `/proc` (doc_session_daemons.md SD5, SD17). Every question goes through
//! [`Questions`], which the caller implements on a connection that serves nothing (see
//! [`OnBus`]) and bounds with a timeout of its own: nothing here waits for long, and nothing
//! here retries.
//!
//! [`unit_of`] names a peer's unit. [`admit`] admits a peer only when it is the main process
//! of one service of the image, in this order, and refuses at the first step that fails:
//!
//! 1. the bus's credentials of the peer: its user is ours and it carries a pidfd;
//! 2. `GetUnitByPIDFD` on that pidfd names exactly the service's unit;
//! 3. that unit's `MainPID` is the peer's `ProcessID` from the same credentials, its one
//!    `ExecStart` runs exactly the service's executable, and its `FragmentPath` and every
//!    entry of `DropInPaths` lie beneath [`IMAGE_USER_UNITS`], so a transient unit and an
//!    override under `/etc`, `/run` or the user's configuration are refused;
//! 4. after those reads the pidfd still refers to a running process, so the pid compared in
//!    step 3 was not reused.

use std::future::Future;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path};

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::zvariant::{Fd, OwnedObjectPath, OwnedValue};
use zbus::Connection;

/// The user units of the image, read-only under `/usr`.
pub const IMAGE_USER_UNITS: &str = "/usr/lib/systemd/user";

const SYSTEMD: &str = "org.freedesktop.systemd1";

/// The service whose main process [`admit`] admits.
#[derive(Clone, Copy, Debug)]
pub struct Service<'a> {
    /// The unit id, as `GetUnitByPIDFD` returns it.
    pub unit: &'a str,
    /// The path of its one `ExecStart`.
    pub exec: &'a str,
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

/// The three questions, asked of the bus and of the user's systemd.
pub trait Questions {
    /// `GetConnectionCredentials` of `peer`.
    fn credentials(&self, peer: &str) -> impl Future<Output = Result<Credentials, String>> + Send;
    /// `GetUnitByPIDFD`: the unit's object path and its id.
    fn unit_by_pidfd(
        &self,
        pidfd: BorrowedFd<'_>,
    ) -> impl Future<Output = Result<(OwnedObjectPath, String), String>> + Send;
    /// The properties of the unit at `path`.
    fn unit(&self, path: &OwnedObjectPath) -> impl Future<Output = Result<Unit, String>> + Send;
}

/// The unit that holds `peer`'s process: its pidfd from the bus, then `GetUnitByPIDFD`, which a
/// reused pid cannot mislead.
pub async fn unit_of(questions: &impl Questions, peer: &str) -> Result<String, String> {
    let credentials = questions.credentials(peer).await?;
    let pidfd = pidfd_of(&credentials)?;
    Ok(questions.unit_by_pidfd(pidfd).await?.1)
}

/// `Ok` only when `peer` is the main process of `service`, checked as the module says; the
/// error says which check refused it.
pub async fn admit(
    questions: &impl Questions,
    peer: &str,
    service: Service<'_>,
) -> Result<(), String> {
    let credentials = questions.credentials(peer).await?;
    let ours = nix::unistd::getuid().as_raw();
    if credentials.uid != Some(ours) {
        return Err(format!("user {:?} is not {ours}", credentials.uid));
    }
    let pidfd = pidfd_of(&credentials)?;
    let (path, unit) = questions.unit_by_pidfd(pidfd).await?;
    if unit != service.unit {
        return Err(format!("unit {unit} is not {}", service.unit));
    }
    let properties = questions.unit(&path).await?;
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
    Ok(())
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

/// [`Questions`] on the bus: `bus` is the bus's proxy and `systemd` the connection systemd is
/// asked on, both of a connection that serves nothing, subscribes to nothing and owns no
/// name, so a handler can await them (issue #1035).
pub struct OnBus<'a> {
    pub bus: &'a DBusProxy<'static>,
    pub systemd: &'a Connection,
}

impl OnBus<'_> {
    async fn property(
        &self,
        path: &OwnedObjectPath,
        interface: &str,
        name: &str,
    ) -> Result<OwnedValue, String> {
        let reply = self
            .systemd
            .call_method(
                Some(SYSTEMD),
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

impl Questions for OnBus<'_> {
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
        pidfd: BorrowedFd<'_>,
    ) -> Result<(OwnedObjectPath, String), String> {
        let reply = self
            .systemd
            .call_method(
                Some(SYSTEMD),
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

    async fn unit(&self, path: &OwnedObjectPath) -> Result<Unit, String> {
        const SERVICE: &str = "org.freedesktop.systemd1.Service";
        const UNIT: &str = "org.freedesktop.systemd1.Unit";
        let convert = |name: &str, e: zbus::zvariant::Error| format!("{name}: {e}");
        let main_pid = u32::try_from(self.property(path, SERVICE, "MainPID").await?)
            .map_err(|e| convert("MainPID", e))?;
        let exec_start = Vec::<Exec>::try_from(self.property(path, SERVICE, "ExecStart").await?)
            .map_err(|e| convert("ExecStart", e))?
            .into_iter()
            .map(|exec| exec.0)
            .collect();
        let fragment_path = String::try_from(self.property(path, UNIT, "FragmentPath").await?)
            .map_err(|e| convert("FragmentPath", e))?;
        let drop_in_paths =
            Vec::<String>::try_from(self.property(path, UNIT, "DropInPaths").await?)
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
    const PID: u32 = 4242;

    /// What the fake bus and systemd answer. Its "pidfd" is the read end of a pipe, which
    /// polls readable, as a pidfd of an ended process does, once its write end is closed.
    struct Fake {
        uid: Option<u32>,
        pid: Option<u32>,
        pidfd: bool,
        unit: &'static str,
        properties: Unit,
        /// Keeps the pipe's write end open: the "process" runs.
        _writer: Option<OwnedFd>,
        reader: OwnedFd,
    }

    impl Fake {
        fn portal() -> Fake {
            let (reader, writer) = nix::unistd::pipe().expect("pipe");
            Fake {
                uid: Some(nix::unistd::getuid().as_raw()),
                pid: Some(PID),
                pidfd: true,
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
                _writer: Some(writer),
                reader,
            }
        }
    }

    impl Questions for Fake {
        async fn credentials(&self, _peer: &str) -> Result<Credentials, String> {
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

        async fn unit_by_pidfd(
            &self,
            _pidfd: BorrowedFd<'_>,
        ) -> Result<(OwnedObjectPath, String), String> {
            let path = OwnedObjectPath::try_from("/org/freedesktop/systemd1/unit/x")
                .map_err(|e| e.to_string())?;
            Ok((path, self.unit.into()))
        }

        async fn unit(&self, _path: &OwnedObjectPath) -> Result<Unit, String> {
            Ok(self.properties.clone())
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
        let error = now(admit(fake, ":1.7", PORTAL)).expect_err("refused");
        assert!(error.contains(why), "{error}");
    }

    #[test]
    fn the_portal_with_the_image_s_drop_in_is_admitted() {
        let fake = Fake::portal();
        assert_eq!(now(admit(&fake, ":1.7", PORTAL)), Ok(()));
        assert_eq!(now(unit_of(&fake, ":1.7")).as_deref(), Ok(PORTAL.unit));
    }

    #[test]
    fn another_user_is_refused() {
        let mut fake = Fake::portal();
        fake.uid = fake.uid.map(|u| u + 1);
        refused(&fake, "is not");
        fake.uid = None;
        refused(&fake, "user None");
    }

    #[test]
    fn no_pidfd_is_refused() {
        let mut fake = Fake::portal();
        fake.pidfd = false;
        refused(&fake, "no ProcessFD");
        assert!(now(unit_of(&fake, ":1.7")).is_err());
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
        fake.pid = Some(PID + 1);
        refused(&fake, "is not the main process");
        fake.pid = None;
        refused(&fake, "is not the main process");
        fake.pid = Some(0);
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
        fake._writer = None;
        refused(&fake, "ended");
    }
}
