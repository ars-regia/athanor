//! Who a client is, and what that grants it (doc_own_stack.md OS4, C1 task 5). The identity is
//! read once, when the client connects: the peer's pidfd from `SO_PEERPIDFD`, then through its
//! pid the executable and the cgroup, from which the systemd unit is derived. An allow-list
//! under `/usr` maps (executable, unit) pairs to privileged globals. A client that `exec`s after connecting keeps
//! what it connected as; unconfined code running as the user is out of scope (OS4).

use crate::Error;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

/// Where the image installs the allow-list.
pub const ALLOW_LIST: &str = "/usr/share/athanor/compositor/privileged-clients";

/// A privileged global, named in the allow-list by its interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Privilege {
    /// `zwlr_layer_shell_v1`: its top and overlay layers; the bottom and background layers are
    /// every unconfined client's.
    TopLayers,
    ToplevelList,
    ToplevelCaptureSource,
    /// `ext_output_image_capture_source_manager_v1`: a source of an output's pixels, by its
    /// `wl_output`.
    OutputCaptureSource,
    ImageCopyCapture,
    SessionLock,
    /// `ext_workspace_manager_v1`: the workspaces, which a client may switch.
    Workspaces,
    /// `zwlr_output_manager_v1`: the outputs' configuration, the control center's.
    OutputManagement,
    /// `zwlr_output_power_manager_v1`: the outputs turned off and on, the idle daemon's.
    OutputPower,
    /// `zwp_virtual_keyboard_manager_v1`: keys typed as if on the hardware keyboard.
    VirtualKeyboard,
    /// `zwlr_virtual_pointer_manager_v1`: a pointer moved and clicked as if by the hardware one.
    VirtualPointer,
    /// `zwp_input_method_manager_v2`: the seat's input method, which is told the focused text
    /// field's text, commits text into it and may take the keyboard.
    InputMethod,
    /// `athanor_toplevel_manager_v1`: every window's state, read only.
    ToplevelState,
    /// `athanor_toplevel_actions_v1`: every window activated, minimised, restored or closed.
    ToplevelActions,
    /// `athanor_keyboard_group_manager_v1`: the keyboard's layout group, followed and switched.
    KeyboardGroup,
    /// `ext_data_control_manager_v1`: the clipboard and the primary selection, read and set
    /// without the keyboard focus, a clipboard manager's.
    ExtDataControl,
    /// `zwlr_data_control_manager_v1`: the same through the wlroots protocol, which
    /// wl-clipboard 2.2 speaks.
    WlrDataControl,
}

impl Privilege {
    const ALL: [(Self, &'static str); 17] = [
        (Self::TopLayers, "zwlr_layer_shell_v1"),
        (Self::ToplevelList, "ext_foreign_toplevel_list_v1"),
        (
            Self::ToplevelCaptureSource,
            "ext_foreign_toplevel_image_capture_source_manager_v1",
        ),
        (
            Self::OutputCaptureSource,
            "ext_output_image_capture_source_manager_v1",
        ),
        (Self::ImageCopyCapture, "ext_image_copy_capture_manager_v1"),
        (Self::SessionLock, "ext_session_lock_manager_v1"),
        (Self::Workspaces, "ext_workspace_manager_v1"),
        (Self::OutputManagement, "zwlr_output_manager_v1"),
        (Self::OutputPower, "zwlr_output_power_manager_v1"),
        (Self::VirtualKeyboard, "zwp_virtual_keyboard_manager_v1"),
        (Self::VirtualPointer, "zwlr_virtual_pointer_manager_v1"),
        (Self::InputMethod, "zwp_input_method_manager_v2"),
        (Self::ToplevelState, "athanor_toplevel_manager_v1"),
        (Self::ToplevelActions, "athanor_toplevel_actions_v1"),
        (Self::KeyboardGroup, "athanor_keyboard_group_manager_v1"),
        (Self::ExtDataControl, "ext_data_control_manager_v1"),
        (Self::WlrDataControl, "zwlr_data_control_manager_v1"),
    ];

    fn named(interface: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, name)| *name == interface)
            .map(|(privilege, _)| *privilege)
    }
}

/// The interface of every privileged global, as the allow-list names it.
#[cfg(test)]
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    Privilege::ALL.iter().map(|(_, name)| *name)
}

/// A process, as the compositor reads it at connect.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    /// What `/proc/<pid>/exe` names, as the process's own mount namespace has it.
    pub executable: PathBuf,
    /// The systemd unit of its cgroup, if it is in one.
    pub unit: Option<String>,
}

/// A file as `stat` names it, whatever path leads to it: its device and inode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

/// Reads a process by its pid, and a file by its path in the compositor's own mount namespace:
/// `/proc` and the file system in the compositor, a fake in the fixture.
pub trait Process {
    /// What `/proc/<pid>/exe` names: a `readlink`, which does not reach the file's own file
    /// system.
    fn executable(&self, pid: i32) -> io::Result<PathBuf>;
    /// The contents of `/proc/<pid>/cgroup`.
    fn cgroup(&self, pid: i32) -> io::Result<String>;
    /// The file `/proc/<pid>/exe` opens, whichever mount namespace it is in: a `stat`, which
    /// reaches the file system the process runs from and blocks while a FUSE server stalls.
    fn executable_file(&self, pid: i32) -> io::Result<FileId>;
    /// The file at `path`, as the compositor sees it.
    fn file(&self, path: &Path) -> io::Result<FileId>;
}

/// The processes of `/proc`.
pub struct Proc;

impl Process for Proc {
    fn executable(&self, pid: i32) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
    }
    fn executable_file(&self, pid: i32) -> io::Result<FileId> {
        self.file(Path::new(&format!("/proc/{pid}/exe")))
    }
    fn cgroup(&self, pid: i32) -> io::Result<String> {
        std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
    }
    fn file(&self, path: &Path) -> io::Result<FileId> {
        use std::os::unix::fs::MetadataExt;
        // `metadata` follows `/proc/<pid>/exe` to the file itself, across mount namespaces.
        let metadata = std::fs::metadata(path)?;
        Ok(FileId {
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
}

/// The identity of the process at the other end of `socket`, and what `list` grants it. The
/// executable, the cgroup and, for a grant, the executable's file are read through the pid of
/// the peer's pidfd, which is then read again: unless it still names the same live process,
/// the reads may be another process's, and the peer has no identity.
pub fn identify(
    socket: BorrowedFd<'_>,
    process: &dyn Process,
    list: &AllowList,
) -> Result<(Identity, Vec<Privilege>), Error> {
    let pidfd = so_peerpidfd(socket)?;
    let pid = pidfd_pid(pidfd.as_fd())?;
    let executable = process.executable(pid)?;
    let cgroup = process.cgroup(pid)?;
    let identity = Identity {
        executable,
        unit: unit(&cgroup).map(str::to_owned),
    };
    let grants = list.grants(&identity, pid, process);
    if pidfd_pid(pidfd.as_fd())? != pid {
        return Err("the peer exited while it was read".into());
    }
    Ok((identity, grants))
}

/// The peer's pidfd, from `SO_PEERPIDFD` (Linux 6.5, older than every kernel Athanor ships).
/// There is no fallback: `SO_PEERCRED` names a pid, which can be reused before it is opened,
/// and is 0 for a peer outside the compositor's pid namespace. Without it, no identity.
#[allow(unsafe_code)]
fn so_peerpidfd(socket: BorrowedFd<'_>) -> io::Result<OwnedFd> {
    let mut fd: libc::c_int = -1;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `fd` and `len` are live locals, and `len` is the size of `fd`: getsockopt writes
    // at most that many bytes into it.
    let done = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERPIDFD,
            (&raw mut fd).cast(),
            &mut len,
        )
    };
    if done != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOPROTOOPT) {
            return Err(io::Error::other(
                "the kernel has no SO_PEERPIDFD (Linux 6.5)",
            ));
        }
        return Err(error);
    }
    if fd < 0 {
        return Err(io::Error::other("SO_PEERPIDFD returned no descriptor"));
    }
    // SAFETY: on success the kernel installed a new pidfd at `fd` that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The pid `pidfd` names in the compositor's pid namespace, from its fdinfo: -1 once the process
/// is reaped (its pid free for reuse), 0 if it is outside the namespace; both are refused.
fn pidfd_pid(pidfd: BorrowedFd<'_>) -> Result<i32, Error> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd()))?;
    let pid: i32 = info
        .lines()
        .find_map(|line| line.strip_prefix("Pid:"))
        .ok_or("the pidfd's fdinfo names no pid")?
        .trim()
        .parse()?;
    if pid <= 0 {
        return Err(format!("the peer is gone or outside the namespace (pid {pid})").into());
    }
    Ok(pid)
}

/// The systemd unit of a process from its `/proc/<pid>/cgroup` (cgroup v2): the first component
/// of the path that is not a slice, and under a user manager (`user@<uid>.service`) the first
/// one below it. The first, not the last: a unit with `Delegate=yes` may name its own
/// sub-cgroups as it likes, and one called `athanor-bar.service` stays inside its unit.
pub fn unit(cgroup: &str) -> Option<&str> {
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let not_slice = |part: &&str| !part.ends_with(".slice");
    let mut unit = parts.find(not_slice)?;
    if unit.starts_with("user@") && unit.ends_with(".service") {
        unit = parts.find(not_slice)?;
    }
    (unit.ends_with(".service") || unit.ends_with(".scope")).then_some(unit)
}

/// The allow-list: which executable in which unit is granted which privileged globals. One
/// grant per line, `<executable> <unit> <interface>...`, separated by spaces; `#` starts a
/// comment line. The executable is an absolute path under `/usr`, the unit a `.service` or
/// `.scope`, each interface one of `Privilege`'s.
#[derive(Debug, Default, PartialEq)]
pub struct AllowList(Vec<(PathBuf, String, Vec<Privilege>)>);

impl AllowList {
    /// The allow-list at `path`; empty, and logged, when it is missing, unreadable or malformed.
    pub fn load(path: &Path) -> Self {
        let parsed = std::fs::read_to_string(path)
            .map_err(Error::from)
            .and_then(|text| Self::parse(&text));
        parsed.unwrap_or_else(|error| {
            tracing::warn!(path = %path.display(), %error, "no privileged global is granted");
            Self::default()
        })
    }

    /// The allow-list in `text`, or an error naming the first line that is not a grant.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut grants = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |what: &str| format!("line {}: {what}: {line}", number + 1);
            let mut fields = line.split_whitespace();
            let (Some(executable), Some(unit)) = (fields.next(), fields.next()) else {
                return Err(bad("an executable and a unit").into());
            };
            if !executable.starts_with("/usr/")
                || Path::new(executable)
                    .components()
                    .any(|c| c == Component::ParentDir)
            {
                return Err(bad("the executable is not a path under /usr").into());
            }
            if !(unit.ends_with(".service") || unit.ends_with(".scope")) || unit.contains('/') {
                return Err(bad("the unit is not a service or a scope").into());
            }
            let privileges = fields
                .map(|name| {
                    Privilege::named(name).ok_or_else(|| bad(&format!("{name} is not privileged")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if privileges.is_empty() {
                return Err(bad("no interface").into());
            }
            grants.push((PathBuf::from(executable), unit.to_owned(), privileges));
        }
        Ok(Self(grants))
    }

    /// What `identity`, the process `pid`, is granted: the interfaces of every line naming its
    /// executable and unit, where the executable's path, read by `process` in the compositor's
    /// mount namespace, is the very file the process runs. A path alone is not enough: a
    /// sandbox whose own `/usr` holds a file at the bar's path reads back the bar's path.
    pub fn grants(&self, identity: &Identity, pid: i32, process: &dyn Process) -> Vec<Privilege> {
        let Some(unit) = &identity.unit else {
            return Vec::new();
        };
        let named: Vec<_> = self
            .0
            .iter()
            .filter(|(executable, u, _)| *executable == identity.executable && u == unit)
            .collect();
        if named.is_empty() {
            return Vec::new();
        }
        // Only a client that names an allow-listed path and unit gets here: the stat can block
        // on its own file system, and the compositor with it.
        let Ok(file) = process.executable_file(pid) else {
            return Vec::new();
        };
        named
            .into_iter()
            .filter(|(executable, _, _)| process.file(executable).is_ok_and(|f| f == file))
            .flat_map(|(_, _, privileges)| privileges.iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    const BAR: &str = "/usr/bin/athanor-bar athanor-bar.service zwlr_layer_shell_v1 ext_foreign_toplevel_list_v1\n";

    /// The bar's executable file, as the compositor sees it.
    const BAR_FILE: FileId = FileId { dev: 1, ino: 1 };

    fn bar() -> Identity {
        Identity {
            executable: "/usr/bin/athanor-bar".into(),
            unit: Some("athanor-bar.service".into()),
        }
    }

    /// The bar's process, and one that runs another file at the bar's path.
    const BAR_PID: i32 = 1;
    const IMPOSTOR_PID: i32 = 2;

    /// The compositor's file system, the bar's executable and nothing else, and the files of the
    /// bar's process and of the impostor's.
    struct Files;

    impl Process for Files {
        fn executable(&self, _: i32) -> io::Result<PathBuf> {
            Err(io::ErrorKind::NotFound.into())
        }
        fn cgroup(&self, _: i32) -> io::Result<String> {
            Err(io::ErrorKind::NotFound.into())
        }
        fn executable_file(&self, pid: i32) -> io::Result<FileId> {
            if pid == BAR_PID {
                Ok(BAR_FILE)
            } else {
                Ok(FileId { dev: 1, ino: 2 })
            }
        }
        fn file(&self, path: &Path) -> io::Result<FileId> {
            if path == Path::new("/usr/bin/athanor-bar") {
                Ok(BAR_FILE)
            } else {
                Err(io::ErrorKind::NotFound.into())
            }
        }
    }

    #[test]
    fn the_unit_is_the_first_below_the_slices_and_the_user_manager() {
        let user =
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/athanor-bar.service\n";
        assert_eq!(unit(user), Some("athanor-bar.service"));
        let system = "0::/system.slice/greetd.service\n";
        assert_eq!(unit(system), Some("greetd.service"));
        let session = "0::/user.slice/user-1000.slice/session-2.scope\n";
        assert_eq!(unit(session), Some("session-2.scope"));
        // A delegated unit naming its sub-cgroup after the bar is still itself.
        let delegated = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/x.service/athanor-bar.service\n";
        assert_eq!(unit(delegated), Some("x.service"));
        // cgroup v1 only, the root, a container's namespace, or a slice alone: no unit.
        assert_eq!(unit("1:name=systemd:/athanor-bar.service\n"), None);
        assert_eq!(unit("0::/\n"), None);
        assert_eq!(unit("0::/user.slice\n"), None);
        assert_eq!(
            unit("0::/user.slice/user-1000.slice/user@1000.service\n"),
            None
        );
    }

    #[test]
    fn a_grant_needs_both_the_executable_and_the_unit() {
        let list = AllowList::parse(BAR).expect("allow-list");
        assert_eq!(
            list.grants(&bar(), BAR_PID, &Files),
            [Privilege::TopLayers, Privilege::ToplevelList]
        );
        let other_executable = Identity {
            executable: "/usr/bin/sh".into(),
            ..bar()
        };
        assert!(list.grants(&other_executable, BAR_PID, &Files).is_empty());
        let other_unit = Identity {
            unit: Some("app-sh.scope".into()),
            ..bar()
        };
        assert!(list.grants(&other_unit, BAR_PID, &Files).is_empty());
        let no_unit = Identity {
            unit: None,
            ..bar()
        };
        assert!(list.grants(&no_unit, BAR_PID, &Files).is_empty());
    }

    /// A file at the bar's path that is not the bar's file, as in a sandbox with its own `/usr`,
    /// is granted nothing.
    #[test]
    fn a_grant_needs_the_very_file_at_the_executable_s_path() {
        let list = AllowList::parse(BAR).expect("allow-list");
        assert!(list.grants(&bar(), IMPOSTOR_PID, &Files).is_empty());
    }

    #[test]
    fn a_missing_or_unreadable_allow_list_grants_nothing() {
        let missing = Path::new("/nonexistent/athanor/privileged-clients");
        assert!(AllowList::load(missing)
            .grants(&bar(), BAR_PID, &Files)
            .is_empty());
        // A directory cannot be read as a file.
        assert!(AllowList::load(Path::new("/"))
            .grants(&bar(), BAR_PID, &Files)
            .is_empty());
    }

    #[test]
    fn a_malformed_allow_list_grants_nothing() {
        for bad in [
            "/usr/bin/athanor-bar\n",
            "/usr/bin/athanor-bar athanor-bar.service\n",
            "/home/u/bar athanor-bar.service zwlr_layer_shell_v1\n",
            "/usr/../home/u/bar athanor-bar.service zwlr_layer_shell_v1\n",
            "/usr/bin/.. athanor-bar.service zwlr_layer_shell_v1\n",
            "/usr/bin/athanor-bar athanor-bar zwlr_layer_shell_v1\n",
            "/usr/bin/athanor-bar app.slice/athanor-bar.service zwlr_layer_shell_v1\n",
            "/usr/bin/athanor-bar athanor-bar.service wl_compositor\n",
        ] {
            assert!(AllowList::parse(&format!("{BAR}{bad}")).is_err(), "{bad}");
            let path =
                std::env::temp_dir().join(format!("athanor-allow-list-{}", std::process::id()));
            std::fs::write(&path, format!("{BAR}{bad}")).expect("write");
            let loaded = AllowList::load(&path);
            std::fs::remove_file(&path).expect("remove");
            assert!(loaded.grants(&bar(), BAR_PID, &Files).is_empty(), "{bad}");
        }
    }

    /// The allow-list the image installs parses.
    #[test]
    fn the_shipped_allow_list_parses() {
        let shipped =
            AllowList::parse(include_str!("../data/privileged-clients")).expect("allow-list");
        assert!(!shipped.grants(&bar(), BAR_PID, &Files).is_empty());
    }

    /// The shipped allow-list lets the bar, the dock, the launcher and the control center read
    /// every window's state, and all but the control center act on windows.
    #[test]
    fn only_the_bar_the_dock_and_the_launcher_act_on_windows() {
        /// Every path is the file of every process.
        struct Any;
        impl Process for Any {
            fn executable(&self, _: i32) -> io::Result<PathBuf> {
                Err(io::ErrorKind::NotFound.into())
            }
            fn cgroup(&self, _: i32) -> io::Result<String> {
                Err(io::ErrorKind::NotFound.into())
            }
            fn executable_file(&self, _: i32) -> io::Result<FileId> {
                Ok(BAR_FILE)
            }
            fn file(&self, _: &Path) -> io::Result<FileId> {
                Ok(BAR_FILE)
            }
        }
        let shipped =
            AllowList::parse(include_str!("../data/privileged-clients")).expect("allow-list");
        for (name, acts) in [
            ("athanor-bar", true),
            ("athanor-control-center", false),
            ("athanor-dock", true),
            ("athanor-launcher", true),
        ] {
            let client = Identity {
                executable: format!("/usr/bin/{name}").into(),
                unit: Some(format!("{name}.service")),
            };
            let granted = shipped.grants(&client, BAR_PID, &Any);
            assert!(granted.contains(&Privilege::ToplevelState), "{name} reads");
            assert_eq!(
                granted.contains(&Privilege::ToplevelActions),
                acts,
                "{name} acts"
            );
        }
    }

    /// The shipped allow-list lets the bar alone follow and switch the keyboard group: the
    /// dock, the launcher and the control center never call `watch_keyboard_group`.
    #[test]
    fn only_the_bar_follows_the_keyboard_group() {
        /// Every path is the file of every process.
        struct Any;
        impl Process for Any {
            fn executable(&self, _: i32) -> io::Result<PathBuf> {
                Err(io::ErrorKind::NotFound.into())
            }
            fn cgroup(&self, _: i32) -> io::Result<String> {
                Err(io::ErrorKind::NotFound.into())
            }
            fn executable_file(&self, _: i32) -> io::Result<FileId> {
                Ok(BAR_FILE)
            }
            fn file(&self, _: &Path) -> io::Result<FileId> {
                Ok(BAR_FILE)
            }
        }
        let shipped =
            AllowList::parse(include_str!("../data/privileged-clients")).expect("allow-list");
        for (name, follows) in [
            ("athanor-bar", true),
            ("athanor-control-center", false),
            ("athanor-dock", false),
            ("athanor-launcher", false),
        ] {
            let client = Identity {
                executable: format!("/usr/bin/{name}").into(),
                unit: Some(format!("{name}.service")),
            };
            let granted = shipped.grants(&client, BAR_PID, &Any);
            assert_eq!(
                granted.contains(&Privilege::KeyboardGroup),
                follows,
                "{name} follows the keyboard group"
            );
        }
    }

    /// A process whose pid cannot be read has no identity.
    #[test]
    fn an_unreadable_peer_has_no_identity() {
        struct Unreadable;
        impl Process for Unreadable {
            fn executable(&self, _: i32) -> io::Result<PathBuf> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
            fn executable_file(&self, pid: i32) -> io::Result<FileId> {
                Files.executable_file(pid)
            }
            fn file(&self, path: &Path) -> io::Result<FileId> {
                Files.file(path)
            }
            fn cgroup(&self, _: i32) -> io::Result<String> {
                Ok("0::/app.slice/athanor-bar.service\n".into())
            }
        }
        let (server, _client) = UnixStream::pair().expect("socket pair");
        assert!(identify(server.as_fd(), &Unreadable, &AllowList::default()).is_err());
    }

    /// A peer killed and reaped from inside one of the reads of its pid.
    struct Exits {
        child: std::sync::Mutex<std::process::Child>,
        /// Killed in the `stat` of its executable, which runs only for a grant; otherwise in the
        /// `readlink`.
        at_the_stat: bool,
    }

    impl Exits {
        fn exit(&self) -> io::Result<()> {
            let mut child = self.child.lock().expect("child");
            child.kill()?;
            child.wait().map(drop)
        }
    }

    impl Process for Exits {
        fn executable(&self, _: i32) -> io::Result<PathBuf> {
            if !self.at_the_stat {
                self.exit()?;
            }
            Ok("/usr/bin/athanor-bar".into())
        }
        fn executable_file(&self, _: i32) -> io::Result<FileId> {
            if self.at_the_stat {
                self.exit()?;
            }
            Ok(BAR_FILE)
        }
        fn file(&self, path: &Path) -> io::Result<FileId> {
            Files.file(path)
        }
        fn cgroup(&self, _: i32) -> io::Result<String> {
            Ok("0::/app.slice/athanor-bar.service\n".into())
        }
    }

    /// What `identify` reads of a peer that `Exits` kills.
    fn read_a_peer_that_exits(at_the_stat: bool) -> Result<(Identity, Vec<Privilege>), Error> {
        let path = std::env::temp_dir().join(format!(
            "athanor-identity-{}-{at_the_stat}.sock",
            std::process::id()
        ));
        let listener = std::os::unix::net::UnixListener::bind(&path).expect("listen");
        let child = std::process::Command::new(std::env::current_exe().expect("executable"))
            .args(["--exact", "identity::tests::peer", "--ignored", "--quiet"])
            .env("ATHANOR_IDENTITY_PEER", &path)
            .spawn()
            .expect("the peer");
        let (server, _) = listener.accept().expect("the peer connects");
        std::fs::remove_file(&path).expect("remove");
        let exits = Exits {
            child: std::sync::Mutex::new(child),
            at_the_stat,
        };
        identify(
            server.as_fd(),
            &exits,
            &AllowList::parse(BAR).expect("allow-list"),
        )
    }

    /// A peer that exits, and is reaped, between the pidfd and the reads of its pid: what was
    /// read may be another process's, so it has no identity.
    #[test]
    fn a_peer_that_exits_while_it_is_read_has_no_identity() {
        let read = read_a_peer_that_exits(false);
        assert!(read.is_err(), "{read:?}");
    }

    /// The same, when it exits in the `stat` of its executable, after its path and unit matched
    /// a line: the pid is read again after the `stat` too.
    #[test]
    fn a_peer_that_exits_while_its_file_is_read_has_no_identity() {
        let read = read_a_peer_that_exits(true);
        assert!(read.is_err(), "{read:?}");
    }

    /// The peer of `read_a_peer_that_exits`: it connects and waits to be killed.
    #[test]
    #[ignore = "run by read_a_peer_that_exits"]
    fn peer() {
        use std::io::Read;
        if let Some(path) = std::env::var_os("ATHANOR_IDENTITY_PEER") {
            let mut stream = UnixStream::connect(path).expect("connect");
            stream.read_exact(&mut [0]).expect("killed before any byte");
        }
    }

    /// A socket with no peer has no pidfd, and so no identity: no other way to a pid is tried.
    #[test]
    fn a_socket_with_no_peer_has_no_identity() {
        let unbound = std::os::unix::net::UnixDatagram::unbound().expect("socket");
        assert!(identify(unbound.as_fd(), &Proc, &AllowList::default()).is_err());
    }

    /// The production path, on the test's own process at the other end of a socket pair.
    #[test]
    fn the_identity_of_this_process_is_read_through_its_pidfd() {
        let (server, _client) = UnixStream::pair().expect("socket pair");
        // SO_PEERPIDFD is the only way to the peer's pidfd: an identity proves the kernel has it.
        let (identity, _) = identify(server.as_fd(), &Proc, &AllowList::default())
            .expect("identity through SO_PEERPIDFD");
        eprintln!("peer identity through SO_PEERPIDFD: {identity:?}");
        let executable = std::env::current_exe().expect("executable");
        assert_eq!(
            identity.executable,
            executable.canonicalize().expect("canonical")
        );
        // The file /proc/<pid>/exe opens is the one at the executable's path.
        let pid = i32::try_from(std::process::id()).expect("pid");
        assert_eq!(
            Proc.executable_file(pid).expect("stat"),
            Proc.file(&executable).expect("stat")
        );
        let cgroup = std::fs::read_to_string("/proc/self/cgroup").expect("cgroup");
        assert_eq!(identity.unit.as_deref(), unit(&cgroup));
    }

    /// The shell clients the shipped allow-list grants, by crate under `forge/specs`, with the
    /// interfaces each uses (doc_session.md, SN12; #683, widened). `Client::connect` binds every
    /// privileged global the compositor offers it; a client is granted only the ones it uses:
    /// the dock lists windows, the launcher lists and captures them, the control center only
    /// draws on the overlay layer. Then the upstream clients of doc_session.md revision 4,
    /// SN12 and SN15, which no crate of the tree builds: the locker, the idle daemon and
    /// cosmic-osd.
    const SHELL_CLIENTS: [(&str, &str, &[&str]); 7] = [
        (
            "athanor-bar",
            "athanor-bar.service",
            &[
                "zwlr_layer_shell_v1",
                "ext_foreign_toplevel_list_v1",
                "ext_workspace_manager_v1",
                "athanor_toplevel_manager_v1",
                "athanor_toplevel_actions_v1",
                "athanor_keyboard_group_manager_v1",
            ],
        ),
        (
            "athanor-launcher",
            "athanor-launcher.service",
            &[
                "zwlr_layer_shell_v1",
                "ext_foreign_toplevel_list_v1",
                "ext_foreign_toplevel_image_capture_source_manager_v1",
                "ext_image_copy_capture_manager_v1",
                "ext_workspace_manager_v1",
                "athanor_toplevel_manager_v1",
                "athanor_toplevel_actions_v1",
            ],
        ),
        (
            "athanor-dock",
            "athanor-dock.service",
            &[
                "zwlr_layer_shell_v1",
                "ext_foreign_toplevel_list_v1",
                "ext_workspace_manager_v1",
                "athanor_toplevel_manager_v1",
                "athanor_toplevel_actions_v1",
            ],
        ),
        (
            "athanor-control-center",
            "athanor-control-center.service",
            &[
                "zwlr_layer_shell_v1",
                "zwlr_output_manager_v1",
                "ext_workspace_manager_v1",
                "ext_foreign_toplevel_list_v1",
                "athanor_toplevel_manager_v1",
            ],
        ),
        (
            "cosmic-greeter",
            "cosmic-greeter-locker.scope",
            &["ext_session_lock_manager_v1"],
        ),
        (
            "cosmic-idle",
            "cosmic-idle.scope",
            &["zwlr_output_power_manager_v1", "zwlr_layer_shell_v1"],
        ),
        ("cosmic-osd", "cosmic-osd.service", &["zwlr_layer_shell_v1"]),
    ];

    /// Crates that draw on the top or overlay layer without a line: the compositor itself, and
    /// the greeter, which runs on cosmic-comp in greetd's session until it moves
    /// (doc_own_stack.md, section 6, step 5).
    const OUTSIDE_THE_LIST: [&str; 2] = ["athanor-compositor", "athanor-greeter-ui"];

    /// Every `.rs` file under `dir`.
    fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }

    /// The crates under `forge/specs` whose sources contain one of `needles`.
    fn crates_with(needles: &[&str]) -> Vec<String> {
        let specs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut crates = Vec::new();
        for package in std::fs::read_dir(&specs).expect("forge/specs") {
            let package = package.expect("entry").path();
            let name = package
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            let src = package.join(format!("{name}-1.0.0/src"));
            if !src.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            sources(&src, &mut files);
            let uses = files.iter().any(|file| {
                let text = std::fs::read_to_string(file).expect("source");
                needles.iter().any(|needle| text.contains(needle))
            });
            if uses {
                crates.push(name);
            }
        }
        crates.sort();
        crates
    }

    #[test]
    fn every_shipped_shell_client_on_the_upper_layers_or_privileged_globals_has_its_line() {
        let found = crates_with(&["Layer::Top", "Layer::Overlay", "Client::connect"]);
        // The scan must see the clients it is about, or it proves nothing.
        assert!(found.iter().any(|name| name == "athanor-bar"), "{found:?}");
        for name in found {
            assert!(
                OUTSIDE_THE_LIST.contains(&name.as_str())
                    || SHELL_CLIENTS.iter().any(|(client, _, _)| *client == name),
                "{name} draws on the top or overlay layer or connects to the compositor's \
                 privileged globals, and has no line in data/privileged-clients"
            );
        }

        let shipped =
            AllowList::parse(include_str!("../data/privileged-clients")).expect("allow-list");
        let expected: Vec<_> = SHELL_CLIENTS
            .iter()
            .map(|(name, unit, interfaces)| {
                let privileges = interfaces
                    .iter()
                    .map(|interface| Privilege::named(interface).expect("privileged"))
                    .collect::<Vec<_>>();
                (
                    PathBuf::from(format!("/usr/bin/{name}")),
                    unit.to_string(),
                    privileges,
                )
            })
            .collect();
        assert_eq!(shipped.0, expected);
        // Only the locker locks the session (SN15).
        assert!(shipped.0.iter().all(|(path, unit, privileges)| {
            !privileges.contains(&Privilege::SessionLock)
                || (path == Path::new("/usr/bin/cosmic-greeter")
                    && unit == "cosmic-greeter-locker.scope")
        }));
    }
}
