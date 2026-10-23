//! The pointer's image: the themed cursor the session names (`doc_own_stack.md` OS15).

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{render_elements, Kind};
use smithay::backend::renderer::{ImportAll, ImportMem, Renderer, Texture};
use smithay::desktop::utils::bbox_from_surface_tree;
use smithay::input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, Size, Transform};
use smithay::wayland::compositor::with_states;

use crate::state::State;

/// The cursor size when `XCURSOR_SIZE` names none, libXcursor's default.
const DEFAULT_SIZE: u32 = 24;

/// The square drawn when the theme has no cursor: C1's pointer, so that it is never invisible.
const FALLBACK_SIDE: i32 = 12;
const FALLBACK_RGBA: [u8; 4] = [230, 230, 230, 255];

/// The largest cursor surface a client may set, in logical pixels a side: a larger one gets
/// the default cursor, so that no client covers the screen with its cursor.
const MAX_CLIENT_CURSOR: i32 = 256;

render_elements! {
    pub PointerElement<R> where R: ImportAll + ImportMem;
    Memory=MemoryRenderBufferRenderElement<R>,
    Surface=WaylandSurfaceRenderElement<R>,
}

/// One cursor image, ready to draw.
pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// The point of the image at the pointer's location, in logical pixels.
    pub hotspot: Point<i32, Logical>,
}

/// The cursor theme, with the images loaded so far.
pub struct Cursor {
    theme: String,
    size: u32,
    images: HashMap<(CursorIcon, i32), CursorImage>,
}

impl Cursor {
    /// The theme and size the session exports before it starts the compositor, read once.
    pub fn from_env() -> Self {
        let theme = std::env::var("XCURSOR_THEME")
            .ok()
            .filter(|theme| !theme.is_empty())
            .unwrap_or_else(|| "default".to_owned());
        let size = size_from(std::env::var("XCURSOR_SIZE").ok().as_deref());
        Self::new(&theme, size)
    }

    pub fn new(theme: &str, size: u32) -> Self {
        Self {
            theme: theme.to_owned(),
            size,
            images: HashMap::new(),
        }
    }

    /// The image of `icon` for an output at `scale`, loaded on first use.
    pub fn image(&mut self, icon: CursorIcon, scale: f64) -> &CursorImage {
        let scale = buffer_scale(scale);
        let (theme, size) = (&self.theme, self.size);
        self.images
            .entry((icon, scale))
            .or_insert_with(|| load(theme, icon, size, scale))
    }
}

impl State {
    /// The cursor the pointer shows now: `effective` of the status a client set, with the
    /// client the pointer is over.
    pub fn shown_cursor(&self) -> CursorImageStatus {
        let focus = self
            .seat
            .get_pointer()
            .and_then(|pointer| pointer.current_focus())
            .and_then(|surface| surface.client())
            .map(|client| client.id());
        let (status, owner) = &self.cursor_status;
        effective(status, owner.as_ref(), focus.as_ref())
    }
}

/// `status` while its owner holds the pointer's focus, else the default: a client's cursor is
/// never shown over another client's surface or over the background.
pub fn effective(
    status: &CursorImageStatus,
    owner: Option<&ClientId>,
    focus: Option<&ClientId>,
) -> CursorImageStatus {
    match owner {
        Some(owner) if focus == Some(owner) => status.clone(),
        _ => CursorImageStatus::default_named(),
    }
}

/// The buffer scale of an output's cursor: its scale rounded up, so a fractional output
/// downscales a sharper image.
fn buffer_scale(scale: f64) -> i32 {
    // `as` saturates, and NaN becomes 0: anything but a positive scale is drawn at 1.
    (scale.ceil() as i32).max(1)
}

/// `XCURSOR_SIZE` as a positive number, else the default.
fn size_from(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|&size| size > 0)
        .unwrap_or(DEFAULT_SIZE)
}

/// The image whose nominal size is nearest `size`.
fn nearest(images: &[xcursor::parser::Image], size: u32) -> Option<&xcursor::parser::Image> {
    images.iter().min_by_key(|image| image.size.abs_diff(size))
}

/// Loads `icon` from `theme`, trying its name, its other names and then `default`; the
/// fallback square when none loads.
// ponytail: first frame only; animated cursors (wait, progress) need a timer on the frame delays.
fn load(theme: &str, icon: CursorIcon, size: u32, scale: i32) -> CursorImage {
    let themed = xcursor::CursorTheme::load(theme);
    let wanted = size.saturating_mul(scale.unsigned_abs());
    let names = std::iter::once(icon.name())
        .chain(icon.alt_names().iter().copied())
        .chain(std::iter::once("default"));
    let found = names
        .filter_map(|name| themed.load_icon(name))
        .find_map(|path| {
            let content = std::fs::read(&path)
                .map_err(|error| {
                    tracing::warn!(path = %path.display(), %error, "the cursor file is unreadable");
                })
                .ok()?;
            let Some(images) = xcursor::parser::parse_xcursor(&content) else {
                tracing::warn!(path = %path.display(), "the cursor file does not parse");
                return None;
            };
            let image = nearest(&images, wanted)?;
            let (width, height) = (
                i32::try_from(image.width).ok()?,
                i32::try_from(image.height).ok()?,
            );
            let hotspot = (
                i32::try_from(image.xhot).ok()?,
                i32::try_from(image.yhot).ok()?,
            );
            Some(CursorImage {
                buffer: MemoryRenderBuffer::from_slice(
                    &image.pixels_rgba,
                    Fourcc::Abgr8888,
                    (width, height),
                    scale,
                    Transform::Normal,
                    None,
                ),
                hotspot: Point::from((hotspot.0 / scale, hotspot.1 / scale)),
            })
        });
    found.unwrap_or_else(|| {
        tracing::warn!(
            theme,
            icon = icon.name(),
            "no cursor in the theme: the pointer is a square"
        );
        fallback()
    })
}

fn fallback() -> CursorImage {
    let pixels = [FALLBACK_RGBA; (FALLBACK_SIDE * FALLBACK_SIDE) as usize].concat();
    CursorImage {
        buffer: MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Abgr8888,
            (FALLBACK_SIDE, FALLBACK_SIDE),
            1,
            Transform::Normal,
            None,
        ),
        hotspot: Point::from((0, 0)),
    }
}

/// The pointer's elements on an output at `scale`, `at` relative to the output, all drawn as
/// cursors: the cursor `status` names, or the theme's default in place of a client's surface
/// too large to be a cursor. An image the renderer cannot take is left out: the frame is drawn
/// without the pointer rather than not at all.
pub fn pointer_elements<R>(
    renderer: &mut R,
    cursor: &mut Cursor,
    status: &CursorImageStatus,
    at: Point<f64, Logical>,
    scale: f64,
) -> Vec<PointerElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let icon = match status {
        CursorImageStatus::Hidden => return Vec::new(),
        CursorImageStatus::Surface(surface)
            if drawable(bbox_from_surface_tree(surface, (0, 0)).size) =>
        {
            let hotspot = with_states(surface, |states| {
                states
                    .data_map
                    .get::<CursorImageSurfaceData>()
                    .map(|attributes| {
                        attributes.lock().map_or_else(
                            |poisoned| poisoned.into_inner().hotspot,
                            |attributes| attributes.hotspot,
                        )
                    })
            })
            .unwrap_or_default();
            let location = (at - hotspot.to_f64()).to_physical(scale).to_i32_round();
            return render_elements_from_surface_tree(
                renderer,
                surface,
                location,
                scale,
                1.0,
                Kind::Cursor,
            )
            .into_iter()
            .map(PointerElement::Surface)
            .collect();
        }
        CursorImageStatus::Surface(_) => CursorIcon::Default,
        CursorImageStatus::Named(icon) => *icon,
    };
    let image = cursor.image(icon, scale);
    let location = (at - image.hotspot.to_f64()).to_physical(scale);
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        location,
        &image.buffer,
        None,
        None,
        None,
        Kind::Cursor,
    ) {
        Ok(element) => vec![PointerElement::Memory(element)],
        Err(error) => {
            tracing::warn!(%error, "the pointer's image cannot be drawn");
            Vec::new()
        }
    }
}

/// Whether a client's cursor surface of `size` is drawn.
fn drawable(size: Size<i32, Logical>) -> bool {
    size.w <= MAX_CLIENT_CURSOR && size.h <= MAX_CLIENT_CURSOR
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::renderer::damage::OutputDamageTracker;
    use smithay::backend::renderer::element::Element;
    use smithay::backend::renderer::pixman::PixmanRenderer;
    use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
    use smithay::utils::Rectangle;

    /// An image of nominal size `size`, `size`x`size` pixels, as a theme's file holds it.
    fn image(size: u32) -> xcursor::parser::Image {
        let pixels = vec![0; (size * size * 4) as usize];
        xcursor::parser::Image {
            size,
            width: size,
            height: size,
            xhot: 0,
            yhot: 0,
            delay: 0,
            pixels_argb: pixels.clone(),
            pixels_rgba: pixels,
        }
    }

    #[test]
    fn the_size_defaults_to_24() {
        assert_eq!(size_from(None), 24);
        assert_eq!(size_from(Some("")), 24);
        assert_eq!(size_from(Some("0")), 24);
        assert_eq!(size_from(Some("big")), 24);
        assert_eq!(size_from(Some("48")), 48);
    }

    #[test]
    fn the_nearest_size_is_picked() {
        let images = [image(24), image(32), image(48)];
        assert_eq!(nearest(&images, 24).map(|i| i.size), Some(24));
        assert_eq!(nearest(&images, 36).map(|i| i.size), Some(32));
        assert_eq!(nearest(&images, 64).map(|i| i.size), Some(48));
        assert!(nearest(&[], 24).is_none());
    }

    #[test]
    fn a_missing_theme_falls_back_to_the_square() {
        let mut cursor = Cursor::new("athanor-no-such-theme", 24);
        let image = cursor.image(CursorIcon::Default, 1.0);
        // The fallback is the 12-pixel light square C1 drew, whose pixels
        // `the_pointer_is_drawn_at_its_hotspot` reads.
        assert_eq!(image.hotspot, Point::from((0, 0)));
    }

    #[test]
    fn the_pointer_is_drawn_at_its_hotspot() {
        let mut renderer = PixmanRenderer::new().expect("pixman");
        let mut cursor = Cursor::new("athanor-no-such-theme", 24);
        let at = Point::from((40.0, 30.0));
        let named = CursorImageStatus::default_named();
        let elements = pointer_elements(&mut renderer, &mut cursor, &named, at, 1.0);
        assert!(!elements.is_empty(), "the pointer is drawn");
        assert!(elements.iter().all(|e| e.kind() == Kind::Cursor));

        let mut buffer = renderer
            .create_buffer(Fourcc::Abgr8888, (64, 64).into())
            .expect("a buffer");
        let mut target = renderer.bind(&mut buffer).expect("bound");
        let mut damage = OutputDamageTracker::new((64, 64), 1.0, Transform::Normal);
        damage
            .render_output(&mut renderer, &mut target, 0, &elements, [0.0; 4])
            .expect("drawn");
        let region = Rectangle::from_size((64, 64).into());
        let mapping = renderer
            .copy_framebuffer(&target, region, Fourcc::Abgr8888)
            .expect("read back");
        let pixels = renderer.map_texture(&mapping).expect("mapped");
        let pixel = |x: usize, y: usize| <[u8; 4]>::try_from(&pixels[(y * 64 + x) * 4..][..4]).ok();
        assert_eq!(pixel(40, 30), Some(FALLBACK_RGBA), "the hotspot");
        assert_eq!(pixel(51, 41), Some(FALLBACK_RGBA), "the square's last pixel");
        assert_eq!(pixel(39, 30), Some([0; 4]), "left of the hotspot");
        assert_eq!(pixel(52, 30), Some([0; 4]), "right of the square");

        let hidden = CursorImageStatus::Hidden;
        assert!(pointer_elements(&mut renderer, &mut cursor, &hidden, at, 1.0).is_empty());
    }

    #[test]
    fn a_large_cursor_is_not_drawn() {
        assert!(drawable((256, 256).into()));
        assert!(!drawable((257, 16).into()));
        assert!(!drawable((16, 257).into()));
    }
}
