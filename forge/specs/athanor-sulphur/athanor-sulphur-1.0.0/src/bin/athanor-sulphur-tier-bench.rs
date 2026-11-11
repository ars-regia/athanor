//! Spike VL-S5 (doc_visual_language.md, VL13): what a frame and tier M2's live blur cost on
//! this machine's GPU, measured rather than estimated.
//!
//! Usage: athanor-sulphur-tier-bench [RENDER_NODE] [WIDTH HEIGHT] [FRAMES]
//!
//! On the render node (default /dev/dri/renderD128), off screen, it draws a scene the size of
//! an output: a wallpaper and three translucent windows, every frame whole, as a frame with
//! full damage. Each case is drawn FRAMES times (default 300) after 30 frames of warm-up:
//!   - `scene`: the scene alone, the cost every M2 figure adds to;
//!   - `dock-N` and `full-N`: the scene, then a dual Kawase blur of N passes down and N up
//!     from half resolution (`material.blur` = 20 px, `material.saturate` = 1.2) of the
//!     dock's region (1280 x 96 logical at scale 1, centred at the bottom) or of the whole
//!     output, composited back over the scene.
//! A frame is timed from its first GL call to its sync point signalled, so the figure is
//! the frame's wall time on the GPU plus its submission, an upper bound of the GPU time.
//! It prints the median, the 95th percentile and the maximum of each case, in milliseconds.
//!
//! ponytail: a spike tool on a spike branch; the compositor's own measurement and blur are
//! written after the spike's results are recorded.

use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::{DrmDeviceFd, DrmNode};
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType, UniformValue,
};
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::{ApiDevice, GpuManager};
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::{Bind, Color32F, Frame, ImportMem, Offscreen, Renderer, Texture};
use smithay::utils::{Buffer, DeviceFd, Physical, Rectangle, Size, Transform};
use std::fs::OpenOptions;
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

type Error = Box<dyn std::error::Error>;

const FORMAT: Fourcc = Fourcc::Abgr8888;
const WARM_UP: usize = 30;
/// Half a texel of the source, times this, is each tap's offset.
const OFFSET: f32 = 2.5;
const SATURATE: f32 = 1.2;

const HEAD: &str = r#"
//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
uniform vec2 halfpixel;
uniform float saturate;
varying vec2 v_coords;
"#;

const DOWN: &str = r#"
void main() {
    vec2 uv = v_coords;
    vec4 sum = texture2D(tex, uv) * 4.0;
    sum += texture2D(tex, uv - halfpixel);
    sum += texture2D(tex, uv + halfpixel);
    sum += texture2D(tex, uv + vec2(halfpixel.x, -halfpixel.y));
    sum += texture2D(tex, uv - vec2(halfpixel.x, -halfpixel.y));
    gl_FragColor = sum / 8.0 * alpha;
}
"#;

const UP: &str = r#"
void main() {
    vec2 uv = v_coords;
    vec2 h = halfpixel;
    vec4 sum = texture2D(tex, uv + vec2(-h.x * 2.0, 0.0));
    sum += texture2D(tex, uv + vec2(-h.x, h.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(0.0, h.y * 2.0));
    sum += texture2D(tex, uv + vec2(h.x, h.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(h.x * 2.0, 0.0));
    sum += texture2D(tex, uv + vec2(h.x, -h.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(0.0, -h.y * 2.0));
    sum += texture2D(tex, uv + vec2(-h.x, -h.y)) * 2.0;
    vec4 color = sum / 12.0;
    float grey = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    color.rgb = clamp(mix(vec3(grey), color.rgb, saturate), 0.0, color.a);
    gl_FragColor = color * alpha;
}
"#;

struct Blur {
    down: GlesTexProgram,
    up: GlesTexProgram,
    /// Level k is the region's size halved k + 1 times; the last up pass draws on the scene.
    levels: Vec<GlesTexture>,
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let node = args.first().map_or("/dev/dri/renderD128", String::as_str);
    let width: i32 = args.get(1).map_or(Ok(1920), |a| a.parse())?;
    let height: i32 = args.get(2).map_or(Ok(1080), |a| a.parse())?;
    let frames: usize = args.get(3).map_or(Ok(300), |a| a.parse())?;

    let file = OpenOptions::new().read(true).write(true).open(node)?;
    let fd = DrmDeviceFd::new(DeviceFd::from(OwnedFd::from(file)));
    let drm_node = DrmNode::from_file(&fd)?;
    let mut api = GbmGlesBackend::<GlesRenderer, DrmDeviceFd>::default();
    api.add_node(drm_node, GbmDevice::new(fd.clone())?)?;
    let mut manager = GpuManager::new(api)?;
    let mut devices = manager.devices_mut()?;
    let renderer = devices
        .find(|d| d.node() == &drm_node)
        .map(ApiDevice::renderer_mut)
        .ok_or("no renderer for the node")?;

    let uniforms = [
        UniformName::new("halfpixel", UniformType::_2f),
        UniformName::new("saturate", UniformType::_1f),
    ];
    let down = renderer.compile_custom_texture_shader(format!("{HEAD}{DOWN}"), &uniforms)?;
    let up = renderer.compile_custom_texture_shader(format!("{HEAD}{UP}"), &uniforms)?;

    let size = Size::<i32, Physical>::from((width, height));
    let wallpaper = pattern(renderer, (width, height), 255, 0)?;
    let window = pattern(renderer, (width * 5 / 8, height * 5 / 8), 235, 1)?;
    let mut target: GlesTexture =
        Offscreen::<GlesTexture>::create_buffer(renderer, FORMAT, Size::from((width, height)))?;

    let dock = Rectangle::<i32, Physical>::new(
        ((width - 1280.min(width)) / 2, height - 96 - 8).into(),
        (1280.min(width), 96).into(),
    );
    let full = Rectangle::<i32, Physical>::from_size(size);

    println!("node {node}, {width}x{height}, {frames} frames per case");
    let mut cases: Vec<(String, Option<(Rectangle<i32, Physical>, usize)>)> =
        vec![("scene".into(), None)];
    for passes in [2, 3, 4] {
        cases.push((format!("dock-{passes}"), Some((dock, passes))));
        cases.push((format!("full-{passes}"), Some((full, passes))));
    }
    let mut kits = Vec::new();
    for (name, blur) in cases {
        let kit = match blur {
            Some((region, passes)) => Some((
                region,
                Blur {
                    down: down.clone(),
                    up: up.clone(),
                    levels: levels(renderer, region.size, passes)?,
                },
            )),
            None => None,
        };
        kits.push((name, kit, Vec::with_capacity(frames)));
    }
    // The cases take turns, frame by frame: an integrated GPU raises its clock with the load,
    // and a case drawn alone, before or after the others, would be timed at another clock.
    for i in 0..WARM_UP + frames {
        for (_, kit, times) in &mut kits {
            let started = Instant::now();
            // One wait a frame, as a compositor waits: the passes of one context complete in
            // order, so the last sync point covers them all.
            let mut sync = draw(renderer, &mut target, size, &wallpaper, &window, i)?;
            if let Some((region, blur)) = kit {
                sync = blurred(renderer, &mut target, size, *region, blur)?;
            }
            sync.wait()?;
            if i >= WARM_UP {
                times.push(started.elapsed());
            }
        }
    }
    for (name, _, times) in &mut kits {
        report(name, times);
    }
    Ok(())
}

/// A texture of `size` with a pattern that a blur cannot skip, at `alpha` of 255.
fn pattern(
    renderer: &mut GlesRenderer,
    (w, h): (i32, i32),
    alpha: u8,
    seed: u32,
) -> Result<GlesTexture, Error> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = (x as u32)
                .wrapping_mul(2_654_435_761)
                .wrapping_add((y as u32).wrapping_mul(40_503))
                .wrapping_add(seed.wrapping_mul(97));
            let a = u32::from(alpha);
            // Premultiplied, as the renderer expects.
            data.extend_from_slice(&[
                ((v & 0xff) * a / 255) as u8,
                (((v >> 8) & 0xff) * a / 255) as u8,
                (((v >> 16) & 0xff) * a / 255) as u8,
                alpha,
            ]);
        }
    }
    Ok(renderer.import_memory(&data, FORMAT, Size::from((w, h)), false)?)
}

/// The region's levels: half its size, then half again, `passes` times.
fn levels(
    renderer: &mut GlesRenderer,
    region: Size<i32, Physical>,
    passes: usize,
) -> Result<Vec<GlesTexture>, Error> {
    let mut levels = Vec::new();
    let (mut w, mut h) = (region.w, region.h);
    for _ in 0..passes {
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        levels.push(Offscreen::<GlesTexture>::create_buffer(
            renderer,
            FORMAT,
            Size::<i32, Buffer>::from((w, h)),
        )?);
    }
    Ok(levels)
}

/// The scene, whole: the wallpaper and three windows, moved by `i` so no frame repeats.
fn draw(
    renderer: &mut GlesRenderer,
    target: &mut GlesTexture,
    size: Size<i32, Physical>,
    wallpaper: &GlesTexture,
    window: &GlesTexture,
    i: usize,
) -> Result<SyncPoint, Error> {
    let mut fb = renderer.bind(target)?;
    let mut frame = renderer.render(&mut fb, size, Transform::Normal)?;
    let all = [Rectangle::from_size(size)];
    frame.clear(Color32F::new(0.15, 0.16, 0.16, 1.0), &all)?;
    let wsize = Size::<i32, Buffer>::from((size.w, size.h));
    frame.render_texture_from_to(
        wallpaper,
        Rectangle::from_size(wsize).to_f64(),
        Rectangle::from_size(size),
        &all,
        &all,
        Transform::Normal,
        1.0,
        None,
        &[],
    )?;
    let win = Size::<i32, Buffer>::from((size.w * 5 / 8, size.h * 5 / 8));
    for k in 0..3i32 {
        let shift = ((i as i32) * (k + 1)) % (size.w / 4);
        let dst = Rectangle::<i32, Physical>::new(
            (size.w / 16 * (k + 1) + shift, size.h / 12 * (k + 1)).into(),
            (win.w, win.h).into(),
        );
        frame.render_texture_from_to(
            window,
            Rectangle::from_size(win).to_f64(),
            dst,
            &[Rectangle::from_size(dst.size)],
            &[],
            Transform::Normal,
            1.0,
            None,
            &[],
        )?;
    }
    Ok(frame.finish()?)
}

/// Blurs `region` of `target` in place: down from the target through the levels, up back,
/// and the last up pass composited over the region with the saturation.
fn blurred(
    renderer: &mut GlesRenderer,
    target: &mut GlesTexture,
    size: Size<i32, Physical>,
    region: Rectangle<i32, Physical>,
    blur: &mut Blur,
) -> Result<SyncPoint, Error> {
    let tsize = Size::<i32, Buffer>::from((size.w, size.h));
    let mut source = target.clone();
    let mut src = Rectangle::<i32, Buffer>::new(
        (region.loc.x, region.loc.y).into(),
        (region.size.w, region.size.h).into(),
    );
    let mut src_size = tsize;
    // Down.
    for k in 0..blur.levels.len() {
        let level_size = blur.levels[k].size();
        let dst = Rectangle::<i32, Physical>::from_size((level_size.w, level_size.h).into());
        pass(
            renderer,
            &mut blur.levels[k],
            &source,
            src,
            src_size,
            dst,
            &blur.down,
            1.0,
        )?;
        source = blur.levels[k].clone();
        src = Rectangle::from_size(level_size);
        src_size = level_size;
    }
    // Up, through the levels in reverse, the last onto the region of the target.
    for k in (0..blur.levels.len()).rev() {
        if k == 0 {
            return pass(
                renderer, target, &source, src, src_size, region, &blur.up, SATURATE,
            );
        } else {
            let level_size = blur.levels[k - 1].size();
            let dst = Rectangle::<i32, Physical>::from_size((level_size.w, level_size.h).into());
            pass(
                renderer,
                &mut blur.levels[k - 1],
                &source,
                src,
                src_size,
                dst,
                &blur.up,
                1.0,
            )?;
            source = blur.levels[k - 1].clone();
            src = Rectangle::from_size(level_size);
            src_size = level_size;
        }
    }
    Err("a blur needs at least one level".into())
}

#[allow(clippy::too_many_arguments)]
fn pass(
    renderer: &mut GlesRenderer,
    into: &mut GlesTexture,
    source: &GlesTexture,
    src: Rectangle<i32, Buffer>,
    src_size: Size<i32, Buffer>,
    dst: Rectangle<i32, Physical>,
    program: &GlesTexProgram,
    saturate: f32,
) -> Result<SyncPoint, Error> {
    let into_size = into.size();
    let mut fb = renderer.bind(into)?;
    let mut frame = renderer.render(
        &mut fb,
        (into_size.w, into_size.h).into(),
        Transform::Normal,
    )?;
    let uniforms = [
        Uniform::new(
            "halfpixel",
            UniformValue::_2f(
                OFFSET * 0.5 / src_size.w as f32,
                OFFSET * 0.5 / src_size.h as f32,
            ),
        ),
        Uniform::new("saturate", UniformValue::_1f(saturate)),
    ];
    let damage = [Rectangle::from_size(dst.size)];
    frame.render_texture_from_to(
        source,
        src.to_f64(),
        dst,
        &damage,
        &damage,
        Transform::Normal,
        1.0,
        Some(program),
        &uniforms,
    )?;
    Ok(frame.finish()?)
}

fn report(name: &str, times: &mut [Duration]) {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1_000.0;
    let at = |q: f64| times[((times.len() as f64 * q) as usize).min(times.len() - 1)];
    println!(
        "{name:8} median {:6.3} ms  p95 {:6.3} ms  max {:6.3} ms",
        ms(at(0.5)),
        ms(at(0.95)),
        ms(*times.last().unwrap_or(&Duration::ZERO))
    );
}
