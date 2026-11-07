//! The daemon's own Landlock (`doc_session_daemons.md` SD17), applied at start while the
//! process has one thread, before any GIO object and before the Tokio thread, as athanor-idle
//! applies its own: read-only beneath the trees below, read-write beneath `$XDG_RUNTIME_DIR/dconf`
//! alone, and no TCP. dconf's client opens its shared-memory flag, `$XDG_RUNTIME_DIR/dconf/user`,
//! for writing; refused, it logs a critical line and reads the database again at every access.
//! dconf's directory is granted only when it is private to the user (maintainer decision,
//! 2026-11-07). The crash reports SD17 grants (`doc_session.md` SN8) are not written yet, so
//! their directory is not granted until they are.
//!
//! - `/usr` and `/etc`: the libraries, GIO's modules, the GSettings schemas of the image,
//!   dconf's profile and system databases, the locale.
//! - The user's dconf database, `$XDG_CONFIG_HOME/dconf`, which GSettings reads directly; its
//!   writes go to dconf's service on the bus.
//!
//! Connecting to the buses is not a filesystem access Landlock mediates.

use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use athanor_unit::dirs::private_dir;
use athanor_unit::sandbox;

/// What the daemon reads and the directories it writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grants {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
}

/// The grants from the environment as `var` reads it. `$XDG_RUNTIME_DIR/dconf` is granted when
/// `private_dir` accepts it, and a warning says why when it does not. `schemas` is one more
/// schema directory to read, which only a test names: the environment never widens the grants.
pub fn daemon(
    var: impl Fn(&str) -> Option<OsString>,
    schemas: Option<&Path>,
) -> Result<Grants, Box<dyn Error>> {
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let runtime = absolute("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not an absolute path")?;
    let config = absolute("XDG_CONFIG_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".config")))
        .ok_or("neither XDG_CONFIG_HOME nor HOME is an absolute path")?;

    let mut write = Vec::new();
    match private_dir(&runtime, "dconf") {
        Ok(dconf) => write.push(dconf),
        Err(error) => tracing::warn!(%error, "dconf's runtime directory is not granted"),
    }
    let mut read: Vec<PathBuf> = vec!["/usr".into(), "/etc".into(), config.join("dconf")];
    read.extend(schemas.map(Path::to_path_buf));
    Ok(Grants { read, write })
}

impl Grants {
    /// Restricts the calling thread and those it starts afterwards. The caller has proved the
    /// process single-threaded (`sandbox::ensure_single_threaded`).
    pub fn apply(&self) -> Result<(), Box<dyn Error>> {
        let read: Vec<&Path> = self.read.iter().map(PathBuf::as_path).collect();
        let write: Vec<&Path> = self.write.iter().map(PathBuf::as_path).collect();
        sandbox::restrict(&read, &write)?;
        sandbox::deny_tcp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn dconf_is_granted_only_while_it_is_private_and_the_environment_never_widens() {
        let base =
            std::env::temp_dir().join(format!("athanor-sessiond-grants-{}", std::process::id()));
        let runtime = base.join("runtime");
        std::fs::create_dir_all(runtime.join("dconf")).expect("mkdir");
        let var = |name: &str| {
            let value = match name {
                "HOME" => base.join("home"),
                "XDG_RUNTIME_DIR" => runtime.clone(),
                // A unit's environment can be changed by the user: it grants nothing.
                "GSETTINGS_SCHEMA_DIR" | "XDG_DATA_DIRS" => PathBuf::from("/"),
                _ => return None,
            };
            Some(value.into_os_string())
        };
        let mode = |mode| {
            std::fs::set_permissions(runtime.join("dconf"), std::fs::Permissions::from_mode(mode))
                .expect("chmod")
        };

        mode(0o755);
        let grants = daemon(var, None).expect("grants");
        assert!(grants.write.is_empty(), "{grants:?}");
        assert_eq!(
            grants.read,
            [
                PathBuf::from("/usr"),
                PathBuf::from("/etc"),
                base.join("home/.config/dconf"),
            ]
        );
        assert!(
            !base.join("home/.local/state").exists(),
            "nothing is created"
        );

        mode(0o700);
        let grants = daemon(var, Some(Path::new("/opt/schemas"))).expect("grants");
        assert_eq!(grants.write, [runtime.join("dconf")]);
        assert_eq!(grants.read.last(), Some(&PathBuf::from("/opt/schemas")));
        std::fs::remove_dir_all(&base).expect("cleanup");

        let unset = daemon(|name| (name == "HOME").then(|| "/home/u".into()), None);
        assert_eq!(
            unset.expect_err("no runtime directory").to_string(),
            "XDG_RUNTIME_DIR is not an absolute path"
        );
    }
}
