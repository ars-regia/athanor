//! The overlay's Landlock (doc_osd.md OD12), applied at start while the process has one
//! thread, before the audio model's thread, the Tokio runtime and any GIO object. Step 1 grants
//! what volume and the microphone use, and nothing of the later steps (backlights, the COSMIC
//! files, i2c-dev):
//!
//! - read beneath `/usr` and `/etc`: the libraries, the icon themes and fonts of the image,
//!   fontconfig's configuration, the GSettings schemas, dconf's profile, libpulse's
//!   `client.conf` and the machine id;
//! - read the user's fonts, icon themes and fontconfig configuration, the dconf database
//!   GSettings reads for the appearance and the reduced-motion preference (T1), the schema
//!   directories GLib reads besides `/usr`, and libpulse's `~/.config/pulse` (its cookie);
//! - write beneath the cache, `$XDG_CACHE_HOME`, which the unit sets to its `CacheDirectory`
//!   (fontconfig's cache), and beneath `$XDG_RUNTIME_DIR/dconf`, whose shared-memory flag
//!   dconf's client opens for writing to learn of a change. Each is granted only when it passes
//!   `dirs::private_dir` (not a link, a directory of the user, mode 0700), by the descriptor
//!   that check opened, so a link swapped in for either path afterwards widens nothing.
//!
//! Nothing beneath `/proc` (maintainer decision, 2026-11-07): Landlock cannot narrow it to one
//! process, and it would expose the environment of every process of the user.
//!
//! Connecting to the Wayland socket, the session bus and the sound server's socket is not a
//! filesystem access Landlock mediates, so none of the three is granted.

use std::error::Error;
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use athanor_unit::dirs::{self, Dirs};
use athanor_unit::sandbox;

/// What the overlay reads and the directories it writes, held open.
#[derive(Debug)]
pub struct Grants {
    pub read: Vec<PathBuf>,
    pub write: Vec<OwnedFd>,
}

/// The overlay's grants: the cache and dconf's runtime directory, each when it passes
/// `dirs::private_dir`, which creates it if missing. One that fails is not granted, and says so.
pub fn overlay(dirs: &Dirs, var: impl Fn(&str) -> Option<OsString>) -> Grants {
    let mut write = Vec::new();
    let cache = match (dirs.cache.parent(), dirs.cache.file_name().and_then(|n| n.to_str())) {
        (Some(parent), Some(name)) => dirs::private_dir(parent, name),
        _ => Err(std::io::Error::other("the cache is the root directory")),
    };
    match cache {
        Ok(cache) => write.push(cache),
        Err(error) => tracing::warn!(%error, "the cache is not granted"),
    }
    match dirs::private_dir(&dirs.runtime, "dconf") {
        Ok(dconf) => write.push(dconf),
        Err(error) => tracing::warn!(%error, "dconf's runtime directory is not granted"),
    }
    Grants::new(write, var)
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

impl Grants {
    /// From the environment as `var` reads it and the write directories `write`.
    pub fn new(write: Vec<OwnedFd>, var: impl Fn(&str) -> Option<OsString>) -> Grants {
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
        if let Some(config) = &config {
            read.extend(["dconf", "fontconfig", "pulse"].map(|d| config.join(d)));
        }
        if let Some(home) = &home {
            read.extend([".fonts", ".icons"].map(|d| home.join(d)));
        }
        read.extend(list(&var, "GSETTINGS_SCHEMA_DIR"));
        for dir in data.iter().chain(&data_dirs) {
            read.extend(["fonts", "icons", "glib-2.0/schemas"].map(|d| dir.join(d)));
        }
        read.retain(|p| !p.starts_with("/usr") || p == Path::new("/usr"));
        read.dedup();
        Grants { read, write }
    }

    /// Restricts the calling thread and those it starts afterwards, and denies TCP. The
    /// caller has proved the process single-threaded (`sandbox::ensure_single_threaded`).
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
                "/home/u/.config/fontconfig",
                "/home/u/.config/pulse",
                "/home/u/.fonts",
                "/home/u/.icons",
                "/opt/schemas",
                "/home/u/.local/share/fonts",
                "/home/u/.local/share/icons",
                "/home/u/.local/share/glib-2.0/schemas",
                "/var/lib/flatpak/exports/share/fonts",
                "/var/lib/flatpak/exports/share/icons",
                "/var/lib/flatpak/exports/share/glib-2.0/schemas",
            ]
        );
    }

    #[test]
    fn a_cache_that_is_a_link_or_not_private_is_not_granted() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("athanor-osd-cache-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        for dir in ["cache", "elsewhere", "runtime"] {
            std::fs::create_dir_all(base.join(dir)).expect("mkdir");
        }
        std::os::unix::fs::symlink(base.join("elsewhere"), base.join("cache/athanor-osd"))
            .expect("link");
        std::fs::create_dir(base.join("cache/open")).expect("mkdir");
        std::fs::set_permissions(base.join("cache/open"), std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let granted = |cache: &str| {
            let dirs = Dirs {
                unit_runtime: base.join("runtime/athanor-osd"),
                failures: base.join("runtime/athanor-osd/failures"),
                config: base.join("config"),
                cache: base.join(cache),
                runtime: base.join("runtime"),
            };
            overlay(&dirs, env(&[])).write.len()
        };
        // dconf's directory alone, created private, then the cache too.
        let found = [granted("cache/athanor-osd"), granted("cache/open"), granted("cache/new")];
        std::fs::remove_dir_all(&base).ok();
        assert_eq!(found, [1, 1, 2], "a link, mode 0755, created private");
    }

    #[test]
    fn nothing_beneath_the_home_without_an_absolute_home() {
        let grants = Grants::new(Vec::new(), env(&[("HOME", "relative")]));
        assert!(grants.read.iter().all(|p| !p.starts_with("relative")));
        assert_eq!(grants.read[..2], [Path::new("/usr"), Path::new("/etc")]);
    }
}
