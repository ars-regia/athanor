//! Landlock at start, for the shell's units, the update notifier and the layout chooser (the
//! greeter keeps a ruleset of its own): first prove the process is single-threaded, then
//! restrict. A headless unit names the trees it reads and the one directory it writes
//! (`restrict`). A GTK program cannot name what it reads (icon themes, fonts, the GL
//! driver, glycin's loaders), so it restricts writes only (`restrict_writes`).
//! Both name where the program connects (the runtime directory with the compositor and the
//! accessibility bus, `session_bus`, `system_bus`; `$NOTIFY_SOCKET` always): connecting to
//! any other pathname socket is denied (ABI 9), a write grant included, as is every TCP
//! bind and connect, connecting to an abstract socket or signalling a process outside the
//! sandbox. Socket pairs and descriptors inherited from systemd are not mediated.

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};

use landlock::{
    Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd,
    RestrictSelfAttr, Ruleset, RulesetAttr, RulesetCreated, RulesetCreatedAttr, RulesetError,
    Scope, ABI,
};

/// The Landlock ABI every ruleset here requires: 9 is the first that mediates connecting
/// to a pathname Unix socket. A kernel below it is an error, not a best effort.
pub const ABI_REQUIRED: ABI = ABI::V9;

/// Fails unless the calling process has exactly one thread. The rulesets confine every
/// thread (ABI 8); this stays as the second guarantee that nothing ran before them.
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

/// Read-only beneath each of `read` that exists, read-write beneath `write`, connecting to
/// the sockets beneath each of `connect` that exists, nothing else.
pub fn restrict(read: &[&Path], write: &Path, connect: &[&Path]) -> Result<(), Box<dyn Error>> {
    let all = AccessFs::from_all(ABI_REQUIRED);
    let mut ruleset = ruleset(all)?;
    ruleset = grant(ruleset, read, AccessFs::from_read(ABI_REQUIRED))?;
    ruleset = grant(ruleset, &[write], all & !AccessFs::ResolveUnix)?;
    ruleset = grant_connect(ruleset, connect)?;
    ruleset.restrict_self()?;
    Ok(())
}

/// Reads stay open. Every write access beneath each of `write` that exists; writing to and
/// controlling existing device files, and nothing else, beneath each of `devices` that
/// exists (`/dev/dri`); connecting to the sockets beneath each of `connect` that exists. No
/// other write anywhere, truncation included.
pub fn restrict_writes(
    write: &[&Path],
    devices: &[&Path],
    connect: &[&Path],
) -> Result<(), Box<dyn Error>> {
    let writes = AccessFs::from_write(ABI_REQUIRED);
    let mut ruleset = ruleset(writes)?;
    ruleset = grant(ruleset, write, writes & !AccessFs::ResolveUnix)?;
    ruleset = grant(ruleset, devices, AccessFs::WriteFile | AccessFs::IoctlDev)?;
    ruleset = grant_connect(ruleset, connect)?;
    ruleset.restrict_self()?;
    Ok(())
}

/// A ruleset that handles `fs`, every TCP bind and connect (no port is ever granted), and
/// the abstract Unix socket and signal scopes, enforced on every thread of the process.
fn ruleset(fs: BitFlags<AccessFs>) -> Result<RulesetCreated, RulesetError> {
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(fs)?
        .handle_access(AccessNet::from_all(ABI_REQUIRED))?
        .scope(Scope::from_all(ABI_REQUIRED))?
        .create()?
        .all_threads(true)
}

/// The session bus's socket: the path `$DBUS_SESSION_BUS_ADDRESS` names, else `bus` in
/// `$XDG_RUNTIME_DIR`.
pub fn session_bus() -> Option<PathBuf> {
    match env::var("DBUS_SESSION_BUS_ADDRESS") {
        Ok(address) => bus_socket(&address),
        Err(_) => env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("bus")),
    }
}

/// The system bus's socket: the path `$DBUS_SYSTEM_BUS_ADDRESS` names, else D-Bus's default.
pub fn system_bus() -> Option<PathBuf> {
    match env::var("DBUS_SYSTEM_BUS_ADDRESS") {
        Ok(address) => bus_socket(&address),
        Err(_) => Some(PathBuf::from("/run/dbus/system_bus_socket")),
    }
}

/// The first `unix:path=` of a D-Bus address, its value unescaped; an address with none
/// (abstract, TCP) names no socket Landlock grants.
fn bus_socket(address: &str) -> Option<PathBuf> {
    address
        .split(';')
        .filter_map(|entry| entry.strip_prefix("unix:"))
        .flat_map(|keys| keys.split(','))
        .find_map(|key| key.strip_prefix("path="))
        .and_then(unescape)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// A D-Bus address value with its `%XX` escapes decoded; a malformed escape is `None`.
fn unescape(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut rest = value.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// Connecting beneath each of `connect`, and to the socket `$NOTIFY_SOCKET` names, which
/// `notify::notify_ready` reaches after the sandbox. systemd's user manager names a path;
/// an abstract name is outside the abstract socket scope, and the notification fails.
fn grant_connect(ruleset: RulesetCreated, connect: &[&Path]) -> Result<RulesetCreated, Box<dyn Error>> {
    let notify = env::var_os("NOTIFY_SOCKET")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let mut sockets = connect.to_vec();
    sockets.extend(notify.as_deref());
    grant(ruleset, &sockets, AccessFs::ResolveUnix.into())
}

/// `access` beneath each of `paths` that exists.
fn grant(
    mut ruleset: RulesetCreated,
    paths: &[&Path],
    access: BitFlags<AccessFs>,
) -> Result<RulesetCreated, Box<dyn Error>> {
    for path in paths.iter().filter(|path| path.exists()) {
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, access))?;
    }
    Ok(ruleset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind::PermissionDenied;
    use std::net::{TcpListener, TcpStream};
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    /// Names the fixture directory of the child process `confined` starts.
    const CHILD_BASE: &str = "ATHANOR_UNIT_SANDBOX_TEST_BASE";

    /// The kernel's Landlock ABI, read without restricting anything.
    fn kernel_abi() -> ABI {
        landlock::RestrictSelf::default()
            .no_new_privs(false)
            .apply()
            .expect("read the Landlock status")
            .landlock
            .into()
    }

    /// Runs `body` on a fresh fixture directory in a child process that runs this test alone:
    /// the rulesets confine every thread of the process, the test harness's included.
    fn confined(test: &str, body: impl FnOnce(&Path)) {
        if let Some(base) = std::env::var_os(CHILD_BASE) {
            body(Path::new(&base));
            return;
        }
        let base = std::env::temp_dir().join(format!(
            "athanor-unit-landlock-{}-{test}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).expect("mkdir");
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([&format!("sandbox::tests::{test}"), "--exact", "--test-threads=1"])
            .env(CHILD_BASE, &base)
            .status()
            .expect("start the test binary");
        std::fs::remove_dir_all(&base).expect("cleanup");
        assert!(status.success(), "{test} failed in its child process");
    }

    /// Directories under `base`, each holding a file.
    fn dirs<const N: usize>(base: &Path, names: [&str; N]) -> [PathBuf; N] {
        names.map(|name| {
            let dir = base.join(name);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join("file"), b"x").expect("write");
            dir
        })
    }

    /// What exists before the sandbox: a socket beneath the `connect` grant and one beneath
    /// a write grant, which connects nowhere, an abstract socket, a TCP listener and a process (`cat`, which exits when this
    /// process does and closes its input).
    struct Outside {
        granted: PathBuf,
        hidden: PathBuf,
        abstract_addr: SocketAddr,
        _listeners: [UnixListener; 3],
        tcp: TcpListener,
        process: Child,
    }

    impl Outside {
        fn new(granted_dir: &Path, hidden_dir: &Path) -> Self {
            let (granted, hidden) = (granted_dir.join("socket"), hidden_dir.join("socket"));
            let abstract_addr = SocketAddr::from_abstract_name(format!(
                "athanor-unit-landlock-{}",
                std::process::id()
            ))
            .expect("abstract name");
            Outside {
                _listeners: [
                    UnixListener::bind(&granted).expect("bind"),
                    UnixListener::bind(&hidden).expect("bind"),
                    UnixListener::bind_addr(&abstract_addr).expect("bind the abstract socket"),
                ],
                granted,
                hidden,
                abstract_addr,
                tcp: TcpListener::bind("127.0.0.1:0").expect("bind TCP"),
                process: Command::new("cat")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .expect("start cat"),
            }
        }

        /// From inside the sandbox: only the granted socket connects.
        fn assert_only_the_granted_socket_connects(mut self) {
            assert!(UnixStream::connect(&self.granted).is_ok(), "a granted socket");
            for (what, denied) in [
                ("a socket beneath a write grant", UnixStream::connect(&self.hidden).err()),
                ("an abstract socket", UnixStream::connect_addr(&self.abstract_addr).err()),
                ("TCP connect", TcpStream::connect(self.tcp.local_addr().expect("port")).err()),
                ("TCP bind", TcpListener::bind("127.0.0.1:0").err()),
                ("a signal", self.process.kill().err()),
            ] {
                assert_eq!(denied.map(|err| err.kind()), Some(PermissionDenied), "{what}");
            }
        }
    }

    #[test]
    fn restrict_confines_reads_writes_sockets_tcp_and_signals() {
        confined("restrict_confines_reads_writes_sockets_tcp_and_signals", |base| {
            let [readable, writable, hidden, sockets] =
                dirs(base, ["readable", "writable", "hidden", "sockets"]);
            if kernel_abi() < ABI_REQUIRED {
                assert!(
                    restrict(&[&readable], &writable, &[&sockets]).is_err(),
                    "a kernel below Landlock ABI 9 is refused"
                );
                return;
            }
            let outside = Outside::new(&sockets, &writable);
            restrict(&[&readable], &writable, &[&sockets]).expect("Landlock enforced");
            assert!(std::fs::read(readable.join("file")).is_ok());
            assert_eq!(
                std::fs::write(readable.join("new"), b"x").expect_err("read-only").kind(),
                PermissionDenied
            );
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert_eq!(
                std::fs::read(hidden.join("file")).expect_err("not granted").kind(),
                PermissionDenied
            );
            outside.assert_only_the_granted_socket_connects();
        });
    }

    #[test]
    fn bus_socket_takes_the_first_unix_path_unescaped() {
        for (address, socket) in [
            ("unix:path=/run/user/1000/bus", Some("/run/user/1000/bus")),
            ("unix:guid=1,path=/tmp/dbus-a%2db,x=y", Some("/tmp/dbus-a-b")),
            ("unix:abstract=/tmp/dbus-x;unix:path=/run/bus", Some("/run/bus")),
            ("unix:abstract=/tmp/dbus-x", None),
            ("tcp:host=localhost,port=1", None),
            ("unix:path=relative", None),
            ("unix:path=/bad%2", None),
            ("unix:path=/bad%zz", None),
        ] {
            assert_eq!(bus_socket(address), socket.map(PathBuf::from), "{address}");
        }
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
        confined("restrict_writes_leaves_reads_open_and_grants_writes_by_kind", |base| {
            let [writable, device, closed, sockets] =
                dirs(base, ["writable", "device", "closed", "sockets"]);
            let missing = base.join("missing");
            let grants = |connect: &Path| {
                restrict_writes(&[&writable, &missing], &[&device], &[connect])
            };
            if kernel_abi() < ABI_REQUIRED {
                assert!(grants(&sockets).is_err(), "a kernel below Landlock ABI 9 is refused");
                return;
            }
            let outside = Outside::new(&sockets, &writable);
            grants(&sockets).expect("Landlock enforced");
            assert!(std::fs::read(closed.join("file")).is_ok(), "reads stay open");
            assert_eq!(
                std::fs::write(closed.join("new"), b"x").expect_err("not granted").kind(),
                PermissionDenied
            );
            assert!(std::fs::write(writable.join("new"), b"x").is_ok());
            assert!(
                std::fs::OpenOptions::new().write(true).open(device.join("file")).is_ok(),
                "an existing file under a device grant opens for writing"
            );
            assert_eq!(
                std::fs::write(device.join("new"), b"x")
                    .expect_err("a device grant creates nothing")
                    .kind(),
                PermissionDenied
            );
            assert_eq!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(device.join("file"))
                    .expect_err("a device grant truncates nothing")
                    .kind(),
                PermissionDenied
            );
            assert!(
                std::fs::write(writable.join("file"), b"y").is_ok(),
                "a write grant truncates"
            );
            outside.assert_only_the_granted_socket_connects();
        });
    }
}
