//! The daemon's own Landlock (`doc_session_daemons.md` SD17), applied at start while the
//! process has one thread, before any GIO object and before the Tokio thread, as athanor-idle
//! applies its own: read-only beneath the trees below, read-write beneath its crash reports
//! and `$XDG_RUNTIME_DIR/dconf` alone, and no TCP. dconf's client opens its shared-memory flag,
//! `$XDG_RUNTIME_DIR/dconf/user`, for writing; refused, it logs a critical line and reads the
//! database again at every access. dconf's directory is granted only when it is private to the
//! user (maintainer decision, 2026-11-07).
//!
//! - `/usr` and `/etc`: the libraries, GIO's modules, the GSettings schemas of the image,
//!   dconf's profile and system databases, the locale.
//! - The user's dconf database, `$XDG_CONFIG_HOME/dconf`, which GSettings reads directly; its
//!   writes go to dconf's service on the bus.
//! - `$GSETTINGS_SCHEMA_DIR`, when it is set.
//!
//! Connecting to the buses is not a filesystem access Landlock mediates.

use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use athanor_unit::dirs::{private_dir, Dirs};
use athanor_unit::sandbox;

const UNIT: &str = "athanor-sessiond";

/// What the daemon reads and the directories it writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grants {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
}

/// The grants from the environment as `var` reads it. Creates
/// `$XDG_STATE_HOME/athanor/crash-reports` (SD17, amended 2026-10-06, #156) if missing; grants
/// `$XDG_RUNTIME_DIR/dconf` when `private_dir` accepts it, and says why when it does not.
pub fn daemon(var: impl Fn(&str) -> Option<OsString>) -> Result<Grants, Box<dyn Error>> {
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let dirs = Dirs::from_vars(UNIT, &var)
        .ok_or("XDG_RUNTIME_DIR, or both XDG_CONFIG_HOME and HOME, is not an absolute path")?;
    let state = absolute("XDG_STATE_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/state")))
        .ok_or("neither XDG_STATE_HOME nor HOME is an absolute path")?;
    let crash_reports = state.join("athanor/crash-reports");
    std::fs::create_dir_all(&crash_reports)?;

    let mut write = vec![crash_reports];
    match private_dir(&dirs.runtime, "dconf") {
        Ok(dconf) => write.push(dconf),
        Err(error) => tracing::warn!(%error, "dconf's runtime directory is not granted"),
    }
    let mut read: Vec<PathBuf> = vec!["/usr".into(), "/etc".into(), dirs.config.join("dconf")];
    if let Some(schemas) = var("GSETTINGS_SCHEMA_DIR") {
        read.extend(std::env::split_paths(&schemas).filter(|p| p.is_absolute()));
    }
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
    fn dconf_is_granted_only_while_it_is_private() {
        let base =
            std::env::temp_dir().join(format!("athanor-sessiond-grants-{}", std::process::id()));
        let runtime = base.join("runtime");
        std::fs::create_dir_all(runtime.join("dconf")).expect("mkdir");
        let var = |name: &str| {
            let value = match name {
                "HOME" => base.join("home"),
                "XDG_RUNTIME_DIR" => runtime.clone(),
                "GSETTINGS_SCHEMA_DIR" => PathBuf::from("/opt/schemas:relative"),
                _ => return None,
            };
            Some(value.into_os_string())
        };
        let mode = |mode| {
            std::fs::set_permissions(runtime.join("dconf"), std::fs::Permissions::from_mode(mode))
                .expect("chmod")
        };

        mode(0o755);
        let grants = daemon(var).expect("grants");
        assert_eq!(
            grants.write,
            [base.join("home/.local/state/athanor/crash-reports")]
        );
        assert!(grants.write[0].is_dir(), "created");
        assert_eq!(
            grants.read,
            [
                PathBuf::from("/usr"),
                PathBuf::from("/etc"),
                base.join("home/.config/dconf"),
                PathBuf::from("/opt/schemas"),
            ]
        );

        mode(0o700);
        let grants = daemon(var).expect("grants");
        assert_eq!(grants.write.get(1), Some(&runtime.join("dconf")));
        std::fs::remove_dir_all(&base).expect("cleanup");
    }
}
