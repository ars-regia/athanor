//! ext-image-copy-capture-v1 of a window, named through ext-foreign-toplevel-list-v1 and the
//! toplevel capture source (doc_own_stack.md, C1 task 4). A frame is drawn off screen at scale 1
//! and copied into the client's wl_shm buffer at the next frame of the outputs. Nothing is
//! captured while the session is locked (spike Q7).

use crate::state::{Backend, State};
use crate::Error;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::AsRenderElements;
use smithay::backend::renderer::gles::GlesTexture;
use smithay::backend::renderer::{Bind, ExportMem, ImportAll, Offscreen, Renderer, Texture};
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_shm};
use smithay::utils::{Buffer, Point, Rectangle, Size, Transform};
use smithay::wayland::foreign_toplevel_list::{
    ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
};
use smithay::wayland::image_capture_source::{
    ImageCaptureSource, ImageCaptureSourceHandler, ToplevelCaptureSourceHandler,
    ToplevelCaptureSourceState,
};
use smithay::wayland::image_copy_capture::{
    BufferConstraints, CaptureFailureReason, Frame, FrameRef, ImageCopyCaptureHandler,
    ImageCopyCaptureState, Session, SessionRef,
};
use smithay::wayland::shm::with_buffer_contents_mut;
use std::time::Duration;

/// The format a window is read back in: R, G, B, A bytes, swapped into the client's B, G, R, A.
const READ_FORMAT: smithay::backend::allocator::Fourcc = crate::headless::FRAME_FORMAT;

/// The longest side of a window that can be captured: a window's geometry follows its
/// subsurfaces, which a client places where it likes, and the compositor reads it into memory.
const MAX_SIDE: i32 = 8192;

/// The window a capture source names: the identifier of its foreign toplevel handle.
struct Captured(String);

impl ForeignToplevelListHandler for State {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevel_state
    }
}

impl ImageCaptureSourceHandler for State {}

impl ToplevelCaptureSourceHandler for State {
    fn toplevel_capture_source_state(&mut self) -> &mut ToplevelCaptureSourceState {
        &mut self.toplevel_capture_state
    }

    fn toplevel_source_created(
        &mut self,
        source: ImageCaptureSource,
        toplevel: ForeignToplevelHandle,
    ) {
        source
            .user_data()
            .insert_if_missing_threadsafe(|| Captured(toplevel.identifier()));
    }
}

impl ImageCopyCaptureHandler for State {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.image_copy_capture_state
    }

    /// The window's size, in wl_shm only; none while locked, which refuses the session.
    // ponytail: no dmabuf capture; offer it when a client streams a window (the portal, task 7).
    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        if self.lock.is_some() {
            return None;
        }
        let size = self.captured(source)?.geometry().size;
        Some(constraints(size.w, size.h))
    }

    fn new_session(&mut self, session: Session) {
        self.capture_sessions.push(session);
    }

    /// Queued for the next frame of the outputs, which paces a client capturing continuously.
    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        if self.lock.is_some() {
            frame.fail(CaptureFailureReason::Stopped);
            return;
        }
        self.captures.push((session.clone(), frame));
    }

    fn frame_aborted(&mut self, frame: FrameRef) {
        self.captures.retain(|(_, queued)| *queued != frame);
    }

    fn session_destroyed(&mut self, session: SessionRef) {
        self.capture_sessions.retain(|kept| *kept != session);
        self.captures.retain(|(queued, _)| *queued != session);
    }
}

/// Buffers of `width` by `height` pixels in wl_shm, with or without alpha.
fn constraints(width: i32, height: i32) -> BufferConstraints {
    BufferConstraints {
        size: Size::from((width, height)),
        shm: vec![wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888],
        dma: None,
    }
}

impl State {
    /// The window `source` names, if it is still mapped and of a size that can be captured.
    fn captured(&self, source: &ImageCaptureSource) -> Option<Window> {
        let Captured(id) = source.user_data().get::<Captured>()?;
        self.space
            .elements()
            .find(|window| {
                window
                    .user_data()
                    .get::<ForeignToplevelHandle>()
                    .is_some_and(|handle| &handle.identifier() == id)
            })
            .filter(|window| {
                let size = window.geometry().size;
                (1..=MAX_SIDE).contains(&size.w) && (1..=MAX_SIDE).contains(&size.h)
            })
            .cloned()
    }

    /// Answers the queued frames: each window is drawn and copied into its client's buffer.
    /// A buffer of the wrong size fails, and its session learns the window's new size.
    // ponytail: the whole window each time, with full damage; damage since the session's last
    // frame when a client streams one.
    pub fn serve_captures(&mut self) {
        self.image_copy_capture_state.cleanup();
        for (session, frame) in std::mem::take(&mut self.captures) {
            if self.lock.is_some() {
                frame.fail(CaptureFailureReason::Stopped);
                continue;
            }
            let Some(window) = self.captured(&session.source()) else {
                // Dropping the session sends the `stopped` that `failed(stopped)` points to.
                frame.fail(CaptureFailureReason::Stopped);
                self.capture_sessions.retain(|kept| *kept != session);
                continue;
            };
            let size = window.geometry().size;
            let read = match &mut self.backend {
                Backend::Headless(headless) => headless.read_window(&window),
                Backend::Nested(nested) => {
                    read_window::<_, GlesTexture>(nested.backend.renderer(), &window)
                }
            };
            match read.and_then(|pixels| copy_into(&frame.buffer(), size.w, size.h, &pixels)) {
                Ok(true) => {
                    frame.success(Transform::Normal, None, Duration::from(self.clock.now()))
                }
                Ok(false) => {
                    session.update_constraints(constraints(size.w, size.h));
                    frame.fail(CaptureFailureReason::BufferConstraints);
                }
                Err(error) => {
                    tracing::warn!(%error, "a window could not be captured");
                    frame.fail(CaptureFailureReason::Unknown);
                }
            }
        }
    }
}

/// Draws `window` off screen at scale 1 and reads its pixels back, `READ_FORMAT`, row by row
/// from the top.
pub fn read_window<R, T>(renderer: &mut R, window: &Window) -> Result<Vec<u8>, Error>
where
    R: Renderer + ImportAll + ExportMem + Offscreen<T> + Bind<T>,
    R::TextureId: Clone + Texture + 'static,
    R::Error: 'static,
{
    let geometry = window.geometry();
    let size = Size::<i32, Buffer>::from((geometry.size.w, geometry.size.h));
    let mut buffer = renderer.create_buffer(READ_FORMAT, size)?;
    let mut target = renderer.bind(&mut buffer)?;
    // The window's geometry, without its shadow, starts at the buffer's origin.
    let location = (Point::default() - geometry.loc).to_physical(1);
    let elements: Vec<WaylandSurfaceRenderElement<R>> =
        window.render_elements(renderer, location, 1.0.into(), 1.0);
    let mut tracker = OutputDamageTracker::new((size.w, size.h), 1.0, Transform::Normal);
    let result = tracker
        .render_output(renderer, &mut target, 0, &elements, [0.0; 4])
        .map_err(|error| format!("{error:?}"))?;
    result.sync.wait()?;
    let mapping = renderer.copy_framebuffer(&target, Rectangle::from_size(size), READ_FORMAT)?;
    Ok(renderer.map_texture(&mapping)?.to_vec())
}

/// Copies `pixels`, `width` by `height` in `READ_FORMAT`, into the client's wl_shm `buffer`,
/// swapping R and B. False when the buffer is not of that size or format.
#[allow(unsafe_code)]
fn copy_into(buffer: &WlBuffer, width: i32, height: i32, pixels: &[u8]) -> Result<bool, Error> {
    let row = usize::try_from(width)? * 4;
    let rows = usize::try_from(height)?;
    if pixels.len() != row * rows {
        return Err("the window was read back at another size".into());
    }
    let swapped: Vec<u8> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [b, g, r, a])
        .collect();
    let copied = with_buffer_contents_mut(buffer, |ptr, len, data| {
        let fits = data.width == width
            && data.height == height
            && matches!(
                data.format,
                wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888
            );
        let (Ok(offset), Ok(stride)) = (usize::try_from(data.offset), usize::try_from(data.stride))
        else {
            return false;
        };
        let end = stride
            .checked_mul(rows.saturating_sub(1))
            .and_then(|last| last.checked_add(offset)?.checked_add(row));
        if !fits || stride < row || end.is_none_or(|end| end > len) {
            return false;
        }
        for (y, line) in swapped.chunks_exact(row).enumerate() {
            let at = offset + y * stride;
            // SAFETY: `ptr` maps `len` bytes of the client's pool for the length of this
            // callback, and `at + row` is within them (checked above). No reference into the
            // pool is made, so the client writing it meanwhile cannot break aliasing; at worst
            // it reads a torn frame of its own making. `line` is the compositor's own memory and
            // does not overlap the pool.
            unsafe { std::ptr::copy_nonoverlapping(line.as_ptr(), ptr.wrapping_add(at), row) };
        }
        true
    })?;
    Ok(copied)
}
