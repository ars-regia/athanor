//! The daemon's own Landlock (doc_session_daemons.md, SD17), applied at start while the process
//! has one thread, before any GIO object and before the Tokio thread: read-only beneath the
//! trees below, read-write beneath `$XDG_RUNTIME_DIR/athanor-idle` and `$XDG_RUNTIME_DIR/dconf`
//! alone, and no TCP. dconf's client opens its shared-memory flag, `$XDG_RUNTIME_DIR/dconf/user`,
//! for writing; refused, it logs a critical line and reads the database again at every access.
//!
//! - `/usr` and `/etc`: the libraries, GIO's modules, the GSettings schemas of the image,
//!   dconf's profile and system databases, the locale.
//! - The user's dconf database, `$XDG_CONFIG_HOME/dconf`, which GSettings reads directly; its
//!   writes go to dconf's service on the bus.
//! - The schema directories GLib reads besides `/usr`: `$GSETTINGS_SCHEMA_DIR`,
//!   `$XDG_DATA_HOME/glib-2.0/schemas` and those of `$XDG_DATA_DIRS`.
//! - The store of cosmic-settings' Power page it follows (SD3), or, while it does not exist,
//!   its nearest existing parent up to `$XDG_CONFIG_HOME/cosmic`, so a store created later
//!   is read.
//!
//! Connecting to the Wayland socket and to the buses is not a filesystem access Landlock
//! mediates.

use std::error::Error;
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use athanor_unit::{dirs, sandbox};

/// What the daemon reads and the directories it writes, held open as `dirs::private_dir`
/// checked them.
#[derive(Debug)]
pub struct Grants {
    pub read: Vec<PathBuf>,
    pub write: Vec<OwnedFd>,
}

/// `name` as an absolute path, or `None`.
fn absolute(var: &impl Fn(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    var(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The absolute entries of the colon-separated list `name`.
fn list(var: &impl Fn(&str) -> Option<OsString>, name: &str) -> Vec<PathBuf> {
    var(name)
        .map(|value| {
            std::env::split_paths(&value)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

/// The daemon's directory, `$XDG_RUNTIME_DIR/athanor-idle`, where it keeps `inhibitors.json`
/// (`dirs::private_dir`, an error when it is not the daemon's), and its grants: that directory
/// and, when it passes the same check, dconf's.
pub fn daemon(
    runtime: &Path,
    power_page: Option<&Path>,
    var: impl Fn(&str) -> Option<OsString>,
) -> std::io::Result<(PathBuf, Grants)> {
    let mut write = vec![dirs::private_dir(runtime, "athanor-idle")?];
    match dirs::private_dir(runtime, "dconf") {
        Ok(dconf) => write.push(dconf),
        Err(error) => tracing::warn!(%error, "dconf's runtime directory is not granted"),
    }
    Ok((
        runtime.join("athanor-idle"),
        Grants::new(write, power_page, var),
    ))
}

impl Grants {
    /// From the environment as `var` reads it, the Power page's store `power_page` and the
    /// write directories `write`.
    pub fn new(
        write: Vec<OwnedFd>,
        power_page: Option<&Path>,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Grants {
        let home = absolute(&var, "HOME");
        let config =
            absolute(&var, "XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")));
        let data = absolute(&var, "XDG_DATA_HOME")
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        let mut data_dirs = list(&var, "XDG_DATA_DIRS");
        if data_dirs.is_empty() {
            data_dirs = vec!["/usr/local/share".into(), "/usr/share".into()];
        }

        let mut read: Vec<PathBuf> = vec!["/usr".into(), "/etc".into()];
        read.extend(config.iter().map(|c| c.join("dconf")));
        read.extend(list(&var, "GSETTINGS_SCHEMA_DIR"));
        read.extend(
            data.iter()
                .chain(&data_dirs)
                .map(|d| d.join("glib-2.0/schemas")),
        );
        // `<config>/cosmic/com.system76.CosmicIdle/v1`, or the nearest of its two parents.
        if let Some(store) = power_page {
            read.extend(
                store
                    .ancestors()
                    .take(3)
                    .find(|p| p.is_dir())
                    .map(Path::to_path_buf),
            );
        }
        read.retain(|p| !p.starts_with("/usr") || p == Path::new("/usr"));
        read.dedup();
        Grants { read, write }
    }

    /// Restricts the calling thread and those it starts afterwards. The caller has proved the
    /// process single-threaded (`sandbox::ensure_single_threaded`).
    pub fn apply(&self) -> Result<(), Box<dyn Error>> {
        let read: Vec<&Path> = self.read.iter().map(PathBuf::as_path).collect();
        sandbox::restrict(&read, &self.write)?;
        sandbox::deny_tcp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn the_grants_follow_the_environment() {
        let grants = Grants::new(
            Vec::new(),
            None,
            env(&[
                ("HOME", "/home/u"),
                ("GSETTINGS_SCHEMA_DIR", "/opt/schemas:relative"),
                ("XDG_DATA_DIRS", "/usr/share:/var/lib/flatpak/exports/share"),
            ]),
        );
        let read: Vec<&str> = grants.read.iter().filter_map(|p| p.to_str()).collect();
        assert_eq!(
            read,
            [
                "/usr",
                "/etc",
                "/home/u/.config/dconf",
                "/opt/schemas",
                "/home/u/.local/share/glib-2.0/schemas",
                "/var/lib/flatpak/exports/share/glib-2.0/schemas",
            ]
        );
    }

    #[test]
    fn the_power_page_or_its_nearest_existing_parent_is_read() {
        let base = std::env::temp_dir().join(format!("athanor-idle-grants-{}", std::process::id()));
        let cosmic = base.join("cosmic");
        let store = cosmic.join("component/v1");
        std::fs::create_dir_all(&cosmic).expect("mkdir");
        let read = |store: &Path| Grants::new(Vec::new(), Some(store), env(&[])).read;
        assert!(read(&store).contains(&cosmic), "the store is not there yet");
        std::fs::create_dir_all(&store).expect("mkdir");
        assert!(read(&store).contains(&store));
        assert!(!read(&store).contains(&cosmic));
        std::fs::remove_dir_all(&base).expect("cleanup");
    }
}
