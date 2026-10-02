use landlock::{
    AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, ABI,
};
use std::env;
use std::path::{Path, PathBuf};

/// Fails unless the calling process has exactly one thread.
///
/// Landlock confines the thread that calls `restrict_self` and the threads it creates
/// afterwards, not threads that already exist. The sandbox is therefore only complete
/// when it is applied while the process is still single-threaded, and this is checked
/// against the kernel's own thread list rather than assumed.
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

/// Confines the process's writes with Landlock.
///
/// The greeter runs inside the sandbox athanor-greeter-client builds, where $HOME and
/// the runtime directory are private tmpfs mounts. This ruleset restates in-process what
/// the greeter needs to write, so that the confinement survives a wrapper that lost a
/// line: /tmp, the runtime directory, and the GPU nodes. It writes no configuration and
/// no state. Reads are left alone: fonts, icons and catalogs come from /usr.
///
/// The ruleset is a hard requirement: a kernel without Landlock, or one that cannot
/// enforce every requested right, is an error rather than a best-effort no-op, so the
/// caller can refuse to run unconfined. Call it before any other thread exists (see
/// [`ensure_single_threaded`]).
pub fn apply_landlock_sandbox() -> Result<(), Box<dyn std::error::Error>> {
    restrict_writes_to(&grants())
}

/// The DRM device directory. GPU rendering opens its card and render nodes read-write,
/// and under a ruleset that handles write accesses opening a file for writing needs
/// `WriteFile`. Reads, directory listing and ioctls are not handled by the ABI V1 write
/// set, so they need no grant; creating or removing anything here is not granted.
const DRM_DEVICE_DIR: &str = "/dev/dri";

/// Every grant of the sandbox: /tmp and the runtime directory with the whole write set,
/// and the GPU nodes with `WriteFile` alone. The greeter has no systemd unit -- it is
/// confined by athanor-greeter-client and by this ruleset, and by nothing else.
fn grants() -> Vec<(PathBuf, BitFlags<AccessFs>)> {
    let write_access = AccessFs::from_write(ABI::V1);
    let mut grants: Vec<_> = writable_paths()
        .into_iter()
        .map(|path| (path, write_access))
        .collect();
    grants.push((PathBuf::from(DRM_DEVICE_DIR), AccessFs::WriteFile.into()));
    grants.extend(
        nvidia_device_nodes(Path::new("/dev"))
            .into_iter()
            .map(|node| (node, AccessFs::WriteFile.into())),
    );
    grants
}

/// The NVIDIA driver's nodes in `dev`, the same set athanor-greeter-client binds: the
/// control and modeset nodes and one numbered node per GPU. The proprietary EGL stack
/// opens them read-write next to the DRM nodes; denied, it falls back to running the
/// setuid nvidia-modprobe, which the sandbox refuses as well, and the greeter renders
/// without the GPU. Each node is granted by itself rather than `/dev` as a whole, and
/// the nvidia-uvm nodes stay out: they serve CUDA only.
fn nvidia_device_nodes(dev: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dev) else {
        return Vec::new();
    };
    let mut nodes: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter(|name| {
            name.to_str().is_some_and(|name| match name {
                "nvidiactl" | "nvidia-modeset" => true,
                _ => name.strip_prefix("nvidia").is_some_and(|gpu| {
                    !gpu.is_empty() && gpu.bytes().all(|byte| byte.is_ascii_digit())
                }),
            })
        })
        .map(|name| dev.join(name))
        .collect();
    nodes.sort();
    nodes
}

/// Restricts the calling thread, and the threads it creates, to the write accesses
/// granted beneath each path; a path that does not exist is skipped.
fn restrict_writes_to(
    grants: &[(PathBuf, BitFlags<AccessFs>)],
) -> Result<(), Box<dyn std::error::Error>> {
    let write_access = AccessFs::from_write(ABI::V1);

    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(write_access)?
        .create()?;

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
    if let Some(dir) = env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        paths.push(PathBuf::from(dir));
    }
    paths.retain(|path| Path::new(path).is_absolute());
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writable_set_is_tmp_and_the_runtime_directory() {
        // Environment variables are process-wide; keep the test to a single thread of use.
        env::set_var("HOME", "/var/home/tester");
        env::set_var("XDG_CONFIG_HOME", "/var/home/tester/.config");
        env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        assert_eq!(
            writable_paths(),
            vec![PathBuf::from("/tmp"), PathBuf::from("/run/user/1000")]
        );
    }

    /// Two sibling directories made for one test: `allowed` is granted, `denied` is not
    /// and can never be beneath `allowed`, wherever the temporary directory lives.
    fn probe_dirs(test: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = env::temp_dir().join(format!("athanor-landlock-{}-{test}", std::process::id()));
        let (allowed, denied) = (base.join("allowed"), base.join("denied"));
        std::fs::create_dir_all(&allowed).expect("create allowed dir");
        std::fs::create_dir_all(&denied).expect("create denied dir");
        (base, allowed, denied)
    }

    fn assert_denied(path: &Path) {
        let err = std::fs::write(path, b"x").expect_err("write outside the set must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn sandbox_is_enforced_on_writes_outside_the_allowed_set() {
        let (base, allowed, denied) = probe_dirs("enforced");
        // Landlock confines the calling thread and its future children only, so the
        // restriction stays inside this thread and the rest of the test binary is free.
        std::thread::spawn(move || {
            restrict_writes_to(&[(allowed.clone(), AccessFs::from_write(ABI::V1))])
                .expect("Landlock must be enforced, not skipped");
            assert_denied(&denied.join("probe"));
            std::fs::write(allowed.join("probe"), b"x")
                .expect("write in the allowed set must succeed");
        })
        .join()
        .expect("sandbox test thread");
        std::fs::remove_dir_all(base).expect("cleanup outside the sandboxed thread");
    }

    #[test]
    fn threads_created_after_the_sandbox_inherit_it() {
        let (base, allowed, denied) = probe_dirs("inherit");
        std::thread::spawn(move || {
            restrict_writes_to(&[(allowed, AccessFs::from_write(ABI::V1))])
                .expect("Landlock must be enforced, not skipped");
            std::thread::spawn(move || assert_denied(&denied.join("probe")))
                .join()
                .expect("child thread");
        })
        .join()
        .expect("sandbox test thread");
        std::fs::remove_dir_all(base).expect("cleanup outside the sandboxed thread");
    }

    #[test]
    fn device_nodes_are_granted_write_file_and_nothing_else() {
        let devices: Vec<_> = grants()
            .into_iter()
            .filter(|(path, _)| path.starts_with("/dev"))
            .collect();
        assert_eq!(devices[0].0, PathBuf::from("/dev/dri"));
        assert!(devices
            .iter()
            .all(|(_, access)| *access == BitFlags::from(AccessFs::WriteFile)));
    }

    #[test]
    fn nvidia_grants_are_the_control_modeset_and_gpu_nodes_only() {
        let (base, dev, _) = probe_dirs("nvidia-nodes");
        for name in [
            "nvidiactl",
            "nvidia-modeset",
            "nvidia0",
            "nvidia12",
            "nvidia-uvm",
            "nvidia-uvm-tools",
            "nvidia",
            "nvidia0x",
            "sda",
        ] {
            std::fs::write(dev.join(name), b"").expect("create the stand-in node");
        }
        let names: Vec<_> = nvidia_device_nodes(&dev)
            .into_iter()
            .map(|node| node.file_name().expect("node name").to_owned())
            .collect();
        assert_eq!(
            names,
            ["nvidia-modeset", "nvidia0", "nvidia12", "nvidiactl"]
        );
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn a_write_file_grant_opens_existing_nodes_read_write_but_creates_nothing() {
        let (base, nodes, _) = probe_dirs("write-file");
        let node = nodes.join("renderD128");
        std::fs::write(&node, b"").expect("create the stand-in node");
        std::thread::spawn(move || {
            restrict_writes_to(&[(nodes.clone(), AccessFs::WriteFile.into())])
                .expect("Landlock must be enforced, not skipped");
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&node)
                .expect("an existing node opens read-write, as the DRM open path does");
            std::fs::read_dir(&nodes).expect("the directory stays listable");
            assert_denied(&nodes.join("card9"));
        })
        .join()
        .expect("sandbox test thread");
        std::fs::remove_dir_all(base).expect("cleanup outside the sandboxed thread");
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
