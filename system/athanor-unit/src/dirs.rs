//! The directories a user unit of the shell uses, from the environment its unit gives it. A relative or
//! empty value counts as unset (XDG Base Directory Specification).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use nix::fcntl::OFlag;

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
/// path after the check widens nothing.
pub fn private_dir(runtime: &Path, name: &str) -> io::Result<Checked> {
    let dir = runtime.join(name);
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let refuse = |why: String| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {why}", dir.display()),
        )
    };
    // std adds O_CLOEXEC.
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_DIRECTORY).bits())
        .open(&dir)
        .map_err(|e| refuse(format!("is not a directory, or is a link: {e}")))?;
    let owner = fs::metadata(runtime)?.uid();
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
/// open is refused, and one swapped in after it widens nothing.
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

/// `path` opened with O_PATH, O_NOFOLLOW and `kind`, and what fstat says of it; refused when it
/// is a link, or not a directory when `kind` holds O_DIRECTORY.
fn not_a_link(path: &Path, kind: OFlag) -> io::Result<(fs::File, fs::Metadata)> {
    // std adds O_CLOEXEC. With O_PATH and O_NOFOLLOW a link opens as itself: refused below.
    let opened = fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_PATH | OFlag::O_NOFOLLOW | kind).bits())
        .open(path)?;
    let found = opened.metadata()?;
    if found.file_type().is_symlink() {
        return Err(refused(path, "is a link"));
    }
    if kind.contains(OFlag::O_DIRECTORY) && !found.is_dir() {
        return Err(refused(path, "is not a directory"));
    }
    Ok((opened, found))
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
