//! The directories a user unit of the shell uses, from the environment its unit gives it. A relative or
//! empty value counts as unset (XDG Base Directory Specification).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;

/// The most links a walk follows, as the kernel's own resolution (MAXSYMLINKS).
const MAX_LINKS: usize = 40;

/// A descriptor this module opened and checked: what a Landlock write grant names
/// (`sandbox::restrict`, `sandbox::restrict_writes`). Its field is private, so only the
/// functions here build one, and a grant is never a descriptor opened some other way:
///
/// ```compile_fail
/// fn grant(dir: std::fs::File) {
///     let _ = athanor_unit::sandbox::restrict_writes(&[dir], &[]);
/// }
/// ```
#[derive(Debug)]
pub struct Checked(OwnedFd);

impl AsFd for Checked {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

/// `runtime/name`, created with mode 0700 if missing, opened once without following a link and
/// checked on that descriptor: a directory, owned by the owner of `runtime`, of mode 0700.
/// Another process of the user could otherwise point a confined daemon's writes elsewhere. The
/// descriptor is what a Landlock grant names (`sandbox::restrict`): a link swapped in for the
/// path after the check widens nothing. `runtime` is reached by `walk`, and `name` is created
/// and opened in that descriptor, so a link in `runtime` or above it is refused unless a
/// directory of root's alone holds it (#1061).
pub fn private_dir(runtime: &Path, name: &str) -> io::Result<Checked> {
    let dir = runtime.join(name);
    let refuse = |why: String| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {why}", dir.display()),
        )
    };
    if Path::new(name).file_name() != Some(OsStr::new(name)) {
        return Err(refuse(
            "is not one name in its runtime directory".to_owned(),
        ));
    }
    let parent = walk(&dir, runtime)?;
    match nix::sys::stat::mkdirat(Some(parent.as_raw_fd()), name, Mode::S_IRWXU) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(e) => return Err(e.into()),
    }
    let opened = open_at(&parent, OsStr::new(name), OFlag::O_DIRECTORY)
        .map_err(|e| refuse(format!("is not a directory, or is a link: {e}")))?;
    let owner = parent.metadata()?.uid();
    let found = opened.metadata()?;
    if !found.file_type().is_dir() {
        return Err(refuse("is not a directory".to_owned()));
    }
    if found.uid() != owner {
        return Err(refuse("is not owned by the user".to_owned()));
    }
    if found.permissions().mode() & 0o7777 != 0o700 {
        return Err(refuse("does not have mode 0700".to_owned()));
    }
    Ok(Checked(opened.into()))
}

/// `path`, a directory a write grant names: opened once with O_PATH, without following a link
/// in its last component, and checked on that descriptor: a directory owned by the process's
/// user or by root (`/tmp`), never another user's. The descriptor is what the grant names
/// (`sandbox::restrict`, `sandbox::restrict_writes`), so a link put at the path before the
/// open is refused, and one swapped in after it widens nothing. The components above the final
/// one are reached by `walk`, so a link in one of them is refused unless a directory of root's
/// alone holds it (#1061).
pub fn owned_dir(path: &Path) -> io::Result<Checked> {
    let (opened, found) = not_a_link(path, OFlag::O_DIRECTORY)?;
    if found.uid() != 0 && found.uid() != nix::unistd::geteuid().as_raw() {
        return Err(refused(path, "is owned by another user"));
    }
    Ok(Checked(opened.into()))
}

/// `path`, a node of the system's that a `WriteFile` grant names (`/dev/dri`, `/dev/null`):
/// opened as `owned_dir` opens a directory, of any kind other than a link. Its owner is not
/// checked: no user can put anything at those paths, and in a user namespace that does not map
/// root (the shell rig's container, the greeter's sandbox) their owner reads as the overflow id.
pub fn owned_node(path: &Path) -> io::Result<Checked> {
    not_a_link(path, OFlag::empty()).map(|(opened, _)| Checked(opened.into()))
}

/// Each of `paths` that exists, opened by `open` (`owned_dir`, `owned_node`). One that fails its
/// check is left out with a warning, so that a grant only narrows; a missing one is skipped, as
/// Landlock has nothing to hold on to.
pub fn existing(paths: &[&Path], open: fn(&Path) -> io::Result<Checked>) -> Vec<Checked> {
    paths
        .iter()
        .filter_map(|path| match open(path) {
            Ok(fd) => Some(fd),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "not granted: it fails the check");
                None
            }
        })
        .collect()
}

/// `private_dir` of each of `names` in `runtime`. One that fails is left out with a warning, so
/// that a grant only narrows.
pub fn private_dirs(runtime: &Path, names: &[&str]) -> Vec<Checked> {
    names
        .iter()
        .filter_map(|name| {
            private_dir(runtime, name)
                .map_err(
                    |error| tracing::warn!(%error, dir = name, "not granted: it fails the check"),
                )
                .ok()
        })
        .collect()
}

fn refused(path: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{} {why}", path.display()),
    )
}

/// `path` opened with O_PATH, O_NOFOLLOW and `kind` in the directory `walk` reaches above it,
/// and what fstat says of it; refused when it is a link, or not a directory when `kind` holds
/// O_DIRECTORY.
fn not_a_link(path: &Path, kind: OFlag) -> io::Result<(fs::File, fs::Metadata)> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(refused(path, "names no final component"));
    };
    // With O_PATH and O_NOFOLLOW a link opens as itself: refused below.
    let opened = open_at(&walk(path, dir)?, name, kind)?;
    let found = opened.metadata()?;
    if found.file_type().is_symlink() {
        return Err(refused(path, "is a link"));
    }
    if kind.contains(OFlag::O_DIRECTORY) && !found.is_dir() {
        return Err(refused(path, "is not a directory"));
    }
    Ok((opened, found))
}

/// The directory `dir` names, on behalf of `path`, reached from `/` one component at a time:
/// each is opened with O_PATH and O_NOFOLLOW in the descriptor of the one before, so the kernel
/// never resolves more than one name. A link on the way is read with readlinkat and its target
/// walked in turn, at most `MAX_LINKS` of them, only when the directory that holds it is one
/// `follows_links` trusts (`/home` links to `var/home` in `/`); any other link is refused. `..`
/// goes back to the descriptor the walk held before, never above `/`, so it is the directory
/// the walk came through and not one the kernel looks up again after a rename.
fn walk(path: &Path, dir: &Path) -> io::Result<fs::File> {
    if !dir.is_absolute() {
        return Err(refused(path, "is not an absolute path"));
    }
    let mut here = (root()?, PathBuf::from("/"));
    let mut above = Vec::new();
    let mut todo = names(dir);
    let mut links = 0;
    while let Some(name) = todo.pop() {
        if name == ".." {
            if let Some(up) = above.pop() {
                here = up;
            }
            continue;
        }
        let at = here.1.join(&name);
        // Without O_DIRECTORY, which would fail on a link before it could be told from a file:
        // fstat tells a directory from a link on the descriptor.
        let opened = open_at(&here.0, &name, OFlag::empty())?;
        let found = opened.metadata()?;
        if found.is_dir() {
            above.push(std::mem::replace(&mut here, (opened, at)));
            continue;
        }
        if !found.file_type().is_symlink() {
            return Err(Errno::ENOTDIR.into());
        }
        let holder = here.0.metadata()?;
        if !follows_links(holder.uid(), holder.mode()) {
            let why = format!(
                "has a link on the way, {}, in a directory that is not root's alone",
                at.display()
            );
            return Err(refused(path, &why));
        }
        links += 1;
        if links > MAX_LINKS {
            return Err(refused(
                path,
                "has more links on the way than the kernel follows",
            ));
        }
        let target = PathBuf::from(nix::fcntl::readlinkat(
            Some(here.0.as_raw_fd()),
            name.as_os_str(),
        )?);
        if target.as_os_str().is_empty() {
            return Err(Errno::ENOENT.into());
        }
        if target.is_absolute() {
            here = (root()?, PathBuf::from("/"));
            above.clear();
        }
        todo.extend(names(&target));
    }
    Ok(here.0)
}

/// Whether `walk` follows a link in a directory of owner `uid` and mode `mode`: only in one of
/// root's that neither its group nor others can write, where no user can put a link.
fn follows_links(uid: u32, mode: u32) -> bool {
    uid == 0 && mode & 0o022 == 0
}

/// The names of `path` after its root, `..` included, last first: `walk` pops the next one.
fn names(path: &Path) -> Vec<OsString> {
    let mut names: Vec<OsString> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
        })
        .collect();
    names.reverse();
    names
}

/// `/`, opened with O_PATH: where `walk` starts. std adds O_CLOEXEC.
fn root() -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_PATH | OFlag::O_DIRECTORY).bits())
        .open("/")
}

/// `name`, one component, opened in `dir` with O_PATH, O_NOFOLLOW, O_CLOEXEC and `kind`.
#[allow(unsafe_code)]
fn open_at(dir: &fs::File, name: &OsStr, kind: OFlag) -> io::Result<fs::File> {
    let fd = nix::fcntl::openat(
        Some(dir.as_raw_fd()),
        name,
        OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | kind,
        Mode::empty(),
    )?;
    // SAFETY: on success openat returned a new descriptor that nothing else owns.
    Ok(fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// The unit's own runtime directory, `%t/<unit>`.
    pub unit_runtime: PathBuf,
    /// The crash-loop record (doc_shell.md, SH8), in the unit's runtime directory.
    pub failures: PathBuf,
    /// `$XDG_CONFIG_HOME`: the layout document and the favourites.
    pub config: PathBuf,
    /// `$XDG_CACHE_HOME`: GTK's and fontconfig's caches.
    pub cache: PathBuf,
    /// `$XDG_RUNTIME_DIR`: the socket directories of launched applications (BR2) and dconf.
    pub runtime: PathBuf,
}

impl Dirs {
    /// `unit` names the unit's own runtime directory, `athanor-bar` or `athanor-dock`.
    pub fn from_vars(unit: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<Dirs> {
        let absolute = |name: &str| var(name).map(PathBuf::from).filter(|dir| dir.is_absolute());
        let home = absolute("HOME");
        let config = absolute("XDG_CONFIG_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".config")))?;
        let cache =
            absolute("XDG_CACHE_HOME").or_else(|| home.as_ref().map(|home| home.join(".cache")))?;
        let runtime = absolute("XDG_RUNTIME_DIR")?;
        // RuntimeDirectory=<unit> athanor (the second entry is launch()'s own use, for a
        // started application's security context) makes systemd set RUNTIME_DIRECTORY to both
        // paths, colon-separated in the declared order: not a single path, so `absolute` above
        // cannot be used here. Picking the entry named after the unit keeps this right
        // regardless of that order.
        let unit_runtime = var("RUNTIME_DIRECTORY")
            .and_then(|value| value.into_string().ok())
            .and_then(|value| {
                value
                    .split(':')
                    .map(PathBuf::from)
                    .find(|dir| dir.file_name() == Some(OsStr::new(unit)))
            })
            .filter(|dir| dir.is_absolute())
            .unwrap_or_else(|| runtime.join(unit));
        Some(Dirs {
            failures: unit_runtime.join("failures"),
            unit_runtime,
            config,
            cache,
            runtime,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox;
    use std::os::unix::fs::DirBuilderExt;

    fn runtime(name: &str) -> PathBuf {
        let runtime =
            std::env::temp_dir().join(format!("athanor-unit-{name}-{}", std::process::id()));
        fs::remove_dir_all(&runtime).ok();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&runtime)
            .expect("runtime");
        runtime
    }

    #[test]
    fn a_private_dir_is_created_or_checked() {
        let runtime = runtime("private");
        let made = runtime.join("d");
        private_dir(&runtime, "d").expect("created");
        assert_eq!(
            fs::metadata(&made).expect("made").permissions().mode() & 0o7777,
            0o700
        );
        private_dir(&runtime, "d").expect("an existing one that passes");

        fs::set_permissions(&made, fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(private_dir(&runtime, "d").is_err(), "mode 0755");
        fs::remove_dir(&made).expect("rmdir");

        fs::write(&made, b"").expect("file");
        assert!(private_dir(&runtime, "d").is_err(), "a file");
        fs::remove_dir_all(&runtime).expect("cleanup");
    }

    #[test]
    fn a_link_swapped_in_after_creation_is_refused() {
        let runtime = runtime("swapped");
        private_dir(&runtime, "d").expect("created");
        fs::rename(runtime.join("d"), runtime.join("moved")).expect("rename");
        std::os::unix::fs::symlink(runtime.join("moved"), runtime.join("d")).expect("symlink");
        let refused = private_dir(&runtime, "d").expect_err("a link to a good directory");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied, "{refused}");
        fs::remove_dir_all(&runtime).expect("cleanup");
    }

    #[test]
    fn the_grant_follows_the_descriptor_not_the_path() {
        let runtime = runtime("grant");
        let checked = private_dir(&runtime, "d").expect("created");
        // After the check, the path is swapped for a link to another directory of the user.
        fs::rename(runtime.join("d"), runtime.join("moved")).expect("rename");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(runtime.join("elsewhere"))
            .expect("elsewhere");
        std::os::unix::fs::symlink(runtime.join("elsewhere"), runtime.join("d")).expect("symlink");
        let thread_runtime = runtime.clone();
        // Landlock binds the thread that applies it: the test's other threads stay free.
        std::thread::spawn(move || {
            sandbox::restrict(&[], &[checked]).expect("confined");
            fs::write(thread_runtime.join("moved/file"), b"x").expect("the checked directory");
            let error = fs::write(thread_runtime.join("d/file"), b"x").expect_err("the link");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        })
        .join()
        .expect("the confined thread");
        assert!(!runtime.join("elsewhere/file").exists());
        fs::remove_dir_all(&runtime).expect("cleanup");
    }

    #[test]
    fn an_owned_dir_is_a_directory_and_never_a_link() {
        let runtime = runtime("owned");
        let (dir, file, link) = (runtime.join("d"), runtime.join("f"), runtime.join("l"));
        fs::create_dir(&dir).expect("mkdir");
        fs::write(&file, b"").expect("file");
        std::os::unix::fs::symlink(&dir, &link).expect("symlink");
        owned_dir(&dir).expect("a directory of the user's");
        assert!(owned_dir(&file).is_err(), "a file");
        assert!(owned_dir(&link).is_err(), "a link to a directory");
        owned_node(&file).expect("a node of any kind");
        assert!(owned_node(&link).is_err(), "a link");
        assert_eq!(
            existing(&[&dir, &link, &runtime.join("missing")], owned_dir).len(),
            1,
            "the link is left out and the missing one skipped"
        );
        fs::remove_dir_all(&runtime).expect("cleanup");
    }

    #[test]
    fn a_link_put_at_a_granted_path_does_not_widen_the_grant() {
        let runtime = runtime("link-grant");
        let (granted, elsewhere, cache) = (
            runtime.join("granted"),
            runtime.join("elsewhere"),
            runtime.join("cache"),
        );
        fs::create_dir(&granted).expect("granted");
        fs::create_dir(&elsewhere).expect("elsewhere");
        // Another process of the user puts a link at a path the program grants, before the grant.
        std::os::unix::fs::symlink(&elsewhere, &cache).expect("symlink");
        let thread_runtime = runtime.clone();
        std::thread::spawn(move || {
            let write = existing(&[&thread_runtime.join("granted"), &cache], owned_dir);
            sandbox::restrict_writes(&write, &[]).expect("confined");
            fs::write(thread_runtime.join("granted/file"), b"x").expect("the checked directory");
            let error = fs::write(cache.join("file"), b"x").expect_err("the link");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        })
        .join()
        .expect("the confined thread");
        assert!(!elsewhere.join("file").exists());
        fs::remove_dir_all(&runtime).expect("cleanup");
    }

    #[test]
    fn a_link_on_the_way_in_a_directory_the_user_writes_is_refused() {
        let runtime = runtime("hop");
        let (shared, real) = (runtime.join("shared"), runtime.join("real"));
        // Writable by others as well, so the link is refused whoever runs the test: the shell
        // rig runs it as root, and a directory of root's alone would pass.
        fs::create_dir(&shared).expect("shared");
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).expect("chmod");
        fs::create_dir(&real).expect("real");
        private_dir(&real, "d").expect("a private directory");
        fs::write(real.join("f"), b"").expect("file");
        let hop = shared.join("hop");
        std::os::unix::fs::symlink(&real, &hop).expect("symlink");
        owned_dir(&real.join("d")).expect("the directory itself");
        owned_node(&real.join("f")).expect("the node itself");
        for (what, opened) in [
            ("owned_dir", owned_dir(&hop.join("d"))),
            ("owned_node", owned_node(&hop.join("f"))),
            ("private_dir", private_dir(&hop, "d")),
        ] {
            let error = opened.expect_err(what);
            assert_eq!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{what}: {error}"
            );
            assert!(error.to_string().contains("shared/hop"), "{what}: {error}");
        }
        fs::remove_dir_all(&runtime).expect("cleanup");
    }

    #[test]
    fn a_link_is_followed_only_in_a_directory_of_roots_alone() {
        assert!(follows_links(0, 0o040755), "/");
        assert!(follows_links(0, 0o040555), "/ on a bootc system");
        assert!(!follows_links(1000, 0o040700), "a directory of the user's");
        assert!(
            !follows_links(1000, 0o040755),
            "a directory of the user's that others read"
        );
        assert!(!follows_links(0, 0o040775), "its group writes it");
        assert!(!follows_links(0, 0o041777), "others write it, as /tmp");
    }

    #[test]
    fn a_link_in_a_directory_of_roots_alone_is_followed() {
        // Fedora's /bin links to usr/bin in /, and /var/run to ../run in /var: the walk reaches
        // the directory the kernel does, `..` included. A system without them skips that pair.
        for (link, target) in [("/bin", "/usr/bin"), ("/var/run", "/run")] {
            let link = Path::new(link);
            if !fs::symlink_metadata(link).is_ok_and(|found| found.file_type().is_symlink()) {
                continue;
            }
            let walked = walk(link, link).expect("walked").metadata().expect("fstat");
            let followed = fs::metadata(target).expect("the target");
            assert_eq!(
                (walked.dev(), walked.ino()),
                (followed.dev(), followed.ino()),
                "{}",
                link.display()
            );
        }
        let up = walk(Path::new("/usr/.."), Path::new("/usr/..")).expect("/usr/..");
        assert_eq!(
            up.metadata().expect("fstat").ino(),
            fs::metadata("/").expect("/").ino()
        );
        assert!(
            walk(Path::new("usr"), Path::new("usr")).is_err(),
            "a relative path"
        );
    }

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn the_unit_directories_come_first() {
        let dirs = Dirs::from_vars(
            "athanor-bar",
            vars(&[
                ("HOME", "/home/u"),
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_CACHE_HOME", "/k"),
                ("XDG_RUNTIME_DIR", "/run/user/1"),
                ("RUNTIME_DIRECTORY", "/run/user/1/athanor-bar"),
            ]),
        );
        assert_eq!(
            dirs,
            Some(Dirs {
                unit_runtime: PathBuf::from("/run/user/1/athanor-bar"),
                failures: PathBuf::from("/run/user/1/athanor-bar/failures"),
                config: PathBuf::from("/c"),
                cache: PathBuf::from("/k"),
                runtime: PathBuf::from("/run/user/1"),
            })
        );
    }

    #[test]
    fn the_runtime_directory_env_var_lists_more_than_one_path() {
        // The real unit declares RuntimeDirectory=athanor-bar athanor: systemd sets
        // RUNTIME_DIRECTORY to both paths, colon-separated.
        let dirs = Dirs::from_vars(
            "athanor-bar",
            vars(&[
                ("HOME", "/home/u"),
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_CACHE_HOME", "/k"),
                ("XDG_RUNTIME_DIR", "/run/user/1"),
                (
                    "RUNTIME_DIRECTORY",
                    "/run/user/1/athanor-bar:/run/user/1/athanor",
                ),
            ]),
        )
        .expect("dirs");
        assert_eq!(
            dirs.failures,
            PathBuf::from("/run/user/1/athanor-bar/failures")
        );
    }

    #[test]
    fn the_athanor_bar_entry_is_found_regardless_of_its_position() {
        let dirs = Dirs::from_vars(
            "athanor-bar",
            vars(&[
                ("HOME", "/home/u"),
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_CACHE_HOME", "/k"),
                ("XDG_RUNTIME_DIR", "/run/user/1"),
                (
                    "RUNTIME_DIRECTORY",
                    "/run/user/1/athanor:/run/user/1/athanor-bar",
                ),
            ]),
        )
        .expect("dirs");
        assert_eq!(
            dirs.failures,
            PathBuf::from("/run/user/1/athanor-bar/failures")
        );
    }

    #[test]
    fn the_unit_name_picks_its_own_entry() {
        // Two units of one session share XDG_RUNTIME_DIR; each finds its own entry.
        let dirs = Dirs::from_vars(
            "athanor-dock",
            vars(&[
                ("HOME", "/home/u"),
                ("XDG_RUNTIME_DIR", "/run/user/1"),
                (
                    "RUNTIME_DIRECTORY",
                    "/run/user/1/athanor:/run/user/1/athanor-dock",
                ),
            ]),
        )
        .expect("dirs");
        assert_eq!(
            dirs.failures,
            PathBuf::from("/run/user/1/athanor-dock/failures")
        );
        let unset = Dirs::from_vars(
            "athanor-dock",
            vars(&[("HOME", "/home/u"), ("XDG_RUNTIME_DIR", "/run/user/1")]),
        )
        .expect("dirs");
        assert_eq!(
            unset.unit_runtime,
            PathBuf::from("/run/user/1/athanor-dock")
        );
    }

    #[test]
    fn home_fills_in_and_relative_or_empty_values_count_as_unset() {
        let dirs = Dirs::from_vars(
            "athanor-bar",
            vars(&[
                ("HOME", "/home/u"),
                ("XDG_CONFIG_HOME", "relative"),
                ("XDG_CACHE_HOME", ""),
                ("XDG_RUNTIME_DIR", "/run/user/1"),
                ("RUNTIME_DIRECTORY", "also/relative"),
            ]),
        )
        .expect("dirs");
        assert_eq!(dirs.config, PathBuf::from("/home/u/.config"));
        assert_eq!(dirs.cache, PathBuf::from("/home/u/.cache"));
        assert_eq!(
            dirs.failures,
            PathBuf::from("/run/user/1/athanor-bar/failures")
        );
    }

    #[test]
    fn without_a_runtime_directory_or_a_home_there_are_none() {
        assert_eq!(
            Dirs::from_vars("athanor-bar", vars(&[("HOME", "/home/u")])),
            None
        );
        assert_eq!(
            Dirs::from_vars("athanor-bar", vars(&[("XDG_RUNTIME_DIR", "/run/user/1")])),
            None
        );
        assert_eq!(
            Dirs::from_vars(
                "athanor-bar",
                vars(&[("HOME", "home"), ("XDG_RUNTIME_DIR", "/run/user/1")])
            ),
            None
        );
    }
}
