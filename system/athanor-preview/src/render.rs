//! Asks athanor-preview-render for a bitmap (doc_launcher.md, LA9): the file goes in on
//! standard input, opened here; the helper runs in a transient unit of the user manager
//! with no network, no home, no /run (so no bus) and a cap on memory and tasks.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use athanor_unit::text::line;

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
    #[error("the decoder failed with exit status {0}")]
    Failed(i32),
    #[error("the decode was cancelled")]
    Cancelled,
    #[error("the decoder did not finish in time")]
    Timeout,
    #[error("the decoder's answer is malformed: {0}")]
    Malformed(&'static str),
}

/// The properties every decode has: nothing of the user's, nothing of /run (the system bus
/// and every daemon socket live there; `$XDG_RUNTIME_DIR` too), no network, no swap.
const COMMON: [&str; 12] = [
    "ProtectHome=yes",
    "ProtectSystem=strict",
    "NoNewPrivileges=yes",
    "PrivateNetwork=yes",
    "PrivateIPC=yes",
    "TemporaryFileSystem=/tmp",
    "TemporaryFileSystem=/run",
    "RuntimeMaxSec=5",
    "MemoryMax=512M",
    "MemorySwapMax=0",
    // `systemctl stop` waits this long before it kills what ignores SIGTERM.
    "TimeoutStopSec=1",
    "RestrictAddressFamilies=AF_UNIX",
];

/// The properties of the unit for one decode (doc_launcher.md, LA9).
///
/// Images: glycin starts bubblewrap, which needs user namespaces, netlink and the mount
/// calls, and its loaders run threads (the smallest working TasksMax measured on a 16-core
/// machine is 44: rayon sizes its pool by core count, so the cap scales with it).
/// PDFs: poppler runs in the helper itself, outside bubblewrap, so the unit alone confines
/// it: no namespaces, no mount or privileged calls, a handful of tasks (2 measured).
fn properties(kind: Kind) -> Vec<String> {
    let mut properties: Vec<String> = COMMON.iter().map(|property| (*property).to_owned()).collect();
    match kind {
        Kind::Image => {
            let cores = std::thread::available_parallelism().map_or(1, usize::from);
            properties.retain(|property| !property.starts_with("RestrictAddressFamilies="));
            properties.push("RestrictAddressFamilies=AF_UNIX AF_NETLINK".to_owned());
            properties.push("SystemCallFilter=@system-service @mount @privileged".to_owned());
            properties.push(format!("TasksMax={}", 32 + 4 * cores));
        }
        Kind::Pdf => {
            properties.push("SystemCallFilter=@system-service".to_owned());
            properties.push("RestrictNamespaces=yes".to_owned());
            properties.push("TasksMax=8".to_owned());
        }
    }
    properties
}

/// The arguments of `systemd-run` for one decode, in the named unit.
fn argv(kind: Kind, side: u32, unit: &str) -> Vec<String> {
    let name = match kind {
        Kind::Image => "image",
        Kind::Pdf => "pdf",
    };
    let mut argv: Vec<String> = ["systemd-run", "--user", "--pipe", "--quiet", "--wait", "--collect"]
        .map(str::to_owned)
        .to_vec();
    argv.push(format!("--unit={unit}"));
    for property in properties(kind) {
        argv.push("-p".to_owned());
        argv.push(property);
    }
    argv.extend([HELPER.to_owned(), name.to_owned(), side.to_string()]);
    argv
}

/// The longest frame the helper may write: the header and a full 1024 × 1024 bitmap.
const MAX_FRAME: usize = 13 + MAX_SIDE as usize * MAX_SIDE as usize * 4;
/// How much of the helper's standard error is logged.
const STDERR_LOG: usize = 512;

/// Names the units of this process: `athanor-preview-<pid>-<n>`.
static UNITS: AtomicU64 = AtomicU64::new(0);

/// Runs the helper in its own unit and returns its bitmap. When `cancellable` fires or the
/// deadline passes, the unit is stopped, not only systemd-run: the caller may start the
/// next decode once this returns.
pub async fn render(file: File, kind: Kind, side: u32, cancellable: &gio::Cancellable) -> Result<gdk::MemoryTexture, RenderError> {
    let unit = format!("athanor-preview-{}-{}", std::process::id(), UNITS.fetch_add(1, Ordering::Relaxed));
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE);
    launcher.take_stdin_fd(Some(OwnedFd::from(file)));
    let argv = argv(kind, side.clamp(1, MAX_SIDE), &unit);
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let process = launcher.spawn(&argv).map_err(RenderError::Spawn)?;
    let answer = gio::CancellableFuture::new(glib::future_with_timeout(DEADLINE, collect(&process)), cancellable.clone()).await;
    let answer = match answer {
        Ok(Ok(Ok(answer))) => answer,
        Ok(Ok(Err(err))) => {
            stop(&process, &unit).await;
            return Err(err);
        }
        Ok(Err(_)) => {
            stop(&process, &unit).await;
            return Err(RenderError::Timeout);
        }
        Err(_) => {
            stop(&process, &unit).await;
            return Err(RenderError::Cancelled);
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

/// Stops the unit and waits until it is gone. Terminating systemd-run does not stop the
/// unit it started (it stays active until RuntimeMaxSec), so the unit is stopped by name;
/// systemd-run is terminated afterwards so that it is reaped. A stop that fails (the unit
/// was not created yet) is logged: RuntimeMaxSec still ends the decode.
async fn stop(process: &gio::Subprocess, unit: &str) {
    let stopped = gio::Subprocess::newv(
        &["systemctl", "--user", "stop", &format!("{unit}.service")].map(std::ffi::OsStr::new),
        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE,
    );
    match stopped {
        Ok(stopped) => match glib::future_with_timeout(Duration::from_secs(3), stopped.wait_check_future()).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(unit, "the decoder's unit was not stopped: {err}"),
            Err(_) => {
                tracing::warn!(unit, "stopping the decoder's unit took too long");
                stopped.force_exit();
            }
        },
        Err(err) => tracing::warn!(unit, "systemctl did not start: {err}"),
    }
    process.send_signal(libc::SIGTERM);
}

/// What the helper wrote, read through a fixed buffer: a helper that was subverted cannot
/// make the launcher hold more than one frame of the largest size. A failed helper's
/// standard error is logged, a bounded prefix of it and cleaned.
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
        if let Some(stderr) = process.stderr_pipe() {
            if let Ok((bytes, read, _)) = stderr.read_all_future(vec![0_u8; STDERR_LOG], glib::Priority::DEFAULT).await {
                let said = String::from_utf8_lossy(&bytes[..read.min(STDERR_LOG)]);
                tracing::warn!("the decoder said: {}", line(&said, STDERR_LOG));
            }
        }
        return Err(RenderError::Failed(process.exit_status()));
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
    fn both_kinds_hide_run_and_cap_memory_and_tasks() {
        for kind in [Kind::Image, Kind::Pdf] {
            let argv = argv(kind, 64, "athanor-preview-1-0");
            for wanted in ["TemporaryFileSystem=/run", "MemorySwapMax=0", "MemoryMax=512M", "RuntimeMaxSec=5", "--unit=athanor-preview-1-0"] {
                assert!(argv.iter().any(|a| a == wanted), "{kind:?} lacks {wanted}");
            }
            assert!(argv.iter().any(|a| a.starts_with("TasksMax=")), "{kind:?} has no task cap");
        }
    }

    #[test]
    fn a_pdf_gets_no_namespaces_and_no_privileged_calls() {
        let pdf = argv(Kind::Pdf, 64, "u");
        assert!(pdf.iter().any(|a| a == "RestrictNamespaces=yes"));
        assert!(pdf.iter().any(|a| a == "RestrictAddressFamilies=AF_UNIX"));
        assert!(pdf.iter().all(|a| !a.contains("@mount") && !a.contains("@privileged") && !a.contains("AF_NETLINK")));
        let image = argv(Kind::Image, 64, "u");
        assert!(image.iter().any(|a| a.contains("@mount @privileged")));
        assert!(image.iter().all(|a| a != "RestrictNamespaces=yes"));
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
