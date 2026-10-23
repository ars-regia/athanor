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
use wayland_client::globals::GlobalList;
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

use crate::connection::{bind, fresh, Client, Error, State};
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
        self.wake();
    }

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

/// An unlinked file in `$XDG_RUNTIME_DIR/athanor`, private to this process and the
/// compositor. That directory is the one every confined shell program may write: the bar
/// and the launcher create it before they confine themselves, and their Landlock rulesets
/// grant it and nothing else of the runtime directory.
fn shm_file(len: u32) -> std::io::Result<File> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not set to an absolute path",
            )
        })?;
    let dir = crate::launch::runtime_subdir(&runtime)?;
    let path = dir.join(format!("capture-{}", unit::random()));
    let file = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path)?;
    std::fs::remove_file(&path)?;
    file.set_len(u64::from(len))?;
    Ok(file)
}

/// The capture globals, bound at the first capture (`Client::capture`): a client that never
/// captures never binds them. Every name of them in this crate is in this file.
#[derive(Default)]
pub(crate) struct Globals {
    sources: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    image_copy: Option<ExtImageCopyCaptureManagerV1>,
}

/// A capture global, bound at the first capture that needs it. One the compositor does not
/// offer is looked for again at the next capture.
fn bound<I>(slot: &mut Option<I>, globals: &GlobalList, qh: &QueueHandle<State>) -> Option<I>
where
    I: Proxy + Clone + 'static,
    State: Dispatch<I, ()>,
{
    if slot.is_none() {
        *slot = bind(globals, qh, 1..=1);
    }
    slot.clone()
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
            let state = &mut *state;
            let toplevel = state.toplevels.get(&window).ok_or(Error::NoWindow)?;
            let (registry, qh) = (&self.inner.registry, &self.inner.qh);
            let sources = bound(&mut state.globals.capture.sources, registry, qh)
                .ok_or(Error::Unavailable("ext_foreign_toplevel_image_capture_source_manager_v1"))?;
            let manager = bound(&mut state.globals.capture.image_copy, registry, qh)
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
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};

    const TOPLEVEL_LIST: &str = "ext_foreign_toplevel_list_v1";
    const OFFERED: [&str; 3] = [
        TOPLEVEL_LIST,
        "ext_foreign_toplevel_image_capture_source_manager_v1",
        "ext_image_copy_capture_manager_v1",
    ];

    fn message(object: u32, opcode: u32, args: &[u8]) -> Vec<u8> {
        let size = u32::try_from(8 + args.len()).expect("size");
        let mut out = object.to_ne_bytes().to_vec();
        out.extend((size << 16 | opcode).to_ne_bytes());
        out.extend(args);
        out
    }

    fn string(text: &str) -> Vec<u8> {
        let mut out = u32::try_from(text.len() + 1).expect("length").to_ne_bytes().to_vec();
        out.extend(text.as_bytes());
        out.push(0);
        out.resize(out.len().next_multiple_of(4), 0);
        out
    }

    /// One fake compositor's client at a time: `Client::connect_wayland` acquires the default
    /// main context, which a test on another thread may hold.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// A compositor speaking the wire protocol: it offers `OFFERED`, answers each sync, records
    /// the interface of every bind, and announces one window on the toplevel list, until the
    /// client hangs up.
    fn serve(mut stream: UnixStream, bound: &Mutex<Vec<String>>) {
        let word = |bytes: &[u8], at: usize| {
            u32::from_ne_bytes(bytes[at..at + 4].try_into().expect("word"))
        };
        let mut registry = None;
        let mut header = [0; 8];
        while stream.read_exact(&mut header).is_ok() {
            let (object, size_opcode) = (word(&header, 0), word(&header, 4));
            let mut body = vec![0; (size_opcode >> 16) as usize - 8];
            stream.read_exact(&mut body).expect("body");
            let mut reply = Vec::new();
            match (object, size_opcode & 0xffff) {
                // wl_display.get_registry: the globals.
                (1, 1) => {
                    let id = word(&body, 0);
                    registry = Some(id);
                    for (name, interface) in (1u32..).zip(OFFERED) {
                        let version = 1u32.to_ne_bytes();
                        let args = [&name.to_ne_bytes()[..], &string(interface), &version].concat();
                        reply.extend(message(id, 0, &args));
                    }
                }
                // wl_display.sync: wl_callback.done, then wl_display.delete_id.
                (1, 0) => {
                    let callback = word(&body, 0);
                    reply.extend(message(callback, 0, &0u32.to_ne_bytes()));
                    reply.extend(message(1, 1, &callback.to_ne_bytes()));
                }
                // wl_registry.bind: name, then the interface as a string.
                (id, 0) if Some(id) == registry => {
                    let length = word(&body, 4) as usize;
                    let interface = String::from_utf8_lossy(&body[8..8 + length - 1]);
                    if interface == TOPLEVEL_LIST {
                        // ext_foreign_toplevel_list_v1.toplevel, a server-allocated handle.
                        let list = word(&body, 8 + length.next_multiple_of(4) + 4);
                        reply.extend(message(list, 0, &0xff00_0000u32.to_ne_bytes()));
                    }
                    bound.lock().expect("lock").push(interface.into_owned());
                }
                _ => {}
            }
            stream.write_all(&reply).expect("reply");
        }
    }

    /// Least privilege: a client that never captures never binds the capture globals, even
    /// where the compositor offers them; only `Client::capture` binds them.
    #[test]
    fn connecting_binds_no_capture_global() {
        let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (ours, theirs) = UnixStream::pair().expect("socket pair");
        let bound = Arc::new(Mutex::new(Vec::new()));
        let record = bound.clone();
        std::thread::spawn(move || serve(theirs, &record));
        let connection = Connection::from_socket(ours).expect("connection");
        let client = Client::connect_wayland(&connection).expect("client");
        let bound = bound.lock().expect("lock").clone();
        assert!(bound.iter().any(|i| i == TOPLEVEL_LIST), "{bound:?}");
        assert!(!OFFERED[1..].iter().any(|c| bound.iter().any(|i| i == c)), "{bound:?}");
        drop(client);
    }

    /// A caller that captures binds both capture globals the compositor offers, once across
    /// its captures. Without `wl_shm` each capture then stops before it asks for a frame.
    #[test]
    fn capturing_binds_each_capture_global_once() {
        let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (ours, theirs) = UnixStream::pair().expect("socket pair");
        let bound = Arc::new(Mutex::new(Vec::new()));
        let record = bound.clone();
        std::thread::spawn(move || serve(theirs, &record));
        let connection = Connection::from_socket(ours).expect("connection");
        let client = Client::connect_wayland(&connection).expect("client");
        let window = *client.inner.state.borrow().toplevels.keys().next().expect("a window");
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        for _ in 0..2 {
            let mut capture = std::pin::pin!(client.capture(window));
            let answer = std::future::Future::poll(capture.as_mut(), &mut context);
            assert!(matches!(answer, Poll::Ready(Err(Error::Unavailable("wl_shm")))), "{answer:?}");
        }
        client.flush().expect("flush");
        connection.roundtrip().expect("roundtrip");
        let bound = bound.lock().expect("lock").clone();
        for capture in &OFFERED[1..] {
            assert_eq!(bound.iter().filter(|i| i == capture).count(), 1, "{bound:?}");
        }
        drop(client);
    }

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
