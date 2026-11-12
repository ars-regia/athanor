//! A label that wraps, as GTK's `GtkLabel` with `wrap`, `max-width-chars`, `lines` and
//! `ellipsize` at the end: see [`Wrap`] and [`crate::Ui::set_wrap`].

use std::num::NonZeroU32;

use parley::Layout;

use crate::text::{Brush, TextContext, TextStyle};

/// How a [`crate::Kind::Label`] wraps. A wrapping label asks its parent for the width of its
/// longest word, or of `max_width_chars` characters when that is wider, and never for more than
/// its text on one line, as GTK's minimum and `max-width-chars` ask; it then breaks its lines
/// at the width its place gives it. A column or a group gives it its whole width and a row its
/// cell, as they give a row. Start from [`Wrap::default`], which wraps without other limits, and
/// set the fields that differ.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Wrap {
    /// The width the label asks for at most, in characters of its font, as GTK's
    /// `max-width-chars`: what keeps a long sentence from making its window wide. A character is
    /// Pango's approximate width, the wider of the mean of a sample sentence's characters and of
    /// its digits'.
    pub max_width_chars: Option<NonZeroU32>,
    /// The most lines the label shows, as GTK's `lines` with `ellipsize` at the end: a text that
    /// needs more is cut, and its last line shown ends with an ellipsis. Assistive technologies
    /// still read the whole text.
    pub lines: Option<NonZeroU32>,
}

/// Pango's sample sentence for English, whose mean character gives the approximate width.
const SAMPLE: &str = "The quick brown fox jumps over the lazy dog.";
const DIGITS: &str = "0123456789";
const ELLIPSIS: char = '\u{2026}';

/// Pango's approximate character width at `size`: the wider of the sample's mean character and
/// its mean digit.
fn char_width(cx: &mut TextContext, size: f32, style: &TextStyle) -> f32 {
    let mut mean = |text: &str| {
        let width = cx.layout_at(text, size, false, style, None).full_width();
        width / text.chars().count() as f32
    };
    mean(SAMPLE).max(mean(DIGITS))
}

/// The width `text` asks for under `wrap`: its longest word, or `max_width_chars` characters when
/// wider, at most its width on one line.
pub(crate) fn request(
    cx: &mut TextContext,
    text: &str,
    (size, rtl): (f32, bool),
    style: &TextStyle,
    wrap: &Wrap,
) -> f32 {
    let widths = cx
        .layout_at(text, size, rtl, style, None)
        .calculate_content_widths();
    let chars = wrap
        .max_width_chars
        .map_or(0.0, |n| n.get() as f32 * char_width(cx, size, style));
    widths.max.min(widths.min.max(chars))
}

/// `text` broken at `width`, cut to `wrap.lines` lines with an ellipsis at the end of the last one
/// when it needs more.
pub(crate) fn layout(
    cx: &mut TextContext,
    text: &str,
    (size, rtl): (f32, bool),
    style: &TextStyle,
    wrap: &Wrap,
    width: f32,
) -> Layout<Brush> {
    let full = cx.layout_at(text, size, rtl, style, Some(width));
    let Some(limit) = wrap.lines.map(|n| n.get() as usize) else {
        return full;
    };
    let Some(last) = full
        .get(limit.saturating_sub(1))
        .filter(|_| full.len() > limit)
    else {
        return full;
    };
    // The text up to the end of the last line shown, then one character fewer until it fits with
    // the ellipsis: Pango's end ellipsis, by characters.
    let mut cut = text[..last.text_range().end].trim_end().to_owned();
    loop {
        let candidate = format!("{cut}{ELLIPSIS}");
        let shaped = cx.layout_at(&candidate, size, rtl, style, Some(width));
        if shaped.len() <= limit || cut.is_empty() {
            return shaped;
        }
        cut.pop();
        cut.truncate(cut.trim_end().len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, Theme, Ui, WidgetId};

    const SENTENCE: &str = "Your layout.toml could not be read and was left unchanged. The nearest \
        layout is in use; choosing one here replaces it.";

    fn wrap(max_width_chars: u32, lines: u32) -> Wrap {
        Wrap {
            max_width_chars: NonZeroU32::new(max_width_chars),
            lines: NonZeroU32::new(lines),
        }
    }

    /// A window at least 300 px wide that holds one label.
    fn window(text: &str, how: Option<Wrap>, rtl: bool) -> (Ui, WidgetId) {
        let mut ui = Ui::new("Wrap", Theme::default(), rtl);
        ui.min_width = 300.0;
        let label = ui.add(Ui::ROOT, Kind::Label(text.into()));
        if let Some(how) = how {
            assert!(ui.set_wrap(label, Some(how)));
        }
        ui.layout();
        (ui, label)
    }

    /// Whether every line's ink lies within `width`: the whitespace that hangs past a line's end
    /// edge, the left one in right to left, is not drawn.
    fn inked_within(layout: &Layout<Brush>, width: f32, rtl: bool) -> bool {
        layout.lines().all(|line| {
            let m = line.metrics();
            let (left, right) = if rtl {
                (m.inline_min_coord + m.hanging_advance, m.inline_max_coord)
            } else {
                (m.inline_min_coord, m.inline_max_coord - m.hanging_advance)
            };
            left >= -0.5 && right <= width + 0.5
        })
    }

    fn shaped(ui: &Ui, id: WidgetId) -> &Layout<Brush> {
        ui.widgets[id].text.as_ref().expect("a laid out label")
    }

    #[test]
    fn a_wrapping_label_keeps_its_window_narrow_and_grows_it_down() {
        let (plain, label) = window(SENTENCE, None, false);
        assert!(plain.size().0 > 300.0, "one line: {}", plain.size().0);
        assert_eq!(shaped(&plain, label).len(), 1);
        let (short, _) = window("Short.", Some(Wrap::default()), false);
        for rtl in [false, true] {
            let (ui, label) = window(SENTENCE, Some(Wrap::default()), rtl);
            assert_eq!(ui.size().0, 300.0, "rtl {rtl}");
            let r = ui.widget(label).rect;
            let margin = ui.theme.margin;
            assert_eq!((r.x, r.w), (margin, 300.0 - 2.0 * margin), "rtl {rtl}");
            let layout = shaped(&ui, label);
            assert!(layout.len() > 1, "rtl {rtl}");
            assert!(inked_within(layout, r.w, rtl), "rtl {rtl}");
            assert_eq!(r.h, layout.height(), "rtl {rtl}");
            assert!(ui.size().1 > short.size().1, "rtl {rtl}");
        }
    }

    #[test]
    fn max_width_chars_caps_the_width_asked_and_a_short_text_asks_its_own() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        let at = (15.0, false);
        let ten = request(&mut cx, SENTENCE, at, &style, &wrap(10, 0));
        let forty = request(&mut cx, SENTENCE, at, &style, &wrap(40, 0));
        assert!(ten < forty, "{ten} < {forty}");
        assert!(forty < cx.layout(SENTENCE, 15.0, false, &style).width());
        let hi = request(&mut cx, "Hi", at, &style, &wrap(40, 0));
        assert!((hi - cx.layout("Hi", 15.0, false, &style).width()).abs() < 0.5);
        let word = request(&mut cx, SENTENCE, at, &style, &Wrap::default());
        let longest = cx.layout("unchanged.", 15.0, false, &style).width();
        assert!((word - longest).abs() < 1.0, "the longest word: {word} against {longest}");
    }

    #[test]
    fn a_line_limit_ends_the_last_line_with_an_ellipsis() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        for rtl in [false, true] {
            for limit in [1, 2] {
                let layout = layout(&mut cx, SENTENCE, (15.0, rtl), &style, &wrap(0, limit), 160.0);
                assert_eq!(layout.len(), limit as usize, "rtl {rtl}");
                assert!(inked_within(&layout, 160.0, rtl), "rtl {rtl}");
                // The shaped text is a prefix of the sentence and the ellipsis.
                let end = layout.lines().last().map_or(0, |l| l.text_range().end);
                let kept = end - ELLIPSIS.len_utf8();
                assert!(kept < SENTENCE.len() && SENTENCE.is_char_boundary(kept), "rtl {rtl}");
            }
        }
        let fits = layout(&mut cx, "Fits.", (15.0, false), &style, &wrap(0, 1), 160.0);
        assert_eq!(fits.lines().last().map(|l| l.text_range().end), Some(5));
    }

    #[test]
    fn a_cut_label_keeps_its_whole_text() {
        let (ui, label) = window(SENTENCE, Some(wrap(0, 2)), false);
        assert_eq!(shaped(&ui, label).len(), 2);
        assert!(matches!(&ui.widget(label).kind, Kind::Label(t) if t == SENTENCE));
    }

    #[test]
    fn set_wrap_takes_only_a_label_and_follows_a_change_of_text() {
        let mut ui = Ui::new("Wrap", Theme::default(), false);
        ui.min_width = 300.0;
        let button = ui.add(Ui::ROOT, Kind::Button("Go".into()));
        assert!(!ui.set_wrap(button, Some(Wrap::default())));
        let label = ui.add(Ui::ROOT, Kind::Label("Short.".into()));
        assert!(ui.set_wrap(label, Some(Wrap::default())));
        ui.layout();
        assert_eq!(shaped(&ui, label).len(), 1);
        ui.set_text(label, SENTENCE);
        assert_eq!(ui.layout().0, 300.0);
        assert!(shaped(&ui, label).len() > 1);
        assert!(ui.set_wrap(label, None));
        assert!(ui.layout().0 > 300.0, "on one line it widens the window again");
    }

    #[test]
    fn a_wrapping_label_in_a_row_takes_its_cell() {
        let mut ui = Ui::new("Wrap", Theme::default(), false);
        ui.min_width = 400.0;
        let row = ui.add(Ui::ROOT, Kind::Row);
        ui.add(row, Kind::Button("Left".into()));
        let label = ui.add(row, Kind::Label(SENTENCE.into()));
        assert!(ui.set_wrap(label, Some(Wrap::default())));
        assert_eq!(ui.layout().0, 400.0);
        let cell = ui.widget(label).rect;
        let layout = shaped(&ui, label);
        assert!(layout.len() > 1);
        assert!(inked_within(layout, cell.w, false));
        assert!(ui.widget(row).rect.h >= cell.h, "the row is as tall as the wrapped text");
    }
}
