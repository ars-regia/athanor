//! What the magnifier and the colour filters do to an output's frame (doc_accessibility.md
//! AX10): every element scaled about the focal point, then, with a filter or the inversion on,
//! the whole frame, cursor included, drawn into an offscreen buffer and shown through one colour
//! pass. Captures are drawn without either (`capture.rs`).
//!
//! While a pass is on, the frame is one element, composited: no client buffer is scanned out
//! directly and the cursor plane is not used.

use crate::a11y::{Affine, Look};
use crate::state::BACKGROUND;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::{Error as DamageError, OutputDamageTracker};
use smithay::backend::renderer::element::utils::RescaleRenderElement;
use smithay::backend::renderer::element::{Element, Id, RenderElement, RenderElementStates};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
    UniformType, UniformValue,
};
use smithay::backend::renderer::pixman::{PixmanError, PixmanFrame, PixmanRenderer, PixmanTexture};
use smithay::backend::renderer::utils::{CommitCounter, OpaqueRegions};
use smithay::backend::renderer::{
    Bind, ExportMem, Frame, ImportAll, ImportMem, Offscreen, Renderer, RendererSuper, Texture,
};
use smithay::output::{Output, OutputNoMode};
use smithay::reexports::pixman::Image;
use smithay::utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Size, Transform};
use std::cell::RefCell;

/// The layout of the offscreen buffer and of what is read back of it.
const FORMAT: Fourcc = Fourcc::Abgr8888;

/// `elements` as the magnifier shows them: each scaled by the level about the focal point, or
/// as they are without the magnifier.
pub fn zoomed<E: Element>(
    elements: Vec<E>,
    zoom: Option<(Point<i32, Physical>, f64)>,
) -> Vec<RescaleRenderElement<E>> {
    let (origin, level) = zoom.unwrap_or((Point::default(), 1.0));
    elements
        .into_iter()
        .map(|element| RescaleRenderElement::from_element(element, origin, level))
        .collect()
}

/// Draws `elements` on `output` into `framebuffer` as `look` says, `age` frames after its
/// previous contents. With a colour pass, the states are those of `elements`, not of the one
/// element the frame is then made of.
pub fn draw<'d, R, E>(
    renderer: &mut R,
    framebuffer: &mut R::Framebuffer<'_>,
    output: &Output,
    age: usize,
    elements: Vec<E>,
    damage_tracker: &'d mut OutputDamageTracker,
    look: Look,
) -> Result<smithay::backend::renderer::damage::RenderOutputResult<'d>, DamageError<R::Error>>
where
    R: Filters,
    R::TextureId: Clone + Texture + 'static,
    E: RenderElement<R>,
    FilterElement<R::TextureId>: RenderElement<R>,
{
    let elements = zoomed(elements, look.zoom);
    let Some(affine) = look.filter else {
        return damage_tracker.render_output(renderer, framebuffer, age, &elements, BACKGROUND);
    };
    let (element, states) = filtered(renderer, output, &elements, affine)?;
    let mut result =
        damage_tracker.render_output(renderer, framebuffer, age, &[element], BACKGROUND)?;
    result.states = states;
    Ok(result)
}

/// A renderer the colour pass can draw with: into an offscreen buffer, then shown through
/// the pass.
pub trait Filters: Renderer + ImportAll + Sized {
    /// The offscreen buffer.
    type Offscreen: 'static;

    /// A new offscreen buffer of `size`.
    fn offscreen(&mut self, size: Size<i32, BufferCoords>) -> Result<Self::Offscreen, Self::Error>;

    /// `offscreen`, bound to be drawn into.
    fn bind_offscreen<'a>(
        &mut self,
        offscreen: &'a mut Self::Offscreen,
    ) -> Result<Self::Framebuffer<'a>, Self::Error>;

    /// The texture of `offscreen`, `size` large, that the frame's element draws.
    fn shown(
        &mut self,
        offscreen: &mut Self::Offscreen,
        size: Size<i32, Physical>,
        affine: &Affine,
    ) -> Result<Self::TextureId, Self::Error>;
}

/// The offscreen frame of an output, kept between frames so that only what changed is drawn
/// again into it.
struct Pass<B, T> {
    buffer: B,
    size: Size<i32, Physical>,
    scale: f64,
    tracker: OutputDamageTracker,
    id: Id,
    commit: CommitCounter,
    affine: Affine,
    /// The texture last shown, while nothing changed since.
    shown: Option<T>,
}

/// The element that shows an output's frame through the colour pass, and the states of what
/// the frame was drawn from.
pub type Filtered<R> = (FilterElement<<R as RendererSuper>::TextureId>, RenderElementStates);

/// Draws `elements` into the offscreen frame of `output`, and returns the element that shows
/// it through `affine`, with the states of `elements`.
pub fn filtered<R, E>(
    renderer: &mut R,
    output: &Output,
    elements: &[E],
    affine: Affine,
) -> Result<Filtered<R>, DamageError<R::Error>>
where
    R: Filters,
    R::TextureId: Clone + Texture + 'static,
    E: RenderElement<R>,
{
    let mode = output.current_mode().ok_or(OutputNoMode)?;
    // The elements are laid out before the output's transform, which the frame applies.
    let size = output.current_transform().transform_size(mode.size);
    let scale = output.current_scale().fractional_scale();
    let cell = output
        .user_data()
        .get_or_insert(|| RefCell::new(None::<Pass<R::Offscreen, R::TextureId>>));
    let mut kept = cell.borrow_mut();
    // Taken, and put back only once drawn: a pass that failed starts again whole.
    let (mut pass, age) = match kept.take() {
        Some(pass) if pass.size == size && pass.scale == scale => (pass, 1),
        _ => {
            let buffer = renderer
                .offscreen(Size::from((size.w, size.h)))
                .map_err(DamageError::Rendering)?;
            let pass = Pass {
                buffer,
                size,
                scale,
                tracker: OutputDamageTracker::new(size, scale, Transform::Normal),
                id: Id::new(),
                commit: CommitCounter::default(),
                affine,
                shown: None,
            };
            (pass, 0)
        }
    };
    let mut target = renderer
        .bind_offscreen(&mut pass.buffer)
        .map_err(DamageError::Rendering)?;
    let drawn = pass
        .tracker
        .render_output(renderer, &mut target, age, elements, BACKGROUND)?;
    let changed = drawn.damage.is_some();
    let states = drawn.states;
    drop(target);
    let texture = match pass.shown.clone() {
        Some(texture) if !changed && pass.affine == affine => texture,
        _ => {
            let texture = renderer
                .shown(&mut pass.buffer, size, &affine)
                .map_err(DamageError::Rendering)?;
            pass.commit.increment();
            pass.affine = affine;
            pass.shown = Some(texture.clone());
            texture
        }
    };
    let element = FilterElement {
        id: pass.id.clone(),
        commit: pass.commit,
        texture,
        size,
        affine,
    };
    *kept = Some(pass);
    Ok((element, states))
}

/// An output's frame, drawn offscreen, shown through the colour pass.
pub struct FilterElement<T> {
    id: Id,
    commit: CommitCounter,
    texture: T,
    size: Size<i32, Physical>,
    affine: Affine,
}

impl<T: Texture> Element for FilterElement<T> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        Rectangle::from_size(Size::from((f64::from(self.size.w), f64::from(self.size.h))))
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        Rectangle::from_size(self.size)
    }

    // The offscreen frame is drawn on the opaque background.
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::from_slice(&[Rectangle::from_size(self.size)])
    }
}

/// The texture shader of the pass: Smithay's, with the affine map applied before the alpha.
const SHADER: &str = r#"#version 100

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
uniform mat3 filter_matrix;
uniform float filter_offset;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif
    color.rgb = clamp(filter_matrix * color.rgb + vec3(filter_offset * color.a), 0.0, color.a);
    color = color * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// The compiled shader, kept with the EGL context; none when it did not compile, which was
/// logged once, and the frame is then shown without the pass.
struct FilterProgram(Option<GlesTexProgram>);

impl Filters for GlesRenderer {
    type Offscreen = GlesTexture;

    fn offscreen(&mut self, size: Size<i32, BufferCoords>) -> Result<GlesTexture, GlesError> {
        Offscreen::<GlesTexture>::create_buffer(self, FORMAT, size)
    }

    fn bind_offscreen<'a>(
        &mut self,
        offscreen: &'a mut GlesTexture,
    ) -> Result<Self::Framebuffer<'a>, GlesError> {
        Bind::<GlesTexture>::bind(self, offscreen)
    }

    fn shown(
        &mut self,
        offscreen: &mut GlesTexture,
        _size: Size<i32, Physical>,
        _affine: &Affine,
    ) -> Result<GlesTexture, GlesError> {
        if self
            .egl_context()
            .user_data()
            .get::<FilterProgram>()
            .is_none()
        {
            let compiled = self.compile_custom_texture_shader(
                SHADER,
                &[
                    UniformName::new("filter_matrix", UniformType::Matrix3x3),
                    UniformName::new("filter_offset", UniformType::_1f),
                ],
            );
            let program = compiled
                .inspect_err(|error| {
                    tracing::error!(%error, "the colour filter's shader did not compile: no filter is drawn")
                })
                .ok();
            self.egl_context()
                .user_data()
                .insert_if_missing(|| FilterProgram(program));
        }
        Ok(offscreen.clone())
    }
}

impl RenderElement<GlesRenderer> for FilterElement<GlesTexture> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), GlesError> {
        let program = frame
            .egl_context()
            .user_data()
            .get::<FilterProgram>()
            .and_then(|program| program.0.clone());
        let uniforms = match program {
            Some(_) => vec![
                Uniform::new(
                    "filter_matrix",
                    UniformValue::Matrix3x3 {
                        matrices: vec![self.affine.columns()],
                        transpose: false,
                    },
                ),
                Uniform::new("filter_offset", UniformValue::_1f(self.affine.offset)),
            ],
            None => Vec::new(),
        };
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            opaque_regions,
            Transform::Normal,
            1.0,
            program.as_ref(),
            &uniforms,
        )
    }
}

impl Filters for PixmanRenderer {
    type Offscreen = Image<'static, 'static>;

    fn offscreen(&mut self, size: Size<i32, BufferCoords>) -> Result<Self::Offscreen, PixmanError> {
        Offscreen::<Image<'static, 'static>>::create_buffer(self, FORMAT, size)
    }

    fn bind_offscreen<'a>(
        &mut self,
        offscreen: &'a mut Self::Offscreen,
    ) -> Result<Self::Framebuffer<'a>, PixmanError> {
        Bind::<Image<'static, 'static>>::bind(self, offscreen)
    }

    /// The pass on the CPU: the frame read back, filtered pixel by pixel, and imported.
    fn shown(
        &mut self,
        offscreen: &mut Self::Offscreen,
        size: Size<i32, Physical>,
        affine: &Affine,
    ) -> Result<PixmanTexture, PixmanError> {
        let size = Size::<i32, BufferCoords>::from((size.w, size.h));
        let target = Bind::<Image<'static, 'static>>::bind(self, offscreen)?;
        let mapping = self.copy_framebuffer(&target, Rectangle::from_size(size), FORMAT)?;
        let mut pixels = self.map_texture(&mapping)?.to_vec();
        drop(target);
        for pixel in pixels.as_chunks_mut::<4>().0 {
            *pixel = affine.apply(*pixel);
        }
        self.import_memory(&pixels, FORMAT, size, false)
    }
}

impl RenderElement<PixmanRenderer> for FilterElement<PixmanTexture> {
    fn draw(
        &self,
        frame: &mut PixmanFrame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), PixmanError> {
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            opaque_regions,
            Transform::Normal,
            1.0,
        )
    }
}
