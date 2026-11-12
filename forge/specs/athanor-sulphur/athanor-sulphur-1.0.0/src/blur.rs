//! The blur of the materials (`doc_visual_language.md` VL13): a dual Kawase blur from half
//! resolution, `material.saturate` applied in its last pass, after spike VL-S5's bench. The
//! region is first copied out of what it blurs, as a compositor drawing into a swapchain
//! buffer must copy its backdrop, then halved `PASSES` times and doubled back, the last pass
//! drawn over the region.
//!
//! The buffers are sized for the largest region they blur, and a smaller region uses part of
//! them: every tap is clamped to the part of its source the region fills, so that nothing left
//! there by another region bleeds into the blur. The probe copies its region to the backdrop's
//! top left corner; the background effect (`background_effect.rs`) keeps a backdrop the size
//! of the output, copies each region to its own place in it, and blurs it there in place.

use crate::Error;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType, UniformValue,
};
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::{Bind, Frame, Offscreen, Renderer, Texture};
use smithay::utils::{Buffer, Physical, Rectangle, Size, Transform};

/// `material.saturate` of the Calmo tokens (`system/athanor-style/calmo/tokens.toml`,
/// `[sal.material]`), which a test keeps equal.
pub const SATURATE: f32 = 1.2;
/// The passes down and up for `material.blur`, 20 px, which a test keeps. VL13 left 3 or 4 to
/// be chosen by eye against a Gaussian blur of 20 px; the maintainer chose 3 on 2026-11-12. On
/// a busy backdrop, text and window edges over the wallpaper, against Gaussians with
/// `material.saturate`, 3 passes fit best at a standard deviation of 14 to 17 px (RMSE 0.040,
/// 0.042 at 20), and 4 passes at 30 to 40 px (RMSE 0.066 at 20), far too soft. The GL test
/// of the background effect measures the softness, so that a change here fails it.
pub const PASSES: usize = 3;
/// Half a texel of the source, times this, is each tap's offset.
const OFFSET: f32 = 2.5;
const FORMAT: Fourcc = Fourcc::Abgr8888;

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
// The centres of the first and the last texel of the part of the source the region fills.
uniform vec4 bounds;
varying vec2 v_coords;

vec4 tap(vec2 uv) {
    return texture2D(tex, clamp(uv, bounds.xy, bounds.zw));
}
"#;

const DOWN: &str = r#"
void main() {
    vec2 uv = v_coords;
    vec4 sum = tap(uv) * 4.0;
    sum += tap(uv - halfpixel);
    sum += tap(uv + halfpixel);
    sum += tap(uv + vec2(halfpixel.x, -halfpixel.y));
    sum += tap(uv - vec2(halfpixel.x, -halfpixel.y));
    gl_FragColor = sum / 8.0 * alpha;
}
"#;

const UP: &str = r#"
void main() {
    vec2 uv = v_coords;
    vec2 h = halfpixel;
    vec4 sum = tap(uv + vec2(-h.x * 2.0, 0.0));
    sum += tap(uv + vec2(-h.x, h.y)) * 2.0;
    sum += tap(uv + vec2(0.0, h.y * 2.0));
    sum += tap(uv + vec2(h.x, h.y)) * 2.0;
    sum += tap(uv + vec2(h.x * 2.0, 0.0));
    sum += tap(uv + vec2(h.x, -h.y)) * 2.0;
    sum += tap(uv + vec2(0.0, -h.y * 2.0));
    sum += tap(uv + vec2(-h.x, -h.y)) * 2.0;
    vec4 color = sum / 12.0;
    float grey = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    color.rgb = clamp(mix(vec3(grey), color.rgb, saturate), 0.0, color.a);
    gl_FragColor = color * alpha;
}
"#;

/// The two programs, compiled once per GL context and kept in it.
#[derive(Clone)]
struct Programs {
    down: GlesTexProgram,
    up: GlesTexProgram,
}

/// What a GL context keeps of the programs: none when they did not compile there.
struct Kept(Option<Programs>);

/// The programs of `renderer`'s context, compiled at the first call.
fn programs(renderer: &mut GlesRenderer) -> Result<Programs, Error> {
    if let Some(kept) = renderer.egl_context().user_data().get::<Kept>() {
        return kept
            .0
            .clone()
            .ok_or_else(|| "the blur's programs do not compile in this context".into());
    }
    let uniforms = [
        UniformName::new("halfpixel", UniformType::_2f),
        UniformName::new("saturate", UniformType::_1f),
        UniformName::new("bounds", UniformType::_4f),
    ];
    let compiled = renderer
        .compile_custom_texture_shader(format!("{HEAD}{DOWN}"), &uniforms)
        .and_then(|down| {
            renderer
                .compile_custom_texture_shader(format!("{HEAD}{UP}"), &uniforms)
                .map(|up| Programs { down, up })
        });
    let kept = match compiled {
        Ok(programs) => Kept(Some(programs)),
        Err(error) => {
            tracing::warn!(%error, "the blur's programs do not compile: regions are not blurred");
            Kept(None)
        }
    };
    let programs = kept.0.clone();
    renderer
        .egl_context()
        .user_data()
        .insert_if_missing(|| kept);
    programs.ok_or_else(|| "the blur's programs do not compile in this context".into())
}

/// The last pass of a blur: the program, the part of the texture it reads and its uniforms,
/// which whoever draws the region gives to `render_texture_from_to`.
pub struct Last {
    pub program: GlesTexProgram,
    pub texture: GlesTexture,
    pub from: Rectangle<i32, Buffer>,
    pub uniforms: [Uniform<'static>; 3],
}

/// The programs and the buffers of the blur of regions up to one size.
pub struct Blur {
    programs: Programs,
    /// The region copied out of the target, which the first pass reads.
    backdrop: GlesTexture,
    /// Level k is the region halved k + 1 times.
    levels: Vec<GlesTexture>,
}

impl Blur {
    /// The blur of regions up to `size`.
    pub fn new(renderer: &mut GlesRenderer, size: Size<i32, Physical>) -> Result<Self, Error> {
        let programs = programs(renderer)?;
        let (mut w, mut h) = (size.w.max(1), size.h.max(1));
        let backdrop = Offscreen::<GlesTexture>::create_buffer(renderer, FORMAT, (w, h).into())?;
        let mut levels = Vec::with_capacity(PASSES);
        for _ in 0..PASSES {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            levels.push(Offscreen::<GlesTexture>::create_buffer(
                renderer,
                FORMAT,
                (w, h).into(),
            )?);
        }
        Ok(Self {
            programs,
            backdrop,
            levels,
        })
    }

    /// The buffer a region is copied into before `passes` or `in_place`.
    pub fn backdrop(&mut self) -> &mut GlesTexture {
        &mut self.backdrop
    }

    /// The backdrop, to be drawn from after `in_place`.
    pub fn texture(&self) -> &GlesTexture {
        &self.backdrop
    }

    /// Blurs `rect` of the backdrop in place: down through the levels, up back, and the last
    /// pass, with the saturation, drawn back over `rect`.
    pub fn in_place(
        &mut self,
        renderer: &mut GlesRenderer,
        rect: Rectangle<i32, Buffer>,
    ) -> Result<(), Error> {
        let last = self.passes(renderer, rect)?;
        let to = Rectangle::<i32, Physical>::new(
            (rect.loc.x, rect.loc.y).into(),
            (rect.size.w, rect.size.h).into(),
        );
        drop(pass(
            renderer,
            &mut self.backdrop,
            &last.texture,
            last.from,
            to,
            Some(&last.program),
            &last.uniforms,
        )?);
        Ok(())
    }

    /// Blurs `region` of `target` in place: the region copied out, down through the levels,
    /// up back, and the last pass drawn over the region with the saturation.
    pub fn blur(
        &mut self,
        renderer: &mut GlesRenderer,
        target: &mut GlesTexture,
        region: Rectangle<i32, Physical>,
    ) -> Result<SyncPoint, Error> {
        let size = Size::<i32, Buffer>::from((region.size.w, region.size.h));
        let copied = Rectangle::<i32, Buffer>::new((region.loc.x, region.loc.y).into(), size);
        let source = target.clone();
        // The copy: a pass with the stock program.
        drop(pass(
            renderer,
            &mut self.backdrop,
            &source,
            copied,
            Rectangle::from_size((size.w, size.h).into()),
            None,
            &[],
        )?);
        let last = self.passes(renderer, Rectangle::from_size(size))?;
        pass(
            renderer,
            target,
            &last.texture,
            last.from,
            region,
            Some(&last.program),
            &last.uniforms,
        )
    }

    /// Down and up through the levels from `rect` of the backdrop, all but the last pass,
    /// which the caller draws from what this returns.
    pub fn passes(
        &mut self,
        renderer: &mut GlesRenderer,
        rect: Rectangle<i32, Buffer>,
    ) -> Result<Last, Error> {
        let mut source = self.backdrop.clone();
        let mut from = rect
            .intersection(Rectangle::from_size(source.size()))
            .ok_or("the region lies outside the blur's buffers")?;
        let mut sizes = Vec::with_capacity(self.levels.len());
        for level in &mut self.levels {
            let to = Rectangle::from_size(clamp(halved(from.size), level.size()));
            let uniforms = taps(&source, from, 1.0);
            drop(pass(
                renderer,
                level,
                &source,
                from,
                Rectangle::from_size((to.size.w, to.size.h).into()),
                Some(&self.programs.down),
                &uniforms,
            )?);
            sizes.push(to.size);
            source = level.clone();
            from = to;
        }
        for k in (0..self.levels.len().saturating_sub(1)).rev() {
            let to = sizes.get(k).copied().unwrap_or(from.size);
            let uniforms = taps(&source, from, 1.0);
            let level = &mut self.levels[k];
            drop(pass(
                renderer,
                level,
                &source,
                from,
                Rectangle::from_size((to.w, to.h).into()),
                Some(&self.programs.up),
                &uniforms,
            )?);
            source = level.clone();
            from = Rectangle::from_size(to);
        }
        let uniforms = taps(&source, from, SATURATE);
        Ok(Last {
            program: self.programs.up.clone(),
            texture: source,
            from,
            uniforms,
        })
    }
}

/// `size` halved, at least a pixel.
fn halved(size: Size<i32, Buffer>) -> Size<i32, Buffer> {
    ((size.w / 2).max(1), (size.h / 2).max(1)).into()
}

/// `size`, at least a pixel and at most `most`.
fn clamp(size: Size<i32, Buffer>, most: Size<i32, Buffer>) -> Size<i32, Buffer> {
    (
        size.w.clamp(1, most.w.max(1)),
        size.h.clamp(1, most.h.max(1)),
    )
        .into()
}

/// The uniforms of a pass that reads `used` of `source`.
fn taps(source: &GlesTexture, used: Rectangle<i32, Buffer>, saturate: f32) -> [Uniform<'static>; 3] {
    let size = source.size();
    let (w, h) = (size.w.max(1) as f32, size.h.max(1) as f32);
    let (x, y) = (used.loc.x as f32, used.loc.y as f32);
    let (uw, uh) = (x + used.size.w.max(1) as f32, y + used.size.h.max(1) as f32);
    [
        Uniform::new(
            "halfpixel",
            UniformValue::_2f(OFFSET * 0.5 / w, OFFSET * 0.5 / h),
        ),
        Uniform::new("saturate", UniformValue::_1f(saturate)),
        Uniform::new(
            "bounds",
            UniformValue::_4f(
                (x + 0.5) / w,
                (y + 0.5) / h,
                (uw - 0.5) / w,
                (uh - 0.5) / h,
            ),
        ),
    ]
}

/// Draws `from` of `source` over `to` of `into` with `program`, or the stock one.
fn pass(
    renderer: &mut GlesRenderer,
    into: &mut GlesTexture,
    source: &GlesTexture,
    from: Rectangle<i32, Buffer>,
    to: Rectangle<i32, Physical>,
    program: Option<&GlesTexProgram>,
    uniforms: &[Uniform<'_>],
) -> Result<SyncPoint, Error> {
    let into_size = into.size();
    let mut fb = renderer.bind(into)?;
    let mut frame = renderer.render(
        &mut fb,
        (into_size.w, into_size.h).into(),
        Transform::Normal,
    )?;
    let damage = [Rectangle::from_size(to.size)];
    frame.render_texture_from_to(
        source,
        from.to_f64(),
        to,
        &damage,
        &damage,
        Transform::Normal,
        1.0,
        program,
        uniforms,
    )?;
    Ok(frame.finish()?)
}
#[cfg(test)]
mod tests {
    /// The blur's values are the Calmo tokens' `material.blur` and `material.saturate`.
    #[test]
    fn vl13_the_blur_takes_the_calmo_material_tokens() {
        let tokens = include_str!("../../../../../system/athanor-style/calmo/tokens.toml");
        let material = tokens
            .split("[sal.material]")
            .nth(1)
            .and_then(|rest| rest.split("\n[").next())
            .expect("a [sal.material] table");
        let value = |key: &str| {
            material
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key} = ")))
                .map(str::trim)
                .expect("the key")
                .to_owned()
        };
        assert_eq!(value("blur"), "20", "PASSES is chosen for 20 px");
        assert_eq!(
            value("saturate").parse::<f32>().expect("saturate"),
            super::SATURATE
        );
    }
}
