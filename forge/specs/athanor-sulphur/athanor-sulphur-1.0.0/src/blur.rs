//! The blur of the materials (`doc_visual_language.md` VL13): a dual Kawase blur from half
//! resolution, `material.saturate` applied in its last pass, after spike VL-S5's bench. The
//! region is first copied out of what it blurs, as a compositor drawing into a swapchain
//! buffer must copy its backdrop, then halved `PASSES` times and doubled back, the last pass
//! drawn over the region.

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
/// The passes down and up for `material.blur`, 20 px, which a test keeps: VL13 leaves 3 or 4
/// to be chosen by eye against a Gaussian blur of 20 px; 3 is the spike's probe.
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

/// The programs and the buffers of the blur of a region of one size.
pub struct Blur {
    down: GlesTexProgram,
    up: GlesTexProgram,
    /// The region copied out of the target, which the first pass reads.
    backdrop: GlesTexture,
    /// Level k is the region halved k + 1 times.
    levels: Vec<GlesTexture>,
}

impl Blur {
    /// The blur of a region of `size`.
    pub fn new(renderer: &mut GlesRenderer, size: Size<i32, Physical>) -> Result<Self, Error> {
        let uniforms = [
            UniformName::new("halfpixel", UniformType::_2f),
            UniformName::new("saturate", UniformType::_1f),
        ];
        let down = renderer.compile_custom_texture_shader(format!("{HEAD}{DOWN}"), &uniforms)?;
        let up = renderer.compile_custom_texture_shader(format!("{HEAD}{UP}"), &uniforms)?;
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
            down,
            up,
            backdrop,
            levels,
        })
    }

    /// Blurs `region` of `target` in place: the region copied out, down through the levels,
    /// up back, and the last pass drawn over the region with the saturation.
    pub fn blur(
        &mut self,
        renderer: &mut GlesRenderer,
        target: &mut GlesTexture,
        region: Rectangle<i32, Physical>,
    ) -> Result<SyncPoint, Error> {
        let backdrop_size = self.backdrop.size();
        let copied = Rectangle::<i32, Buffer>::new(
            (region.loc.x, region.loc.y).into(),
            (region.size.w, region.size.h).into(),
        );
        let whole = Rectangle::from_size((backdrop_size.w, backdrop_size.h).into());
        let source = target.clone();
        // The copy: a pass with the stock program.
        drop(pass(
            renderer,
            &mut self.backdrop,
            &source,
            copied,
            whole,
            None,
            &[],
        )?);
        let mut source = self.backdrop.clone();
        let mut source_size = backdrop_size;
        for level in &mut self.levels {
            let level_size = level.size();
            let uniforms = taps(source_size, 1.0);
            let into = Rectangle::from_size((level_size.w, level_size.h).into());
            drop(pass(
                renderer,
                level,
                &source,
                Rectangle::from_size(source_size),
                into,
                Some(&self.down),
                &uniforms,
            )?);
            source = level.clone();
            source_size = level_size;
        }
        for k in (0..self.levels.len().saturating_sub(1)).rev() {
            let level = &mut self.levels[k];
            let level_size = level.size();
            let uniforms = taps(source_size, 1.0);
            let into = Rectangle::from_size((level_size.w, level_size.h).into());
            drop(pass(
                renderer,
                level,
                &source,
                Rectangle::from_size(source_size),
                into,
                Some(&self.up),
                &uniforms,
            )?);
            source = level.clone();
            source_size = level_size;
        }
        let uniforms = taps(source_size, SATURATE);
        pass(
            renderer,
            target,
            &source,
            Rectangle::from_size(source_size),
            region,
            Some(&self.up),
            &uniforms,
        )
    }
}

/// The uniforms of a pass that reads a source of `size`.
fn taps(size: Size<i32, Buffer>, saturate: f32) -> [Uniform<'static>; 2] {
    let (w, h) = (size.w.max(1) as f32, size.h.max(1) as f32);
    [
        Uniform::new(
            "halfpixel",
            UniformValue::_2f(OFFSET * 0.5 / w, OFFSET * 0.5 / h),
        ),
        Uniform::new("saturate", UniformValue::_1f(saturate)),
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
