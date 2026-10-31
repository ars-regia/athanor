//! The focus outline and the border of confined windows (doc_own_stack.md decision 31). A mark is
//! stacked with its window, under whatever stands above it, so `stack` builds what Smithay's
//! `space_render_elements` does with the marks between the windows, for `draw_view` and for the
//! DRM path alike. Corners are small bitmaps, not a shader, because the renderers include Pixman.

use crate::focus_theme::Outline;
use crate::state::{full_screen_area, inset, laid_out, unconfined};
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{AsRenderElements, Id, Kind};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{Color32F, ImportAll, ImportMem, Renderer, Texture};
use smithay::desktop::utils::bbox_from_surface_tree;
use smithay::desktop::{layer_map_for_output, LayerSurface, PopupManager, Space, Window};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform};
use smithay::wayland::compositor::with_states;
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
/// The ring of the focus outline, after the border's in a surface's `Rings`.
const OUTLINE_RING: usize = BORDER.len();

smithay::backend::renderer::element::render_elements! {
    pub Stacked<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Line=SolidColorRenderElement,
    Corner=MemoryRenderBufferRenderElement<R>,
}

/// The black behind a full-screen window, whose element is `surface`'s for its whole life.
struct Backdrop(Id);

/// What the marks of one frame need from the session, read before the backend is borrowed.
pub struct Marks {
    pub focused: Option<Window>,
    /// `None` when the setting is off.
    pub outline: Option<Outline>,
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
    render_elements_from_surface_tree::<_, Stacked<R>>(
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
    if let Some(outline) = marks
        .outline
        .filter(|_| focused && !full_screen && !maximised)
    {
        let inside = if is_confined { BORDER_WIDTH } else { 0 };
        ring(
            renderer,
            elements,
            surface,
            OUTLINE_RING,
            grown(
                place.physical(frame),
                inside * place.pixels(1) + place.pixels(outline.width),
            ),
            place.pixels(outline.width),
            (WINDOW_RADIUS + inside + outline.width, [true; 4]),
            place.scale,
            outline.colour,
        );
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

/// The focused window, when `output` is the one it has whole and its client has answered: it
/// stands above the top layer there, and on no other output it reaches.
pub fn lifted<'a>(
    focused: Option<&'a Window>,
    space: &Space<Window>,
    output: &Output,
) -> Option<&'a Window> {
    let shown = space.output_geometry(output)?;
    focused.filter(|window| {
        full_screen_area(window) == Some(shown)
            && window.toplevel().is_some_and(|toplevel| {
                toplevel.with_committed_state(|state| {
                    state.is_some_and(|state| {
                        state.states.contains(xdg_toplevel::State::Fullscreen)
                    })
                })
            })
            && space.element_location(window).is_some()
    })
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
                elements.extend(AsRenderElements::<R>::render_elements::<Stacked<R>>(
                    layer,
                    renderer,
                    geometry.loc.to_physical_precise_round(scale),
                    Scale::from(scale),
                    1.0,
                ));
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
