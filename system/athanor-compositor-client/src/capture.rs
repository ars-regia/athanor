//! A window's picture for the launcher's preview (doc_launcher.md, LA6), through
//! ext-image-copy-capture-v1 into a shared-memory buffer.

use std::fs::{File, OpenOptions};
use std::future::poll_fn;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::task::{Poll, Waker};
use std::time::Duration;

use gtk4::{gdk, glib};
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

use crate::connection::{fresh, Client, Error, State};
use crate::model::WindowId;
use crate::unit;

/// A frame takes one repaint; a window that is not repainted in time is shown without
/// its picture.
const TIMEOUT: Duration = Duration::from_millis(500);
/// The compositor reports the size, so it is bounded on both sides: a side, and the whole
/// buffer (4096 × 4096 pixels of 4 bytes, and a 5K window fits).
const MAX_SIDE: u32 = 8192;
const MAX_BYTES: u32 = 4096 * 4096 * 4;

/// One capture in flight. Dropping it destroys its protocol objects.
pub(crate) struct Pending {
    /// The capture ends with this window: it is dropped when the toplevel closes, so no
    /// object of the capture outlives the handle it was made from.
    pub(crate) window: WindowId,
    source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureSessionV1,
    size: Option<(u32, u32)>,
    format: Option<wl_shm::Format>,
    frame: Option<ExtImageCopyCaptureFrameV1>,
    buffer: Option<Shm>,
    result: Option<Result<gdk::MemoryTexture, Error>>,
    waker: Option<Waker>,
}

struct Shm {
    file: File,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    width: u32,
    height: u32,
    stride: u32,
    len: u32,
}

impl Pending {
    pub(crate) fn finish(&mut self, result: Result<gdk::MemoryTexture, Error>) {
        self.result.get_or_insert(result);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Pending {
    /// Wakes the waiting call without a result: it reports the capture as lost.
    pub(crate) fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// Destroys the objects in the reverse of their creation: frame, buffer, pool, session,
/// source.
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(frame) = self.frame.take() {
            frame.destroy();
        }
        if let Some(shm) = self.buffer.take() {
            shm.buffer.destroy();
            shm.pool.destroy();
        }
        self.session.destroy();
        self.source.destroy();
    }
}

/// How much a shared-memory format is preferred; 0 for one GDK cannot show.
fn rank(format: wl_shm::Format) -> u8 {
    match format {
        wl_shm::Format::Argb8888 | wl_shm::Format::Abgr8888 => 2,
        wl_shm::Format::Xrgb8888 | wl_shm::Format::Xbgr8888 => 1,
        _ => 0,
    }
}

/// wl_shm formats name the bytes of a little-endian 32-bit word.
fn memory_format(format: wl_shm::Format) -> Option<gdk::MemoryFormat> {
    match format {
        wl_shm::Format::Argb8888 => Some(gdk::MemoryFormat::B8g8r8a8Premultiplied),
        wl_shm::Format::Xrgb8888 => Some(gdk::MemoryFormat::B8g8r8x8),
        wl_shm::Format::Abgr8888 => Some(gdk::MemoryFormat::R8g8b8a8Premultiplied),
        wl_shm::Format::Xbgr8888 => Some(gdk::MemoryFormat::R8g8b8x8),
        _ => None,
    }
}

/// Stride and length of a buffer of 4-byte pixels, or `None` when it is empty or too big.
/// Every value it returns is below `i32::MAX`.
fn layout(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let stride = width.checked_mul(4)?;
    let len = stride.checked_mul(height)?;
    (len <= MAX_BYTES).then_some((stride, len))
}

/// An unlinked file in `$XDG_RUNTIME_DIR`, private to this process and the compositor.
fn shm_file(len: u32) -> std::io::Result<File> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set")
    })?;
    let path = std::path::Path::new(&dir).join(format!("athanor-capture-{}", unit::random()));
    let file = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path)?;
    std::fs::remove_file(&path)?;
    file.set_len(u64::from(len))?;
    Ok(file)
}

/// Ends a capture, whichever way its call ends.
struct Forget<'a> {
    client: &'a Client,
    id: u64,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        match self.client.inner.state.try_borrow_mut() {
            Ok(mut state) => {
                state.captures.remove(&self.id);
            }
            Err(_) => tracing::warn!("a capture could not be ended while the state was in use"),
        }
    }
}

impl Client {
    /// The picture of a window, as large as the window.
    pub async fn capture(&self, window: WindowId) -> Result<gdk::MemoryTexture, Error> {
        self.live()?;
        let id = fresh();
        {
            let mut state = self.inner.state.borrow_mut();
            let toplevel = state.toplevels.get(&window).ok_or(Error::NoWindow)?;
            let sources = state
                .globals
                .capture_sources
                .as_ref()
                .ok_or(Error::Unavailable("ext_foreign_toplevel_image_capture_source_manager_v1"))?;
            let manager = state
                .globals
                .image_copy
                .as_ref()
                .ok_or(Error::Unavailable("ext_image_copy_capture_manager_v1"))?;
            if state.globals.shm.is_none() {
                return Err(Error::Unavailable("wl_shm"));
            }
            let source = sources.create_source(&toplevel.ext, &self.inner.qh, ());
            let session = manager.create_session(&source, Options::empty(), &self.inner.qh, id);
            state.captures.insert(
                id,
                Pending {
                    window,
                    source,
                    session,
                    size: None,
                    format: None,
                    frame: None,
                    buffer: None,
                    result: None,
                    waker: None,
                },
            );
        }
        // Removes the capture when the call ends or is dropped before it does.
        let guard = Forget { client: self, id };
        self.flush()?;
        let answer = glib::future_with_timeout(
            TIMEOUT,
            poll_fn(|cx| {
                let mut state = self.inner.state.borrow_mut();
                let Some(pending) = state.captures.get_mut(&id) else {
                    return Poll::Ready(Err(Error::Capture("the capture was lost")));
                };
                match pending.result.take() {
                    Some(result) => Poll::Ready(result),
                    None => {
                        pending.waker = Some(cx.waker().clone());
                        Poll::Pending
                    }
                }
            }),
        )
        .await;
        // Destroys the frame, the buffer, the session and the source.
        drop(guard);
        self.flush()?;
        answer.unwrap_or(Err(Error::Capture("the compositor did not answer in time")))
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, u64> for State {
    fn event(
        state: &mut State,
        session: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        id: &u64,
        _: &Connection,
        qh: &QueueHandle<State>,
    ) {
        let shm = state.globals.shm.clone();
        let Some(pending) = state.captures.get_mut(id) else { return };
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => {
                pending.size = Some((width, height));
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat { format: WEnum::Value(format) } => {
                if rank(format) > pending.format.map_or(0, rank) {
                    pending.format = Some(format);
                }
            }
            ext_image_copy_capture_session_v1::Event::Done => {
                if pending.frame.is_some() {
                    return;
                }
                let (Some((width, height)), Some(format), Some(shm)) = (pending.size, pending.format, shm) else {
                    pending.finish(Err(Error::Capture("no shared-memory format GDK can show")));
                    return;
                };
                let Some((stride, len)) = layout(width, height) else {
                    pending.finish(Err(Error::Capture("the window is too large to preview")));
                    return;
                };
                let file = match shm_file(len) {
                    Ok(file) => file,
                    Err(err) => {
                        tracing::warn!("the capture buffer was not created: {err}");
                        pending.finish(Err(Error::Capture("no buffer")));
                        return;
                    }
                };
                // `layout` bounds every value below i32::MAX.
                let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
                let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, format, qh, ());
                let frame = session.create_frame(qh, *id);
                frame.attach_buffer(&buffer);
                frame.damage_buffer(0, 0, width as i32, height as i32);
                frame.capture();
                pending.frame = Some(frame);
                pending.buffer = Some(Shm { file, pool, buffer, width, height, stride, len });
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                pending.finish(Err(Error::Capture("the window went away")));
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, u64> for State {
    fn event(
        state: &mut State,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let Some(pending) = state.captures.get_mut(id) else { return };
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => {
                let result = match (&pending.buffer, pending.format.and_then(memory_format)) {
                    (Some(shm), Some(memory)) => {
                        let mut bytes = vec![0; shm.len as usize];
                        match shm.file.read_exact_at(&mut bytes, 0) {
                            Ok(()) => Ok(gdk::MemoryTexture::new(
                                shm.width as i32,
                                shm.height as i32,
                                memory,
                                &glib::Bytes::from_owned(bytes),
                                shm.stride as usize,
                            )),
                            Err(err) => {
                                tracing::warn!("the capture buffer was not read: {err}");
                                Err(Error::Capture("the buffer could not be read"))
                            }
                        }
                    }
                    _ => Err(Error::Capture("a frame without a buffer")),
                };
                pending.finish(result);
            }
            ext_image_copy_capture_frame_v1::Event::Failed { .. } => {
                pending.finish(Err(Error::Capture("the compositor refused the frame")));
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(State: ExtImageCopyCaptureManagerV1);
delegate_noop!(State: ExtImageCaptureSourceV1);
delegate_noop!(State: wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_buffer::WlBuffer);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_formats_come_first_and_unknown_ones_are_ignored() {
        assert!(rank(wl_shm::Format::Argb8888) > rank(wl_shm::Format::Xrgb8888));
        assert!(rank(wl_shm::Format::Abgr8888) > rank(wl_shm::Format::Xbgr8888));
        assert_eq!(rank(wl_shm::Format::Rgb565), 0);
        assert_eq!(memory_format(wl_shm::Format::Argb8888), Some(gdk::MemoryFormat::B8g8r8a8Premultiplied));
        assert_eq!(memory_format(wl_shm::Format::Rgb565), None);
    }

    #[test]
    fn a_buffer_is_bounded() {
        assert_eq!(layout(1920, 1080), Some((7680, 7680 * 1080)));
        assert_eq!(layout(0, 1080), None);
        assert_eq!(layout(MAX_SIDE + 1, 10), None);
        assert_eq!(layout(MAX_SIDE, MAX_SIDE), None, "over the byte cap");
        assert_eq!(layout(5120, 2880), Some((20480, 20480 * 2880)));
        assert_eq!(layout(u32::MAX, u32::MAX), None);
    }
}
