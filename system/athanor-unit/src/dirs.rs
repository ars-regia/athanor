//! The directories a user unit of the shell uses, from the environment its unit gives it. A relative or
//! empty value counts as unset (XDG Base Directory Specification).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;

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

/// A node `owned_node` opened, whose owner it does not check: what a `WriteFile` grant names
/// (`sandbox::restrict_writes`), never a directory grant, so it cannot widen one:
///
/// ```compile_fail
/// fn grant(node: athanor_unit::dirs::CheckedNode) {
///     let _ = athanor_unit::sandbox::restrict_writes(&[node], &[]);
/// }
/// ```
#[derive(Debug)]
pub struct CheckedNode(OwnedFd);

impl AsFd for CheckedNode {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

/// `runtime/name`, created with mode 0700 if missing, opened once without following a link and
/// checked on that descriptor: a directory, owned by the owner of `runtime`, of mode 0700.
/// Another process of the user could otherwise point a confined daemon's writes elsewhere. The
/// descriptor is what a Landlock grant names (`sandbox::restrict`): a link swapped in for the
/// path after the check widens nothing. `runtime` is reached by `walk`, which refuses a link
/// in it or above it unless root alone writes the directory that holds the link; `name` is
/// created and opened relative to that descriptor, and is never a link.
pub fn private_dir(runtime: &Path, name: &str) -> io::Result<Checked> {
    let dir = runtime.join(name);
    let refuse = |why: String| refused(&dir, &why);
    if Path::new(name).file_name() != Some(OsStr::new(name)) {
        return Err(refuse("is not one name in its directory".to_owned()));
    }
    let parent = walk(runtime, &dir)?;
    match nix::sys::stat::mkdirat(&parent, name, Mode::S_IRWXU) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(e) => return Err(e.into()),
    }
    let opened = open_last(&parent, OsStr::new(name), OFlag::O_DIRECTORY)
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
/// open is refused, and one swapped in after it widens nothing. The components above the last
/// are reached by `walk`, which refuses a link in them unless root alone writes the directory
/// that holds it (`/home`, a link to `var/home`).
pub fn owned_dir(path: &Path) -> io::Result<Checked> {
    checked_dir(path, true)
}

/// `owned_dir` for a directory only the process's own user may own, root included, so that a
/// root-owned directory is never granted. The path is walked as `owned_dir` walks it: a link
/// in a directory that root alone does not write (a dotfiles tree) is refused, and a caller
/// whose path may hold one resolves it first (the layout chooser canonicalises its document's
/// directory). The owner is read from the same descriptor the grant names.
pub fn own_dir(path: &Path) -> io::Result<Checked> {
    checked_dir(path, false)
}

fn checked_dir(path: &Path, root_ok: bool) -> io::Result<Checked> {
    let (opened, found) = not_a_link(path, OFlag::O_DIRECTORY)?;
    let owner = found.uid();
    if owner != nix::unistd::geteuid().as_raw() && !(root_ok && owner == 0) {
        return Err(refused(path, "is owned by another user"));
    }
    Ok(Checked(opened.into()))
}

/// `path`, a node of the system's that a `WriteFile` grant names (`/dev/dri`, `/dev/null`):
/// opened as `owned_dir` opens a directory, of any kind other than a link. Its owner is not
/// checked: no user can put anything at those paths, and in a user namespace that does not map
/// root (the shell rig's container, the greeter's sandbox) their owner reads as the overflow id.
pub fn owned_node(path: &Path) -> io::Result<CheckedNode> {
    not_a_link(path, OFlag::empty()).map(|(opened, _)| CheckedNode(opened.into()))
}

/// Each of `paths` that exists, opened by `open` (`owned_dir`, `owned_node`). One that fails its
/// check is left out with a warning, so that a grant only narrows; a missing one is skipped, as
/// Landlock has nothing to hold on to.
pub fn existing<T>(paths: &[&Path], open: fn(&Path) -> io::Result<T>) -> Vec<T> {
    paths
        .iter()
        .filter_map(|path| match open(path) {
            Ok(fd) => Some(fd),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                let path = crate::text::line(&path.display().to_string(), usize::MAX);
                tracing::warn!(%error, %path, "not granted: it fails the check");
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
    // The path, a link's name and its target are the user's to choose: one line, so that a
    // newline in them cannot forge a record of the journal the refusal is logged to.
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        crate::text::line(&format!("{} {why}", path.display()), usize::MAX),
    )
}

/// `path` opened with O_PATH, O_NOFOLLOW and `kind`, its last component relative to the
/// directory `walk` reached, and what fstat says of it; refused when it is a link, or not a
/// directory when `kind` holds O_DIRECTORY.
fn not_a_link(path: &Path, kind: OFlag) -> io::Result<(fs::File, fs::Metadata)> {
    if path.as_os_str().is_empty() {
        return Err(io::ErrorKind::NotFound.into());
    }
    let opened = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => open_last(&walk(parent, path)?, name, kind)?,
        // `/`, or a path that ends in `..`: no last name that could be a link.
        _ => walk(path, path)?,
    };
    let found = opened.metadata()?;
    if found.file_type().is_symlink() {
        return Err(refused(path, "is a link"));
    }
    if kind.contains(OFlag::O_DIRECTORY) && !found.is_dir() {
        return Err(refused(path, "is not a directory"));
    }
    Ok((opened, found))
}

/// The links one walk follows before it fails with ELOOP, as the kernel's own limit.
const MAX_LINKS: usize = 40;

/// The directory `path` names, reached one component at a time: each opened with O_PATH and
/// O_NOFOLLOW relative to the descriptor of the one before, never by path. A link is followed
/// only when the directory that holds it is owned by root and not writable by group or others
/// (`/home`, a link to `var/home`), and its target is walked by the same rule; any other link
/// is refused, as another process of the user could put it there to move a grant to another
/// directory the user writes. `shown` is the path the refusal names.
fn walk(path: &Path, shown: &Path) -> io::Result<fs::File> {
    let mut dir = start(path.has_root())?;
    // The path of `dir` as reached, for the refusal to name the link: every link on the way is
    // resolved here, so `..` takes off the last name.
    let mut reached = PathBuf::from(if path.has_root() { "/" } else { "" });
    let mut rest = components(path);
    let mut links = 0;
    while let Some(name) = rest.pop() {
        let next = open_last(&dir, &name, OFlag::empty())?;
        if !next.metadata()?.file_type().is_symlink() {
            dir = next;
            if name == ".." {
                reached.pop();
            } else {
                reached.push(&name);
            }
            continue;
        }
        let target = PathBuf::from(nix::fcntl::readlinkat(&dir, name.as_os_str())?);
        let holder = dir.metadata()?;
        if holder.uid() != 0 || holder.mode() & 0o022 != 0 {
            return Err(refused(
                shown,
                &format!(
                    "passes through {} -> {}, a link in a directory that root alone does not \
                     write; replace the link with a real directory",
                    reached.join(&name).display(),
                    target.display()
                ),
            ));
        }
        links += 1;
        if links > MAX_LINKS {
            return Err(Errno::ELOOP.into());
        }
        if target.has_root() {
            dir = start(true)?;
            reached = PathBuf::from("/");
        }
        rest.extend(components(&target));
    }
    Ok(dir)
}

/// `/`, or the working directory, where a walk starts.
fn start(absolute: bool) -> io::Result<fs::File> {
    let from = if absolute { "/" } else { "." };
    let flags = OFlag::O_PATH | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC;
    Ok(nix::fcntl::openat(nix::fcntl::AT_FDCWD, from, flags, Mode::empty())?.into())
}

/// `name` in `dir`, opened with O_PATH, O_NOFOLLOW, O_CLOEXEC and `kind`: a link opens as
/// itself.
fn open_last(dir: &fs::File, name: &OsStr, kind: OFlag) -> io::Result<fs::File> {
    let flags = OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | kind;
    Ok(nix::fcntl::openat(dir, name, flags, Mode::empty())?.into())
}

/// The names of `path`, last first, for a walk to pop: the root and `.` are left out, since a
/// walk starts at `/` or the working directory and `.` moves nowhere; `..` is opened as a name,
/// so the kernel resolves it from the directory reached, as a lookup by path would.
fn components(path: &Path) -> Vec<OsString> {
    path.components()
        .rev()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_owned()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
        })
        .collect()
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

    /// Removes a link, or a directory and what it holds, when dropped: after a failed
    /// assertion too, so that a red run leaves nothing behind in /tmp.
    struct Removed(PathBuf);

    impl Drop for Removed {
        fn drop(&mut self) {
            let removed = match fs::symlink_metadata(&self.0) {
                Ok(found) if found.is_dir() => fs::remove_dir_all(&self.0),
                _ => fs::remove_file(&self.0),
            };
            if let Err(error) = removed {
                eprintln!("cleanup of {}: {error}", self.0.display());
            }
        }
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
    fn an_own_dir_refuses_a_directory_of_root() {
        // "/" must be root's and not ours: not so when the suite runs as root, nor in a user
        // namespace that does not map root, where "/" reads as the overflow id.
        if nix::unistd::geteuid().is_root() || fs::metadata("/").map(|m| m.uid()).ok() != Some(0) {
            return;
        }
        owned_dir(Path::new("/")).expect("root's directory passes owned_dir");
        let error = own_dir(Path::new("/")).expect_err("root's directory");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().starts_with("/ is owned by another user"));
        let runtime = runtime("own");
        own_dir(&runtime).expect("a directory of the user's");
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
    fn a_link_in_an_intermediate_component_is_refused() {
        let runtime = runtime("intermediate");
        let _removed = Removed(runtime.clone());
        let (real, link) = (runtime.join("real"), runtime.join("link"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&real)
            .expect("real");
        private_dir(&real, "d").expect("the directory itself");
        // Another process of the user puts a link in a directory it writes, above the grant.
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        if nix::unistd::geteuid().is_root() {
            // Run as root (the shell rig), the holder is root's 0700, whose links are followed:
            // writable by its group, it is no longer root's alone.
            fs::set_permissions(&runtime, fs::Permissions::from_mode(0o770)).expect("chmod");
        }
        for (what, error) in [
            ("owned_dir", owned_dir(&link.join("d")).map(drop)),
            ("own_dir", own_dir(&link.join("d")).map(drop)),
            ("owned_node", owned_node(&link.join("d")).map(drop)),
            ("private_dir", private_dir(&link, "d").map(drop)),
            ("private_dir, new", private_dir(&link, "new").map(drop)),
        ] {
            let error = error.expect_err(what);
            assert_eq!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{what}: {error}"
            );
        }
        assert!(
            !real.join("new").exists(),
            "nothing is made through the link"
        );
        let error = owned_dir(&link.join("d")).expect_err("owned_dir");
        // The refusal names the link where the walk reached it, past every link it followed
        // (the temporary directory may lie under /home, a link to var/home).
        let reached = fs::canonicalize(&runtime).expect("canonical").join("link");
        assert_eq!(
            error.to_string(),
            format!(
                "{} passes through {} -> {}, a link in a directory that root alone does not \
                 write; replace the link with a real directory",
                link.join("d").display(),
                reached.display(),
                real.display()
            )
        );
    }

    #[test]
    fn a_private_dir_refusal_is_one_line() {
        let runtime = runtime("private-newline");
        let _removed = Removed(runtime.clone());
        // A runtime path holding a newline and a journal priority, its directory of mode 0755.
        let inner = runtime.join("a\n<2>b");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&inner)
            .expect("inner");
        fs::create_dir(inner.join("d")).expect("d");
        fs::set_permissions(inner.join("d"), fs::Permissions::from_mode(0o755)).expect("chmod");
        let message = private_dir(&inner, "d").expect_err("mode 0755").to_string();
        assert!(!message.contains('\n'), "{message:?}");
        assert!(
            message.contains("a<2>b/d does not have mode 0700"),
            "{message:?}"
        );
    }

    #[test]
    fn a_refusal_naming_a_link_is_one_line() {
        let runtime = runtime("newline");
        let _removed = Removed(runtime.clone());
        // A link whose name and target hold a newline and a journal priority.
        let link = runtime.join("li\n<2>nk");
        std::os::unix::fs::symlink("x\n<2>forged", &link).expect("symlink");
        if nix::unistd::geteuid().is_root() {
            fs::set_permissions(&runtime, fs::Permissions::from_mode(0o770)).expect("chmod");
        }
        let message = owned_dir(&link.join("d"))
            .expect_err("the link")
            .to_string();
        assert!(!message.contains('\n'), "{message:?}");
        assert!(message.contains("li<2>nk -> x<2>forged"), "{message:?}");
    }

    #[test]
    fn a_link_in_a_directory_others_may_write_is_refused() {
        // /tmp is root's but writable by everyone, so a link in it is not root's alone. Not
        // checkable where /tmp is not root's 1777 (a user namespace that does not map root).
        let tmp = fs::metadata("/tmp").expect("/tmp");
        if tmp.uid() != 0 || tmp.mode() & 0o7777 != 0o1777 {
            eprintln!("skipped: /tmp is not root's with mode 1777 here");
            return;
        }
        let runtime = runtime("writable-holder");
        let link = PathBuf::from(format!("/tmp/athanor-unit-link-{}", std::process::id()));
        fs::remove_file(&link).ok();
        let (_runtime_removed, _link_removed) = (Removed(runtime.clone()), Removed(link.clone()));
        std::os::unix::fs::symlink(&runtime, &link).expect("symlink");
        fs::create_dir(runtime.join("d")).expect("mkdir");
        let error = owned_dir(&link.join("d")).expect_err("a link in /tmp");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
        let reached = fs::canonicalize("/tmp")
            .expect("canonical")
            .join(format!("athanor-unit-link-{}", std::process::id()));
        assert_eq!(
            error.to_string(),
            format!(
                "{} passes through {} -> {}, a link in a directory that root alone does not \
                 write; replace the link with a real directory",
                link.join("d").display(),
                reached.display(),
                runtime.display()
            )
        );
    }

    #[test]
    fn a_link_in_a_directory_only_root_writes_is_followed() {
        // Not checkable where "/" is not root's (a user namespace that does not map root, where
        // it reads as the overflow id): every link is refused there.
        if fs::metadata("/").map(|m| m.uid()).ok() != Some(0) {
            eprintln!("skipped: / is not root's here");
            return;
        }
        // /proc is root's, mode 0555, and /proc/self a link in it: on every Linux host.
        owned_dir(Path::new("/proc/self/fd")).expect("through /proc/self");
        // On Athanor /home is a link to var/home, and $HOME lies beneath it.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        match home.filter(|home| home.starts_with("/home/")) {
            Some(home)
                if fs::symlink_metadata("/home").is_ok_and(|m| m.file_type().is_symlink())
                    && fs::metadata(&home)
                        .is_ok_and(|m| m.uid() == nix::unistd::geteuid().as_raw()) =>
            {
                owned_dir(&home).expect("$HOME, through /home");
                own_dir(&home).expect("$HOME, through /home");
            }
            _ => eprintln!(
                "skipped: /home is not a link here, or $HOME is not the user's beneath it"
            ),
        }
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
