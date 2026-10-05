//! The picture files a peer's notification names (ruling 8): which paths are read at all,
//! and the bytes of a file only when it is a small PNG. Shared by the bar and the control
//! center, which both draw notifications.

use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

const MAX_PATH: usize = 4096;
const ICON_FILE_BYTES: u64 = 1024 * 1024;
const ICON_FILE_SIDE: u32 = 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// An absolute path with no `..` and no control character: the only files a card reads.
#[must_use]
pub fn is_icon_file(path: &str) -> bool {
    let as_path = Path::new(path);
    path.len() <= MAX_PATH
        && as_path.is_absolute()
        && !path.chars().any(char::is_control)
        && as_path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

/// The bytes of a picture file, only when it is a regular file of at most 1 MiB holding a
/// PNG whose header declares at most 1024 × 1024 (ruling 8). Links are resolved and the
/// type is read before the open, so a FIFO or a device node (opening some has side
/// effects) is never opened. The open refuses a link swapped in since, and cannot block or
/// take a terminal; the open file is checked again. `None` means "show the generic icon";
/// the reason is not worth a log line per redraw.
#[must_use]
pub fn read_icon_file(path: &str) -> Option<Vec<u8>> {
    if !is_icon_file(path) {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    if !std::fs::metadata(&real).ok()?.is_file() {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_NOFOLLOW)
        .open(&real)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > ICON_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    // The file may have grown since `metadata`: read one byte past the bound to see it.
    file.take(ICON_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let within = u64::try_from(bytes.len()).is_ok_and(|len| len <= ICON_FILE_BYTES);
    (within && png_within(&bytes, ICON_FILE_SIDE)).then_some(bytes)
}

/// A PNG signature, then the IHDR chunk, whose width and height are big-endian at bytes
/// 16 and 20, each within `1..=max_side`. GDK decodes only what passes.
fn png_within(bytes: &[u8], max_side: u32) -> bool {
    let side = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|four| <[u8; 4]>::try_from(four).ok())
            .map(u32::from_be_bytes)
    };
    bytes.starts_with(PNG_SIGNATURE)
        && bytes.get(12..16) == Some(b"IHDR".as_slice())
        && [side(16), side(20)]
            .iter()
            .all(|value| value.is_some_and(|value| (1..=max_side).contains(&value)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    #[test]
    fn only_absolute_clean_paths_are_files() {
        assert!(is_icon_file("/usr/share/icons/a.png"));
        let long = format!("/{}", "a".repeat(MAX_PATH));
        for bad in [
            "",
            "a.png",
            "/usr/../etc/shadow",
            "/usr/a\n.png",
            long.as_str(),
        ] {
            assert!(!is_icon_file(bad), "{bad:?}");
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("athanor-bar-notices-{}-{name}", std::process::id()))
    }

    fn read_written(name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
        let path = scratch(name);
        fs::write(&path, bytes).unwrap();
        let read = read_icon_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        read
    }

    #[test]
    fn a_png_within_the_bounds_is_read() {
        assert_eq!(read_written("ok.png", &png(48, 48)), Some(png(48, 48)));
    }

    #[test]
    fn a_png_over_the_side_bound_is_refused() {
        assert_eq!(read_written("wide.png", &png(60_000, 60_000)), None);
        assert_eq!(read_written("zero.png", &png(0, 16)), None);
    }

    #[test]
    fn a_file_that_is_not_a_png_is_refused() {
        assert_eq!(read_written("text.png", b"hello"), None);
        assert_eq!(read_written("empty.png", b""), None);
    }

    #[test]
    fn a_file_over_the_byte_bound_is_refused() {
        let mut bytes = png(8, 8);
        bytes.resize(usize::try_from(ICON_FILE_BYTES).unwrap() + 1, 0);
        assert_eq!(read_written("big.png", &bytes), None);
    }

    #[test]
    fn a_fifo_is_not_read() {
        let path = scratch("fifo");
        assert!(Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let read = read_icon_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        assert_eq!(
            read, None,
            "and the call returned: nothing blocked on the FIFO"
        );
    }

    #[test]
    fn a_device_is_not_read() {
        assert_eq!(read_icon_file("/dev/zero"), None);
    }
}
