//! Who is at the other end of a Unix socket (`doc_lock_and_prompts.md` LP13, as
//! `doc_session_daemons.md` SD5 and SD15 apply it): the peer's process, named by the pidfd of
//! `SO_PEERPIDFD`, must sit in one unit of the user's manager and run one executable. The
//! cgroup and the executable are read from `/proc/<pid>`, and the pidfd is read again after
//! them: the pid must be unchanged and the process still running, so what was read is that
//! process's and not a recycled pid's. A peer with no pidfd is refused; so is every read that
//! fails.

#![allow(unsafe_code)]

use std::fmt;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use nix::libc;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

/// Why a peer is not admitted.
#[derive(Debug)]
pub enum Refused {
    /// The socket names no pidfd: no peer, a kernel without `SO_PEERPIDFD`, or a peer reaped.
    NoPidfd(io::Error),
    /// The process left, or its pid changed, while it was read.
    Gone,
    /// `/proc/<pid>` could not be read.
    Unreadable(io::Error),
    /// The process runs in another unit: `cgroup` is what was read.
    Cgroup(String),
    /// The process runs another executable: what was read.
    Exe(PathBuf),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::NoPidfd(error) => write!(f, "the peer has no pidfd: {error}"),
            Refused::Gone => f.write_str("the peer exited while it was read"),
            Refused::Unreadable(error) => write!(f, "the peer's process cannot be read: {error}"),
            Refused::Cgroup(cgroup) => write!(f, "the peer runs in another unit: {cgroup:?}"),
            Refused::Exe(exe) => write!(f, "the peer runs another program: {exe:?}"),
        }
    }
}

impl std::error::Error for Refused {}

/// The cgroup of `/proc/<pid>/cgroup`, without its newline, of the unit `unit` in the slice
/// `slice` of the user manager of `uid`: what a process of that unit reads, exactly.
pub fn user_unit_cgroup(uid: u32, slice: &str, unit: &str) -> String {
    format!("0::/user.slice/user-{uid}.slice/user@{uid}.service/{slice}/{unit}")
}

/// Admits the peer of `socket` only when its process is in the unit `unit` of the slice `slice`
/// of this process's own user manager and runs `exe`.
pub fn admit_user_unit(
    socket: BorrowedFd<'_>,
    slice: &str,
    unit: &str,
    exe: &Path,
) -> Result<(), Refused> {
    let cgroup = user_unit_cgroup(nix::unistd::getuid().as_raw(), slice, unit);
    let pidfd = peer_pidfd(socket).map_err(Refused::NoPidfd)?;
    check(
        &cgroup,
        exe,
        || pidfd_pid(pidfd.as_fd()),
        |pid| std::fs::read_to_string(format!("/proc/{pid}/cgroup")),
        |pid| std::fs::read_link(format!("/proc/{pid}/exe")),
        || running(pidfd.as_fd()),
    )
}

/// The rule over its readers: the pid of the pidfd, the cgroup and the executable of that pid,
/// then the pid again, unchanged, and the process still running.
fn check(
    cgroup: &str,
    exe: &Path,
    mut pid: impl FnMut() -> io::Result<Option<i32>>,
    read_cgroup: impl FnOnce(i32) -> io::Result<String>,
    read_exe: impl FnOnce(i32) -> io::Result<PathBuf>,
    running: impl FnOnce() -> io::Result<bool>,
) -> Result<(), Refused> {
    let Some(first) = pid().map_err(Refused::Unreadable)? else {
        return Err(Refused::Gone);
    };
    let read = read_cgroup(first).map_err(Refused::Unreadable)?;
    let program = read_exe(first).map_err(Refused::Unreadable)?;
    if pid().map_err(Refused::Unreadable)? != Some(first)
        || !running().map_err(Refused::Unreadable)?
    {
        return Err(Refused::Gone);
    }
    if read.strip_suffix('\n').unwrap_or(&read) != cgroup {
        return Err(Refused::Cgroup(read));
    }
    if program != exe {
        return Err(Refused::Exe(program));
    }
    Ok(())
}

/// The pidfd of the peer of `socket`.
fn peer_pidfd(socket: BorrowedFd<'_>) -> io::Result<OwnedFd> {
    let mut fd: libc::c_int = -1;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `fd` and `len` are live locals, and `len` is the size of `fd`: getsockopt writes
    // at most that many bytes into it.
    let done = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERPIDFD,
            std::ptr::from_mut(&mut fd).cast(),
            &mut len,
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    if fd < 0 {
        return Err(io::Error::other("SO_PEERPIDFD returned no descriptor"));
    }
    // SAFETY: on success the kernel installed a new pidfd at `fd` that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The pid `pidfd` names, from its fdinfo; `None` once the process is reaped (-1) or outside
/// this process's pid namespace (0).
fn pidfd_pid(pidfd: BorrowedFd<'_>) -> io::Result<Option<i32>> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd()))?;
    let pid = info
        .lines()
        .find_map(|line| line.strip_prefix("Pid:"))
        .ok_or_else(|| io::Error::other("the pidfd's fdinfo names no pid"))?
        .trim()
        .parse::<i32>()
        .map_err(io::Error::other)?;
    Ok((pid > 0).then_some(pid))
}

/// Whether the process of `pidfd` still runs: a pidfd polls readable once it has exited.
fn running(pidfd: BorrowedFd<'_>) -> io::Result<bool> {
    let mut fds = [PollFd::new(pidfd, PollFlags::POLLIN)];
    poll(&mut fds, PollTimeout::ZERO)?;
    // Bits poll(2) names that nix does not know read as `None`: not proved running.
    Ok(fds[0].revents().is_some_and(|events| events.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::net::UnixStream;

    const WANT: &str = "0::/user.slice/user-1000.slice/user@1000.service/session.slice/a.service";

    fn run(pids: &[Option<i32>], cgroup: &str, exe: &str, alive: bool) -> Result<(), Refused> {
        let reads = Cell::new(0);
        check(
            WANT,
            Path::new("/usr/bin/a"),
            || {
                let pid = pids[reads.get()];
                reads.set(reads.get() + 1);
                Ok(pid)
            },
            |_| Ok(cgroup.to_owned()),
            |_| Ok(PathBuf::from(exe)),
            || Ok(alive),
        )
    }

    #[test]
    fn the_unit_and_program_named_are_admitted() {
        assert_eq!(user_unit_cgroup(1000, "session.slice", "a.service"), WANT);
        let ok = run(
            &[Some(7), Some(7)],
            &format!("{WANT}\n"),
            "/usr/bin/a",
            true,
        );
        assert!(ok.is_ok(), "{ok:?}");
    }

    #[test]
    fn another_unit_or_program_is_refused() {
        for cgroup in [
            "0::/user.slice/user-1001.slice/user@1001.service/session.slice/a.service\n",
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/a.service\n",
            &format!("{WANT}/sub\n"),
            &format!("{WANT}\n\n"),
            "",
        ] {
            let refused = run(&[Some(7), Some(7)], cgroup, "/usr/bin/a", true);
            assert!(
                matches!(refused, Err(Refused::Cgroup(_))),
                "{cgroup:?}: {refused:?}"
            );
        }
        for exe in ["/usr/bin/b", "/usr/bin/a (deleted)", "/var/usr/bin/a"] {
            let refused = run(&[Some(7), Some(7)], WANT, exe, true);
            assert!(
                matches!(refused, Err(Refused::Exe(_))),
                "{exe}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_peer_that_left_while_read_is_refused() {
        for (pids, alive) in [
            (&[None][..], true),
            (&[Some(7), None], true),
            (&[Some(7), Some(8)], true),
            (&[Some(7), Some(7)], false),
        ] {
            let refused = run(pids, WANT, "/usr/bin/a", alive);
            assert!(
                matches!(refused, Err(Refused::Gone)),
                "{pids:?} {alive}: {refused:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_process_is_refused() {
        let refused = check(
            WANT,
            Path::new("/usr/bin/a"),
            || Ok(Some(7)),
            |_| Err(io::ErrorKind::PermissionDenied.into()),
            |_| Ok(PathBuf::from("/usr/bin/a")),
            || Ok(true),
        );
        assert!(
            matches!(refused, Err(Refused::Unreadable(_))),
            "{refused:?}"
        );
    }

    /// This test process is the peer of its own socket pair: its own unit and program pass,
    /// any other is refused.
    #[test]
    fn a_real_peer_is_read_through_its_pidfd() {
        let (a, _b) = UnixStream::pair().expect("socket pair");
        let pidfd = peer_pidfd(a.as_fd()).expect("SO_PEERPIDFD");
        assert_eq!(
            pidfd_pid(pidfd.as_fd()).expect("fdinfo"),
            Some(std::process::id() as i32)
        );
        assert!(running(pidfd.as_fd()).expect("poll"));
        let own = std::fs::read_to_string("/proc/self/cgroup").expect("own cgroup");
        let exe = std::env::current_exe().expect("own executable");
        let checked = check(
            own.trim_end(),
            &exe,
            || pidfd_pid(pidfd.as_fd()),
            |pid| std::fs::read_to_string(format!("/proc/{pid}/cgroup")),
            |pid| std::fs::read_link(format!("/proc/{pid}/exe")),
            || running(pidfd.as_fd()),
        );
        assert!(checked.is_ok(), "{checked:?}");
        let other = admit_user_unit(a.as_fd(), "session.slice", "nothing.service", &exe);
        assert!(matches!(other, Err(Refused::Cgroup(_))), "{other:?}");
    }

    #[test]
    fn a_socket_with_no_peer_is_refused() {
        let socket = std::os::unix::net::UnixDatagram::unbound().expect("socket");
        let refused = admit_user_unit(
            socket.as_fd(),
            "session.slice",
            "a.service",
            Path::new("/a"),
        );
        assert!(matches!(refused, Err(Refused::NoPidfd(_))), "{refused:?}");
    }
}
