//! Drawing: the whole window as one scene, at any scale. [`render`] rasterises it with vello_cpu
//! into a premultiplied RGBA pixmap; `gpu.rs` hands the same scene to vello_gpu.

use crate::text::Brush;
use crate::theme::Rgba;
use crate::widget::{Kind, Ui, WindowButton};
use parley::{Layout, PositionedLayoutItem};
use vello_cpu::kurbo::{Affine, BezPath, Circle, Line, Rect, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::{Color, FontData};
use vello_cpu::{Glyph, Pixmap, RenderContext};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// The calls the window's scene makes, which vello_cpu's `RenderContext` and vello_gpu's
/// `Scene` both offer under the same names, on the types of their shared vello_common.
pub trait Canvas {
    type Resources;
    fn set_transform(&mut self, transform: Affine);
    fn set_paint(&mut self, color: Color);
    fn set_stroke(&mut self, stroke: Stroke);
    fn fill_rect(&mut self, rect: &Rect);
    fn fill_path(&mut self, path: &BezPath);
    fn stroke_path(&mut self, path: &BezPath);
    fn push_clip_path(&mut self, path: &BezPath);
    fn pop_clip(&mut self);
    /// Fills a run of glyphs of one font and size, hinted.
    fn glyphs(
        &mut self,
        resources: &mut Self::Resources,
        font: &FontData,
        size: f32,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) -> Result<(), Error>;
}

macro_rules! canvas {
    ($canvas:ty, $resources:ty) => {
        impl Canvas for $canvas {
            type Resources = $resources;
            fn set_transform(&mut self, transform: Affine) {
                <$canvas>::set_transform(self, transform);
            }
            fn set_paint(&mut self, color: Color) {
                <$canvas>::set_paint(self, color);
            }
            fn set_stroke(&mut self, stroke: Stroke) {
                <$canvas>::set_stroke(self, stroke);
            }
            fn fill_rect(&mut self, rect: &Rect) {
                <$canvas>::fill_rect(self, rect);
            }
            fn fill_path(&mut self, path: &BezPath) {
                <$canvas>::fill_path(self, path);
            }
            fn stroke_path(&mut self, path: &BezPath) {
                <$canvas>::stroke_path(self, path);
            }
            fn push_clip_path(&mut self, path: &BezPath) {
                <$canvas>::push_clip_path(self, path);
            }
            fn pop_clip(&mut self) {
                <$canvas>::pop_clip(self);
            }
            fn glyphs(
                &mut self,
                resources: &mut $resources,
                font: &FontData,
                size: f32,
                glyphs: impl Iterator<Item = Glyph> + Clone,
            ) -> Result<(), Error> {
                self.glyph_run(resources, font)
                    .font_size(size)
                    .hint(true)
                    .fill_glyphs(glyphs)?;
                Ok(())
            }
        }
    };
}

canvas!(RenderContext, vello_cpu::Resources);
canvas!(vello_gpu::Scene, vello_gpu::Resources);

fn color(c: Rgba) -> Color {
    let [r, g, b, a] = c.0;
    Color::from_rgba8(r, g, b, a)
}

fn rounded(r: crate::widget::Rect, outset: f32, radius: f32) -> BezPath {
    let o = f64::from(outset);
    RoundedRect::new(
        f64::from(r.x) - o,
        f64::from(r.y) - o,
        f64::from(r.x + r.w) + o,
        f64::from(r.y + r.h) + o,
        (f64::from(radius) + o).max(0.0),
    )
    .to_path(0.1)
}

fn draw_text<C: Canvas>(
    ctx: &mut C,
    resources: &mut C::Resources,
    layout: &Layout<Brush>,
    (x, y): (f32, f32),
    paint: Rgba,
) -> Result<(), Error> {
    ctx.set_paint(color(paint));
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let glyphs = run.positioned_glyphs().map(|g| Glyph {
                id: g.id,
                x: x + g.x,
                y: y + g.y,
            });
            ctx.glyphs(resources, run.run().font(), run.run().font_size(), glyphs)?;
            // An input method's preedit is underlined, from the font's own metrics.
            if let Some(underline) = &run.style().underline {
                let metrics = run.run().font_metrics();
                let size = underline.size.unwrap_or(metrics.underline_size).max(1.0);
                let offset = underline.offset.unwrap_or(metrics.underline_offset);
                let top = y + run.baseline() - offset;
                let left = x + run.offset();
                ctx.fill_rect(&Rect::new(
                    f64::from(left),
                    f64::from(top),
                    f64::from(left + run.advance()),
                    f64::from(top + size),
                ));
            }
        }
    }
    Ok(())
}

/// Lays the window out and draws it with vello_cpu at `scale` physical pixels per logical pixel.
pub fn render(ui: &mut Ui, scale: f32) -> Result<Pixmap, Error> {
    let (width, height) = layout(ui, scale)?;
    let mut ctx = RenderContext::new(width, height);
    let mut resources = vello_cpu::Resources::new();
    paint(ui, scale, &mut ctx, &mut resources)?;
    ctx.flush();
    let mut pixmap = Pixmap::new(width, height);
    ctx.render(&mut pixmap, &mut resources);
    Ok(pixmap)
}

/// Lays the window out; returns its size in physical pixels at `scale`.
pub fn layout(ui: &mut Ui, scale: f32) -> Result<(u16, u16), Error> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(format!("scale {scale} is not a positive number").into());
    }
    // Whole logical pixels: an integer buffer scale then divides the buffer, as wl_surface
    // requires, and the background covers every pixel.
    let (w, h) = ui.layout();
    let width = u16::try_from((w.ceil() * scale).ceil() as u32)?;
    let height = u16::try_from((h.ceil() * scale).ceil() as u32)?;
    Ok((width, height))
}

/// Draws the window, laid out by [`layout`], on `ctx` at `scale`.
pub fn paint<C: Canvas>(
    ui: &mut Ui,
    scale: f32,
    ctx: &mut C,
    resources: &mut C::Resources,
) -> Result<(), Error> {
    let (w, h) = ui.size();
    let (w, h) = (w.ceil(), h.ceil());
    ctx.set_transform(Affine::scale(f64::from(scale)));
    let t = ui.theme.clone();
    ctx.set_paint(color(t.background));
    ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));

    let focus = ui.focus;
    for id in 0..ui.widgets.len() {
        let widget = &mut ui.widgets[id];
        let r = widget.rect;
        match &mut widget.kind {
            Kind::Column | Kind::Row | Kind::Group(_) => {}
            Kind::TitleBar => {
                ctx.set_paint(color(t.title_bar));
                ctx.fill_rect(&Rect::new(
                    f64::from(r.x),
                    f64::from(r.y),
                    f64::from(r.x + r.w),
                    f64::from(r.bottom()),
                ));
                ctx.set_paint(color(t.border));
                ctx.fill_rect(&Rect::new(
                    f64::from(r.x),
                    f64::from(r.bottom() - 1.0),
                    f64::from(r.x + r.w),
                    f64::from(r.bottom()),
                ));
            }
            // A round button with its mark drawn as lines, as GTK's symbolic icons.
            Kind::WindowButton(button) => {
                let (cx, cy) = (f64::from(r.center_x()), f64::from(r.y + r.h / 2.0));
                ctx.set_paint(color(t.surface));
                ctx.fill_path(&Circle::new((cx, cy), f64::from(r.w / 2.0)).to_path(0.1));
                let arm = f64::from(r.w) * 0.16;
                let lines = match button {
                    WindowButton::Minimize => {
                        vec![Line::new((cx - arm, cy + arm), (cx + arm, cy + arm))]
                    }
                    WindowButton::Close => vec![
                        Line::new((cx - arm, cy - arm), (cx + arm, cy + arm)),
                        Line::new((cx + arm, cy - arm), (cx - arm, cy + arm)),
                    ],
                };
                ctx.set_paint(color(t.text));
                ctx.set_stroke(Stroke::new(1.5));
                for line in lines {
                    ctx.stroke_path(&line.to_path(0.1));
                }
            }
            Kind::Label(_) => {
                if let Some(layout) = &widget.text {
                    draw_text(ctx, resources, layout, (r.x, r.y), t.text)?;
                }
            }
            Kind::Button(_) | Kind::Toggle { .. } => {
                let active = matches!(widget.kind, Kind::Toggle { active: true, .. });
                let (fill, ink) = if active {
                    (t.accent, t.on_accent)
                } else {
                    (t.surface, t.text)
                };
                ctx.set_paint(color(fill));
                ctx.fill_path(&rounded(r, 0.0, t.radius));
                if let Some(layout) = &widget.text {
                    let at = (
                        r.x + (r.w - layout.width()) / 2.0,
                        r.y + (r.h - layout.height()) / 2.0,
                    );
                    draw_text(ctx, resources, layout, at, ink)?;
                }
            }
            Kind::Field(field) => {
                ctx.set_paint(color(t.on_accent));
                ctx.fill_path(&rounded(r, 0.0, t.radius));
                ctx.set_paint(color(t.border));
                ctx.set_stroke(Stroke::new(1.0));
                ctx.stroke_path(&rounded(r, -0.5, t.radius));
                let layout = field.layout(&mut ui.text);
                let x = if ui.rtl {
                    r.x + r.w - t.padding - layout.width()
                } else {
                    r.x + t.padding
                };
                let y = r.y + (r.h - layout.height()) / 2.0;
                let layout = layout.clone();
                // Text longer than the field stays inside it.
                ctx.push_clip_path(&rounded(r, 0.0, t.radius));
                draw_text(ctx, resources, &layout, (x, y), t.text)?;
                if focus == Some(id) {
                    if let Some(c) = field.caret(&mut ui.text, 1.5) {
                        let (x, y) = (f64::from(x), f64::from(y));
                        ctx.set_paint(color(t.text));
                        ctx.fill_rect(&Rect::new(c.x0 + x, c.y0 + y, c.x1 + x, c.y1 + y));
                    }
                }
                ctx.pop_clip();
            }
        }
        if focus == Some(id) {
            ctx.set_paint(color(t.focus));
            ctx.set_stroke(Stroke::new(f64::from(t.focus_width)));
            ctx.stroke_path(&rounded(r, t.focus_width, t.radius));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chooser;
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn hash(pixmap: &Pixmap) -> u64 {
        let mut h = DefaultHasher::new();
        pixmap.data_as_u8_slice().hash(&mut h);
        h.finish()
    }

    /// A field holding "abc", composed or committed, drawn without focus so no caret shows.
    fn field(composed: bool) -> Result<Pixmap, Error> {
        let mut ui = crate::Ui::new("Field", crate::Theme::default(), false);
        let field = ui.field(crate::Ui::ROOT, "Name", "");
        ui.set_focus(Some(field));
        if composed {
            ui.compose("abc", None);
        } else {
            ui.commit("abc");
        }
        ui.set_focus(None);
        render(&mut ui, 1.0)
    }

    #[test]
    fn a_preedit_is_drawn_underlined() -> Result<(), Error> {
        let (composed, committed) = (field(true)?, field(false)?);
        let width = usize::from(composed.width());
        let rows: Vec<usize> = composed
            .data_as_u8_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(committed.data_as_u8_slice().as_chunks::<4>().0)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i / width)
            .collect();
        // The same glyphs, and one thin line more under them.
        let (top, bottom) = (rows.iter().min(), rows.iter().max());
        assert!(rows.len() >= 10, "{} pixels differ", rows.len());
        assert!(
            bottom.zip(top).is_some_and(|(b, t)| b - t <= 2),
            "rows {top:?} to {bottom:?}"
        );
        Ok(())
    }

    #[test]
    fn a_scale_that_is_not_positive_is_refused() {
        let mut ui = chooser::build(&athanor_i18n::Catalog::empty(), false).ui;
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(render(&mut ui, scale).is_err(), "scale {scale}");
        }
    }

    #[test]
    fn the_chooser_renders_deterministically_at_both_scales() -> Result<(), Error> {
        let mut ui = chooser::build(&athanor_i18n::Catalog::empty(), false).ui;
        let w = ui.size().0.ceil();
        for scale in [1.0, 1.5] {
            let first = render(&mut ui, scale)?;
            let second = render(&mut ui, scale)?;
            assert_eq!(hash(&first), hash(&second), "scale {scale}");
            assert_eq!(first.width(), (w * scale).ceil() as u16);

            // The active toggles are filled with the accent, and the text adds the
            // antialiased shades between the theme's colours.
            let pixels = first.data_as_u8_slice().as_chunks::<4>().0;
            let [r, g, b, _] = ui.theme.accent.0;
            assert!(pixels.contains(&[r, g, b, 255]), "scale {scale}");
            let shades: std::collections::HashSet<&[u8; 4]> = pixels.iter().collect();
            assert!(shades.len() > 16, "scale {scale}: {} shades", shades.len());
        }
        Ok(())
    }
}
