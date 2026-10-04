use landlock::{
    Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, RestrictSelfAttr, Ruleset,
    RulesetAttr, RulesetCreatedAttr, Scope, ABI,
};
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

/// Fails unless the calling process has exactly one thread.
///
/// The ruleset confines every thread of the process (Landlock ABI 8); this stays as the
/// second guarantee that nothing ran before it, checked against the kernel's own thread
/// list rather than assumed.
pub fn ensure_single_threaded() -> Result<(), Box<dyn std::error::Error>> {
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    if threads != 1 {
        return Err(format!("{threads} threads exist; Landlock would leave all but one unconfined").into());
    }
    Ok(())
}

/// Confines the process's writes with Landlock, mirroring the unit's sandbox.
///
/// The unit (`athanor-shell.service`) already runs the shell with
/// `ProtectSystem=strict`, `ProtectHome=read-only`, `ConfigurationDirectory=athanor`,
/// `StateDirectory=athanor` and `PrivateTmp=`: writes are only possible under the
/// configuration and state directories, the runtime directory and `/tmp`. This ruleset
/// restates that write set in-process, so that the confinement survives a unit that
/// lost its hardening. Reads are deliberately left alone: a desktop shell reads desktop
/// entries, icons, wallpapers and its own configuration from the home directory, and a
/// read policy written here would have to duplicate the unit's grants by hand. The
/// previous version of this policy did exactly that, denied reads of
/// `$XDG_CONFIG_HOME/athanor`, and left the shell unable to read the configuration it
/// had just written (theme.css "Permission denied" at every start, and the desktop
/// widgets rewriting widgets.json in a loop until systemd-oomd killed the process).
///
/// It connects to the sockets in the runtime directory (the compositor, the session and
/// accessibility buses) and to the system bus, and to no other pathname socket; it binds
/// and connects no TCP port, connects to no abstract socket and signals no process
/// outside the sandbox.
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
const DEVICE_ACCESS: BitFlags<AccessFs> = landlock::make_bitflags!(AccessFs::{WriteFile | IoctlDev});

/// D-Bus's default system bus socket.
const SYSTEM_BUS_SOCKET: &str = "/run/dbus/system_bus_socket";

/// Every grant of the sandbox: the unit's writable directories with the write set, the
/// DRM nodes with [`DEVICE_ACCESS`], and connecting beneath the runtime directory and to
/// the system bus.
fn grants() -> Vec<(PathBuf, BitFlags<AccessFs>)> {
    let write_access = write_access();
    let mut grants: Vec<_> = writable_paths(|name| env::var_os(name)).into_iter().map(|path| (path, write_access)).collect();
    grants.push((PathBuf::from(DRM_DEVICE_DIR), DEVICE_ACCESS));
    grants.extend(connect_paths(|name| env::var_os(name)).into_iter().map(|path| (path, AccessFs::ResolveUnix.into())));
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
fn restrict_writes_to(grants: &[(PathBuf, BitFlags<AccessFs>)]) -> Result<(), Box<dyn std::error::Error>> {
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

/// The directories the unit lets the shell write to, from the environment `var` reads.
fn writable_paths(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = var("HOME").map(PathBuf::from);
    let xdg_dir = |name: &str, fallback: &str| {
        var(name)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(fallback)))
    };

    let mut paths = vec![PathBuf::from("/tmp")];
    if let Some(dir) = xdg_dir("XDG_CONFIG_HOME", ".config") {
        paths.push(dir.join("athanor"));
    }
    if let Some(dir) = xdg_dir("XDG_STATE_HOME", ".local/state") {
        paths.push(dir.join("athanor"));
    }
    paths.extend(runtime_dir(&var));
    paths.retain(|path| path.is_absolute());
    paths
}

/// Where the shell connects: the runtime directory and the system bus.
fn connect_paths(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    runtime_dir(&var).into_iter().chain([PathBuf::from(SYSTEM_BUS_SOCKET)]).collect()
}

fn runtime_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    var("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|dir| dir.is_absolute())
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
    const CHILD_BASE: &str = "ATHANOR_SHELL_SANDBOX_TEST_BASE";

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
            .args([&format!("{}::{test}", test_module()), "--exact", "--test-threads=1"])
            .env(CHILD_BASE, &base)
            .status()
            .expect("start the test binary");
        std::fs::remove_dir_all(&base).expect("cleanup");
        assert!(status.success(), "{test} failed in its child process");
    }

    /// This module's path as the test harness names it, without the crate.
    fn test_module() -> &'static str {
        module_path!().split_once("::").expect("a module inside the crate").1
    }

    fn assert_denied(path: &Path) {
        let err = std::fs::write(path, b"x").expect_err("write outside the set must fail");
        assert_eq!(err.kind(), PermissionDenied);
    }

    #[test]
    fn writable_set_follows_the_xdg_variables() {
        // A fixed environment, not the process's: other tests change XDG_CONFIG_HOME.
        let var = |name: &str| {
            [("HOME", "/var/home/tester"), ("XDG_CONFIG_HOME", "/var/home/tester/.config"), ("XDG_RUNTIME_DIR", "/run/user/1000")]
                .into_iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        };

        let paths = writable_paths(var);
        assert!(paths.contains(&PathBuf::from("/tmp")));
        assert!(paths.contains(&PathBuf::from("/var/home/tester/.config/athanor")));
        assert!(paths.contains(&PathBuf::from("/var/home/tester/.local/state/athanor")));
        assert!(paths.contains(&PathBuf::from("/run/user/1000")));
        assert_eq!(paths.len(), 4);
        assert_eq!(connect_paths(var), vec![PathBuf::from("/run/user/1000"), PathBuf::from(SYSTEM_BUS_SOCKET)]);
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
                SocketAddr::from_abstract_name(format!("athanor-shell-{}", std::process::id()))
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
