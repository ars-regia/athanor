//! The directories a user unit of the shell uses, from the environment its unit gives it. A relative or
//! empty value counts as unset (XDG Base Directory Specification).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// `runtime/name`, created with mode 0700; one that exists already must be a directory, not a
/// link, owned by the owner of `runtime` and of mode 0700, or it is refused: another process of
/// the user could otherwise point a confined daemon's writes elsewhere.
pub fn private_dir(runtime: &Path, name: &str) -> io::Result<PathBuf> {
    let dir = runtime.join(name);
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => return Ok(dir),
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    let owner = fs::metadata(runtime)?.uid();
    let found = fs::symlink_metadata(&dir)?;
    let refuse = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {why}", dir.display()),
        ))
    };
    if !found.file_type().is_dir() {
        return refuse("is not a directory");
    }
    if found.uid() != owner {
        return refuse("is not owned by the user");
    }
    if found.permissions().mode() & 0o7777 != 0o700 {
        return refuse("does not have mode 0700");
    }
    Ok(dir)
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

    #[test]
    fn a_private_dir_is_created_or_checked() {
        let runtime =
            std::env::temp_dir().join(format!("athanor-unit-private-{}", std::process::id()));
        fs::remove_dir_all(&runtime).ok();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&runtime)
            .expect("runtime");
        let made = private_dir(&runtime, "d").expect("created");
        assert_eq!(made, runtime.join("d"));
        assert_eq!(
            fs::metadata(&made).expect("made").permissions().mode() & 0o7777,
            0o700
        );
        assert_eq!(
            private_dir(&runtime, "d").expect("an existing one that passes"),
            made
        );

        fs::set_permissions(&made, fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(private_dir(&runtime, "d").is_err(), "mode 0755");
        fs::remove_dir(&made).expect("rmdir");

        let elsewhere = runtime.join("elsewhere");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&elsewhere)
            .expect("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, &made).expect("symlink");
        assert!(private_dir(&runtime, "d").is_err(), "a link to a directory");
        fs::remove_file(&made).expect("rm");

        fs::write(&made, b"").expect("file");
        assert!(private_dir(&runtime, "d").is_err(), "a file");
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
