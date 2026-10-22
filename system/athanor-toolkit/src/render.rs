//! Drawing: the whole window as one scene, at any scale. [`render`] rasterises it with vello_cpu
//! into a premultiplied RGBA pixmap; `gpu.rs` hands the same scene to vello_gpu.

use crate::text::Brush;
use crate::theme::Rgba;
use crate::widget::{
    knob_rect, label_x, mark_rect, mark_size, slider_knob, slider_knob_x, Kind, Ui,
    WindowButton,
};
use parley::{Layout, PositionedLayoutItem};
use vello_cpu::kurbo::{Affine, BezPath, Circle, Line, Rect, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::{Color, FontData, ImageSampler};
use std::sync::Arc;
use vello_cpu::{Glyph, Image, ImageSource, Pixmap, RenderContext};

/// The error of drawing and of the GPU renderer: any error that can cross threads.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// The calls the window's scene makes, which vello_cpu's `RenderContext` and vello_gpu's
/// `Scene` both offer under the same names, on the types of their shared vello_common.
pub trait Canvas {
    /// What the canvas draws glyphs with and keeps between calls: a glyph cache or a font store.
    type Resources;
    /// Sets the transform applied to what is drawn next.
    fn set_transform(&mut self, transform: Affine);
    /// Sets the colour of the next fills.
    fn set_paint(&mut self, color: Color);
    /// Sets the stroke of the next strokes.
    fn set_stroke(&mut self, stroke: Stroke);
    /// Fills a rectangle with the current paint.
    fn fill_rect(&mut self, rect: &Rect);
    /// Fills a path with the current paint.
    fn fill_path(&mut self, path: &BezPath);
    /// Strokes a path with the current stroke and paint.
    fn stroke_path(&mut self, path: &BezPath);
    /// Fills `rect` with an image, scaled to it on each axis: the caller keeps the aspect ratio.
    /// vello_cpu reads the pixmap as it draws; vello_gpu draws the copy uploaded to its atlas
    /// before the frame (`gpu.rs`), and draws nothing for a pixmap it has no copy of.
    fn fill_image(&mut self, resources: &Self::Resources, rect: &Rect, pixmap: &Arc<Pixmap>);
    /// Limits what is drawn next to the inside of a path, until the matching [`Canvas::pop_clip`].
    fn push_clip_path(&mut self, path: &BezPath);
    /// Ends the innermost clip.
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
    ($canvas:ty, $resources:ty, $image_source:path, $glyph_resources:path) => {
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
            fn fill_image(&mut self, resources: &$resources, rect: &Rect, pixmap: &Arc<Pixmap>) {
                let Some(source) = $image_source(resources, pixmap) else {
                    return;
                };
                let image = Image {
                    image: source,
                    sampler: ImageSampler::default(),
                };
                <$canvas>::set_paint(self, image);
                <$canvas>::set_paint_transform(
                    self,
                    Affine::translate((rect.x0, rect.y0))
                        * Affine::scale_non_uniform(
                            rect.width() / f64::from(pixmap.width()),
                            rect.height() / f64::from(pixmap.height()),
                        ),
                );
                <$canvas>::fill_rect(self, rect);
                <$canvas>::set_paint_transform(self, Affine::IDENTITY);
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
                self.glyph_run($glyph_resources(resources), font)
                    .font_size(size)
                    .hint(true)
                    .fill_glyphs(glyphs)?;
                Ok(())
            }
        }
    };
}

/// vello_cpu draws an image straight from its pixmap.
fn cpu_image(_: &vello_cpu::Resources, pixmap: &Arc<Pixmap>) -> Option<ImageSource> {
    Some(ImageSource::Pixmap(Arc::clone(pixmap)))
}

/// vello_cpu's resources are its glyph cache.
fn cpu_glyphs(resources: &mut vello_cpu::Resources) -> &mut vello_cpu::Resources {
    resources
}

canvas!(RenderContext, vello_cpu::Resources, cpu_image, cpu_glyphs);
#[cfg(feature = "gpu")]
canvas!(
    vello_gpu::Scene,
    crate::gpu::Resources,
    crate::gpu::Resources::image,
    crate::gpu::Resources::glyphs
);

fn color(c: Rgba) -> Color {
    let [r, g, b, a] = c.0;
    Color::from_rgba8(r, g, b, a)
}

/// `base` with `part` of `ink` mixed in: a hovered control's fill, darker or lighter toward its
/// text as libadwaita's, whose buttons go from 10 to 15 percent of the text colour on hover.
fn mix(base: Rgba, ink: Rgba, part: f32) -> Rgba {
    let channel = |b: u8, i: u8| (f32::from(b) + (f32::from(i) - f32::from(b)) * part).round() as u8;
    let ([r, g, b, a], [ri, gi, bi, _]) = (base.0, ink.0);
    Rgba([channel(r, ri), channel(g, gi), channel(b, bi), a])
}

/// How much of its text colour a hovered control's fill takes.
const HOVER: f32 = 0.05;

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
    Ok((physical(w, scale)?, physical(h, scale)?))
}

/// `logical` pixels, rounded up to whole ones, in physical pixels at `scale`, as
/// wp_fractional_scale_v1 computes them: in integers from the scale's 120ths, rounding half away
/// from zero. Integer scales give an exact multiple.
fn physical(logical: f32, scale: f32) -> Result<u16, Error> {
    let scale120 = ((scale * 120.0).round() as u64).max(1);
    let logical = logical.ceil() as u64;
    Ok(u16::try_from((logical * scale120 + 60) / 120)?)
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

    let (focus, hover) = (ui.focus, ui.hovered());
    for id in 0..ui.widgets.len() {
        if !ui.is_visible(id) {
            continue;
        }
        // Inside a scroll, only what its viewport, rounded, shows.
        let clip = ui.clip(id);
        if clip.is_some_and(|c| !overlaps(ui.widgets[id].rect, c)) {
            continue;
        }
        if let Some(c) = clip {
            ctx.push_clip_path(&rounded(c, 0.0, t.radius));
        }
        let widget = &mut ui.widgets[id];
        let r = widget.rect;
        let multiline = matches!(widget.kind, Kind::TextArea(_));
        match &mut widget.kind {
            Kind::Column | Kind::Row | Kind::Group(_) | Kind::Menu | Kind::Scroll { .. } => {}
            // A white sheet under its rows, as GTK's list.
            Kind::List { .. } => {
                ctx.set_paint(color(t.on_accent));
                ctx.fill_path(&rounded(r, 0.0, t.radius));
            }
            // A row with its text at the start edge: tinted with the accent when selected, and
            // toward the text under the pointer.
            Kind::ListRow(_) => {
                let selected = widget.parent.and_then(|l| ui.selected(l)) == Some(id);
                let fill = if selected {
                    Some(mix(t.on_accent, t.accent, SELECTED))
                } else {
                    None
                };
                let fill = match (fill, hover == Some(id)) {
                    (fill, true) => Some(mix(fill.unwrap_or(t.on_accent), t.text, HOVER)),
                    (fill, false) => fill,
                };
                if let Some(fill) = fill {
                    ctx.set_paint(color(fill));
                    ctx.fill_path(&rounded(r, -1.0, t.radius - 1.0));
                }
                let widget = &ui.widgets[id];
                if let Some(layout) = &widget.text {
                    let x = if ui.rtl {
                        r.x + r.w - t.padding - layout.width()
                    } else {
                        r.x + t.padding
                    };
                    let at = (x, r.y + (r.h - layout.height()) / 2.0);
                    draw_text(ctx, resources, layout, at, t.text)?;
                }
            }
            Kind::Separator => {
                let y = f64::from((r.y + r.h / 2.0).floor());
                ctx.set_paint(color(t.border));
                ctx.fill_rect(&Rect::new(
                    f64::from(r.x),
                    y,
                    f64::from(r.x + r.w),
                    y + 1.0,
                ));
            }
            // A flat row with its label at the start edge, filled only under the pointer.
            Kind::MenuItem(_) => {
                if hover == Some(id) {
                    ctx.set_paint(color(mix(t.background, t.text, 2.0 * HOVER)));
                    ctx.fill_path(&rounded(r, 0.0, t.radius));
                }
                if let Some(layout) = &widget.text {
                    let x = if ui.rtl {
                        r.x + r.w - t.padding - layout.width()
                    } else {
                        r.x + t.padding
                    };
                    let at = (x, r.y + (r.h - layout.height()) / 2.0);
                    draw_text(ctx, resources, layout, at, t.text)?;
                }
            }
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
                let fill = if hover == Some(id) {
                    mix(t.surface, t.text, HOVER)
                } else {
                    t.surface
                };
                ctx.set_paint(color(fill));
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
            Kind::Image { image, .. } => {
                // The image keeps its aspect ratio, centred in its square.
                let (w, h) = (f64::from(image.width()), f64::from(image.height()));
                let fit = (f64::from(r.w) / w).min(f64::from(r.h) / h);
                let (w, h) = (w * fit, h * fit);
                let x = f64::from(r.x) + (f64::from(r.w) - w) / 2.0;
                let y = f64::from(r.y) + (f64::from(r.h) - h) / 2.0;
                ctx.fill_image(resources, &Rect::new(x, y, x + w, y + h), image.pixmap());
            }
            Kind::Label(_) | Kind::Tooltip(_) => {
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
                let fill = if hover == Some(id) {
                    mix(fill, ink, HOVER)
                } else {
                    fill
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
            Kind::Check { active, .. } | Kind::Switch { active, .. } => {
                let active = *active;
                let is_switch = matches!(widget.kind, Kind::Switch { .. });
                let size = mark_size(is_switch, t.font_size);
                let mark = mark_rect(r, ui.rtl, size, is_switch);
                let fill = if active { t.accent } else { t.surface };
                let radius = if is_switch { mark.h / 2.0 } else { mark.h / 4.0 };
                ctx.set_paint(color(fill));
                ctx.fill_path(&rounded(mark, 0.0, radius));
                if !active {
                    ctx.set_paint(color(t.border));
                    ctx.set_stroke(Stroke::new(1.0));
                    ctx.stroke_path(&rounded(mark, -0.5, radius));
                }
                if is_switch {
                    let knob = knob_rect(mark, ui.rtl, active);
                    let centre = (f64::from(knob.center_x()), f64::from(knob.y + knob.h / 2.0));
                    ctx.set_paint(color(if active { t.on_accent } else { t.border }));
                    ctx.fill_path(&Circle::new(centre, f64::from(knob.w / 2.0)).to_path(0.1));
                } else if active {
                    // A tick drawn as two lines, as the window buttons' marks.
                    let (x, y, s) = (f64::from(mark.x), f64::from(mark.y), f64::from(mark.w));
                    ctx.set_paint(color(t.on_accent));
                    ctx.set_stroke(Stroke::new(1.8));
                    for (from, to) in [((0.24, 0.52), (0.42, 0.70)), ((0.42, 0.70), (0.76, 0.30))] {
                        let line = Line::new((x + from.0 * s, y + from.1 * s), (x + to.0 * s, y + to.1 * s));
                        ctx.stroke_path(&line.to_path(0.1));
                    }
                }
                if let Some(layout) = &widget.text {
                    let x = label_x(r, mark, ui.rtl, is_switch, t.gap, layout.width());
                    let y = r.y + (r.h - layout.height()) / 2.0;
                    draw_text(ctx, resources, layout, (x, y), t.text)?;
                }
            }
            // A track, filled with the accent from min to the knob, and a round knob on it.
            Kind::Slider(slider) => {
                let d = slider_knob(t.font_size);
                let fraction = (slider.value - slider.min) / (slider.max - slider.min);
                let x_min = slider_knob_x(r, ui.rtl, t.font_size, 0.0);
                let x_knob = slider_knob_x(r, ui.rtl, t.font_size, fraction);
                let x_max = slider_knob_x(r, ui.rtl, t.font_size, 1.0);
                let (cy, thick) = (r.y + r.h / 2.0, 4.0);
                let bar = |from: f32, to: f32| crate::widget::Rect {
                    x: from.min(to),
                    y: cy - thick / 2.0,
                    w: (to - from).abs(),
                    h: thick,
                };
                ctx.set_paint(color(t.border));
                ctx.fill_path(&rounded(bar(x_min, x_max), 0.0, thick / 2.0));
                ctx.set_paint(color(t.accent));
                ctx.fill_path(&rounded(bar(x_min, x_knob), 0.0, thick / 2.0));
                let knob = Circle::new((f64::from(x_knob), f64::from(cy)), f64::from(d / 2.0));
                let fill = if hover == Some(id) {
                    mix(t.on_accent, t.text, HOVER)
                } else {
                    t.on_accent
                };
                ctx.set_paint(color(fill));
                ctx.fill_path(&knob.to_path(0.1));
                ctx.set_paint(color(t.border));
                ctx.set_stroke(Stroke::new(1.0));
                let edge = Circle::new(knob.center, knob.radius - 0.5);
                ctx.stroke_path(&edge.to_path(0.1));
            }
            Kind::Field(field) | Kind::TextArea(field) => {
                ctx.set_paint(color(t.on_accent));
                ctx.fill_path(&rounded(r, 0.0, t.radius));
                ctx.set_paint(color(t.border));
                ctx.set_stroke(Stroke::new(1.0));
                ctx.stroke_path(&rounded(r, -0.5, t.radius));
                let offset = field.offset;
                let layout = field.layout(&mut ui.text);
                // A text area's lines are laid out at its width, which aligns them to the start
                // edge; it shows them from its scroll offset down.
                let x = if ui.rtl && !multiline {
                    r.x + r.w - t.padding - layout.width()
                } else {
                    r.x + t.padding
                };
                let y = if multiline {
                    r.y + t.padding / 2.0 - offset
                } else {
                    r.y + (r.h - layout.height()) / 2.0
                };
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
            // A row's ring stays inside it, where its list's edge and the viewport do not cut it.
            let row = matches!(ui.widgets[id].kind, Kind::ListRow(_));
            let (outset, radius) = if row {
                (-t.focus_width / 2.0 - 1.0, t.radius - 1.0)
            } else {
                (t.focus_width, t.radius)
            };
            ctx.set_paint(color(t.focus));
            ctx.set_stroke(Stroke::new(f64::from(t.focus_width)));
            ctx.stroke_path(&rounded(r, outset, radius));
        }
        if clip.is_some() {
            ctx.pop_clip();
        }
    }
    for id in 0..ui.widgets.len() {
        if let Some(bar) = scroll_indicator(ui, id) {
            ctx.set_paint(color(t.border));
            ctx.fill_path(&rounded(bar, 0.0, bar.w / 2.0));
        }
    }
    Ok(())
}

/// How much of the accent a selected list row's fill takes.
const SELECTED: f32 = 0.15;

/// Whether two rectangles share any area.
fn overlaps(a: crate::widget::Rect, b: crate::widget::Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.bottom() && b.y < a.bottom()
}

/// The thin bar at the end edge of the visible scroll `id` whose content is taller than its
/// viewport: as long as the viewport is of the content, at least 16 logical pixels, and as far
/// down it as the content is scrolled.
fn scroll_indicator(ui: &Ui, id: crate::widget::WidgetId) -> Option<crate::widget::Rect> {
    let Kind::Scroll { height, offset } = ui.widgets[id].kind else {
        return None;
    };
    let content = ui.content_height(id);
    if content <= height || !ui.is_visible(id) {
        return None;
    }
    let (r, w, inset) = (ui.widgets[id].rect, 3.0, 2.0);
    let h = (height * height / content).max(16.0).min(height);
    let x = if ui.rtl { r.x + inset } else { r.x + r.w - inset - w };
    let y = r.y + (height - h) * offset / (content - height);
    Some(crate::widget::Rect { x, y, w, h })
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
    fn the_physical_size_rounds_as_the_fractional_scale_protocol_does() -> Result<(), Error> {
        // 101 at 1.25 is 126.25: 126, where a ceiling gives 127.
        assert_eq!(physical(101.0, 150.0 / 120.0)?, 126);
        // 100 at 1.2 is 120, where 1.2 in f32 times 100 rounds up to 121.
        assert_eq!(physical(100.0, 144.0 / 120.0)?, 120);
        // Half a pixel rounds away from zero; integer scales multiply.
        assert_eq!(physical(101.0, 1.5)?, 152);
        assert_eq!(physical(99.5, 2.0)?, 200);
        Ok(())
    }

    #[test]
    fn a_hovered_control_is_drawn_toward_its_text() -> Result<(), Error> {
        let mut ui = crate::Ui::new("Hover", crate::Theme::default(), false);
        let button = ui.add(crate::Ui::ROOT, crate::Kind::Button("Files".into()));
        let plain = render(&mut ui, 1.0)?;
        let r = ui.widget(button).rect;
        ui.pointer_motion(r.x + 1.0, r.y + r.h / 2.0);
        let hovered = render(&mut ui, 1.0)?;
        let t = &ui.theme;
        let fill = mix(t.surface, t.text, HOVER).0;
        let at = |p: &Pixmap| {
            let i = 4 * (usize::from(p.width()) * (r.y + r.h / 2.0) as usize + (r.x + 4.0) as usize);
            p.data_as_u8_slice()[i..i + 4].to_vec()
        };
        assert_eq!(at(&plain), t.surface.0.to_vec());
        assert_eq!(at(&hovered), fill.to_vec());
        assert_ne!(fill, t.surface.0);
        ui.pointer_leave();
        assert_eq!(hash(&render(&mut ui, 1.0)?), hash(&plain));
        Ok(())
    }

    #[test]
    fn a_slider_is_drawn_on_its_track_at_a_fractional_scale() -> Result<(), Error> {
        let mut ui = crate::Ui::new("Slider", crate::Theme::default(), false);
        let id = ui.slider(crate::Ui::ROOT, "Level", 0.0, 100.0, 1.0, 50.0);
        let id = id.unwrap_or(crate::Ui::ROOT);
        let scale = 1.5;
        let pixmap = render(&mut ui, scale)?;
        let (r, font) = (ui.widget(id).rect, ui.theme.font_size);
        let at = |x: f32, y: f32| {
            let (px, py) = ((x * scale) as usize, (y * scale) as usize);
            let i = 4 * (usize::from(pixmap.width()) * py + px);
            pixmap.data_as_u8_slice()[i..i + 4].to_vec()
        };
        let cy = r.y + r.h / 2.0;
        let knob = crate::widget::slider_knob_x(r, false, font, 0.5);
        let (from, to) = (
            crate::widget::slider_knob_x(r, false, font, 0.0),
            crate::widget::slider_knob_x(r, false, font, 1.0),
        );
        let t = &ui.theme;
        // The knob's centre, the accent before it and the plain track after it, at the scaled
        // pixels, and nothing on the rows above the track.
        assert_eq!(at(knob, cy), t.on_accent.0.to_vec());
        assert_eq!(at((from + knob) / 2.0, cy), t.accent.0.to_vec());
        assert_eq!(at((knob + to) / 2.0, cy), t.border.0.to_vec());
        assert_eq!(at((from + knob) / 2.0, cy - 6.0), t.background.0.to_vec());
        // The track and the knob stay inside the widget's rectangle.
        assert!(from - slider_knob_edge(font) >= r.x - 0.01);
        assert!(to + slider_knob_edge(font) <= r.x + r.w + 0.01);
        Ok(())
    }

    fn slider_knob_edge(font: f32) -> f32 {
        crate::widget::slider_knob(font) / 2.0
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

    #[test]
    fn a_scroll_shows_only_its_viewport_and_an_indicator_at_the_end_edge() -> Result<(), Error> {
        for rtl in [false, true] {
            let mut ui = crate::Ui::new("Scroll", crate::Theme::default(), rtl);
            let scroll = ui.scroll(crate::Ui::ROOT, 100.0);
            let list = ui.list(scroll, "Rows");
            let rows: Vec<_> = (1..=50).map(|n| ui.list_row(list, &format!("Row {n}"))).collect();
            ui.add(crate::Ui::ROOT, crate::Kind::Button("After".into()));
            let pixmap = render(&mut ui, 1.0)?;
            let at = |x: f32, y: f32| {
                let i = 4 * (usize::from(pixmap.width()) * y as usize + x as usize);
                pixmap.data_as_u8_slice()[i..i + 4].to_vec()
            };
            let (view, t) = (ui.widget(scroll).rect, &ui.theme);
            // Inside the viewport the list's sheet shows; in the gap under it, the background,
            // though the list runs on far below.
            let middle = view.x + view.w / 2.0;
            assert_eq!(at(middle, view.y + 4.0), t.on_accent.0.to_vec(), "rtl {rtl}");
            assert!(ui.widget(rows[49]).rect.y > view.bottom() + t.margin);
            assert_eq!(at(middle, view.bottom() + 2.0), t.background.0.to_vec(), "rtl {rtl}");
            // The indicator sits at the top of the end edge: the right in LTR, the left in RTL.
            let edge = if rtl { view.x + 3.5 } else { view.x + view.w - 3.5 };
            let far = if rtl { view.x + view.w - 3.5 } else { view.x + 3.5 };
            assert_eq!(at(edge, view.y + 8.0), t.border.0.to_vec(), "rtl {rtl}");
            assert_ne!(at(far, view.y + 8.0), t.border.0.to_vec(), "rtl {rtl}");
            assert_ne!(at(edge, view.bottom() - 8.0), t.border.0.to_vec(), "rtl {rtl}");
        }
        Ok(())
    }
}
