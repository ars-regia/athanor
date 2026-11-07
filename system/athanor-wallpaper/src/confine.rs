//! The daemon's own Landlock (doc_session_daemons.md SD17), applied at start while the process
//! has one thread, before any GIO object: read-only beneath the trees below, read-write beneath
//! `$XDG_RUNTIME_DIR/dconf` alone, and no TCP. dconf's client opens its shared-memory flag,
//! `$XDG_RUNTIME_DIR/dconf/user`, for writing.
//!
//! - `/usr` and `/etc`: the libraries, GIO's modules, the GSettings schemas of the image,
//!   dconf's profile and system databases, fonts, the locale, and the hearth.
//! - `$HOME`, and `$XDG_CONFIG_HOME` and `$XDG_DATA_HOME` where they lie outside it: the user's
//!   pictures, dconf's user database, cosmic-bg's store for the migration (SD16).
//! - The schema directories GLib reads besides `/usr`: `$GSETTINGS_SCHEMA_DIR` and those of
//!   `$XDG_DATA_DIRS`, with their pictures.
//!
//! A picture named elsewhere does not open and shows the hearth. Connecting to the Wayland
//! socket, to the buses and to the decoder's socket is not a filesystem access Landlock
//! mediates.

use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use athanor_unit::sandbox;

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

/// The trees the daemon reads, from the environment as `var` reads it.
pub fn reads(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut read: Vec<PathBuf> = vec!["/usr".into(), "/etc".into()];
    read.extend(absolute(&var, "HOME"));
    read.extend(absolute(&var, "XDG_CONFIG_HOME"));
    read.extend(absolute(&var, "XDG_DATA_HOME"));
    read.extend(list(&var, "GSETTINGS_SCHEMA_DIR"));
    read.extend(list(&var, "XDG_DATA_DIRS"));
    // A tree beneath another one adds nothing.
    let all = read.clone();
    read.retain(|p| !all.iter().any(|q| q != p && p.starts_with(q)));
    let mut seen = Vec::new();
    read.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    read
}

/// dconf's runtime directory, `runtime/dconf`, as the one directory the daemon may write: only
/// once it is checked to be a directory, not a link, of the owner of `runtime`, of mode 0700,
/// or created so. No such check is in a crate the daemon depends on, so the directory is
/// refused, and the daemon does not start.
pub fn dconf_dir(runtime: &Path) -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} is not checked to be the user's own",
            runtime.join("dconf").display()
        ),
    ))
}

/// Restricts the calling thread and those it starts afterwards to reading `read` and writing
/// `write`, with no TCP. The caller has proved the process single-threaded.
pub fn apply(read: &[PathBuf], write: &Path) -> Result<(), Box<dyn Error>> {
    let read: Vec<&Path> = read.iter().map(PathBuf::as_path).collect();
    sandbox::restrict(&read, write)?;
    sandbox::deny_tcp()
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
    fn the_reads_follow_the_environment() {
        let read = reads(env(&[
            ("HOME", "/var/home/u"),
            ("XDG_CONFIG_HOME", "/var/home/u/.config"),
            ("XDG_DATA_HOME", "/srv/u/data"),
            ("GSETTINGS_SCHEMA_DIR", "/opt/schemas:relative"),
            (
                "XDG_DATA_DIRS",
                "/usr/share:/var/lib/flatpak/exports/share:/opt/schemas",
            ),
        ]));
        let read: Vec<&str> = read.iter().filter_map(|p| p.to_str()).collect();
        assert_eq!(
            read,
            [
                "/usr",
                "/etc",
                "/var/home/u",
                "/srv/u/data",
                "/opt/schemas",
                "/var/lib/flatpak/exports/share"
            ]
        );
    }

    #[test]
    fn an_unset_or_relative_environment_reads_the_system_only() {
        let read = reads(env(&[("HOME", "relative")]));
        assert_eq!(read, [PathBuf::from("/usr"), PathBuf::from("/etc")]);
    }

    #[test]
    fn dconf_is_not_granted_unchecked() {
        assert!(dconf_dir(Path::new("/run/user/1000")).is_err());
    }
}
