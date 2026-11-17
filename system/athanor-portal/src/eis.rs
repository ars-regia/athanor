//! The compositor's EIS socket, as the backend reaches it (doc_portal.md PT15;
//! doc_own_stack.md decision 33, wave 2c): `os.athanor.Eis1` at `/os/athanor/Eis1`, under the
//! name `os.athanor.Eis` on the session bus.
//!
//! Before the dialog, and again before each call, the owner of `os.athanor.Eis` is read anew
//! and checked to be the compositor: its process, through the pidfd the bus returns or, when it
//! returns none, through one opened on its pid, lies in the cgroup of `athanor-sulphur.service`
//! and runs `/usr/bin/athanor-sulphur`, the very file at that path. A pid read without a pidfd
//! is asked of the bus again after the reads, so a pid reused in between is refused. The call
//! then goes to the unique name that was checked, never to the well-known one, so the name
//! cannot change hands between the check and the call. Every failure refuses: no owner, a bus
//! that does not answer in time, any read that fails.

#![allow(unsafe_code)]

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::Connection;
use zbus::ObjectPath;

/// The compositor's name and object.
pub const NAME: &str = "os.athanor.Eis";
pub const PATH: &str = "/os/athanor/Eis1";
pub const INTERFACE: &str = "os.athanor.Eis1";
/// The compositor's unit, its slice and its executable.
pub const UNIT: &str = "athanor-sulphur.service";
pub const SLICE: &str = "session.slice";
pub const EXECUTABLE: &str = "/usr/bin/athanor-sulphur";

/// Bound on each check and each call to the bus.
pub const ASK: Duration = Duration::from_secs(2);

/// What the bus says of a peer (`GetConnectionCredentials`).
#[derive(Debug, Default)]
pub struct Credentials {
    pub pid: Option<u32>,
    pub pidfd: Option<OwnedFd>,
}

/// The seam of the owner check: what the bus answers, and what `/proc` says of a pid. CI runs
/// the check through it with fixture answers; the backend through [`OnBus`].
pub trait Identity {
    /// The unique name that owns [`NAME`], `None` when nobody does.
    fn owner(&self) -> impl std::future::Future<Output = Result<Option<String>, String>> + Send;
    /// `GetConnectionCredentials` of `peer`.
    fn credentials(
        &self,
        peer: &str,
    ) -> impl std::future::Future<Output = Result<Credentials, String>> + Send;
    /// `/proc/<pid>/cgroup`.
    fn cgroup(&self, pid: i32) -> io::Result<String>;
    /// The target of `/proc/<pid>/exe`.
    fn executable(&self, pid: i32) -> io::Result<PathBuf>;
    /// Whether `/proc/<pid>/exe` is the file now at `path`: same device and inode.
    fn runs_file(&self, pid: i32, path: &Path) -> io::Result<bool>;
}

/// The cgroup a main process of the compositor's unit reads, for the user `uid`.
pub fn compositor_cgroup(uid: u32) -> String {
    athanor_unit::peer::user_unit_cgroup(uid, SLICE, UNIT)
}

/// The unique name of the compositor, checked as the module says, or why not.
pub async fn compositor(identity: &impl Identity) -> Result<String, String> {
    let owner = identity
        .owner()
        .await?
        .ok_or_else(|| format!("no process owns {NAME}"))?;
    let credentials = identity.credentials(&owner).await?;
    let opened;
    let pidfd = match &credentials.pidfd {
        Some(fd) => fd.as_fd(),
        None => {
            let pid = credentials
                .pid
                .ok_or_else(|| format!("the bus names neither a pidfd nor a pid for {owner}"))?;
            opened = pidfd_open(pid).map_err(|e| format!("no pidfd for pid {pid}: {e}"))?;
            opened.as_fd()
        }
    };
    let pid = pidfd_pid(pidfd)?;
    let want = compositor_cgroup(nix::unistd::getuid().as_raw());
    let cgroup = identity
        .cgroup(pid)
        .map_err(|e| format!("the cgroup of {owner}: {e}"))?;
    if cgroup.strip_suffix('\n').unwrap_or(&cgroup) != want {
        return Err(format!("{owner} is not in {UNIT}: {cgroup:?}"));
    }
    let executable = identity
        .executable(pid)
        .map_err(|e| format!("the executable of {owner}: {e}"))?;
    let runs = identity
        .runs_file(pid, Path::new(EXECUTABLE))
        .map_err(|e| format!("the executable of {owner}: {e}"))?;
    if executable != Path::new(EXECUTABLE) || !runs {
        return Err(format!(
            "{owner} runs {}, not {EXECUTABLE}",
            executable.display()
        ));
    }
    if pidfd_pid(pidfd)? != pid || !athanor_unit::bus_caller::alive(pidfd)? {
        return Err(format!("{owner} exited while it was read"));
    }
    if credentials.pidfd.is_none() {
        // The pidfd was opened after the bus named the pid: the connection, still open with
        // that pid, shows the pid was not reused in between.
        let again = identity.credentials(&owner).await?;
        if again.pid.map(i64::from) != Some(i64::from(pid)) {
            return Err(format!("{owner}'s pid changed while it was read"));
        }
    }
    Ok(owner)
}

fn pidfd_open(pid: u32) -> io::Result<OwnedFd> {
    let pid = nix::libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    // SAFETY: pidfd_open takes a pid and flags by value and touches no memory of this process.
    let fd = unsafe { nix::libc::syscall(nix::libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = nix::libc::c_int::try_from(fd).map_err(io::Error::other)?;
    // SAFETY: on success the kernel installed a new pidfd at `fd` that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The pid `pidfd` names, from its fdinfo. A process reaped (-1) or outside this pid
/// namespace (0) is refused.
fn pidfd_pid(pidfd: BorrowedFd<'_>) -> Result<i32, String> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd()))
        .map_err(|e| format!("the pidfd's fdinfo: {e}"))?;
    let pid: i32 = info
        .lines()
        .find_map(|line| line.strip_prefix("Pid:"))
        .ok_or("the pidfd's fdinfo names no pid")?
        .trim()
        .parse()
        .map_err(|e| format!("the pidfd's pid: {e}"))?;
    if pid <= 0 {
        return Err(format!(
            "the process is gone or outside the namespace (pid {pid})"
        ));
    }
    Ok(pid)
}

/// [`Identity`] on the bus and `/proc`. `bus` is a proxy on a connection that serves nothing,
/// so a handler can await it (issue #1035).
pub struct OnBus {
    pub bus: DBusProxy<'static>,
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

    async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
        let name = BusName::try_from(peer).map_err(|e| e.to_string())?;
        let credentials = self
            .bus
            .get_connection_credentials(name)
            .await
            .map_err(|e| format!("the credentials of {peer}: {e}"))?;
        let pidfd = credentials
            .process_fd()
            .map(|fd| fd.as_fd().try_clone_to_owned())
            .transpose()
            .map_err(|e| format!("the pidfd of {peer}: {e}"))?;
        Ok(Credentials {
            pid: credentials.process_id(),
            pidfd,
        })
    }

    fn cgroup(&self, pid: i32) -> io::Result<String> {
        std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
    }

    fn executable(&self, pid: i32) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
    }

    fn runs_file(&self, pid: i32, path: &Path) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt;
        let running = std::fs::metadata(format!("/proc/{pid}/exe"))?;
        let file = std::fs::metadata(path)?;
        Ok(running.dev() == file.dev() && running.ino() == file.ino())
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
        held: Arc<Mutex<Vec<(String, UnixStream)>>>,
        /// Every `ConnectToEIS` is refused, as a compositor that cannot open a context does.
        pub(crate) refuse: Arc<AtomicBool>,
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
        }
    }

    /// The identity seam: the bus's real answer for the owner and its credentials, with the
    /// pidfd hidden for the pid fallback and the pid changed on every second read for reuse, and
    /// fixture answers for what `/proc` says of the owner's process.
    pub(crate) struct Fixture {
        bus: OnBus,
        pub(crate) cgroup: String,
        pub(crate) executable: PathBuf,
        pub(crate) same_file: bool,
        pub(crate) hide_pidfd: bool,
        pub(crate) reuse_pid: bool,
        asked: Mutex<u32>,
    }

    impl Fixture {
        pub(crate) async fn new(conn: &Connection) -> Fixture {
            Fixture {
                bus: OnBus {
                    bus: DBusProxy::new(conn).await.expect("proxy"),
                },
                cgroup: format!("{}\n", compositor_cgroup(nix::unistd::getuid().as_raw())),
                executable: PathBuf::from(EXECUTABLE),
                same_file: true,
                hide_pidfd: false,
                reuse_pid: false,
                asked: Mutex::new(0),
            }
        }
    }

    impl Identity for Fixture {
        async fn owner(&self) -> Result<Option<String>, String> {
            self.bus.owner().await
        }

        async fn credentials(&self, peer: &str) -> Result<Credentials, String> {
            let mut credentials = self.bus.credentials(peer).await?;
            let asked = {
                let mut asked = self.asked.lock().expect("asked");
                *asked += 1;
                *asked
            };
            if self.hide_pidfd {
                credentials.pidfd = None;
            }
            if self.reuse_pid && asked % 2 == 0 {
                credentials.pid = credentials.pid.map(|pid| pid + 1);
            }
            Ok(credentials)
        }

        fn cgroup(&self, _pid: i32) -> io::Result<String> {
            Ok(self.cgroup.clone())
        }

        fn executable(&self, _pid: i32) -> io::Result<PathBuf> {
            Ok(self.executable.clone())
        }

        fn runs_file(&self, _pid: i32, _path: &Path) -> io::Result<bool> {
            Ok(self.same_file)
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
        let mut identity = Fixture::new(&rig.backend).await;
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
    async fn eis_the_compositor_is_its_unit_and_executable_through_the_pidfd_and_the_pid() {
        let rig = rig("owner").await;
        let owner = rig.compositor.unique_name().expect("name").to_string();
        assert_eq!(
            eis(&rig, |_| ()).await.check().await.expect("admitted"),
            owner
        );
        let by_pid = eis(&rig, |f| f.hide_pidfd = true).await;
        assert_eq!(by_pid.check().await.expect("admitted by pid"), owner);
    }

    #[tokio::test]
    async fn eis_no_owner_another_unit_or_another_executable_is_refused_and_nothing_sent() {
        let rig = rig("refused").await;
        type Change = Box<dyn FnOnce(&mut Fixture)>;
        let refusals: [(&str, Change); 6] = [
            (
                "another unit",
                Box::new(|f| {
                    f.cgroup = athanor_unit::peer::user_unit_cgroup(
                        nix::unistd::getuid().as_raw(),
                        SLICE,
                        "evil.service",
                    )
                }),
            ),
            (
                "app scope",
                Box::new(|f| f.cgroup = "0::/user.slice/app.slice/app-x.scope\n".into()),
            ),
            (
                "a sub-cgroup",
                Box::new(|f| f.cgroup = format!("{}/sub\n", f.cgroup.trim_end())),
            ),
            (
                "another executable",
                Box::new(|f| f.executable = "/usr/bin/sh".into()),
            ),
            ("a replaced file", Box::new(|f| f.same_file = false)),
            (
                "a reused pid",
                Box::new(|f| {
                    f.hide_pidfd = true;
                    f.reuse_pid = true;
                }),
            ),
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
}
