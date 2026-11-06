//! Opening an image (SD6): a path must name a regular file, opened with `O_NOFOLLOW` on its
//! last component and `O_NONBLOCK`, so a FIFO or a device never blocks the daemon.

use crate::{hearth_path, ColorScheme, Source};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;

/// Opens `path` read-only if it is an absolute path to a regular file that is not a symlink.
/// The returned descriptor is the one to decode from: checking and opening are one step.
pub fn open_regular(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an absolute path",
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY)
        .open(path)?;
    let kind = file.metadata()?.file_type();
    if kind.is_file() {
        Ok(file)
    } else {
        let what = if kind.is_fifo() {
            "a FIFO"
        } else {
            "not a regular file"
        };
        Err(io::Error::new(io::ErrorKind::InvalidInput, what))
    }
}

/// Opens what `source` names. A path that is not a regular file, or is missing or
/// unreadable, falls back to the hearth with one warning; an error is returned only when the
/// hearth itself cannot be opened.
pub fn open(source: &Source, scheme: ColorScheme) -> io::Result<File> {
    if let Source::Path(path) = source {
        match open_regular(path) {
            Ok(file) => return Ok(file),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "cannot use the image; showing the hearth");
            }
        }
    }
    open_regular(&hearth_path(scheme))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("athanor-wallpaper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn regular_file_opens() {
        let d = dir("regular");
        std::fs::write(d.join("a.png"), b"x").unwrap();
        assert!(open_regular(&d.join("a.png")).is_ok());
    }

    #[test]
    fn refuses_symlink_directory_missing_relative_and_fifo() {
        let d = dir("refuse");
        std::fs::write(d.join("a.png"), b"x").unwrap();
        symlink(d.join("a.png"), d.join("link.png")).unwrap();
        assert!(open_regular(&d.join("link.png")).is_err());
        assert!(open_regular(&d).is_err());
        assert!(open_regular(&d.join("missing.png")).is_err());
        assert!(open_regular(Path::new("a.png")).is_err());
        let made = std::process::Command::new("mkfifo")
            .arg(d.join("f"))
            .status()
            .unwrap();
        assert!(made.success());
        let error = open_regular(&d.join("f")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
