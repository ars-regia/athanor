//! Asks athanor-preview-render for a bitmap (doc_launcher.md, LA9): the file goes in on
//! standard input, opened here; the helper runs in a transient unit of the user manager
//! with no network, no home, no runtime directory and no bus.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

pub const HELPER: &str = "/usr/libexec/athanor-preview-render";
const MAGIC: &[u8; 5] = b"ATPV1";
const MAX_SIDE: u32 = 1024;
/// The unit's own limit is 5 s; this one also covers the start of systemd-run.
const DEADLINE: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Image,
    Pdf,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the decoder did not start: {0}")]
    Spawn(glib::Error),
    #[error("the decoder's answer cannot be read: {0}")]
    Read(glib::Error),
    #[error("the decoder failed")]
    Failed,
    #[error("the decoder did not finish in time")]
    Timeout,
    #[error("the decoder's answer is malformed: {0}")]
    Malformed(&'static str),
}

/// The arguments of `systemd-run` for one decode; see "The decoder unit" in the plan.
fn argv(kind: Kind, side: u32) -> Vec<String> {
    let kind = match kind {
        Kind::Image => "image",
        Kind::Pdf => "pdf",
    };
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user".to_owned());
    [
        "systemd-run", "--user", "--pipe", "--quiet", "--wait", "--collect",
        "-p", "ProtectHome=yes",
        "-p", &format!("InaccessiblePaths={runtime}"),
        "-p", "ProtectSystem=strict",
        "-p", "NoNewPrivileges=yes",
        "-p", "RestrictAddressFamilies=AF_UNIX AF_NETLINK",
        "-p", "SystemCallFilter=@system-service @mount @privileged",
        "-p", "PrivateNetwork=yes",
        "-p", "PrivateIPC=yes",
        "-p", "TemporaryFileSystem=/tmp",
        "-p", "RuntimeMaxSec=5",
        "-p", "MemoryMax=512M",
        HELPER, kind, &side.to_string(),
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The longest frame the helper may write: the header and a full 1024 × 1024 bitmap.
const MAX_FRAME: usize = 13 + MAX_SIDE as usize * MAX_SIDE as usize * 4;

pub async fn render(file: File, kind: Kind, side: u32) -> Result<gdk::MemoryTexture, RenderError> {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    launcher.take_stdin_fd(Some(OwnedFd::from(file)));
    let argv = argv(kind, side.clamp(1, MAX_SIDE));
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let process = launcher.spawn(&argv).map_err(RenderError::Spawn)?;
    let answer = glib::future_with_timeout(DEADLINE, collect(&process)).await;
    let answer = match answer {
        Ok(Ok(answer)) => answer,
        Ok(Err(err)) => {
            stop(&process);
            return Err(err);
        }
        Err(_) => {
            stop(&process);
            return Err(RenderError::Timeout);
        }
    };
    let (width, height, rgba) = decode(&answer)?;
    Ok(gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &glib::Bytes::from(rgba),
        width as usize * 4,
    ))
}

/// SIGTERM, not SIGKILL: systemd-run stops its unit when it is terminated, and the unit's
/// RuntimeMaxSec ends it anyway if systemd-run is gone.
fn stop(process: &gio::Subprocess) {
    process.send_signal(libc::SIGTERM);
}

/// What the helper wrote, read through a fixed buffer: a helper that was subverted cannot
/// make the launcher hold more than one frame of the largest size.
async fn collect(process: &gio::Subprocess) -> Result<Vec<u8>, RenderError> {
    let stdout = process.stdout_pipe().ok_or(RenderError::Malformed("no output"))?;
    let (mut buffer, read, error) = stdout
        .read_all_future(vec![0_u8; MAX_FRAME + 1], glib::Priority::DEFAULT)
        .await
        .map_err(|(_, err)| RenderError::Read(err))?;
    if let Some(err) = error {
        return Err(RenderError::Read(err));
    }
    if read > MAX_FRAME {
        return Err(RenderError::Malformed("more output than a frame holds"));
    }
    buffer.truncate(read);
    process.wait_future().await.map_err(RenderError::Spawn)?;
    if !process.is_successful() {
        return Err(RenderError::Failed);
    }
    Ok(buffer)
}

/// The size and pixels of an ATPV1 frame, or why it is refused.
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, &[u8]), RenderError> {
    let rest = bytes.strip_prefix(MAGIC).ok_or(RenderError::Malformed("not ATPV1"))?;
    let (Some(width), Some(height)) = (rest.get(0..4), rest.get(4..8)) else {
        return Err(RenderError::Malformed("no size"));
    };
    let width = u32::from_le_bytes([width[0], width[1], width[2], width[3]]);
    let height = u32::from_le_bytes([height[0], height[1], height[2], height[3]]);
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(RenderError::Malformed("a size out of bounds"));
    }
    let pixels = &rest[8..];
    if pixels.len() != width as usize * height as usize * 4 {
        return Err(RenderError::Malformed("a length that does not match the size"));
    }
    Ok((width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, pixels: usize) -> Vec<u8> {
        [b"ATPV1".to_vec(), width.to_le_bytes().to_vec(), height.to_le_bytes().to_vec(), vec![7; pixels * 4]].concat()
    }

    #[test]
    fn a_frame_of_the_right_length_is_accepted() {
        let bytes = frame(2, 3, 6);
        let (width, height, rgba) = decode(&bytes).expect("frame");
        assert_eq!((width, height, rgba.len()), (2, 3, 24));
    }

    #[test]
    fn hostile_frames_are_refused_without_a_panic() {
        let header = |w: u32, h: u32| [b"ATPV1".to_vec(), w.to_le_bytes().to_vec(), h.to_le_bytes().to_vec()].concat();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", vec![]),
            ("magic only", b"ATPV1".to_vec()),
            ("truncated magic", b"ATPV".to_vec()),
            ("truncated size", b"ATPV1\x01\0\0".to_vec()),
            ("no height", [b"ATPV1".to_vec(), 1_u32.to_le_bytes().to_vec()].concat()),
            ("header only", header(1, 1)),
            ("zero width", [header(0, 1), vec![0; 4]].concat()),
            ("zero height", [header(1, 0), vec![0; 4]].concat()),
            ("width 1025", [header(1025, 1), vec![0; 1025 * 4]].concat()),
            ("height 1025", [header(1, 1025), vec![0; 1025 * 4]].concat()),
            ("u32::MAX squared", [header(u32::MAX, u32::MAX), vec![0; 16]].concat()),
            ("trailing byte", [header(1, 1), vec![0; 5]].concat()),
            ("trailing garbage", [header(2, 2), vec![0; 16], b"garbage".to_vec()].concat()),
            ("one byte short", [header(2, 2), vec![0; 15]].concat()),
            ("lowercase magic", [b"atpv1".to_vec(), vec![1, 0, 0, 0, 1, 0, 0, 0], vec![0; 4]].concat()),
        ];
        for (name, bytes) in cases {
            assert!(decode(&bytes).is_err(), "{name} was accepted");
        }
        let largest = [header(1024, 1024), vec![0; 1024 * 1024 * 4]].concat();
        assert_eq!(decode(&largest).map(|(w, h, p)| (w, h, p.len())).ok(), Some((1024, 1024, 1024 * 1024 * 4)));
        let smallest = [header(1, 1), vec![0; 4]].concat();
        assert!(decode(&smallest).is_ok());
    }

    #[test]
    fn every_other_frame_is_refused() {
        assert!(decode(&frame(2, 3, 5)).is_err(), "short");
        assert!(decode(&frame(2, 3, 7)).is_err(), "long");
        assert!(decode(&frame(0, 3, 0)).is_err(), "empty");
        assert!(decode(&frame(2048, 1, 2048)).is_err(), "wider than the helper draws");
        assert!(decode(b"ATPV2\x01\0\0\0\x01\0\0\0\0\0\0\0").is_err(), "another format");
    }
}
