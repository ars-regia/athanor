//! Who may ask (LP8): a peer whose process sits in the lock's own unit, read through a pidfd,
//! of a uid with an active Wayland session. The uid of `SO_PEERCRED` is the only account the
//! helper authenticates; nothing the client writes names one.

use std::error::Error;
use std::fmt;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::time::Duration;

use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};

use crate::ffi;

/// The bound of each logind call.
const LOGIND_TIMEOUT: Duration = Duration::from_secs(5);

/// A session of logind, as far as the check needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub uid: u32,
    pub kind: String,
    pub active: bool,
}

/// Why the peer may not ask.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// Its process is not, or is no longer, the lock's: `cgroup` is what was read.
    Cgroup { cgroup: String },
    /// The process went away, or its pid was recycled, while it was read.
    Gone,
    /// Its uid has no active Wayland session.
    NoSession,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Cgroup { .. } => f.write_str("the peer is not in the lock's unit"),
            Refused::Gone => f.write_str("the peer exited while it was read"),
            Refused::NoSession => f.write_str("the peer's uid has no active Wayland session"),
        }
    }
}

/// The outcome of the check that is not a verdict on the peer: the helper could not read it.
pub type Internal = Box<dyn Error>;

/// The only cgroup a peer may sit in: the lock's unit under the uid's user manager, exactly.
pub fn lock_cgroup(uid: u32) -> String {
    format!("0::/user.slice/user-{uid}.slice/user@{uid}.service/session.slice/athanor-lock.service")
}

/// The pure rule: `cgroup` is exactly one line, the lock's unit of `uid`, and one of `sessions`
/// is of `uid`, of type `wayland` and active.
pub fn admit(uid: u32, cgroup: &str, sessions: &[Session]) -> Result<(), Refused> {
    if !in_lock(uid, cgroup) {
        return Err(Refused::Cgroup {
            cgroup: cgroup.to_owned(),
        });
    }
    let active = |s: &Session| s.uid == uid && s.kind == "wayland" && s.active;
    if !sessions.iter().any(active) {
        return Err(Refused::NoSession);
    }
    Ok(())
}

fn in_lock(uid: u32, cgroup: &str) -> bool {
    cgroup.strip_suffix('\n').unwrap_or(cgroup) == lock_cgroup(uid)
}

/// The check, over its readers: the pid of the peer's pidfd, the cgroup of that pid, the pid
/// read again, which must be unchanged, then the sessions of the uid. A pid is not reused until
/// it is reaped, and a reaped pidfd reads no pid, so an unchanged pid proves the cgroup read
/// was this process's.
pub fn check(
    uid: u32,
    mut pid: impl FnMut() -> Result<Option<i32>, Internal>,
    cgroup: impl FnOnce(i32) -> io::Result<String>,
    sessions: impl FnOnce(u32) -> Result<Vec<Session>, Internal>,
) -> Result<Result<(), Refused>, Internal> {
    let Some(first) = pid()? else {
        return Ok(Err(Refused::Gone));
    };
    let read = match cgroup(first) {
        Ok(read) => read,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Err(Refused::Gone)),
        Err(error) => return Err(error.into()),
    };
    if pid()? != Some(first) {
        return Ok(Err(Refused::Gone));
    }
    // logind is asked only for a peer in the lock's unit.
    if !in_lock(uid, &read) {
        return Ok(Err(Refused::Cgroup { cgroup: read }));
    }
    Ok(admit(uid, &read, &sessions(uid)?))
}

/// The uid of the peer of `socket`, from `SO_PEERCRED`.
pub fn peer_uid(socket: BorrowedFd<'_>) -> nix::Result<u32> {
    Ok(getsockopt(&socket, PeerCredentials)?.uid())
}

/// The check on the peer of `socket`, whose uid is `uid`, against `/proc` and logind.
pub fn check_socket(socket: BorrowedFd<'_>, uid: u32) -> Result<Result<(), Refused>, Internal> {
    let pidfd = ffi::so_peerpidfd(socket)?;
    check(
        uid,
        || pidfd_pid(pidfd.as_fd()),
        |pid| std::fs::read_to_string(format!("/proc/{pid}/cgroup")),
        logind_sessions,
    )
}

/// The pid `pidfd` names, from its fdinfo; `None` once the process is reaped (-1) or outside the
/// helper's pid namespace (0).
fn pidfd_pid(pidfd: BorrowedFd<'_>) -> Result<Option<i32>, Internal> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd()))?;
    let pid: i32 = info
        .lines()
        .find_map(|line| line.strip_prefix("Pid:"))
        .ok_or("the pidfd's fdinfo names no pid")?
        .trim()
        .parse()?;
    Ok((pid > 0).then_some(pid))
}

/// The sessions of `uid`, from logind's `ListSessions` and each session's `Type` and `Active`,
/// over `zbus::blocking` (D29).
fn logind_sessions(uid: u32) -> Result<Vec<Session>, Internal> {
    const LOGIND: &str = "org.freedesktop.login1";
    let bus = zbus::blocking::connection::Builder::system()?
        .method_timeout(LOGIND_TIMEOUT)
        .build()?;
    let listed: Vec<(String, u32, String, String, zbus::zvariant::OwnedObjectPath)> = bus
        .call_method(
            Some(LOGIND),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "ListSessions",
            &(),
        )?
        .body()
        .deserialize()?;
    let property = |path: &zbus::zvariant::OwnedObjectPath, name: &str| {
        bus.call_method(
            Some(LOGIND),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.login1.Session", name),
        )?
        .body()
        .deserialize::<zbus::zvariant::OwnedValue>()
    };
    let mut sessions = Vec::new();
    for (_, session_uid, _, _, path) in listed.into_iter().filter(|s| s.1 == uid) {
        let kind = String::try_from(property(&path, "Type")?)?;
        let active = bool::try_from(property(&path, "Active")?)?;
        sessions.push(Session {
            uid: session_uid,
            kind,
            active,
        });
    }
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::net::UnixStream;

    const UID: u32 = 1000;

    fn wayland(uid: u32, active: bool) -> Session {
        Session {
            uid,
            kind: "wayland".into(),
            active,
        }
    }

    #[test]
    fn the_lock_of_the_uid_with_an_active_wayland_session_is_admitted() {
        let cgroup = format!("{}\n", lock_cgroup(UID));
        assert_eq!(admit(UID, &cgroup, &[wayland(UID, true)]), Ok(()));
    }

    #[test]
    fn any_other_cgroup_is_refused() {
        let other = [
            "0::/user.slice/user-1001.slice/user@1001.service/session.slice/athanor-lock.service\n",
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/athanor-bar.service\n",
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/athanor-lock.service/sub\n",
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-lock.service\n",
            "1:name=systemd:/user.slice\n0::/user.slice/user-1000.slice/user@1000.service/session.slice/athanor-lock.service\n",
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/athanor-lock.service\n\n",
            "",
        ];
        for cgroup in other {
            assert_eq!(
                admit(UID, cgroup, &[wayland(UID, true)]),
                Err(Refused::Cgroup {
                    cgroup: cgroup.into()
                }),
                "{cgroup:?}"
            );
        }
    }

    #[test]
    fn a_uid_without_an_active_wayland_session_is_refused() {
        let cgroup = lock_cgroup(UID);
        let tty = Session {
            uid: UID,
            kind: "tty".into(),
            active: true,
        };
        for sessions in [
            vec![],
            vec![wayland(UID, false)],
            vec![tty],
            vec![wayland(1001, true)],
        ] {
            assert_eq!(
                admit(UID, &cgroup, &sessions),
                Err(Refused::NoSession),
                "{sessions:?}"
            );
        }
    }

    fn run(pids: &[Option<i32>], cgroup: io::Result<String>) -> Result<(), Refused> {
        let reads = Cell::new(0);
        let checked = check(
            UID,
            || {
                let pid = pids[reads.get()];
                reads.set(reads.get() + 1);
                Ok(pid)
            },
            |_| cgroup,
            |_| Ok(vec![wayland(UID, true)]),
        )
        .expect("no internal error");
        assert_eq!(reads.get(), pids.len().min(2));
        checked
    }

    #[test]
    fn a_pid_unchanged_across_the_cgroup_read_is_admitted() {
        assert_eq!(run(&[Some(42), Some(42)], Ok(lock_cgroup(UID))), Ok(()));
    }

    #[test]
    fn a_peer_reaped_or_recycled_during_the_read_is_refused() {
        assert_eq!(
            run(&[Some(42), None], Ok(lock_cgroup(UID))),
            Err(Refused::Gone)
        );
        assert_eq!(
            run(&[Some(42), Some(43)], Ok(lock_cgroup(UID))),
            Err(Refused::Gone)
        );
        assert_eq!(run(&[None], Ok(lock_cgroup(UID))), Err(Refused::Gone));
        assert_eq!(
            run(&[Some(42)], Err(io::ErrorKind::NotFound.into())),
            Err(Refused::Gone)
        );
    }

    #[test]
    fn a_wrong_cgroup_is_refused_without_asking_logind() {
        let checked = check(
            UID,
            || Ok(Some(42)),
            |_| Ok("0::/system.slice/sshd.service\n".into()),
            |_| Err("logind is not asked".into()),
        )
        .expect("refused before logind");
        assert!(matches!(checked, Err(Refused::Cgroup { .. })));
    }

    #[test]
    fn a_failed_logind_call_is_an_internal_error() {
        let checked = check(
            UID,
            || Ok(Some(42)),
            |_| Ok(lock_cgroup(UID)),
            |_| Err("no bus".into()),
        );
        assert!(checked.is_err());
    }

    #[test]
    fn the_peer_of_a_socket_pair_is_this_process() {
        let (ours, _theirs) = UnixStream::pair().expect("a socket pair");
        assert_eq!(peer_uid(ours.as_fd()), Ok(nix::unistd::getuid().as_raw()));
        let pidfd = ffi::so_peerpidfd(ours.as_fd()).expect("SO_PEERPIDFD");
        let pid = pidfd_pid(pidfd.as_fd()).expect("the pidfd's pid");
        assert_eq!(pid, Some(i32::try_from(std::process::id()).expect("a pid")));
    }
}
