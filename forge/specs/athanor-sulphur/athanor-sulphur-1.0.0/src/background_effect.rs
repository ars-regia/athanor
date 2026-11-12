//! ext_background_effect_manager_v1 (doc_own_stack.md decision 35, `standard`): on an output of
//! tier M2 (`material.rs`), what lies under a surface's blur region, windows included, is
//! blurred with `material.blur` and `material.saturate` (VL13, `blur.rs`) and drawn under the
//! surface. On M0 and M1, software rendering's included, the region is drawn as it is, with
//! nothing under it blurred. The client never reads the blurred pixels: they are drawn into
//! the frame only.
//!
//! - The region is the one the surface's commit applied: Smithay keeps it in the surface's
//!   cached state, so that a synchronised subsurface's waits for its parent's commit, and a
//!   commit held back (`handlers.rs`) applies it only when it is released. Smithay posts the
//!   protocol's errors: a second effect object for one surface, and a region set once the
//!   surface is gone.
//! - The blur is one of Smithay's framebuffer effects: the damage tracker draws again what lies
//!   under it whenever that changes, and only then is it copied out and blurred again. While
//!   nothing under it changes, the frame draws the blur kept from the last copy.
//! - Each blur element keeps its own copy of what lies under it, of the size its box shows on
//!   the frame, blurred there in place, and the Kawase levels, from half that size down. The
//!   damage tracker keeps them for the element (its effects cache), so that overlapping regions
//!   share no pixels, a capture's tracker (`capture.rs`) makes its own and frees them with it,
//!   and a region that goes, or an output that drops below M2, lets them go at the tracker's
//!   next frame. No region, no buffers.
//! - Cost: a region is clipped to its surface and to the output, so that one blur is at most the
//!   output's area, however large the region. A region of more than `MOST_RECTS` rectangles, as
//!   the client gives them or as they come out of its subtractions, is taken whole, its
//!   bounding box, so that the region's shape is worked out from at most `MOST_RECTS`
//!   rectangles, once for each region the client sets, and kept as at most `MOST_RECTS`
//!   rectangles. A frame compares the shape a commit applied with the one it kept by pointer,
//!   and finds each element's surface in a map, so that its cost does not grow with what the
//!   client sends.

use crate::blur::Blur;
use crate::state::State;
use smithay::backend::renderer::element::{Element, Id, RenderElement};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture};
use smithay::backend::renderer::pixman::{PixmanError, PixmanFrame, PixmanRenderer};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{
    Bind, BlitFrame, ContextId, Frame, FrameContext, Renderer, TextureFilter,
};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer as BufferCoords, Logical, Physical, Rectangle, Scale, Size};
use smithay::wayland::background_effect::{BackgroundEffectState, ExtBackgroundEffectHandler};
use smithay::wayland::compositor::{
    add_destruction_hook, with_states, Cacheable, RectangleKind, RegionAttributes,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The most rectangles a region's shape is worked out from, or kept as: past it, the region is
/// its bounding box.
pub const MOST_RECTS: usize = 128;

/// Creates the global.
pub fn global(dh: &DisplayHandle) {
    BackgroundEffectState::new::<State>(dh);
}

/// The surfaces that ever set a region, by their elements' id, until they are destroyed: as
/// many as the clients' surfaces at most. Shared with each frame's marks.
pub type Regions = Arc<HashMap<Id, WlSurface>>;

impl ExtBackgroundEffectHandler for State {
    // The region is pending here, until the surface's commit applies it: its shape goes into a
    // cached state of its own, which the commit applies with Smithay's.
    fn set_blur_region(&mut self, surface: WlSurface, region: RegionAttributes) {
        let shape = Arc::new(shape_of(&region));
        set_pending(&surface, Some(shape));
        let id = Id::from_wayland_resource(&surface);
        if self.blurred.contains_key(&id) {
            return;
        }
        // Once for each surface: it stays in the map, its region set or not, until it goes.
        add_destruction_hook::<State, _>(&surface, |state, surface| {
            Arc::make_mut(&mut state.blurred).remove(&Id::from_wayland_resource(surface));
        });
        Arc::make_mut(&mut self.blurred).insert(id, surface);
    }

    fn unset_blur_region(&mut self, surface: WlSurface) {
        set_pending(&surface, None);
    }
}

/// The shape of a surface's region, as its commits apply it: worked out when the client sets
/// the region, and double-buffered as the region is.
#[derive(Clone, Default)]
struct Applied(Option<Arc<Vec<Rectangle<i32, Logical>>>>);

impl Cacheable for Applied {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

fn set_pending(surface: &WlSurface, shape: Option<Arc<Vec<Rectangle<i32, Logical>>>>) {
    with_states(surface, |states| {
        states.cached_state.get::<Applied>().pending().0 = shape;
    });
}

/// The shape of the region `surface`'s commits applied, if any.
#[cfg(test)]
pub fn applied(surface: &WlSurface) -> Option<Vec<Rectangle<i32, Logical>>> {
    with_states(surface, |states| {
        let mut cached = states.cached_state.get::<Applied>();
        cached.current().0.as_deref().cloned()
    })
}

/// What an output keeps for the blur.
#[derive(Default)]
struct Kept {
    /// Whether the output is on M2.
    live: Cell<bool>,
    shared: Rc<Shared>,
}

/// What an output's blur elements share with it: in tests, what they did.
#[derive(Default)]
struct Shared {
    /// How many times a region was copied out and blurred.
    #[cfg(test)]
    blurs: Cell<u64>,
    /// How many copies are held, by every damage tracker.
    #[cfg(test)]
    held: Cell<usize>,
}

/// A blur element's copy of what lies under it, made in one GL context for one size, and
/// where on the frame it was copied from. A dropped GL texture is freed when its renderer next
/// cleans up.
struct Backdrop {
    context: ContextId<GlesTexture>,
    /// The part of the element's box on the frame.
    at: Rectangle<i32, Physical>,
    /// That part, as the frame's pixels are stored.
    size: Size<i32, Physical>,
    blur: Blur,
    #[cfg(test)]
    shared: Rc<Shared>,
}

#[cfg(test)]
impl Drop for Backdrop {
    fn drop(&mut self) {
        self.shared.held.set(self.shared.held.get() - 1);
    }
}

/// The copy a damage tracker keeps for a blur element: none until it is first copied out, or
/// when it could not be.
struct Copied(RefCell<Option<Backdrop>>);

fn kept(output: &Output) -> &Kept {
    output.user_data().get_or_insert(Kept::default)
}

/// Whether `output` blurs regions, as its tier says: when it does not, its elements go, and
/// their copies with them at the next frame.
pub fn set_live(output: &Output, live: bool) {
    kept(output).live.set(live);
}

/// How many copies of what lies under a region `output`'s damage trackers hold.
#[cfg(test)]
pub fn held(output: &Output) -> usize {
    kept(output).shared.held.get()
}

/// How many times a region on `output` was copied out and blurred.
#[cfg(test)]
pub fn blurs(output: &Output) -> u64 {
    kept(output).shared.blurs.get()
}

/// What a surface's blur element keeps between frames: its id, and the shape it was last
/// drawn with, whose change is a commit of the element.
struct Shape {
    id: Id,
    commit: CommitCounter,
    kept: Option<Arc<Vec<Rectangle<i32, Logical>>>>,
}

/// The bounding box of `rects`, if any.
fn bounding<N: smithay::utils::Coordinate, K>(
    rects: impl IntoIterator<Item = Rectangle<N, K>>,
) -> Option<Rectangle<N, K>> {
    rects.into_iter().reduce(|a, b| a.merge(b))
}

/// The rectangles of `region`, added and subtracted in order, apart from one another: at most
/// `MOST_RECTS`, past which the region is its bounding box.
pub fn shape_of(region: &RegionAttributes) -> Vec<Rectangle<i32, Logical>> {
    let sized = |rect: &Rectangle<i32, Logical>| rect.size.w > 0 && rect.size.h > 0;
    let whole = || {
        bounding(
            region
                .rects
                .iter()
                .filter(|(kind, rect)| matches!(kind, RectangleKind::Add) && sized(rect))
                .map(|(_, rect)| *rect),
        )
        .into_iter()
        .collect()
    };
    if region.rects.len() > MOST_RECTS {
        return whole();
    }
    let mut rects: Vec<Rectangle<i32, Logical>> = Vec::new();
    for (kind, rect) in region.rects.iter().filter(|(_, rect)| sized(rect)) {
        rects = Rectangle::subtract_rects_many(rects, [*rect]);
        if matches!(kind, RectangleKind::Add) {
            rects.push(*rect);
        }
        if rects.len() > MOST_RECTS {
            return whole();
        }
    }
    rects
}

/// A region on an output: its bounding box, and its rectangles relative to it.
type Placed = (Rectangle<i32, Physical>, Vec<Rectangle<i32, Physical>>);

/// `rects`, of a surface whose element is at `geometry`, on the output's pixels: clipped to the
/// surface and to `area`, the output, then made relative to their bounding box, which comes
/// first.
pub fn on_output(
    rects: &[Rectangle<i32, Logical>],
    geometry: Rectangle<i32, Physical>,
    scale: Scale<f64>,
    area: Rectangle<i32, Physical>,
) -> Option<Placed> {
    let clip = geometry.intersection(area)?;
    let mut rects: Vec<Rectangle<i32, Physical>> = rects
        .iter()
        .filter_map(|rect| {
            let mut rect = rect.to_f64().to_physical(scale).to_i32_round();
            rect.loc += geometry.loc;
            rect.intersection(clip)
        })
        .collect();
    let bounds = bounding(rects.iter().copied())?;
    for rect in &mut rects {
        rect.loc -= bounds.loc;
    }
    Some((bounds, rects))
}

/// The blur under a surface's region on one output.
pub struct BlurElement {
    id: Id,
    commit: CommitCounter,
    /// The region's bounding box on the output.
    geometry: Rectangle<i32, Physical>,
    /// The region, relative to `geometry`.
    rects: Vec<Rectangle<i32, Physical>>,
    shared: Rc<Shared>,
}

impl BlurElement {
    /// The blur under `surface`'s region, its element at `geometry` on an output of `area`, if
    /// the surface has a region there.
    fn of(
        surface: &WlSurface,
        geometry: Rectangle<i32, Physical>,
        scale: Scale<f64>,
        area: Rectangle<i32, Physical>,
        shared: &Rc<Shared>,
    ) -> Option<Self> {
        let (id, commit, rects) = with_states(surface, |states| {
            let applied = states.cached_state.get::<Applied>().current().0.clone()?;
            let shape = states.data_map.get_or_insert(|| {
                RefCell::new(Shape {
                    id: Id::new(),
                    commit: CommitCounter::default(),
                    kept: None,
                })
            });
            let mut shape = shape.borrow_mut();
            if !shape
                .kept
                .as_ref()
                .is_some_and(|kept| Arc::ptr_eq(kept, &applied))
            {
                shape.kept = Some(applied.clone());
                shape.commit.increment();
            }
            Some((shape.id.clone(), shape.commit, applied))
        })?;
        let (geometry, rects) = on_output(&rects, geometry, scale, area)?;
        Some(Self {
            id,
            commit,
            geometry,
            rects,
            shared: shared.clone(),
        })
    }

    /// The region within `damage`, both relative to `dst`, where the element is drawn: its
    /// geometry, or as the magnifier scales it.
    fn clip(
        &self,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Vec<Rectangle<i32, Physical>> {
        let (from, to) = (self.geometry.size, dst.size);
        let scaled = |rect: Rectangle<i32, Physical>| {
            if from == to {
                return rect;
            }
            let (sx, sy) = (
                f64::from(to.w) / f64::from(from.w.max(1)),
                f64::from(to.h) / f64::from(from.h.max(1)),
            );
            let x0 = (f64::from(rect.loc.x) * sx).floor() as i32;
            let y0 = (f64::from(rect.loc.y) * sy).floor() as i32;
            let x1 = (f64::from(rect.loc.x + rect.size.w) * sx).ceil() as i32;
            let y1 = (f64::from(rect.loc.y + rect.size.h) * sy).ceil() as i32;
            Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into())
        };
        self.rects
            .iter()
            .map(|rect| scaled(*rect))
            .flat_map(|rect| damage.iter().filter_map(move |d| d.intersection(rect)))
            .collect()
    }
}

/// `elements`, front to back, with the blur under each surface of `regions` whose region shows
/// on `output`, right behind the surface's element; only on M2, and with `regions`.
pub fn behind<R, E>(
    output: &Output,
    regions: Option<&HashMap<Id, WlSurface>>,
    elements: Vec<E>,
) -> Vec<Blurred<R, E>>
where
    R: Blurs,
    E: RenderElement<R>,
{
    let kept = kept(output);
    let area = output
        .current_mode()
        .map(|mode| Rectangle::from_size(output.current_transform().transform_size(mode.size)));
    let (Some(regions), Some(area), true) = (regions, area, kept.live.get()) else {
        return elements.into_iter().map(Blurred::Element).collect();
    };
    if regions.is_empty() {
        return elements.into_iter().map(Blurred::Element).collect();
    }
    let scale = Scale::from(output.current_scale().fractional_scale());
    let mut blurred = Vec::with_capacity(elements.len());
    for element in elements {
        let blur = regions
            .get(element.id())
            .filter(|surface| surface.is_alive())
            .and_then(|surface| {
                BlurElement::of(surface, element.geometry(scale), scale, area, &kept.shared)
            });
        blurred.push(Blurred::Element(element));
        if let Some(blur) = blur {
            blurred.push(Blurred::Blur(Behind(blur, PhantomData)));
        }
    }
    blurred
}

smithay::backend::renderer::element::render_elements! {
    pub Blurred<R, E> where R: Blurs;
    Element=E,
    Blur=Behind<E>,
}

/// The blur behind an element of `E`: of a type that names `E`, so that it can never be `E`,
/// and the macro's `From` for either variant does not conflict with the other's.
pub struct Behind<E>(BlurElement, PhantomData<fn() -> E>);

impl<E> Element for Behind<E> {
    fn id(&self) -> &Id {
        &self.0.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.0.commit
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        let size = self.0.geometry.size;
        Rectangle::from_size(Size::from((f64::from(size.w), f64::from(size.h))))
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.0.geometry
    }

    // No opaque region, though the blur is drawn opaque: what lies under it must be drawn
    // before it is copied out.

    fn is_framebuffer_effect(&self) -> bool {
        true
    }
}

impl<R: Blurs, E> RenderElement<R> for Behind<E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), R::Error> {
        // A region never copied out is drawn as it is.
        let Some(cache) = cache else {
            return Ok(());
        };
        let clip = self.0.clip(dst, damage);
        if clip.is_empty() {
            return Ok(());
        }
        R::draw_blur(frame, &self.0, dst, &clip, cache)
    }

    fn capture_framebuffer(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    ) -> Result<(), R::Error> {
        R::capture(frame, &self.0, dst, cache);
        Ok(())
    }
}

/// A renderer that draws the blur, or with nothing to draw it with draws regions as they are.
pub trait Blurs: Renderer {
    /// Copies what lies under `dst` out of the frame and blurs it, in the copy `cache` keeps
    /// for the element; with none kept when it cannot.
    fn capture(
        frame: &mut Self::Frame<'_, '_>,
        element: &BlurElement,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    );

    /// Draws `clip`, relative to `dst`, of the blur `cache` keeps, if any.
    fn draw_blur(
        frame: &mut Self::Frame<'_, '_>,
        element: &BlurElement,
        dst: Rectangle<i32, Physical>,
        clip: &[Rectangle<i32, Physical>],
        cache: &UserDataMap,
    ) -> Result<(), Self::Error>;
}

/// Whether a failed blur was reported: once is enough, the next fail the same way.
static REPORTED: AtomicBool = AtomicBool::new(false);

impl Blurs for GlesRenderer {
    fn capture(
        frame: &mut GlesFrame<'_, '_>,
        element: &BlurElement,
        dst: Rectangle<i32, Physical>,
        cache: &UserDataMap,
    ) {
        let copied = cache.get_or_insert(|| Copied(RefCell::new(None)));
        let mut copied = copied.0.borrow_mut();
        if let Err(error) = blur_under(frame, &mut copied, &element.shared, dst) {
            // What was kept is of another frame: the region is drawn as it is.
            copied.take();
            if !REPORTED.swap(true, Ordering::Relaxed) {
                tracing::warn!(%error, "a region cannot be blurred: it is drawn as it is");
            }
        }
    }

    fn draw_blur(
        frame: &mut GlesFrame<'_, '_>,
        _element: &BlurElement,
        dst: Rectangle<i32, Physical>,
        clip: &[Rectangle<i32, Physical>],
        cache: &UserDataMap,
    ) -> Result<(), GlesError> {
        let Some(copied) = cache.get::<Copied>() else {
            return Ok(());
        };
        let copied = copied.0.borrow();
        let Some(backdrop) = copied.as_ref() else {
            return Ok(());
        };
        // `clip` is relative to `dst`; the copy covers only the part of it on the frame.
        let offset = backdrop.at.loc - dst.loc;
        let shown = Rectangle::from_size(backdrop.at.size);
        let clip: Vec<Rectangle<i32, Physical>> = clip
            .iter()
            .filter_map(|rect| {
                let mut rect = *rect;
                rect.loc -= offset;
                rect.intersection(shown)
            })
            .collect();
        if clip.is_empty() {
            return Ok(());
        }
        let size = backdrop.size;
        let src = Rectangle::<i32, BufferCoords>::from_size((size.w, size.h).into());
        let transform = frame.transformation();
        frame.render_texture_from_to(
            backdrop.blur.texture(),
            src.to_f64(),
            backdrop.at,
            &clip,
            &clip,
            transform,
            1.0,
            None,
            &[],
        )
    }
}

/// Copies what lies under `dst`, as far as it is on `frame`, out into `copied` and blurs it
/// there; makes the copy first, or again for another context or size. `shared` counts both,
/// in tests.
#[cfg_attr(not(test), allow(unused_variables))]
fn blur_under(
    frame: &mut GlesFrame<'_, '_>,
    copied: &mut Option<Backdrop>,
    shared: &Rc<Shared>,
    dst: Rectangle<i32, Physical>,
) -> Result<(), crate::Error> {
    let area = frame.output_size();
    let at = dst
        .intersection(Rectangle::from_size(area))
        .ok_or("the region lies off the frame")?;
    let transform = frame.transformation();
    // The frame's pixels as they are stored: the output's mode, before its transform.
    let stored = transform.transform_rect_in(at, &area);
    let context = frame.context_id();
    if !copied
        .as_ref()
        .is_some_and(|kept| kept.context == context && kept.size == stored.size)
    {
        copied.take();
        let blur = Blur::new(frame.renderer().as_mut(), stored.size)?;
        #[cfg(test)]
        shared.held.set(shared.held.get() + 1);
        *copied = Some(Backdrop {
            context,
            at,
            size: stored.size,
            blur,
            #[cfg(test)]
            shared: shared.clone(),
        });
    }
    let backdrop = copied.as_mut().ok_or("the region's copy was not made")?;
    backdrop.at = at;
    let whole = Rectangle::from_size(stored.size);
    let mut target = frame.renderer().as_mut().bind(backdrop.blur.backdrop())?;
    // The passes are drawn in the same GL context, which orders them after the copy.
    drop(frame.blit_to(&mut target, stored, whole, TextureFilter::Nearest)?);
    drop(target);
    let rect = Rectangle::<i32, BufferCoords>::from_size((stored.size.w, stored.size.h).into());
    backdrop.blur.in_place(frame.renderer().as_mut(), rect)?;
    #[cfg(test)]
    shared.blurs.set(shared.blurs.get() + 1);
    Ok(())
}

// Software rendering is M0: its outputs are not live, and draw no blur.
impl Blurs for PixmanRenderer {
    fn capture(
        _frame: &mut PixmanFrame<'_, '_>,
        _element: &BlurElement,
        _dst: Rectangle<i32, Physical>,
        _cache: &UserDataMap,
    ) {
    }

    fn draw_blur(
        _frame: &mut PixmanFrame<'_, '_>,
        _element: &BlurElement,
        _dst: Rectangle<i32, Physical>,
        _clip: &[Rectangle<i32, Physical>],
        _cache: &UserDataMap,
    ) -> Result<(), PixmanError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect<K>(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, K> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn region(rects: &[(bool, Rectangle<i32, Logical>)]) -> RegionAttributes {
        RegionAttributes {
            rects: rects
                .iter()
                .map(|(add, rect)| {
                    let kind = if *add {
                        RectangleKind::Add
                    } else {
                        RectangleKind::Subtract
                    };
                    (kind, *rect)
                })
                .collect(),
        }
    }

    fn area(rects: &[Rectangle<i32, Logical>]) -> i32 {
        rects.iter().map(|r| r.size.w * r.size.h).sum()
    }

    /// Rectangles added and subtracted in order come out apart from one another.
    #[test]
    fn d35_a_region_is_its_rectangles_added_and_subtracted_in_order() {
        let shape = shape_of(&region(&[
            (true, rect(0, 0, 100, 100)),
            (true, rect(50, 50, 100, 100)),
            (false, rect(0, 0, 10, 10)),
        ]));
        assert_eq!(area(&shape), 100 * 100 + 100 * 100 - 50 * 50 - 10 * 10);
        for (i, a) in shape.iter().enumerate() {
            for b in &shape[i + 1..] {
                assert!(!a.overlaps(*b), "{a:?} and {b:?} overlap");
            }
        }
        // Subtracted before it is added, a rectangle is whole.
        let shape = shape_of(&region(&[
            (false, rect(0, 0, 10, 10)),
            (true, rect(0, 0, 10, 10)),
        ]));
        assert_eq!(shape, [rect(0, 0, 10, 10)]);
        let empty = shape_of(&region(&[
            (true, rect(0, 0, 0, 10)),
            (true, rect(0, 0, 0, 0)),
        ]));
        assert!(empty.is_empty(), "no area, no region");
    }

    /// Past `MOST_RECTS` rectangles, as given or as they come out, a region is its bounding box
    /// of what it adds.
    #[test]
    fn d35_a_region_of_too_many_rectangles_is_its_bounding_box() {
        let many: Vec<(bool, Rectangle<i32, Logical>)> = (0..=MOST_RECTS as i32)
            .map(|i| (true, rect(i * 2, 0, 1, 1)))
            .collect();
        assert_eq!(
            shape_of(&region(&many)),
            [rect(0, 0, MOST_RECTS as i32 * 2 + 1, 1)]
        );
        // Holes punched in a band break it into more pieces than the bound.
        let mut holes = vec![(true, rect(0, 0, 1000, 3))];
        holes.extend((0..MOST_RECTS as i32 - 1).map(|i| (false, rect(i * 4 + 1, 1, 1, 1))));
        assert!(holes.len() <= MOST_RECTS);
        assert_eq!(shape_of(&region(&holes)), [rect(0, 0, 1000, 3)]);
    }

    /// A region is clipped to its surface and to the output, however large, and is relative to
    /// its bounding box; at a fractional scale, on the output's pixels.
    #[test]
    fn d35_a_region_is_clipped_to_its_surface_and_the_output() {
        let geometry = rect(1200, 100, 200, 100);
        let output = rect(0, 0, 1280, 800);
        let huge = [rect(-1_000_000, -1_000_000, 2_000_000, 2_000_000)];
        let (bounds, rects) =
            on_output(&huge, geometry, Scale::from(1.0), output).expect("on the output");
        assert_eq!(
            bounds,
            rect(1200, 100, 80, 100),
            "the surface within the output"
        );
        assert_eq!(rects, [rect(0, 0, 80, 100)]);
        let (bounds, rects) = on_output(
            &[rect(10, 10, 20, 20), rect(40, 10, 20, 20)],
            rect(100, 100, 300, 150),
            Scale::from(1.5),
            output,
        )
        .expect("on the output");
        assert_eq!(bounds, rect(115, 115, 75, 30));
        assert_eq!(rects, [rect(0, 0, 30, 30), rect(45, 0, 30, 30)]);
        assert!(
            on_output(&huge, rect(2000, 0, 10, 10), Scale::from(1.0), output).is_none(),
            "off the output, no blur"
        );
    }
}
