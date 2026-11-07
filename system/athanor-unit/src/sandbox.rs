//! Landlock at start, as the greeter and the notifier do: first prove the process is
//! single-threaded, then restrict. A headless unit names the trees it reads and, by an open
//! descriptor, the directories it writes (`restrict`). A GTK program cannot name what it reads (icon themes,
//! fonts, the GL driver, glycin's loaders), so it restricts writes only (`restrict_writes`), by
//! descriptor as well. The descriptors come from `dirs`, which checks each one it opens.
//! Connecting to a bus socket is not a filesystem access Landlock mediates.
//! A program that has no business on the network also calls `deny_tcp`.

use std::error::Error;
use std::os::fd::AsFd;
use std::path::Path;

use crate::dirs::Checked;
use landlock::{
    Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
    RulesetAttr, RulesetCreatedAttr, ABI,
};

/// Fails unless the calling process has exactly one thread: Landlock confines the calling
/// thread and those it creates afterwards, not threads that already exist.
pub fn ensure_single_threaded() -> Result<(), Box<dyn Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!(
            "{threads} threads exist; Landlock would leave all but one unconfined"
        )
        .into());
    }
    Ok(())
}

/// Read-only beneath each of `read` that exists, read-write beneath each directory `write`
/// holds open, nothing else. A write grant is a `dirs::Checked` descriptor, not a path, so the
/// directory granted is the one `dirs` checked, whatever the path names now.
/// A kernel that cannot enforce the ruleset is an error, not a best effort.
pub fn restrict(read: &[&Path], write: &[Checked]) -> Result<(), Box<dyn Error>> {
    let all = AccessFs::from_all(ABI::V1);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?
        .create()?;
    for path in read.iter().filter(|path| path.exists()) {
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(path)?,
            AccessFs::from_read(ABI::V1),
        ))?;
    }
    for dir in write {
        ruleset = ruleset.add_rule(PathBeneath::new(dir.as_fd(), all))?;
    }
    ruleset.restrict_self()?;
    Ok(())
}

/// Reads stay open. Every write access beneath each directory `write` holds open; writing to
/// existing files, and nothing else, beneath each of `write_file` (a device directory such as
/// `/dev/dri`, or a node). No other write anywhere, truncation included (Landlock ABI 3). As for
/// `restrict`, a grant is a `dirs::Checked` descriptor (`dirs::owned_dir`, `dirs::owned_node`,
/// `dirs::private_dir`), not a path. A kernel that cannot enforce the ruleset is an error, not a
/// best effort.
pub fn restrict_writes(write: &[Checked], write_file: &[Checked]) -> Result<(), Box<dyn Error>> {
    let writes = AccessFs::from_write(ABI::V3);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(writes)?
        .create()?;
    for dir in write {
        ruleset = ruleset.add_rule(PathBeneath::new(dir.as_fd(), writes))?;
    }
    for node in write_file {
        ruleset = ruleset.add_rule(PathBeneath::new(node.as_fd(), AccessFs::WriteFile))?;
    }
    ruleset.restrict_self()?;
    Ok(())
}

/// Denies binding and connecting TCP sockets to the calling thread and every thread and
/// process it starts afterwards (Landlock ABI 4). Unix sockets are not affected. Landlock
/// does not mediate UDP, raw or ICMP sockets: the unit's `RestrictAddressFamilies` closes those.
pub fn deny_tcp() -> Result<(), Box<dyn Error>> {
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessNet::from_all(ABI::V4))?
        .create()?
        .restrict_self()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_tcp_refuses_tcp_and_keeps_unix_sockets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind before the ruleset");
        let port = listener.local_addr().expect("address").port();
        let socket =
            std::env::temp_dir().join(format!("athanor-unit-tcp-{}.sock", std::process::id()));
        let unix = std::os::unix::net::UnixListener::bind(&socket).expect("unix bind");
        let path = socket.clone();
        std::thread::spawn(move || {
            deny_tcp().expect("Landlock ABI 4 must be enforced, not skipped");
            assert_eq!(
                std::net::TcpStream::connect(("127.0.0.1", port))
                    .expect_err("connect is denied")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(
                std::net::TcpListener::bind("127.0.0.1:0").is_err(),
                "bind is denied"
            );
            assert!(
                std::os::unix::net::UnixStream::connect(&path).is_ok(),
                "Unix sockets stay open"
            );
        })
        .join()
        .expect("sandboxed thread");
        drop((listener, unix));
        std::fs::remove_file(socket).expect("cleanup");
    }

    #[test]
    fn reads_outside_the_grants_and_writes_outside_the_write_directory_are_denied() {
        let base =
            std::env::temp_dir().join(format!("athanor-unit-landlock-{}", std::process::id()));
        let (readable, writable, hidden) = (
            base.join("readable"),
            base.join("writable"),
            base.join("hidden"),
        );
        for dir in [&readable, &writable, &hidden] {
            std::fs::create_dir_all(dir).expect("mkdir");
            std::fs::write(dir.join("file"), b"x").expect("write");
        }
        // Landlock confines the calling thread and its children: the test binary stays free.
        std::thread::spawn(move || {
            let writable_dir = crate::dirs::owned_dir(&writable).expect("open");
            restrict(&[&readable], &[writable_dir])
                .expect("Landlock must be enforced, not skipped");
            assert!(std::fs::read(readable.join("file")).is_ok());
            assert_eq!(
                std::fs::write(readable.join("new"), b"x")
                    .expect_err("read-only")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert_eq!(
                std::fs::read(hidden.join("file"))
                    .expect_err("not granted")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
        })
        .join()
        .expect("sandboxed thread");
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_second_thread_fails_the_single_thread_check() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let other = std::thread::spawn(move || wait.recv());
        assert!(ensure_single_threaded().is_err());
        release.send(()).expect("release");
        other.join().expect("join").expect("released");
    }

    #[test]
    fn restrict_writes_leaves_reads_open_and_grants_writes_by_kind() {
        let base = std::env::temp_dir().join(format!(
            "athanor-unit-landlock-writes-{}",
            std::process::id()
        ));
        let (writable, device, closed, missing) = (
            base.join("writable"),
            base.join("device"),
            base.join("closed"),
            base.join("missing"),
        );
        for dir in [&writable, &device, &closed] {
            std::fs::create_dir_all(dir).expect("mkdir");
            std::fs::write(dir.join("file"), b"x").expect("write");
        }
        std::thread::spawn(move || {
            let write = crate::dirs::existing(&[&writable, &missing], crate::dirs::owned_dir);
            assert_eq!(write.len(), 1, "a missing directory is skipped");
            let write_file = crate::dirs::existing(&[&device], crate::dirs::owned_node);
            restrict_writes(&write, &write_file).expect("Landlock must be enforced, not skipped");
            assert!(
                std::fs::read(closed.join("file")).is_ok(),
                "reads stay open"
            );
            assert_eq!(
                std::fs::write(closed.join("new"), b"x")
                    .expect_err("not granted")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(device.join("file"))
                    .is_ok(),
                "an existing file under a file grant opens for writing"
            );
            assert_eq!(
                std::fs::write(device.join("new"), b"x")
                    .expect_err("a file grant creates nothing")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert_eq!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(device.join("file"))
                    .expect_err("a file grant truncates nothing")
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(
                std::fs::write(writable.join("file"), b"y").is_ok(),
                "a write grant truncates"
            );
        })
        .join()
        .expect("sandboxed thread");
        std::fs::remove_dir_all(base).expect("cleanup");
    }
}
