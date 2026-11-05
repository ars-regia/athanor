//! The focus outline and the border of confined windows (doc_own_stack.md decision 31). A mark is
//! stacked with its window, under whatever stands above it, so `stack` builds what Smithay's
//! `space_render_elements` does with the marks between the windows, for `draw_view` and for the
//! DRM path alike. Corners are small bitmaps, not a shader, because the renderers include Pixman.
//!
//! An unconfined window's outline and every window's divider follow cosmic-comp's (#918): a
//! ring along a rounded rectangle whose coverage `Indicator` computes for each pixel of the
//! output. They match the goldens at scale 1, and at scale 1.5 on every side where cosmic-comp
//! places the window as this compositor does, at whole logical coordinates; where it places the
//! window at a fractional offset, they land a pixel apart (#918). The divider lies inside the
//! window's geometry, where decision 31 places no mark, as the maintainer chose on 2026-11-04.

use crate::focus_theme::{Edge, Outline};
use crate::state::{full_screen_area, inset, laid_out, unconfined};
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, KindEvaluation, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::utils::{
    CommitCounter, DamageSet, OpaqueRegions, RendererSurfaceStateUserData,
};
use smithay::backend::renderer::{Color32F, ImportAll, ImportMem, Renderer, Texture};
use smithay::desktop::utils::bbox_from_surface_tree;
use smithay::desktop::{layer_map_for_output, LayerSurface, PopupManager, Space, Window};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform};
use smithay::wayland::compositor::{with_states, with_surface_tree_downward, TraversalAction};
use smithay::wayland::shell::wlr_layer::Layer;
use std::cell::RefCell;

/// The window radius of `doc_visual_language.md`, the corners a mark follows.
const WINDOW_RADIUS: i32 = 15;
/// How far a surface tree reaches past its geometry and is still taken for a shadow. An estimate
/// (decision 31): beyond it, or beyond a quarter of the geometry, the border goes around the tree.
const SHADOW_MARGIN: i32 = 64;
/// Past every output: a client's tree can reach as far as an `i32` does, and a ring's sums must not.
const FAR: i32 = 1 << 24;
/// The border's lines, outside first, each 1 px: the white one touches neither what lies around
/// the window nor what the client paints.
const BORDER: [[f32; 4]; 3] = [
    [0.0, 0.0, 0.0, 1.0],
    [1.0, 1.0, 1.0, 1.0],
    [0.0, 0.0, 0.0, 1.0],
];
const BORDER_WIDTH: i32 = BORDER.len() as i32;
/// The ring of a confined window's focus outline, after the border's in a surface's `Rings`.
const OUTLINE_RING: usize = BORDER.len();
/// An unconfined window's outline and every window's divider, in a surface's `Indicators`.
const OUTLINE: usize = 0;
const DIVIDER: usize = 1;

smithay::backend::renderer::element::render_elements! {
    pub Stacked<R> where R: ImportAll + ImportMem;
    Surface=SurfaceElement<R>,
    Line=SolidColorRenderElement,
    Corner=MemoryRenderBufferRenderElement<R>,
    Pixels=Pixels<R>,
}

/// A bitmap drawn pixel for pixel on the output's pixels at `geometry`. Smithay's own element
/// is sized in logical pixels, which a bitmap of any size cannot be at a fractional scale.
pub struct Pixels<R: Renderer> {
    inner: MemoryRenderBufferRenderElement<R>,
    geometry: Rectangle<i32, Physical>,
}

impl<R: Renderer> Element for Pixels<R> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R> RenderElement<R> for Pixels<R>
where
    R: Renderer + ImportMem,
    R::TextureId: 'static,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), R::Error> {
        self.inner
            .draw(frame, src, dst, damage, opaque_regions, cache)
    }
}

/// A client's surface whose opaque region holds only the output's pixels it covers whole.
/// Smithay rounds each edge of the logical region to the nearest pixel, so that at a fractional
/// scale a pixel the region covers by half, which the client antialiases, counts as opaque and
/// nothing is drawn beneath it (#978). wl_surface.set_opaque_region makes the region a hint
/// that must not grow past what the client paints opaque; a pixel lost costs only overdraw.
pub struct SurfaceElement<R: Renderer> {
    inner: WaylandSurfaceRenderElement<R>,
    /// The surface's opaque region, in its own logical pixels.
    opaque: Vec<Rectangle<i32, Logical>>,
}

impl<R: Renderer + ImportAll> Element for SurfaceElement<R> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        if self.inner.alpha() < 1.0 {
            return OpaqueRegions::default();
        }
        // The buffer is stretched over the element's whole pixels, as its damage is.
        let size = self.inner.geometry(scale).size.to_f64();
        let view = self.inner.view().dst.to_f64();
        let ratio = Scale::from((size.w / view.w, size.h / view.h));
        self.opaque
            .iter()
            .filter_map(|region| covered(*region, ratio))
            .collect()
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R> RenderElement<R> for SurfaceElement<R>
where
    R: Renderer + ImportAll,
    R::TextureId: Texture + 'static,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), R::Error> {
        self.inner
            .draw(frame, src, dst, damage, opaque_regions, cache)
    }

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

/// The pixels `region` covers whole, at `ratio` pixels to a logical pixel: its start rounded up,
/// its end down, and nothing when no pixel is left.
fn covered(region: Rectangle<i32, Logical>, ratio: Scale<f64>) -> Option<Rectangle<i32, Physical>> {
    let exact = region.to_f64().to_physical(ratio);
    let start = exact.loc.to_i32_ceil();
    let end = (exact.loc + exact.size).to_i32_floor();
    (end.x > start.x && end.y > start.y).then(|| Rectangle::from_extremities(start, end))
}

/// The elements of `surface`'s tree, as `render_elements_from_surface_tree` makes them, each
/// with its opaque region kept to the pixels it covers whole. Every client surface the
/// compositor draws goes through it.
pub fn surface_elements<R, E>(
    renderer: &mut R,
    surface: &WlSurface,
    location: impl Into<Point<i32, Physical>>,
    scale: impl Into<Scale<f64>>,
    alpha: f32,
    kind: impl Into<KindEvaluation>,
) -> Vec<E>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + 'static,
    E: From<SurfaceElement<R>>,
{
    let mut regions: Vec<(Id, Vec<Rectangle<i32, Logical>>)> = Vec::new();
    with_surface_tree_downward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |surface, states, _| {
            let opaque = states
                .data_map
                .get::<RendererSurfaceStateUserData>()
                .and_then(|data| data.lock().ok()?.opaque_regions().map(<[_]>::to_vec));
            if let Some(opaque) = opaque {
                regions.push((Id::from_wayland_resource(surface), opaque));
            }
        },
        |_, _, _| true,
    );
    render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<R>>(
        renderer, surface, location, scale, alpha, kind,
    )
    .into_iter()
    .map(|inner| {
        let opaque = regions
            .iter()
            .position(|(id, _)| id == inner.id())
            .map(|at| regions.swap_remove(at).1)
            .unwrap_or_default();
        E::from(SurfaceElement { inner, opaque })
    })
    .collect()
}

/// The black behind a full-screen window, whose element is `surface`'s for its whole life.
struct Backdrop(Id);

/// What the marks of one frame need from the session, read before the backend is borrowed.
pub struct Marks {
    pub focused: Option<Window>,
    /// `None` when the setting is off.
    pub outline: Option<Outline>,
    pub edge: Edge,
}

/// Whether `surface` belongs to a confined client. A surface whose client is gone, or unknown,
/// counts as confined: the mark is what fails safe.
fn confined(surface: &WlSurface) -> bool {
    surface.client().is_none_or(|client| !unconfined(&client))
}

/// The rectangle a border surrounds: the geometry, or the tree when it strays beyond a shadow.
fn framed(
    geometry: Rectangle<i32, Logical>,
    tree: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let right = |r: Rectangle<i32, Logical>| r.loc.x.saturating_add(r.size.w);
    let bottom = |r: Rectangle<i32, Logical>| r.loc.y.saturating_add(r.size.h);
    let reach = |side: i32| SHADOW_MARGIN.min(side / 4);
    let (across, down) = (reach(geometry.size.w), reach(geometry.size.h));
    let strays = geometry.loc.x.saturating_sub(tree.loc.x) > across
        || geometry.loc.y.saturating_sub(tree.loc.y) > down
        || right(tree).saturating_sub(right(geometry)) > across
        || bottom(tree).saturating_sub(bottom(geometry)) > down;
    if strays {
        tree.merge(geometry)
    } else {
        geometry
    }
}

fn grown<Kind>(rectangle: Rectangle<i32, Kind>, by: i32) -> Rectangle<i32, Kind> {
    Rectangle::new(
        rectangle.loc - Point::from((by, by)),
        rectangle.size + Size::from((by * 2, by * 2)),
    )
}

/// One ring's corners and the ids of its lines. A window keeps its own, since the damage
/// tracker tells elements apart by id; `key` says what they were made for.
#[derive(Clone)]
struct Ring {
    key: (i32, i32, [f32; 4]),
    corners: Option<[MemoryRenderBuffer; 4]>,
    lines: [Id; 4],
}

/// The rings of a surface: the border's lines and the outline.
#[derive(Default)]
struct Rings(RefCell<[Option<Ring>; OUTLINE_RING + 1]>);

/// How much of pixel (`across`, `down`) a quarter ring `line` thick covers in a square of side
/// `side`, both counted from the arc's centre.
fn coverage(side: i32, line: i32, across: i32, down: i32) -> f32 {
    let (outer, inner) = (side as f32, (side - line) as f32);
    let from = (across as f32 + 0.5).hypot(down as f32 + 0.5);
    (outer - from + 0.5).clamp(0.0, 1.0) * (from - inner + 0.5).clamp(0.0, 1.0)
}

/// The corner of a ring as a premultiplied bitmap: top left, or at the side `right` and `lower`
/// name.
fn corner(side: i32, line: i32, colour: [f32; 4], right: bool, lower: bool) -> MemoryRenderBuffer {
    let [red, green, blue, alpha] = colour;
    let mut bytes = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let across = if right { x } else { side - 1 - x };
            let down = if lower { y } else { side - 1 - y };
            let cover = coverage(side, line, across, down) * alpha;
            let byte = |channel: f32| (channel * cover * 255.0).round() as u8;
            bytes.extend([byte(blue), byte(green), byte(red), byte(1.0)]);
        }
    }
    MemoryRenderBuffer::from_slice(
        &bytes,
        smithay::backend::allocator::Fourcc::Argb8888,
        (side, side),
        1,
        Transform::Normal,
        None,
    )
}

/// The ring `slot` of `surface`, made anew when what it draws changed.
fn kept_ring(surface: &WlSurface, slot: usize, side: i32, line: i32, colour: [f32; 4]) -> Ring {
    let key = (side, line, colour);
    with_states(surface, |states| {
        let rings = states.data_map.get_or_insert(Rings::default);
        let mut rings = rings.0.borrow_mut();
        match &rings[slot] {
            Some(ring) if ring.key == key => ring.clone(),
            _ => {
                let ring = Ring {
                    key,
                    corners: (side > 0).then(|| {
                        [(false, false), (true, false), (false, true), (true, true)]
                            .map(|(right, lower)| corner(side, line, colour, right, lower))
                    }),
                    lines: std::array::from_fn(|_| Id::new()),
                };
                rings[slot] = Some(ring.clone());
                ring
            }
        }
    })
}

/// A ring just inside `rectangle`, `line` pixels thick. The corners of `round` take `radius`,
/// logical, and the others are square. In the order top left, top right, bottom left and
/// bottom right.
#[allow(clippy::too_many_arguments)]
fn ring<R>(
    renderer: &mut R,
    elements: &mut Vec<Stacked<R>>,
    surface: &WlSurface,
    slot: usize,
    rectangle: Rectangle<i32, Physical>,
    line: i32,
    (radius, round): (i32, [bool; 4]),
    scale: f64,
    colour: [f32; 4],
) where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let (x, y, w, h) = (
        rectangle.loc.x,
        rectangle.loc.y,
        rectangle.size.w,
        rectangle.size.h,
    );
    let logical = radius.min(((w.min(h) / 2) as f64 / scale) as i32);
    let side = ((f64::from(logical) * scale).round() as i32).max(0);
    let kept = kept_ring(surface, slot, side, line, colour);
    let inset = round.map(|round| if round { side } else { 0 });
    let [top_left, top_right, bottom_left, bottom_right] = inset;
    let lines = [
        ((x + top_left, y), (w - top_left - top_right, line)),
        (
            (x + bottom_left, y + h - line),
            (w - bottom_left - bottom_right, line),
        ),
        (
            (x, y + top_left.max(line)),
            (line, h - top_left.max(line) - bottom_left.max(line)),
        ),
        (
            (x + w - line, y + top_right.max(line)),
            (line, h - top_right.max(line) - bottom_right.max(line)),
        ),
    ];
    let [red, green, blue, alpha] = colour;
    let solid = Color32F::new(red * alpha, green * alpha, blue * alpha, alpha);
    for (id, (at, size)) in kept.lines.iter().zip(lines) {
        if size.0 > 0 && size.1 > 0 {
            elements.push(Stacked::Line(SolidColorRenderElement::new(
                id.clone(),
                Rectangle::new(at.into(), size.into()),
                CommitCounter::default(),
                solid,
                Kind::Unspecified,
            )));
        }
    }
    let Some(corners) = &kept.corners else {
        return;
    };
    let places = [
        (x, y),
        (x + w - side, y),
        (x, y + h - side),
        (x + w - side, y + h - side),
    ];
    for ((buffer, place), round) in corners.iter().zip(places).zip(round) {
        if !round {
            continue;
        }
        let drawn = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((f64::from(place.0), f64::from(place.1))),
            buffer,
            None,
            None,
            Some((logical, logical).into()),
            Kind::Unspecified,
        );
        match drawn {
            Ok(corner) => elements.push(Stacked::Corner(corner)),
            Err(error) => tracing::warn!(?error, "a mark's corner could not be imported"),
        }
    }
}

/// A ring `thickness` logical pixels wide just inside a rectangle of `size` logical pixels,
/// drawn on `pixels` of the output at `scale`, with outer corners of `radius`, top left, top
/// right, bottom right and bottom left: the ring cosmic-comp draws as the outline and as the divider.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Indicator {
    size: Size<i32, Logical>,
    pixels: Size<i32, Physical>,
    thickness: i32,
    radius: [i32; 4],
    scale: f64,
}

/// A rectangle with rounded corners, in physical pixels: its left, top, right and bottom sides,
/// and the radius of each corner in the order of `Indicator::radius`.
#[derive(Clone, Copy)]
struct Rounded {
    sides: [f64; 4],
    radius: [f64; 4],
}

impl Rounded {
    /// The radius of `corner`, no more than half the shorter side holds.
    fn radius(&self, corner: usize) -> f64 {
        let [left, top, right, bottom] = self.sides;
        let half = ((right - left).min(bottom - top) / 2.0).max(0.0);
        self.radius[corner].clamp(0.0, half)
    }

    /// The signed distance from (`x`, `y`) to the boundary, negative inside. A point lies
    /// `past_x` beyond the nearer of the left and right sides and `past_y` beyond the nearer of
    /// the top and bottom, both negative within them. Within the radius of both sides it is in a
    /// corner's square, where the boundary is the corner's circle, centred `radius` in from both
    /// sides; anywhere else the boundary nearest to it is the nearer side.
    fn distance(&self, x: f64, y: f64) -> f64 {
        let [left, top, right, bottom] = self.sides;
        let corner = match (x < (left + right) / 2.0, y < (top + bottom) / 2.0) {
            (true, true) => 0,
            (false, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
        };
        let radius = self.radius(corner);
        let past_x = (left - x).max(x - right);
        let past_y = (top - y).max(y - bottom);
        let (from_x, from_y) = (past_x + radius, past_y + radius);
        if from_x > 0.0 && from_y > 0.0 {
            from_x.hypot(from_y) - radius
        } else {
            past_x.max(past_y)
        }
    }

    /// How much of the pixel centred at (`x`, `y`) the shape covers: the share of the pixel a
    /// straight boundary at that distance leaves inside, so the band between covered and not is
    /// one pixel wide and centred on the boundary.
    fn covers(&self, x: f64, y: f64) -> f64 {
        (0.5 - self.distance(x, y)).clamp(0.0, 1.0)
    }
}

/// A rectangle of an indicator's pixels, from its top left pixel.
type Cell = Rectangle<i32, Physical>;

/// The runs of `range` where `covered` holds, each as its first and one past its last.
fn runs(range: std::ops::Range<i32>, covered: impl Fn(i32) -> bool) -> Vec<(i32, i32)> {
    let mut runs: Vec<(i32, i32)> = Vec::new();
    for at in range.filter(|&at| covered(at)) {
        match runs.last_mut() {
            Some((_, end)) if *end == at => *end = at + 1,
            _ => runs.push((at, at + 1)),
        }
    }
    runs
}

impl Indicator {
    /// The ring's outer and inner boundaries, in the output's pixels from its top left pixel.
    /// The outer one is the pixels the ring is drawn on. The inner one is inset by the thickness
    /// at the scale, with each corner's radius smaller by as much, and each of its sides on the
    /// nearer line between two pixels; half way, the line above or to the left, as a rasteriser's
    /// top-left rule takes a pixel whose centre falls on an edge. A straight side then covers
    /// whole pixels: a thickness of 4.5 pixels is 4 on the left and the top and 5 on the right and
    /// the bottom. The chooser's golden at scale 1.5 shows the left, the right and the bottom so;
    /// its top is 5 and its divider a row lower, since cosmic-comp places that window at a
    /// fractional logical offset.
    fn boundaries(&self) -> [Rounded; 2] {
        let (w, h) = (f64::from(self.pixels.w), f64::from(self.pixels.h));
        let thickness = f64::from(self.thickness) * self.scale;
        let radius = self.radius.map(|corner| f64::from(corner) * self.scale);
        let line = |at: f64| (at - 0.5).ceil();
        let outer = Rounded {
            sides: [0.0, 0.0, w, h],
            radius,
        };
        let inner = Rounded {
            sides: [
                line(thickness),
                line(thickness),
                line(w - thickness),
                line(h - thickness),
            ],
            radius: radius.map(|corner| (corner - thickness).max(0.0)),
        };
        [outer, inner]
    }

    /// How much of pixel (`x`, `y`) the ring covers: what its outer boundary covers less what its
    /// inner one does.
    fn coverage(&self, x: i32, y: i32) -> f64 {
        let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        let [outer, inner] = self.boundaries();
        (outer.covers(x, y) - inner.covers(x, y)).max(0.0)
    }

    /// How far in from the left, the top, the right and the bottom the ring's corners and its
    /// inner sides reach, in pixels: past them a pixel is covered as its row says, or its column.
    fn reach(&self) -> [f64; 4] {
        let [outer, inner] = self.boundaries();
        let [left, top, right, bottom] = inner.sides;
        let insets = [
            left,
            top,
            f64::from(self.pixels.w) - right,
            f64::from(self.pixels.h) - bottom,
        ];
        let corners = [[0, 3], [0, 1], [1, 2], [2, 3]];
        std::array::from_fn(|side| {
            corners[side].iter().fold(insets[side], |reach, &corner| {
                reach
                    .max(outer.radius(corner))
                    .max(insets[side] + inner.radius(corner))
            })
        })
    }

    /// The ring's pieces in `colour`: its corners' bitmaps and its lines' ids.
    fn made(&self, colour: [f32; 4]) -> Pieces {
        let (cells, lines) = self.pieces();
        Pieces {
            made_for: (*self, colour),
            bitmaps: cells
                .into_iter()
                .map(|cell| (self.bitmap(cell, colour), cell))
                .collect(),
            lines: lines.into_iter().map(|line| (Id::new(), line)).collect(),
        }
    }

    /// The ring in pieces, from its top left pixel: bitmaps at the corners, where a pixel can
    /// be partly covered, and lines along the sides, where a pixel is covered or not. Past the
    /// corners' reach a pixel is covered as its row says, between the left and right corners,
    /// or as its column says, between the top and bottom ones; inside both, not at all.
    fn pieces(&self) -> (Vec<Cell>, Vec<Cell>) {
        let (w, h) = (self.pixels.w, self.pixels.h);
        if w <= 0 || h <= 0 {
            return (Vec::new(), Vec::new());
        }
        let [left, top, right, bottom] = self.reach();
        let centre = |at: i32| f64::from(at) + 0.5;
        let (width, height) = (f64::from(w), f64::from(h));
        let left = (0..w).take_while(|&x| centre(x) < left).count() as i32;
        let right = (0..w)
            .rev()
            .take_while(|&x| centre(x) > width - right)
            .count() as i32;
        let top = (0..h).take_while(|&y| centre(y) < top).count() as i32;
        let bottom = (0..h)
            .rev()
            .take_while(|&y| centre(y) > height - bottom)
            .count() as i32;
        // On a ring too small for its corners, they meet and take every pixel between them.
        let (right, bottom) = (right.min(w - left), bottom.min(h - top));
        let (middle_w, middle_h) = (w - left - right, h - top - bottom);
        let cells = [
            ((0, 0), (left, top)),
            ((w - right, 0), (right, top)),
            ((0, h - bottom), (left, bottom)),
            ((w - right, h - bottom), (right, bottom)),
        ];
        let bitmaps = cells
            .into_iter()
            .filter(|(_, (cell_w, cell_h))| *cell_w > 0 && *cell_h > 0)
            .map(|(at, size)| Rectangle::new(at.into(), size.into()))
            .collect();
        let mut lines = Vec::new();
        if middle_w > 0 {
            for rows in [0..top, h - bottom..h] {
                for (from, to) in runs(rows, |y| self.coverage(left, y) >= 1.0) {
                    lines.push(Rectangle::new((left, from).into(), (middle_w, to - from).into()));
                }
            }
        }
        if middle_h > 0 {
            for columns in [0..left, w - right..w] {
                for (from, to) in runs(columns, |x| self.coverage(x, top) >= 1.0) {
                    lines.push(Rectangle::new((from, top).into(), (to - from, middle_h).into()));
                }
            }
        }
        (bitmaps, lines)
    }

    /// `cell` of the ring in `colour`, straight, as a premultiplied bitmap.
    fn bitmap(&self, cell: Rectangle<i32, Physical>, colour: [f32; 4]) -> MemoryRenderBuffer {
        let [red, green, blue, alpha] = colour;
        let mut bytes = Vec::with_capacity((cell.size.w * cell.size.h * 4) as usize);
        for y in cell.loc.y..cell.loc.y + cell.size.h {
            for x in cell.loc.x..cell.loc.x + cell.size.w {
                let cover = self.coverage(x, y) as f32 * alpha;
                let byte = |channel: f32| (channel * cover * 255.0).round() as u8;
                bytes.extend([byte(blue), byte(green), byte(red), byte(1.0)]);
            }
        }
        MemoryRenderBuffer::from_slice(
            &bytes,
            smithay::backend::allocator::Fourcc::Argb8888,
            (cell.size.w, cell.size.h),
            1,
            Transform::Normal,
            None,
        )
    }
}

/// An indicator's pieces as last made, from its top left pixel. A window keeps its own, since
/// the damage tracker tells elements apart by id; `made_for` says what they were made for.
#[derive(Clone)]
struct Pieces {
    made_for: (Indicator, [f32; 4]),
    bitmaps: Vec<(MemoryRenderBuffer, Rectangle<i32, Physical>)>,
    lines: Vec<(Id, Rectangle<i32, Physical>)>,
}

/// The pieces of a surface's outline and divider, one of each for every scale it is drawn at, so
/// that a window across outputs of two scales does not make them anew at every frame. Past
/// `SCALES` scales the oldest is dropped.
#[derive(Default)]
struct Indicators(RefCell<Vec<(usize, Pieces)>>);

/// The scales a surface keeps its indicators at: more than the outputs it spans at once.
const SCALES: usize = 4;

impl Indicators {
    /// The indicator `slot` in `shape` and `colour`, made anew when what it draws at that scale
    /// changed.
    fn pieces(&self, slot: usize, shape: Indicator, colour: [f32; 4]) -> Pieces {
        let made_for = (shape, colour);
        let mut kept = self.0.borrow_mut();
        let at_scale = |(kind, pieces): &&mut (usize, Pieces)| {
            *kind == slot && pieces.made_for.0.scale == shape.scale
        };
        let found = kept.iter_mut().find(at_scale);
        match found {
            Some((_, pieces)) if pieces.made_for == made_for => pieces.clone(),
            Some((_, pieces)) => {
                *pieces = shape.made(colour);
                pieces.clone()
            }
            None => {
                let full = kept.iter().filter(|(kind, _)| *kind == slot).count() >= SCALES;
                let oldest = kept.iter().position(|(kind, _)| *kind == slot);
                if let Some(oldest) = oldest.filter(|_| full) {
                    kept.remove(oldest);
                }
                let pieces = shape.made(colour);
                kept.push((slot, pieces.clone()));
                pieces
            }
        }
    }
}

/// The indicator `slot` of `surface`, made anew when what it draws changed.
fn kept_pieces(surface: &WlSurface, slot: usize, shape: Indicator, colour: [f32; 4]) -> Pieces {
    with_states(surface, |states| {
        states
            .data_map
            .get_or_insert(Indicators::default)
            .pieces(slot, shape, colour)
    })
}

/// The indicator `shape` with its top left pixel at `at`, in `colour`, straight.
fn indicator<R>(
    renderer: &mut R,
    elements: &mut Vec<Stacked<R>>,
    surface: &WlSurface,
    slot: usize,
    (shape, at): (Indicator, Point<i32, Physical>),
    colour: [f32; 4],
) where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let pieces = kept_pieces(surface, slot, shape, colour);
    let [red, green, blue, alpha] = colour;
    let solid = Color32F::new(red * alpha, green * alpha, blue * alpha, alpha);
    for (id, line) in pieces.lines {
        elements.push(Stacked::Line(SolidColorRenderElement::new(
            id,
            Rectangle::new(at + line.loc, line.size),
            CommitCounter::default(),
            solid,
            Kind::Unspecified,
        )));
    }
    for (buffer, cell) in &pieces.bitmaps {
        let loc = at + cell.loc;
        let drawn = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((f64::from(loc.x), f64::from(loc.y))),
            buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        );
        match drawn {
            Ok(inner) => elements.push(Stacked::Pixels(Pixels {
                inner,
                geometry: Rectangle::new(loc, cell.size),
            })),
            Err(error) => tracing::warn!(?error, "a mark's corner could not be imported"),
        }
    }
}

/// Where one output draws: its place in the space, and what a border is cut to.
struct Place {
    origin: Point<i32, Logical>,
    scale: f64,
    /// The whole output and its window area, both in the space.
    output: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
}

impl Place {
    fn physical(&self, rectangle: Rectangle<i32, Logical>) -> Rectangle<i32, Physical> {
        let near = |at: Point<i32, Logical>| (at.x.clamp(-FAR, FAR), at.y.clamp(-FAR, FAR));
        let start = rectangle.loc - self.origin;
        let end = start + Point::from((rectangle.size.w, rectangle.size.h));
        Rectangle::<i32, Logical>::from_extremities(near(start), near(end))
            .to_physical_precise_round(Scale::from(self.scale))
    }

    fn pixels(&self, logical: i32) -> i32 {
        ((f64::from(logical) * self.scale).round() as i32).max(1)
    }

    /// The ring cosmic-comp draws over `rectangle`, and the output's pixel of its top left corner.
    fn indicator(
        &self,
        rectangle: Rectangle<i32, Logical>,
        thickness: i32,
        radius: [i32; 4],
    ) -> (Indicator, Point<i32, Physical>) {
        let shown = self.physical(rectangle);
        let shape = Indicator {
            size: rectangle.size,
            pixels: shown.size,
            thickness,
            radius,
            scale: self.scale,
        };
        (shape, shown.loc)
    }
}

/// The border around `frame`, cut to `cut`: outside the frame where it fits and over the
/// client's content where it does not, with square corners where the cut falls.
fn border<R>(
    renderer: &mut R,
    elements: &mut Vec<Stacked<R>>,
    surface: &WlSurface,
    frame: Rectangle<i32, Logical>,
    cut: Rectangle<i32, Logical>,
    place: &Place,
) where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let line = place.pixels(1);
    let around = grown(place.physical(frame), BORDER_WIDTH * line);
    let Some(shown) = around.intersection(place.physical(cut)) else {
        return;
    };
    let (left, top) = (shown.loc.x == around.loc.x, shown.loc.y == around.loc.y);
    let right = shown.size.w + shown.loc.x == around.size.w + around.loc.x;
    let bottom = shown.size.h + shown.loc.y == around.size.h + around.loc.y;
    let round = [left && top, right && top, left && bottom, right && bottom];
    let mut rectangle = shown;
    let mut radius = WINDOW_RADIUS + BORDER_WIDTH;
    for (slot, colour) in BORDER.into_iter().enumerate() {
        let scale = place.scale;
        let rounded = (radius, round);
        ring(
            renderer, elements, surface, slot, rectangle, line, rounded, scale, colour,
        );
        rectangle = Rectangle::new(
            rectangle.loc + Point::from((line, line)),
            (
                (rectangle.size.w - line * 2).max(0),
                (rectangle.size.h - line * 2).max(0),
            )
                .into(),
        );
        radius -= 1;
    }
}

fn surface_tree<R>(
    renderer: &mut R,
    surface: &WlSurface,
    at: Point<i32, Logical>,
    place: &Place,
) -> Vec<Stacked<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    surface_elements::<_, Stacked<R>>(
        renderer,
        surface,
        (at - place.origin).to_physical_precise_round(place.scale),
        place.scale,
        1.0,
        Kind::Unspecified,
    )
}

/// `window` as it stands in the stack, front to back: its popups, each with a border when its
/// client is confined, the window's own marks, and its surface tree.
fn window_elements<R>(
    renderer: &mut R,
    elements: &mut Vec<Stacked<R>>,
    window: &Window,
    located: Rectangle<i32, Logical>,
    marks: &Marks,
    place: &Place,
) where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let Some(toplevel) = window.toplevel() else {
        return;
    };
    let surface = toplevel.wl_surface();
    let offset = window.geometry().loc;
    let drawn_at = located.loc - offset;
    let (full_screen, maximised) = toplevel.with_committed_state(|state| {
        state.map_or((false, false), |state| {
            (
                state.states.contains(xdg_toplevel::State::Fullscreen),
                state.states.contains(xdg_toplevel::State::Maximized),
            )
        })
    });
    let cut = if full_screen {
        place.output
    } else {
        place.area
    };
    let is_confined = confined(surface);
    for (popup, at) in PopupManager::popups_for_surface(surface) {
        let geometry = popup.geometry();
        let popup_surface = popup.wl_surface();
        let corner = located.loc + at;
        let tree_at = corner - geometry.loc;
        if is_confined || confined(popup_surface) {
            let tree = bbox_from_surface_tree(popup_surface, tree_at);
            // A popup with nothing mapped shows nothing to border.
            if !tree.is_empty() {
                // As for a window: a client's geometry reaches no further than what it paints.
                let declared = Rectangle::new(corner, geometry.size);
                let frame = framed(declared.intersection(tree).unwrap_or(tree), tree);
                // Past the window area too, where a client can place a popup.
                border(
                    renderer,
                    elements,
                    popup_surface,
                    frame,
                    place.output,
                    place,
                );
            }
        }
        elements.extend(surface_tree(renderer, popup_surface, tree_at, place));
    }
    let tree = Rectangle::new(window.bbox().loc + drawn_at, window.bbox().size);
    let frame = if is_confined {
        framed(located, tree)
    } else {
        located
    };
    if is_confined {
        border(renderer, elements, surface, frame, cut, place);
    }
    let focused = marks.focused.as_ref() == Some(window);
    let marked = !full_screen && !maximised;
    if let Some(outline) = marks.outline.filter(|_| focused && marked) {
        if is_confined {
            // Outside the border, in whole pixels (decision 31).
            ring(
                renderer,
                elements,
                surface,
                OUTLINE_RING,
                grown(
                    place.physical(frame),
                    BORDER_WIDTH * place.pixels(1) + place.pixels(outline.width),
                ),
                place.pixels(outline.width),
                (WINDOW_RADIUS + BORDER_WIDTH + outline.width, [true; 4]),
                place.scale,
                outline.colour,
            );
        } else {
            // cosmic-comp 1.11.0 draws a focused window's outline over the geometry grown by the
            // width, with corners concentric with the window's, in the hint's colour without its
            // alpha.
            let width = outline.width;
            let radius = marks.edge.radius.map(|corner| corner + width);
            let shape = place.indicator(grown(located, width), width, radius);
            let [red, green, blue, _] = outline.colour;
            indicator(renderer, elements, surface, OUTLINE, shape, [red, green, blue, 1.0]);
        }
    }
    if marked {
        // cosmic-comp 1.11.0 draws its divider as 1 px of `bg_divider` just inside the geometry
        // of every window neither maximised nor full-screen.
        let shape = place.indicator(located, 1, marks.edge.radius);
        indicator(renderer, elements, surface, DIVIDER, shape, marks.edge.divider);
    }
    elements.extend(surface_tree(renderer, surface, drawn_at, place));
    if full_screen && full_screen_area(window) == Some(place.output) {
        // A client smaller than its output: nothing of the session shows below it.
        let id = with_states(surface, |states| {
            let backdrop = states.data_map.get_or_insert(|| Backdrop(Id::new()));
            backdrop.0.clone()
        });
        elements.push(Stacked::Line(SolidColorRenderElement::new(
            id,
            place.physical(place.output),
            CommitCounter::default(),
            Color32F::new(0.0, 0.0, 0.0, 1.0),
            Kind::Unspecified,
        )));
    }
}

/// Whether `window` has the output showing `area` whole and its client has answered a request
/// for it, this one or an earlier one. A null buffer takes the answer back.
pub fn whole_on(window: &Window, area: Rectangle<i32, Logical>) -> bool {
    full_screen_area(window) == Some(area)
        && window.toplevel().is_some_and(|toplevel| {
            toplevel.with_committed_state(|state| {
                state.is_some_and(|state| state.states.contains(xdg_toplevel::State::Fullscreen))
            })
        })
}

/// The focused window, when `output` is the one it has whole and nothing stands above it there:
/// it stands above the top layer, on no other output it reaches, and a dialog of its own stays
/// above it, with the window below the top layer while it does.
pub fn lifted<'a>(
    focused: Option<&'a Window>,
    space: &Space<Window>,
    output: &Output,
) -> Option<&'a Window> {
    let area = space.output_geometry(output)?;
    let top = space
        .elements()
        .rev()
        .find(|window| space.element_bbox(window).is_some_and(|bbox| area.overlaps(bbox)));
    focused.filter(|window| whole_on(window, area) && top == Some(*window))
}

/// The layer surfaces and the windows of `output`, front to back, with the windows' marks.
pub fn stack<R>(
    renderer: &mut R,
    space: &Space<Window>,
    output: &Output,
    marks: &Marks,
) -> Vec<Stacked<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let scale = output.current_scale().fractional_scale();
    let mut elements = Vec::new();
    let map = layer_map_for_output(output);
    let (lower, upper): (Vec<&LayerSurface>, Vec<&LayerSurface>) = map
        .layers()
        .rev()
        .partition(|layer| matches!(layer.layer(), Layer::Background | Layer::Bottom));
    let layers = |renderer: &mut R, layers: Vec<&LayerSurface>, elements: &mut Vec<Stacked<R>>| {
        for layer in layers {
            if let Some(geometry) = map.layer_geometry(layer) {
                // A layer surface's popups stand above it.
                let at = geometry.loc.to_physical_precise_round(scale);
                let surface = layer.wl_surface();
                let tree = |renderer: &mut R, surface: &WlSurface, at: Point<i32, Physical>| {
                    surface_elements::<_, Stacked<R>>(
                        renderer,
                        surface,
                        at,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                };
                for (popup, offset) in PopupManager::popups_for_surface(surface) {
                    let offset = (offset - popup.geometry().loc).to_physical_precise_round(scale);
                    elements.extend(tree(renderer, popup.wl_surface(), at + offset));
                }
                elements.extend(tree(renderer, surface, at));
            }
        }
    };
    let (overlay, top): (Vec<&LayerSurface>, Vec<&LayerSurface>) = upper
        .into_iter()
        .partition(|layer| layer.layer() == Layer::Overlay);
    let lifted = lifted(marks.focused.as_ref(), space, output);
    let places = space.output_geometry(output).map(|geometry| {
        // As `State::window_area_on`, which would take the map's lock again.
        let area = laid_out(output).map(|size| inset(geometry, size, map.non_exclusive_zone()));
        Place {
            origin: geometry.loc,
            scale,
            output: geometry,
            area: area.unwrap_or(geometry),
        }
    });
    layers(renderer, overlay, &mut elements);
    if let Some((window, place)) = lifted.zip(places.as_ref()) {
        if let Some(located) = space.element_geometry(window) {
            window_elements(renderer, &mut elements, window, located, marks, place);
        }
    }
    layers(renderer, top, &mut elements);
    if let Some(place) = &places {
        for window in space.elements().rev().filter(|window| Some(*window) != lifted) {
            let overlaps = space
                .element_bbox(window)
                .is_some_and(|bbox| place.output.overlaps(bbox));
            if let Some(located) = space.element_geometry(window).filter(|_| overlaps) {
                window_elements(renderer, &mut elements, window, located, marks, place);
            }
        }
    }
    layers(renderer, lower, &mut elements);
    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    /// A shadow stays outside the frame; a tree past the margin, or past a quarter of a small
    /// geometry, takes it.
    #[test]
    fn the_frame_is_the_geometry_until_the_tree_strays() {
        let geometry = rectangle(100, 100, 400, 400);
        let shadow = rectangle(36, 36, 528, 528);
        assert_eq!(framed(geometry, shadow), geometry);
        let tree = rectangle(35, 100, 465, 400);
        assert_eq!(framed(geometry, tree), tree);
        let sliver = rectangle(100, 100, 1, 1);
        let around = rectangle(36, 36, 129, 129);
        assert_eq!(framed(sliver, around), around);
    }

    /// A rectangle at the far end of the coordinates comes back within them, sums included.
    #[test]
    fn a_far_rectangle_stays_in_range() {
        let origin = Rectangle::<i32, Logical>::from_size((800, 600).into());
        let place = Place {
            origin: (1280, 0).into(),
            scale: 1.5,
            output: origin,
            area: origin,
        };
        let far = place.physical(rectangle(i32::MAX - 400, i32::MIN, i32::MAX, i32::MAX));
        assert!(far.loc.x.checked_add(far.size.w).is_some());
        assert!(far.loc.y.checked_add(far.size.h).is_some());
    }

    /// The goldens' chooser at scale 1.5 (`chooser-dark-1.5-en`): a window of 420 by 297 at
    /// (216, 66), with Calmo's radius of 12, grown by `grow`.
    fn chooser(thickness: i32, grow: i32) -> (Indicator, Point<i32, Physical>) {
        let output = Rectangle::<i32, Logical>::from_size((853, 533).into());
        let place = Place {
            origin: (0, 0).into(),
            scale: 1.5,
            output,
            area: output,
        };
        let window = grown(rectangle(216, 66, 420, 297), grow);
        place.indicator(window, thickness, [12 + grow; 4])
    }

    /// How many pixels of row `y` the ring covers whole from the left, and from the right.
    fn sides(shape: &Indicator, y: i32) -> (i32, i32) {
        let covered = |x| shape.coverage(x, y) >= 1.0;
        let left = (0..shape.pixels.w).take_while(|&x| covered(x)).count() as i32;
        let right = (0..shape.pixels.w).rev().take_while(|&x| covered(x)).count() as i32;
        (left, right)
    }

    /// At scale 1.5 cosmic-comp's outline of 3 is 4 px on the left and 5 on the right, and its
    /// divider 1 and 2, as the golden shows them: columns 320 to 323 and 954 to 958, and 324 and
    /// 952 to 953.
    #[test]
    fn the_indicators_match_the_golden_at_scale_one_and_a_half() {
        let (outline, at) = chooser(3, 3);
        assert_eq!((at.x, outline.pixels.w), (320, 639));
        assert_eq!(sides(&outline, outline.pixels.h / 2), (4, 5));
        let (divider, at) = chooser(1, 0);
        assert_eq!((at.x, divider.pixels.w), (324, 630));
        assert_eq!(sides(&divider, divider.pixels.h / 2), (1, 2));
    }

    /// How many pixels of column `x` the ring covers whole from the top, and from the bottom.
    fn rows(shape: &Indicator, x: i32) -> (i32, i32) {
        let covered = |y| shape.coverage(x, y) >= 1.0;
        let top = (0..shape.pixels.h).take_while(|&y| covered(y)).count() as i32;
        let bottom = (0..shape.pixels.h).rev().take_while(|&y| covered(y)).count() as i32;
        (top, bottom)
    }

    /// The top and the bottom follow the rule of the left and the right: at scale 1.5 the
    /// outline of 3 is 4 px at the top and 5 at the bottom, and the divider 1 and 2, on the rows
    /// of a window at a whole logical offset.
    #[test]
    fn the_indicators_rows_follow_their_columns() {
        let (outline, at) = chooser(3, 3);
        assert_eq!((at.y, outline.pixels.h), (95, 455));
        assert_eq!(rows(&outline, outline.pixels.w / 2), (4, 5));
        let (divider, at) = chooser(1, 0);
        assert_eq!((at.y, divider.pixels.h), (99, 446));
        assert_eq!(rows(&divider, divider.pixels.w / 2), (1, 2));
    }

    /// A window across outputs of two scales keeps the pieces of both: drawn at one scale and
    /// then at the other, frame after frame, it makes nothing anew.
    #[test]
    fn the_pieces_are_kept_for_each_scale() {
        let kept = Indicators::default();
        let colour = [1.0, 0.0, 0.0, 1.0];
        let shape = |scale: f64| Indicator {
            size: (420, 297).into(),
            pixels: Size::<i32, Logical>::from((420, 297))
                .to_f64()
                .to_physical(scale)
                .to_i32_round(),
            thickness: 1,
            radius: [12; 4],
            scale,
        };
        let ids = |pieces: Pieces| pieces.lines.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        let one = ids(kept.pieces(DIVIDER, shape(1.0), colour));
        let other = ids(kept.pieces(DIVIDER, shape(1.5), colour));
        assert_ne!(one, other);
        for _ in 0..2 {
            assert_eq!(ids(kept.pieces(DIVIDER, shape(1.0), colour)), one);
            assert_eq!(ids(kept.pieces(DIVIDER, shape(1.5), colour)), other);
        }
        let outline = ids(kept.pieces(OUTLINE, shape(1.0), colour));
        assert_eq!(ids(kept.pieces(OUTLINE, shape(1.0), colour)), outline);
        assert_eq!(ids(kept.pieces(DIVIDER, shape(1.0), colour)), one);
        for scale in [1.25, 1.75, 2.0] {
            kept.pieces(DIVIDER, shape(scale), colour);
        }
        let again = ids(kept.pieces(DIVIDER, shape(1.0), colour));
        assert_ne!(again, one, "past four scales the oldest is dropped");
        assert_eq!(ids(kept.pieces(OUTLINE, shape(1.0), colour)), outline);
    }

    /// At scale 1 the outline is as wide as its width on every side, and its corner is round:
    /// the corner pixel is empty, the arc's middle covered, and the inside empty.
    #[test]
    fn an_indicator_is_a_rounded_ring() {
        let shape = Indicator {
            size: (426, 303).into(),
            pixels: (426, 303).into(),
            thickness: 3,
            radius: [15; 4],
            scale: 1.0,
        };
        assert_eq!(sides(&shape, 150), (3, 3));
        let column = |x| (0..303).filter(|&y| shape.coverage(x, y) >= 1.0).count();
        assert_eq!(column(200), 6, "3 at the top and 3 at the bottom");
        assert_eq!(shape.coverage(0, 0), 0.0, "outside the corner");
        // The arc's middle, 13.5 from the corner's centre along the diagonal.
        let on = (15.0 - 13.5 / std::f64::consts::SQRT_2) as i32;
        assert!(shape.coverage(on, on) > 0.5, "on the arc");
        assert_eq!(shape.coverage(20, 20), 0.0, "inside the ring");
        assert_eq!(shape.coverage(200, 150), 0.0, "inside the window");
    }

    /// The pieces draw every pixel of the ring once: a line's pixels are covered whole, and a
    /// pixel outside every piece is not covered at all. A ring smaller than its corners is all
    /// corners.
    #[test]
    fn the_pieces_cover_the_ring_once() {
        let sizes = [((426, 303), 1.0), ((420, 297), 1.5), ((10, 300), 1.5), ((6, 6), 1.0)];
        for (size, scale) in sizes {
            let pixels = Size::<i32, Logical>::from(size)
                .to_f64()
                .to_physical(scale)
                .to_i32_round();
            let shape = Indicator {
                size: size.into(),
                pixels,
                thickness: 3,
                radius: [15, 15, 0, 15],
                scale,
            };
            let (cells, lines) = shape.pieces();
            let mut seen = vec![0u8; (pixels.w * pixels.h) as usize];
            for piece in cells.iter().chain(&lines) {
                for y in piece.loc.y..piece.loc.y + piece.size.h {
                    for x in piece.loc.x..piece.loc.x + piece.size.w {
                        seen[(y * pixels.w + x) as usize] += 1;
                    }
                }
            }
            for line in &lines {
                for y in line.loc.y..line.loc.y + line.size.h {
                    for x in line.loc.x..line.loc.x + line.size.w {
                        assert!(shape.coverage(x, y) >= 1.0, "{size:?}: line at {x}, {y}");
                    }
                }
            }
            for y in 0..pixels.h {
                for x in 0..pixels.w {
                    let times = seen[(y * pixels.w + x) as usize];
                    assert!(times <= 1, "{size:?}: {x}, {y} drawn twice");
                    if times == 0 {
                        assert_eq!(shape.coverage(x, y), 0.0, "{size:?}: {x}, {y} left out");
                    }
                }
            }
        }
        let tiny = Indicator {
            size: (6, 6).into(),
            pixels: (6, 6).into(),
            thickness: 3,
            radius: [15; 4],
            scale: 1.0,
        };
        assert!(tiny.pieces().1.is_empty(), "no line between corners that meet");
    }

    /// The bitmap's outer corner is empty, its arc is drawn, and its inner corner is empty.
    #[test]
    fn a_corner_is_a_quarter_ring() {
        let cover = |x, y| coverage(16, 2, x, y);
        assert_eq!(cover(0, 0), 0.0, "outside the arc");
        assert!(cover(15, 0) > 0.9, "on the arc, at the top");
        assert!(cover(0, 15) > 0.9, "on the arc, at the left");
        assert_eq!(cover(15, 15), 0.0, "inside the ring");
    }
}
