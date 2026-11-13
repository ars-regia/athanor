//! Decision 35: ext-background-effect-v1 (`background_effect.rs`). The region as a commit
//! applies it, a synchronised subsurface's included, and the protocol's errors; M0, M1 and
//! software rendering drawing the region as it is; and on a render node the blur itself, read
//! from the frame: `ATHANOR_RENDER_NODE=/dev/dri/renderD128 cargo test -- --ignored`.

use super::marks::{clear_the_fixture, frame};
use super::material::{on_a_fake_gpu, output, times, LONG};
use super::wave2a::error_of;
use super::*;
use crate::background_effect::{blurs, held, Blurred, MOST_BLURS};
use crate::material::Tier;
use smithay::backend::renderer::element::Element;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::utils::{Rectangle, Scale};
use wayland_client::protocol::{wl_region, wl_subcompositor, wl_subsurface};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1, ext_background_effect_surface_v1,
};

delegate_noop!(Client: ignore ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1);
delegate_noop!(Client: ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1);

/// Where the window is, on the first output, clear of the fixture's.
const AT: (i32, i32) = (300, 200);
/// The window: its top half black and white stripes two pixels wide, its bottom half black on
/// the left and white on the right.
const PARENT: (i32, i32) = (400, 300);
/// The transparent subsurface over it, where in the window, and its size.
const CHILD_AT: (i32, i32) = (50, 25);
const CHILD: (i32, i32) = (300, 250);
/// What the region leaves out, in the subsurface: its top right corner.
const HOLE: (i32, i32, i32, i32) = (250, 0, 50, 60);

/// The window's pattern at `(x, y)` of the window: 0 or 255.
fn pattern(x: i32, y: i32) -> u8 {
    let white = if y < PARENT.1 / 2 {
        x % 4 >= 2
    } else {
        x >= PARENT.0 / 2
    };
    if white {
        255
    } else {
        0
    }
}

/// A `width` by `height` buffer in `format` of `pixels`, four bytes each.
fn shm_buffer(
    local: &mut Local,
    (width, height): (i32, i32),
    format: wl_shm::Format,
    pixels: &[u8],
) -> wl_buffer::WlBuffer {
    let shm: wl_shm::WlShm = local.bind(1);
    let mut file = unlinked().expect("buffer file");
    file.write_all(pixels).expect("pixels");
    let qh = local.queue.handle();
    let pool = shm.create_pool(file.as_fd(), width * height * 4, &qh, ());
    let buffer = pool.create_buffer(0, width, height, width * 4, format, &qh, ());
    pool.destroy();
    local.files.push(file);
    buffer
}

/// A window with the pattern, and over it a synchronised, transparent subsurface with its
/// effect object, no region set yet.
struct Scene {
    local: Local,
    compositor: wl_compositor::WlCompositor,
    parent: wl_surface::WlSurface,
    child: wl_surface::WlSurface,
    /// The window's buffer and the subsurface's.
    shown: wl_buffer::WlBuffer,
    transparent: wl_buffer::WlBuffer,
    _subsurface: wl_subsurface::WlSubsurface,
    manager: ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
    effect: ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
}

impl Scene {
    fn new(run: &mut Running) -> Self {
        clear_the_fixture(run);
        let mut local = Local::connect(run, None);
        local.toplevel(run);
        let window = window_of(run, local.id.as_ref().expect("an id"));
        run.state.space.map_element(window.clone(), AT, true);
        run.state.focus(&window);
        let compositor: wl_compositor::WlCompositor = local.bind(4);
        let subcompositor: wl_subcompositor::WlSubcompositor = local.bind(1);
        let manager: ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1 = local.bind(1);
        let qh = local.queue.handle();
        let (parent, _) = local.window.clone().expect("a window");
        let pixels: Vec<u8> = (0..PARENT.1)
            .flat_map(|y| (0..PARENT.0).map(move |x| pattern(x, y)))
            .flat_map(|v| [v, v, v, 255])
            .collect();
        let shown = shm_buffer(&mut local, PARENT, wl_shm::Format::Xrgb8888, &pixels);
        let child = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&child, &parent, &qh, ());
        subsurface.set_position(CHILD_AT.0, CHILD_AT.1);
        let clear = vec![0; usize::try_from(CHILD.0 * CHILD.1 * 4).expect("size")];
        let transparent = shm_buffer(&mut local, CHILD, wl_shm::Format::Argb8888, &clear);
        child.attach(Some(&transparent), 0, 0);
        child.commit();
        let effect = manager.get_background_effect(&child, &qh, ());
        parent.attach(Some(&shown), 0, 0);
        parent.damage(0, 0, PARENT.0, PARENT.1);
        parent.commit();
        local.roundtrip(run);
        Self {
            local,
            compositor,
            parent,
            child,
            shown,
            transparent,
            _subsurface: subsurface,
            manager,
            effect,
        }
    }

    /// The region: far larger than the subsurface, less `HOLE`.
    fn region(&self) -> wl_region::WlRegion {
        let region = self
            .compositor
            .create_region(&self.local.queue.handle(), ());
        region.add(-50, -50, 1000, 1000);
        region.subtract(HOLE.0, HOLE.1, HOLE.2, HOLE.3);
        region
    }

    /// Sets the region, or none, on the subsurface and commits it, then its parent.
    fn set(&mut self, run: &mut Running, region: Option<&wl_region::WlRegion>) {
        self.effect.set_blur_region(region);
        self.child.commit();
        self.parent.commit();
        self.local.roundtrip(run);
    }

    /// Attaches the subsurface's buffer again, all of it damaged, and commits it, then its
    /// parent: what is drawn over the blur changes, not what lies under it.
    fn redraw_child(&mut self, run: &mut Running) {
        self.child.attach(Some(&self.transparent), 0, 0);
        self.child.damage(0, 0, CHILD.0, CHILD.1);
        self.child.commit();
        self.parent.commit();
        self.local.roundtrip(run);
    }
}

/// Puts the outputs on the fake GPU `gpu`, whose probes give `tier`: another path for each
/// tier, as the tiers are kept for each GPU.
fn on_gpu(run: &mut Running, gpu: &str, tier: Tier) {
    on_a_fake_gpu(run, LONG, move |_, _, _| Ok(times(tier)));
    if let Some(fake) = run.state.materials.fake.as_mut() {
        fake.gpu = PathBuf::from(gpu);
    }
    run.state.refresh_materials();
}

/// The compositor's side of the surface that set a region, and the region applied to it.
fn applied(run: &Running) -> Option<Vec<Rectangle<i32, smithay::utils::Logical>>> {
    let surface = run
        .state
        .blurred
        .values()
        .next()
        .expect("a surface set a region");
    crate::background_effect::applied(surface)
}

/// The blur elements the first output's frame is drawn with, as Pixman draws it.
fn blur_elements(run: &Running) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    let first = output(run, 0);
    let mut renderer = PixmanRenderer::new().expect("a renderer");
    let marks = run.state.marks();
    let elements = crate::marks::stack(&mut renderer, &run.state.space, &first, &marks);
    crate::background_effect::behind(&first, Some(&*marks.blurred), elements)
        .iter()
        .filter(|element| matches!(element, Blurred::Blur(_)))
        .map(|element| element.geometry(Scale::from(1.0)))
        .collect()
}

/// The frame's value at `(x, y)` of the window, of a grey: its red.
fn shown(frame: &[u8], x: i32, y: i32) -> u8 {
    pixel(frame, OUTPUTS[0].width, AT.0 + x, AT.1 + y)[0]
}

/// Every pixel of the window in `rect`, of the window, as the pattern has it.
fn as_it_is(frame: &[u8], (x0, y0, w, h): (i32, i32, i32, i32)) -> bool {
    (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| shown(frame, x, y) == pattern(x, y)))
}

/// The subsurface's area, of the window.
const UNDER: (i32, i32, i32, i32) = (CHILD_AT.0, CHILD_AT.1, CHILD.0, CHILD.1);

/// The region is double-buffered: set, it waits for the subsurface's commit, and as the
/// subsurface is synchronised, for its parent's; unset, likewise.
#[test]
fn d35_the_region_is_applied_by_the_commit_through_a_synchronised_subsurface() {
    let mut run = Running::start(None);
    let mut scene = Scene::new(&mut run);
    let region = scene.region();
    scene.effect.set_blur_region(Some(&region));
    scene.local.roundtrip(&mut run);
    assert_eq!(applied(&run), None, "pending until a commit");
    scene.child.commit();
    scene.local.roundtrip(&mut run);
    assert_eq!(applied(&run), None, "cached until the parent's commit");
    scene.parent.commit();
    scene.local.roundtrip(&mut run);
    let rects = applied(&run).expect("applied");
    let area: i32 = rects.iter().map(|r| r.size.w * r.size.h).sum();
    assert_eq!(area, 1000 * 1000 - HOLE.2 * HOLE.3, "less the hole");
    scene.effect.set_blur_region(None);
    scene.child.commit();
    scene.local.roundtrip(&mut run);
    assert!(
        applied(&run).is_some(),
        "unset, cached until the parent's commit"
    );
    scene.parent.commit();
    scene.local.roundtrip(&mut run);
    assert_eq!(applied(&run), None, "unset");
    run.stop();
}

/// A second effect object for a surface is the manager's `background_effect_exists`.
#[test]
fn d35_a_second_effect_object_is_a_protocol_error() {
    let mut run = Running::start(None);
    let mut scene = Scene::new(&mut run);
    let qh = scene.local.queue.handle();
    let _second = scene.manager.get_background_effect(&scene.child, &qh, ());
    let error = error_of(&mut run, &mut scene.local);
    assert_eq!(error.object_interface, "ext_background_effect_manager_v1");
    assert_eq!(
        error.code,
        u32::from(ext_background_effect_manager_v1::Error::BackgroundEffectExists)
    );
    run.stop();
}

/// A region set once the surface is gone is the effect object's `surface_destroyed`.
#[test]
fn d35_a_region_set_after_the_surface_is_gone_is_a_protocol_error() {
    let mut run = Running::start(None);
    let mut scene = Scene::new(&mut run);
    let qh = scene.local.queue.handle();
    let lone = scene.compositor.create_surface(&qh, ());
    let effect = scene.manager.get_background_effect(&lone, &qh, ());
    lone.destroy();
    let region = scene.region();
    effect.set_blur_region(Some(&region));
    let error = error_of(&mut run, &mut scene.local);
    assert_eq!(error.object_interface, "ext_background_effect_surface_v1");
    assert_eq!(
        error.code,
        u32::from(ext_background_effect_surface_v1::Error::SurfaceDestroyed)
    );
    run.stop();
}

/// On M0, M1 and in software, the region is drawn as it is and nothing is blurred: no blur
/// element on M0 or M1, whatever the client asks; on an output Pixman draws, even one taken
/// for M2, an element clipped to the subsurface and the output, which draws nothing, with no
/// buffers.
#[test]
fn d35_m0_m1_and_software_draw_the_region_as_it_is() {
    let mut run = Running::start(None);
    let mut scene = Scene::new(&mut run);
    let region = scene.region();
    scene.set(&mut run, Some(&region));
    let first = output(&run, 0);
    assert_eq!(run.state.materials.tier_of(Some(&first)), Tier::M0);
    assert!(blur_elements(&run).is_empty(), "M0: no blur");
    assert!(as_it_is(&frame(&mut run), UNDER), "M0: as it is");

    for (tier, what, gpu) in [
        (Tier::M1, "M1", "/dev/dri/renderD-m1"),
        (Tier::M2, "Pixman taken for M2", "/dev/dri/renderD-m2"),
    ] {
        on_gpu(&mut run, gpu, tier);
        let shown_on = first.clone();
        scene.local.pump(&mut run, what, move |_, state| {
            state.materials.tier_of(Some(&shown_on)) == tier && !state.materials.probing()
        });
        scene.local.roundtrip(&mut run);
        assert_eq!(run.state.materials.tier_of(Some(&first)), tier, "{what}");
        let expected = if tier == Tier::M2 {
            let (x, y) = (AT.0 + CHILD_AT.0, AT.1 + CHILD_AT.1);
            vec![Rectangle::new((x, y).into(), CHILD.into())]
        } else {
            Vec::new()
        };
        assert_eq!(blur_elements(&run), expected, "{what}: the elements");
        assert!(as_it_is(&frame(&mut run), UNDER), "{what}: as it is");
        assert_eq!(held(&first), 0, "{what}: no buffers");
    }
    run.stop();
}

/// Mean and standard deviation of `values`.
fn moments(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, variance.sqrt())
}

/// The standard deviation of the blur, in pixels, read from the step in the window's bottom
/// half: the spread of the step's slope, averaged over rows far from the stripes.
fn softness(frame: &[u8]) -> f64 {
    let (x0, x1) = (UNDER.0 + 10, UNDER.0 + UNDER.2 - 10);
    let rows = PARENT.1 / 2 + 80..UNDER.1 + UNDER.3 - 5;
    let n = f64::from(rows.end - rows.start);
    let profile: Vec<f64> = (x0..x1)
        .map(|x| {
            rows.clone()
                .map(|y| f64::from(shown(frame, x, y)))
                .sum::<f64>()
                / n
        })
        .collect();
    let slope: Vec<(f64, f64)> = profile
        .windows(2)
        .enumerate()
        .map(|(i, pair)| (i as f64 + 0.5, pair[1] - pair[0]))
        .collect();
    let total: f64 = slope.iter().map(|(_, d)| d).sum();
    let mean = slope.iter().map(|(x, d)| x * d).sum::<f64>() / total;
    let variance = slope
        .iter()
        .map(|(x, d)| d * (x - mean).powi(2))
        .sum::<f64>()
        / total;
    variance.sqrt()
}

/// The blur on a render node, read from the frame: none without a region; with one, on M2,
/// the stripes under it lose their contrast to their mean, the step under it is as soft as
/// three passes from half resolution make it, and nothing outside the region changes. It is
/// blurred again only when what lies under it changes, and on M1 the region is as it is again
/// and the buffers are gone.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn d35_the_blur_on_a_render_node() {
    let (mut run, mut scene, first) = on_m2();
    let before = frame(&mut run);
    assert!(as_it_is(&before, UNDER), "no region: as it is");
    assert_eq!(held(&first), 0, "no region: no buffers");

    let region = scene.region();
    scene.set(&mut run, Some(&region));
    let shot = frame(&mut run);
    assert_eq!(held(&first), 1, "the region's copy");
    let blurred = blurs(&first);
    assert!(blurred >= 1, "blurred");

    // Outside the subsurface, and in the hole, as before.
    let unchanged = |(x0, y0, w, h): (i32, i32, i32, i32)| {
        (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| shown(&shot, x, y) == shown(&before, x, y)))
    };
    let (width, height) = (OUTPUTS[0].width, OUTPUTS[0].height);
    assert!(
        unchanged((-AT.0, -AT.1, width, AT.1 + CHILD_AT.1)),
        "above it"
    );
    let below = CHILD_AT.1 + CHILD.1;
    assert!(
        unchanged((-AT.0, below, width, height - AT.1 - below)),
        "below it"
    );
    assert!(
        unchanged((-AT.0, CHILD_AT.1, AT.0 + CHILD_AT.0, CHILD.1)),
        "left of it"
    );
    let right = CHILD_AT.0 + CHILD.0;
    assert!(
        unchanged((right, CHILD_AT.1, width - AT.0 - right, CHILD.1)),
        "right of it"
    );
    let hole = (CHILD_AT.0 + HOLE.0, CHILD_AT.1 + HOLE.1, HOLE.2, HOLE.3);
    assert!(unchanged(hole), "the hole");

    // The stripes, clear of the hole, the step and the edges.
    let stripes: Vec<f64> = (UNDER.1 + 20..PARENT.1 / 2 - 60)
        .flat_map(|y| (UNDER.0 + 20..UNDER.0 + HOLE.0 - 20).map(move |x| (x, y)))
        .map(|(x, y)| f64::from(shown(&shot, x, y)))
        .collect();
    let (mean, deviation) = moments(&stripes);
    let softness = softness(&shot);
    eprintln!("stripes: mean {mean:.2}, deviation {deviation:.2}; step: softness {softness:.2} px");
    assert!((mean - 127.5).abs() < 6.0, "the stripes' mean: {mean}");
    assert!(
        deviation < 4.0,
        "the stripes' contrast: {deviation} of 127.5"
    );
    // Three passes from half resolution, against Gaussians, fit best at a standard deviation
    // of 14 to 17 px (`blur::PASSES`); two passes, or a blur from full resolution, would be
    // half as soft, and four passes twice.
    assert!(
        (12.0..=18.0).contains(&softness),
        "the blur's softness, {softness} px, is not three passes from half resolution"
    );

    // Not again while nothing under it changes, nor for what is drawn over it.
    let _ = frame(&mut run);
    assert_eq!(blurs(&first), blurred, "nothing changed");
    // A surface's damage counts only with a buffer attached.
    scene.redraw_child(&mut run);
    let _ = frame(&mut run);
    assert_eq!(blurs(&first), blurred, "only the surface over it changed");
    scene.parent.attach(Some(&scene.shown), 0, 0);
    scene.parent.damage(0, 0, PARENT.0, PARENT.1);
    scene.parent.commit();
    scene.local.roundtrip(&mut run);
    let _ = frame(&mut run);
    assert!(blurs(&first) > blurred, "what lies under it changed");

    // Unset, the region's copy goes with the next frame.
    scene.set(&mut run, None);
    assert!(as_it_is(&frame(&mut run), UNDER), "unset: as it is");
    assert_eq!(held(&first), 0, "unset: the copy is freed");
    scene.set(&mut run, Some(&region));
    let _ = frame(&mut run);
    assert_eq!(held(&first), 1, "set again: a copy");

    on_gpu(&mut run, "/dev/dri/renderD-m1", Tier::M1);
    scene.local.pump(&mut run, "M1", |_, state| {
        state.materials.tier_of(Some(&first)) == Tier::M1 && !state.materials.probing()
    });
    assert!(as_it_is(&frame(&mut run), UNDER), "M1: as it is");
    assert_eq!(held(&first), 0, "M1: the copy is freed");
    run.stop();
}

/// The fixture on the render node `ATHANOR_RENDER_NODE` names, with the scene, and its first
/// output on M2.
fn on_m2() -> (Running, Scene, smithay::output::Output) {
    let node = PathBuf::from(
        std::env::var_os("ATHANOR_RENDER_NODE").expect("ATHANOR_RENDER_NODE names a render node"),
    );
    let mut run = Running::start(Some(&node));
    let mut scene = Scene::new(&mut run);
    let first = output(&run, 0);
    on_a_fake_gpu(&mut run, LONG, |_, _, _| Ok(times(Tier::M2)));
    let on = first.clone();
    scene.local.pump(&mut run, "M2", move |_, state| {
        state.materials.tier_of(Some(&on)) == Tier::M2 && !state.materials.probing()
    });
    (run, scene, first)
}

/// Whether `a` and `b` are the same within `(x0, y0, w, h)` of the output.
fn same_in(a: &[u8], b: &[u8], (x0, y0, w, h): (i32, i32, i32, i32)) -> bool {
    let width = OUTPUTS[0].width;
    (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| pixel(a, width, x, y) == pixel(b, width, x, y)))
}

/// Where the second window is, over the first's subsurface, and its size: it covers the
/// subsurface's bottom right, the step included.
const OVER_AT: (i32, i32) = (450, 300);
const OVER: (i32, i32) = (300, 250);

/// Two regions whose boxes overlap each keep their own copy: when only what lies under the
/// upper one changes, the lower subsurface being drawn again, the upper is blurred again and
/// the lower is not, and neither shows the other's pixels: the frame is as it was.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn d35_overlapping_regions_keep_their_own_copies() {
    let (mut run, mut scene, first) = on_m2();
    let region = scene.region();
    scene.set(&mut run, Some(&region));

    let mut over = Local::connect(&mut run, None);
    over.toplevel(&mut run);
    let window = window_of(&run, over.id.as_ref().expect("an id"));
    run.state.space.map_element(window.clone(), OVER_AT, true);
    run.state.focus(&window);
    let compositor: wl_compositor::WlCompositor = over.bind(4);
    let manager: ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1 = over.bind(1);
    let qh = over.queue.handle();
    let (surface, _) = over.window.clone().expect("a window");
    let clear = vec![0; usize::try_from(OVER.0 * OVER.1 * 4).expect("size")];
    let transparent = shm_buffer(&mut over, OVER, wl_shm::Format::Argb8888, &clear);
    let effect = manager.get_background_effect(&surface, &qh, ());
    let whole = compositor.create_region(&qh, ());
    whole.add(0, 0, OVER.0, OVER.1);
    effect.set_blur_region(Some(&whole));
    surface.attach(Some(&transparent), 0, 0);
    surface.damage(0, 0, OVER.0, OVER.1);
    surface.commit();
    over.roundtrip(&mut run);

    let shot = frame(&mut run);
    assert_eq!(held(&first), 2, "a copy for each region");
    let _ = frame(&mut run);
    let blurred = blurs(&first);
    scene.redraw_child(&mut run);
    let again = frame(&mut run);
    assert_eq!(
        blurs(&first),
        blurred + 1,
        "the upper region alone is blurred again"
    );
    let lower = (AT.0 + CHILD_AT.0, AT.1 + CHILD_AT.1, CHILD.0, CHILD.1);
    let upper = (OVER_AT.0, OVER_AT.1, OVER.0, OVER.1);
    assert!(same_in(&shot, &again, lower), "the lower region, as it was");
    assert!(same_in(&shot, &again, upper), "the upper region, as it was");
    run.stop();
}

/// A capture of the output draws with a damage tracker of its own, at the places the screen's
/// contents have, without the magnifier: it copies and blurs into copies of its own, freed
/// with it, and the screen's blur, drawn again from its own copy, is as it was.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn d35_a_capture_leaves_the_screens_blur_as_it_was() {
    let (mut run, mut scene, first) = on_m2();
    let region = scene.region();
    scene.set(&mut run, Some(&region));
    // The magnifier about the subsurface's middle, which keeps its box on the output.
    let middle = (
        AT.0 + CHILD_AT.0 + CHILD.0 / 2,
        AT.1 + CHILD_AT.1 + CHILD.1 / 2,
    );
    run.state.pointer_to(
        Point::from((f64::from(middle.0), f64::from(middle.1))),
        InputTime::now(),
    );
    run.state.zoom_step(true);
    let shot = frame(&mut run);
    let _ = frame(&mut run);
    let blurred = blurs(&first);
    assert_eq!(held(&first), 1, "the screen's copy");

    let marks = run.state.marks();
    let crate::state::Backend::Headless(headless) = &mut run.state.backend else {
        panic!("the fixture is headless");
    };
    let captured = headless.read_back(
        &crate::capture::Target::Output(first.clone()),
        &run.state.space,
        &marks,
    );
    assert!(captured.is_ok(), "the capture: {:?}", captured.err());
    assert_eq!(blurs(&first), blurred + 1, "the capture blurs for itself");
    assert_eq!(held(&first), 1, "the capture's copy goes with it");

    scene.redraw_child(&mut run);
    let again = frame(&mut run);
    assert_eq!(
        blurs(&first),
        blurred + 1,
        "the screen's blur is not copied again"
    );
    let (width, height) = (OUTPUTS[0].width, OUTPUTS[0].height);
    assert!(
        same_in(&shot, &again, (0, 0, width, height)),
        "the screen's blur, as it was"
    );
    run.stop();
}

/// Under the magnifier, a region whose box reaches past the output is copied, blurred and drawn
/// only where it is on the output, and nothing of it is stretched: the edge between the stripes
/// and the window's black bottom left stays where the magnifier puts it.
#[test]
#[ignore = "needs a render node, named by ATHANOR_RENDER_NODE"]
fn d35_under_the_magnifier_a_region_past_the_output_is_not_stretched() {
    let (mut run, mut scene, _first) = on_m2();
    let region = scene.region();
    scene.set(&mut run, Some(&region));
    // About the window's top left, the subsurface's box reaches past the output's bottom.
    run.state.pointer_to(
        Point::from((f64::from(AT.0), f64::from(AT.1))),
        InputTime::now(),
    );
    run.state.zoom_step(true);
    run.state.zoom_step(true);
    let level = run.state.a11y.level;
    let shot = frame(&mut run);
    let (width, height) = (OUTPUTS[0].width, OUTPUTS[0].height);
    let magnified = |at: i32, of: i32| f64::from(at) + f64::from(of) * level;
    let bottom = magnified(AT.1, CHILD_AT.1 + CHILD.1);
    assert!(
        bottom > f64::from(height),
        "the box reaches past the output: {bottom}"
    );
    // A column of the window's left half, under the region and clear of the hole.
    let x = magnified(AT.0, 100) as i32;
    let edge = magnified(AT.1, PARENT.1 / 2);
    let from = magnified(AT.1, CHILD_AT.1 + 20) as i32;
    let crossing = (from..height)
        .find(|&y| pixel(&shot, width, x, y)[0] < 64)
        .expect("the edge under the region");
    assert!(
        (f64::from(crossing) - edge).abs() <= 3.0,
        "the edge is at {crossing}, not at {edge}: the blur is stretched"
    );
    run.stop();
}

/// A client that sets more regions than an output blurs in a frame has the frontmost blurred,
/// up to `MOST_BLURS`, and the rest drawn as they are: here the scene's subsurface, the
/// bottom one, is past the bound once as many subsurfaces with a region are put over it.
#[test]
fn d35_an_output_blurs_at_most_most_blurs_regions_in_a_frame() {
    let mut run = Running::start(None);
    let mut scene = Scene::new(&mut run);
    let region = scene.region();
    scene.set(&mut run, Some(&region));
    on_gpu(&mut run, "/dev/dri/renderD-m2", Tier::M2);
    let first = output(&run, 0);
    let on = first.clone();
    scene
        .local
        .pump(&mut run, "Pixman taken for M2", move |_, state| {
            state.materials.tier_of(Some(&on)) == Tier::M2 && !state.materials.probing()
        });
    let lowest = Rectangle::new((AT.0 + CHILD_AT.0, AT.1 + CHILD_AT.1).into(), CHILD.into());
    assert_eq!(blur_elements(&run), [lowest], "one region, blurred");

    let subcompositor: wl_subcompositor::WlSubcompositor = scene.local.bind(1);
    let qh = scene.local.queue.handle();
    let side = 10;
    let clear = vec![0; usize::try_from(side * side * 4).expect("size")];
    let mut kept = Vec::new();
    for i in 0..i32::try_from(MOST_BLURS).expect("a small bound") {
        let surface = scene.compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&surface, &scene.parent, &qh, ());
        subsurface.set_position(i * 2 * side, PARENT.1 - side);
        let buffer = shm_buffer(
            &mut scene.local,
            (side, side),
            wl_shm::Format::Argb8888,
            &clear,
        );
        let effect = scene.manager.get_background_effect(&surface, &qh, ());
        let whole = scene.compositor.create_region(&qh, ());
        whole.add(0, 0, side, side);
        effect.set_blur_region(Some(&whole));
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        kept.push((surface, subsurface, effect));
    }
    scene.parent.commit();
    scene.local.roundtrip(&mut run);
    let shown = blur_elements(&run);
    assert_eq!(shown.len(), MOST_BLURS, "the bound: {shown:?}");
    assert!(!shown.contains(&lowest), "the bottom one is past it");
    run.stop();
}
