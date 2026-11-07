//! The daemon's own Landlock (doc_session_daemons.md SD17), applied at start while the process
//! has one thread, before any GIO object: read-only beneath the trees below, read-write beneath
//! `$XDG_RUNTIME_DIR/dconf` alone, and no TCP. dconf's client opens its shared-memory flag,
//! `$XDG_RUNTIME_DIR/dconf/user`, for writing; the directory is granted only when it passes
//! `athanor_unit::dirs::private_dir`, as athanor-idle's is. Refused, nothing is written, and
//! dconf's client logs a critical line and reads the database again at every access.
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

use athanor_unit::{dirs, sandbox};

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

/// dconf's runtime directory, `runtime/dconf`, created with mode 0700 or checked to be a
/// directory, not a link, of the owner of `runtime` and of mode 0700.
pub fn dconf_dir(runtime: &Path) -> io::Result<PathBuf> {
    dirs::private_dir(runtime, "dconf")
}

/// The directories the daemon may write: dconf's, when [`dconf_dir`] admits it, else none.
pub fn writes(runtime: &Path) -> Vec<PathBuf> {
    match dconf_dir(runtime) {
        Ok(dconf) => vec![dconf],
        Err(error) => {
            tracing::warn!(%error, "dconf's runtime directory is not granted");
            Vec::new()
        }
    }
}

/// Restricts the calling thread and those it starts afterwards to reading `read` and writing
/// beneath `write`, with no TCP. The caller has proved the process single-threaded.
pub fn apply(read: &[PathBuf], write: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let read: Vec<&Path> = read.iter().map(PathBuf::as_path).collect();
    let write: Vec<&Path> = write.iter().map(PathBuf::as_path).collect();
    sandbox::restrict(&read, &write)?;
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

    /// The variable that makes the test below its own confined child.
    const CHILD: &str = "ATHANOR_WALLPAPER_CONFINED_CHILD";

    fn mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }

    #[test]
    fn a_dconf_directory_that_fails_the_check_is_not_granted() {
        let runtime =
            std::env::temp_dir().join(format!("athanor-wallpaper-dconf-{}", std::process::id()));
        std::fs::remove_dir_all(&runtime).ok();
        std::fs::create_dir_all(runtime.join("dconf")).expect("mkdir");
        mode(&runtime.join("dconf"), 0o755);
        assert!(writes(&runtime).is_empty());
        mode(&runtime.join("dconf"), 0o700);
        assert_eq!(writes(&runtime), [runtime.join("dconf")]);
        std::fs::remove_dir_all(&runtime).expect("cleanup");
    }

    /// Landlock binds the thread that applies it, so the test runs itself again as a child in a
    /// session of its own, which confines itself as `main` does.
    #[test]
    fn the_confined_daemon_writes_only_dconf_s_directory() {
        let base =
            std::env::temp_dir().join(format!("athanor-wallpaper-confined-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        for dir in ["home/.config", "home/.local/share", "runtime"] {
            std::fs::create_dir_all(base.join(dir)).expect("mkdir");
        }
        mode(&base.join("runtime"), 0o700);
        let home = base.join("home");
        let out = std::process::Command::new(std::env::current_exe().expect("exe"))
            .args([
                "confine::tests::child",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, &base)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", base.join("runtime"))
            .env_remove("GSETTINGS_BACKEND")
            .env_remove("GSETTINGS_SCHEMA_DIR")
            .env_remove("DCONF_PROFILE")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .output()
            .expect("child");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "child: {}{stderr}",
            String::from_utf8_lossy(&out.stdout)
        );
        // Refused its directory, dconf's client says so.
        assert!(!stderr.contains("dconf will not work properly"), "{stderr}");
        assert!(base.join("runtime/dconf/user").is_file(), "{stderr}");
        assert!(!base.join("outside").exists());
        assert!(!base.join("runtime/stray").exists());
        assert!(!home.join("stray").exists());
        std::fs::remove_dir_all(&base).expect("cleanup");
    }

    /// The confined half; it does nothing when the test above did not start it.
    #[test]
    fn child() {
        let Some(base) = std::env::var_os(CHILD).map(PathBuf::from) else {
            return;
        };
        let runtime = base.join("runtime");
        let tcp = std::net::TcpListener::bind("127.0.0.1:0").expect("tcp");
        let write = writes(&runtime);
        assert_eq!(write, [runtime.join("dconf")]);
        apply(&reads(|name| std::env::var_os(name)), &write).expect("confined");

        let refused = |path: &Path| {
            let error = std::fs::write(path, b"x").expect_err("a write outside the grants");
            assert_eq!(
                error.kind(),
                io::ErrorKind::PermissionDenied,
                "{}",
                path.display()
            );
        };
        refused(&base.join("outside"));
        refused(&runtime.join("stray"));
        refused(&base.join("home/stray"));
        let error = std::net::TcpStream::connect(tcp.local_addr().expect("addr")).expect_err("TCP");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

        // dconf's client opens its flag beneath the grant on the first read of a key.
        use gio::prelude::*;
        let backend = gio::SettingsBackend::default();
        assert_eq!(backend.type_().name(), "DConfSettingsBackend");
        let interface = gio::Settings::new("org.gnome.desktop.interface");
        let _ = interface.string("color-scheme");
    }
}
