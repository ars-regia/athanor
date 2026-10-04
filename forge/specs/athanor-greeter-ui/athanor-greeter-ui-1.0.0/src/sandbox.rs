use landlock::{
    Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd,
    RestrictSelfAttr, Ruleset, RulesetAttr, RulesetCreatedAttr, Scope, ABI,
};
use std::env;
use std::path::PathBuf;

/// Fails unless the calling process has exactly one thread.
///
/// The ruleset confines every thread of the process (Landlock ABI 8); this stays as the
/// second guarantee that nothing ran before it, checked against the kernel's own thread
/// list rather than assumed.
pub fn ensure_single_threaded() -> Result<(), Box<dyn std::error::Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!(
            "{threads} threads exist; Landlock would leave all but one unconfined"
        )
        .into());
    }
    Ok(())
}

/// Stops the kernel from writing this process's memory to disk if it dies.
///
/// `panic = "abort"` is set on dev and release: any panic, allocation failure or
/// assertion inside a dependency ends this process with SIGABRT, and systemd-coredump
/// would then write its heap -- the password the user has just typed included -- under
/// /var/lib/systemd/coredump, on the persistent filesystem, outside this sandbox.
/// Landlock does not constrain what the kernel writes on the process's behalf, so the
/// dump has to be refused at the source: `PR_SET_DUMPABLE` to 0 stops the kernel from
/// dumping the process at all, and `RLIMIT_CORE` at 0 covers the ptrace-based paths
/// that ignore it. Both are inherited by nothing this process starts, because it starts
/// nothing.
///
/// Like the Landlock policy this is a hard requirement: a greeter that can dump core is
/// a greeter that can leak a password, so a failure here is fatal rather than logged.
pub fn forbid_core_dumps() -> Result<(), Box<dyn std::error::Error>> {
    nix::sys::prctl::set_dumpable(false)?;
    nix::sys::resource::setrlimit(nix::sys::resource::Resource::RLIMIT_CORE, 0, 0)?;
    Ok(())
}

/// Confines the process with Landlock.
///
/// The greeter runs inside the sandbox athanor-greeter-client builds, where $HOME and
/// the runtime directory are private tmpfs mounts. This ruleset restates in-process what
/// the greeter needs, so that the confinement survives a wrapper that lost a line: it
/// writes /tmp, the runtime directory and the DRM nodes, and connects to the sockets the
/// wrapper binds (the compositor and the filtered buses in the runtime directory, greetd's
/// $GREETD_SOCK, the filtered system bus). It writes no configuration and no state, binds
/// and connects no TCP port, connects to no abstract socket and signals no process outside
/// the sandbox. Reads are left alone: fonts, icons and catalogs come from /usr.
///
/// The ruleset is a hard requirement: a kernel below Landlock ABI 9, or one that cannot
/// enforce every requested right, is an error rather than a best-effort no-op, so the
/// caller can refuse to run unconfined. Call it before any other thread exists (see
/// [`ensure_single_threaded`]).
pub fn apply_landlock_sandbox() -> Result<(), Box<dyn std::error::Error>> {
    restrict_writes_to(&grants())
}

/// The Landlock ABI the ruleset requires: 9 is the first that mediates connecting to a
/// pathname Unix socket.
const ABI_REQUIRED: ABI = ABI::V9;

/// The DRM device directory. GPU rendering opens its card and render nodes read-write and
/// drives them with ioctls; under a ruleset that handles write accesses and device ioctls
/// these need `WriteFile` and `IoctlDev`. Creating or removing anything here is not granted.
const DRM_DEVICE_DIR: &str = "/dev/dri";

/// What a device node is granted: opening it for writing and its ioctls.
const DEVICE_ACCESS: BitFlags<AccessFs> =
    landlock::make_bitflags!(AccessFs::{WriteFile | IoctlDev});

/// The system bus as athanor-greeter-client binds it: xdg-dbus-proxy's filtered socket.
const SYSTEM_BUS_SOCKET: &str = "/run/dbus/system_bus_socket";

/// Every grant of the sandbox: /tmp and the runtime directory with the write set, the DRM
/// nodes with [`DEVICE_ACCESS`], and connecting beneath the runtime directory, to the
/// session bus, to greetd's socket and to the system bus. The greeter has no systemd unit -- it is confined by
/// athanor-greeter-client and by this ruleset, and by nothing else.
fn grants() -> Vec<(PathBuf, BitFlags<AccessFs>)> {
    let write_access = write_access();
    let connect = AccessFs::ResolveUnix.into();
    let mut grants: Vec<_> = writable_paths()
        .into_iter()
        .map(|path| (path, write_access))
        .collect();
    grants.push((PathBuf::from(DRM_DEVICE_DIR), DEVICE_ACCESS));
    grants.extend(
        connect_paths()
            .into_iter()
            .map(|path| (path, connect)),
    );
    grants
}

/// Every write access but connecting: a socket in a writable directory is no grant to
/// connect to it.
fn write_access() -> BitFlags<AccessFs> {
    AccessFs::from_write(ABI_REQUIRED) & !AccessFs::ResolveUnix
}

/// Restricts every thread of the process to the accesses granted beneath each path; a
/// path that does not exist is skipped. TCP and the abstract socket and signal scopes are
/// handled with nothing granted.
fn restrict_writes_to(
    grants: &[(PathBuf, BitFlags<AccessFs>)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_write(ABI_REQUIRED))?
        .handle_access(AccessNet::from_all(ABI_REQUIRED))?
        .scope(Scope::from_all(ABI_REQUIRED))?
        .create()?
        .all_threads(true)?;

    for (path, access) in grants {
        if path.exists() {
            let path_fd = PathFd::new(path)?;
            ruleset = ruleset.add_rule(PathBeneath::new(path_fd, *access))?;
        }
    }

    ruleset.restrict_self()?;
    Ok(())
}

/// The directories the greeter writes to.
fn writable_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("/tmp")];
    paths.extend(runtime_dir());
    paths
}

/// Where the greeter connects: the runtime directory, the session bus wherever
/// `DBUS_SESSION_BUS_ADDRESS` puts it, greetd's socket, the system bus.
fn connect_paths() -> Vec<PathBuf> {
    let greetd = env::var_os("GREETD_SOCK").map(PathBuf::from);
    runtime_dir()
        .into_iter()
        .chain(athanor_unit::sandbox::session_bus())
        .chain(greetd)
        .chain([PathBuf::from(SYSTEM_BUS_SOCKET)])
        .filter(|path| path.is_absolute())
        .collect()
}

fn runtime_dir() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind::PermissionDenied;
    use std::net::{TcpListener, TcpStream};
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    /// Names the fixture directory of the child process `confined` starts.
    const CHILD_BASE: &str = "ATHANOR_GREETER_SANDBOX_TEST_BASE";

    /// The kernel's Landlock ABI, read without restricting anything.
    fn kernel_abi() -> ABI {
        landlock::RestrictSelf::default()
            .no_new_privs(false)
            .apply()
            .expect("read the Landlock status")
            .landlock
            .into()
    }

    /// Runs `body` on a fresh fixture directory, with `allowed` and `denied` in it, in a
    /// child process that runs this test alone: the ruleset confines every thread of the
    /// process, the test harness's included. Below Landlock ABI 9 `body` does not run and
    /// the ruleset must be refused instead.
    fn confined(test: &str, body: impl FnOnce(&Path, PathBuf, PathBuf)) {
        if let Some(base) = env::var_os(CHILD_BASE) {
            let base = PathBuf::from(base);
            let (allowed, denied) = (base.join("allowed"), base.join("denied"));
            std::fs::create_dir_all(&allowed).expect("create allowed dir");
            std::fs::create_dir_all(&denied).expect("create denied dir");
            if kernel_abi() < ABI_REQUIRED {
                assert!(
                    restrict_writes_to(&[(allowed, write_access())]).is_err(),
                    "a kernel below Landlock ABI 9 is refused"
                );
                return;
            }
            body(&base, allowed, denied);
            return;
        }
        let base = env::temp_dir().join(format!("athanor-landlock-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&base).expect("create the fixture dir");
        let status = Command::new(env::current_exe().expect("the test binary"))
            .args([&format!("sandbox::tests::{test}"), "--exact", "--test-threads=1"])
            .env(CHILD_BASE, &base)
            .status()
            .expect("start the test binary");
        std::fs::remove_dir_all(&base).expect("cleanup");
        assert!(status.success(), "{test} failed in its child process");
    }

    fn assert_denied(path: &Path) {
        let err = std::fs::write(path, b"x").expect_err("write outside the set must fail");
        assert_eq!(err.kind(), PermissionDenied);
    }

    #[test]
    fn writable_set_is_tmp_and_the_runtime_directory() {
        // Environment variables are process-wide; keep the test to a single thread of use.
        env::set_var("HOME", "/var/home/tester");
        env::set_var("XDG_CONFIG_HOME", "/var/home/tester/.config");
        env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        env::set_var("GREETD_SOCK", "/run/greetd-1.sock");
        env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/dbus-rig");
        assert_eq!(
            writable_paths(),
            vec![PathBuf::from("/tmp"), PathBuf::from("/run/user/1000")]
        );
        assert_eq!(
            connect_paths(),
            vec![
                PathBuf::from("/run/user/1000"),
                PathBuf::from("/tmp/dbus-rig"),
                PathBuf::from("/run/greetd-1.sock"),
                PathBuf::from(SYSTEM_BUS_SOCKET),
            ]
        );
    }

    /// What exists before the sandbox: a socket beneath a connect grant and one beneath a
    /// write grant, which connects nowhere, an abstract socket, a TCP listener and a
    /// process (`cat`, which exits when this process does and closes its input).
    struct Outside {
        granted: PathBuf,
        writable: PathBuf,
        abstract_addr: SocketAddr,
        _listeners: [UnixListener; 3],
        tcp: TcpListener,
        process: Child,
    }

    impl Outside {
        fn new(granted_dir: &Path, writable_dir: &Path) -> Self {
            let (granted, writable) = (granted_dir.join("socket"), writable_dir.join("socket"));
            let abstract_addr =
                SocketAddr::from_abstract_name(format!("athanor-greeter-{}", std::process::id()))
                    .expect("abstract name");
            Outside {
                _listeners: [
                    UnixListener::bind(&granted).expect("bind"),
                    UnixListener::bind(&writable).expect("bind"),
                    UnixListener::bind_addr(&abstract_addr).expect("bind the abstract socket"),
                ],
                granted,
                writable,
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
                ("a socket beneath a write grant", UnixStream::connect(&self.writable).err()),
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
    fn sandbox_confines_writes_sockets_tcp_and_signals() {
        confined("sandbox_confines_writes_sockets_tcp_and_signals", |base, allowed, denied| {
            let sockets = base.join("sockets");
            std::fs::create_dir_all(&sockets).expect("create the sockets dir");
            let outside = Outside::new(&sockets, &allowed);
            restrict_writes_to(&[
                (allowed.clone(), write_access()),
                (sockets, AccessFs::ResolveUnix.into()),
            ])
            .expect("Landlock must be enforced, not skipped");
            assert_denied(&denied.join("probe"));
            std::fs::write(allowed.join("probe"), b"x")
                .expect("write in the allowed set must succeed");
            outside.assert_only_the_granted_socket_connects();
        });
    }

    #[test]
    fn threads_that_exist_before_the_sandbox_are_confined_too() {
        confined("threads_that_exist_before_the_sandbox_are_confined_too", |_, allowed, denied| {
            let (go, wait) = std::sync::mpsc::channel::<()>();
            let probe = denied.join("probe");
            let earlier = std::thread::spawn(move || {
                wait.recv().expect("go");
                std::fs::write(probe, b"x")
            });
            restrict_writes_to(&[(allowed, write_access())])
                .expect("Landlock must be enforced, not skipped");
            go.send(()).expect("go");
            let err = earlier.join().expect("earlier thread").expect_err("confined");
            assert_eq!(err.kind(), PermissionDenied);
        });
    }

    #[test]
    fn drm_nodes_are_granted_write_file_and_ioctls_and_nothing_else() {
        let drm: Vec<_> = grants()
            .into_iter()
            .filter(|(path, _)| path.starts_with("/dev"))
            .collect();
        assert_eq!(
            drm,
            vec![(
                PathBuf::from("/dev/dri"),
                AccessFs::WriteFile | AccessFs::IoctlDev
            )]
        );
    }

    #[test]
    fn a_device_grant_opens_existing_nodes_read_write_but_creates_nothing() {
        confined("a_device_grant_opens_existing_nodes_read_write_but_creates_nothing", |_, nodes, _| {
            let node = nodes.join("renderD128");
            std::fs::write(&node, b"").expect("create the stand-in node");
            restrict_writes_to(&[(nodes.clone(), DEVICE_ACCESS)])
                .expect("Landlock must be enforced, not skipped");
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&node)
                .expect("an existing node opens read-write, as the DRM open path does");
            std::fs::read_dir(&nodes).expect("the directory stays listable");
            assert_denied(&nodes.join("card9"));
        });
    }

    #[test]
    fn core_dumps_are_refused_by_the_kernel_and_by_the_limit() {
        // This makes the whole test binary non-dumpable, which is harmless: nothing
        // here inspects a core file, and /proc/self stays readable for what the other
        // tests read from it.
        forbid_core_dumps().expect("the greeter must be able to refuse core dumps");
        assert!(
            !nix::sys::prctl::get_dumpable().expect("PR_GET_DUMPABLE"),
            "the kernel must refuse to dump this process"
        );
        assert_eq!(
            nix::sys::resource::getrlimit(nix::sys::resource::Resource::RLIMIT_CORE)
                .expect("getrlimit(RLIMIT_CORE)"),
            (0, 0),
            "no core file may be written, by any path"
        );
    }

    #[test]
    fn a_second_thread_fails_the_single_thread_check() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let other = std::thread::spawn(move || wait.recv());
        assert!(
            ensure_single_threaded().is_err(),
            "a live second thread must be detected"
        );
        release.send(()).expect("release the second thread");
        other.join().expect("second thread").expect("released");
    }
}
