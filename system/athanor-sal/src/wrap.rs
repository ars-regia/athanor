//! A label that wraps, as GTK's `GtkLabel` with `wrap`, `max-width-chars`, `lines` and
//! `ellipsize` at the end: see [`Wrap`] and [`crate::Ui::set_wrap`].

use std::num::NonZeroU32;

use parley::{Layout, Line};

use crate::text::{Brush, TextContext, TextStyle};

/// How a [`crate::Kind::Label`] wraps. A wrapping label asks its parent for the width of its
/// longest word, or of `max_width_chars` characters when that is wider, and never for more than
/// its text on one line, as GTK's minimum and `max-width-chars` ask; it then breaks its lines
/// at the width its place gives it. A column or a group gives it its whole width and a row its
/// cell, as they give a row. Start from [`Wrap::default`], which wraps without other limits, and
/// set the fields that differ: the struct is `#[non_exhaustive]`, so other crates cannot write
/// it as a literal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Wrap {
    /// The width the label asks for at most, in characters of its font, as GTK's
    /// `max-width-chars`: what keeps a long sentence from making its window wide. A character is
    /// Pango's approximate width, the wider of the mean of a sample sentence's characters and of
    /// its digits'.
    pub max_width_chars: Option<NonZeroU32>,
    /// The most lines the label shows, as GTK's `lines` with `ellipsize` at the end: a text that
    /// needs more, or whose last line shown is wider than the label, is cut, and its last line
    /// shown ends with an ellipsis. Assistive technologies still read the whole text.
    pub lines: Option<NonZeroU32>,
}

/// Pango's sample sentence for English, whose mean character gives the approximate width.
const SAMPLE: &str = "The quick brown fox jumps over the lazy dog.";
const DIGITS: &str = "0123456789";
const ELLIPSIS: char = '\u{2026}';
/// The most (size, style) pairs a [`TextContext`] keeps the character width of: a window uses a
/// handful, and the list starts again when it is full.
const CHAR_WIDTHS: usize = 16;

/// Pango's approximate character width at `size`: the wider of the sample's mean character and
/// its mean digit. It depends on the size and the style only, so each pair is shaped once per
/// [`TextContext`], up to [`CHAR_WIDTHS`] pairs. A size that is not a finite number has no
/// width: 0, not kept, so `max_width_chars` asks for nothing.
fn char_width(cx: &mut TextContext, size: f32, style: &TextStyle) -> f32 {
    if !(size * style.scale).is_finite() {
        return 0.0;
    }
    let known = cx
        .char_widths
        .iter()
        .find(|(s, st, _)| *s == size && st == style);
    if let Some(&(_, _, width)) = known {
        return width;
    }
    let mut mean = |text: &str| {
        let width = cx.layout_at(text, size, false, style, None).full_width();
        width / text.chars().count() as f32
    };
    let width = mean(SAMPLE).max(mean(DIGITS));
    if cx.char_widths.len() >= CHAR_WIDTHS {
        cx.char_widths.clear();
    }
    cx.char_widths.push((size, style.clone(), width));
    width
}

/// The narrowest `text` can be under `wrap`, its longest word, and the width it asks for: its
/// longest word, or `max_width_chars` characters when wider, at most its width on one line.
pub(crate) fn request(
    cx: &mut TextContext,
    text: &str,
    (size, rtl): (f32, bool),
    style: &TextStyle,
    wrap: &Wrap,
) -> (f32, f32) {
    let widths = cx
        .layout_at(text, size, rtl, style, None)
        .calculate_content_widths();
    let chars = wrap
        .max_width_chars
        .map_or(0.0, |n| n.get() as f32 * char_width(cx, size, style));
    (widths.min, widths.max.min(widths.min.max(chars)))
}

/// `text` broken at `width`, cut to `wrap.lines` lines with an ellipsis at the end of the last one
/// when it needs more.
pub(crate) fn layout(
    cx: &mut TextContext,
    text: &str,
    at: (f32, bool),
    style: &TextStyle,
    wrap: &Wrap,
    width: f32,
) -> Layout<Brush> {
    let full = cx.layout_at(text, at.0, at.1, style, Some(width));
    match wrap.lines {
        Some(limit) => cut(cx, text, at, style, (limit.get() as usize, width), full).1,
        None => full,
    }
}

/// The text shown of `text`, whose layout at `width` is `full`, and its layout, in `limit` lines:
/// the whole text when it fits; otherwise the most of it that fits with an ellipsis after it, cut
/// between two clusters from the start of the last line shown to the end of the text, which keeps
/// grapheme clusters whole, as Pango's end ellipsis. Pango lays the rest of the text on the last
/// line shown before it cuts it, so a word that broke to the next line is cut where the room ends:
/// "Not connected" in a narrow label shows "Not connec…", not "Not…". The cut is found by
/// bisection over those clusters.
fn cut(
    cx: &mut TextContext,
    text: &str,
    (size, rtl): (f32, bool),
    style: &TextStyle,
    (limit, width): (usize, f32),
    full: Layout<Brush>,
) -> (String, Layout<Brush>) {
    let fits = |layout: &Layout<Brush>| fits(layout, limit, width);
    // An empty text has nothing to cut, though parley gives its line a range past its end.
    if fits(&full) || text.is_empty() {
        return (text.to_owned(), full);
    }
    let shown = limit.min(full.len()).saturating_sub(1);
    let Some(last) = full.get(shown) else {
        return (text.to_owned(), full);
    };
    let end = last.text_range().end.min(text.len());
    let cuts = cuts(full.lines().skip(shown), text);
    let start = cuts[0];
    // Only whitespace after the last line shown, such as a final newline: the text without it.
    if text[end..].trim().is_empty() {
        let kept = text.trim_end();
        let shaped = cx.layout_at(kept, size, rtl, style, Some(width));
        if fits(&shaped) {
            return (kept.to_owned(), shaped);
        }
    }
    let mut shape = |cut: usize| {
        let shown = format!("{}{ELLIPSIS}", text[..cut].trim_end());
        let layout = cx.layout_at(&shown, size, rtl, style, Some(width));
        (shown, layout)
    };
    // The largest cut that fits: less text needs no more room, so every cut below it fits too.
    let (mut lo, mut hi, mut best) = (0, cuts.len(), None);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = shape(cuts[mid]);
        if fits(&candidate.1) {
            best = Some(candidate);
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // Not even the line's start fits: the lines before it and the ellipsis, as narrow as it gets.
    best.unwrap_or_else(|| shape(start))
}

/// Whether `layout` holds at most `limit` lines, the last of them no wider than `width`, its
/// hanging whitespace aside; half a pixel for the rounding of advances. A wider line before it is
/// a word that cannot break, which no cut of the last line narrows.
fn fits(layout: &Layout<Brush>, limit: usize, width: f32) -> bool {
    layout.len() <= limit
        && layout.lines().last().is_none_or(|line| {
            let m = line.metrics();
            m.advance - m.hanging_advance <= width + 0.5
        })
}

/// Where `lines` of `text` can be cut, in order: the start and the end of each, and the start of
/// each of their clusters. Parley's clusters are the extended grapheme clusters of UAX #29, as ICU
/// segments them, so no cut parts a letter from its marks, a flag's two regional indicators or an
/// emoji sequence joined by ZWJ. Never empty when `lines` holds a line.
fn cuts<'a>(lines: impl IntoIterator<Item = Line<'a, Brush>>, text: &str) -> Vec<usize> {
    // Parley's ranges can run one byte past the text, over the line's end: within the text.
    let within = |at: usize| at.min(text.len());
    let mut cuts = Vec::new();
    for line in lines {
        let range = line.text_range();
        let clusters = line.runs().flat_map(|run| run.clusters());
        cuts.extend(clusters.map(|cluster| within(cluster.text_range().start)));
        cuts.extend([within(range.start), within(range.end)]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, Theme, Ui, WidgetId};

    const SENTENCE: &str =
        "Your layout.toml could not be read and was left unchanged. The nearest \
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

    /// The text shown of `text` at `width` in `limit` lines, and its layout.
    fn shown(
        cx: &mut TextContext,
        text: &str,
        rtl: bool,
        limit: usize,
        width: f32,
    ) -> (String, Layout<Brush>) {
        let style = TextStyle::default();
        let full = cx.layout_at(text, 15.0, rtl, &style, Some(width));
        cut(cx, text, (15.0, rtl), &style, (limit, width), full)
    }

    /// `shown`, cut from `text`: a prefix of it, the whitespace before the cut trimmed, and the
    /// ellipsis; returns the prefix.
    fn kept<'a>(shown: &'a str, text: &str) -> &'a str {
        let kept = shown
            .strip_suffix(ELLIPSIS)
            .expect("an ellipsis at the end");
        assert!(
            text.starts_with(kept),
            "{kept:?} is not a prefix of {text:?}"
        );
        kept
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
        let (least, ten) = request(&mut cx, SENTENCE, at, &style, &wrap(10, 0));
        let (_, forty) = request(&mut cx, SENTENCE, at, &style, &wrap(40, 0));
        assert!(ten < forty, "{ten} < {forty}");
        assert!(forty < cx.layout(SENTENCE, 15.0, false, &style).width());
        let (_, hi) = request(&mut cx, "Hi", at, &style, &wrap(40, 0));
        assert!((hi - cx.layout("Hi", 15.0, false, &style).width()).abs() < 0.5);
        let (_, word) = request(&mut cx, SENTENCE, at, &style, &Wrap::default());
        let longest = cx.layout("unchanged.", 15.0, false, &style).width();
        for width in [word, least] {
            assert!(
                (width - longest).abs() < 1.0,
                "the longest word: {width} against {longest}"
            );
        }
    }

    #[test]
    fn the_character_width_is_shaped_once_per_size_and_style() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        let bold = TextStyle {
            weight: 700,
            ..TextStyle::default()
        };
        let at = (15.0, false);
        for text in [SENTENCE, "Hi", SENTENCE] {
            request(&mut cx, text, at, &style, &wrap(40, 0));
        }
        assert_eq!(cx.char_widths.len(), 1);
        request(&mut cx, SENTENCE, at, &bold, &wrap(40, 0));
        request(&mut cx, SENTENCE, (13.0, false), &style, &wrap(40, 0));
        assert_eq!(cx.char_widths.len(), 3);
        // Without a cap, no character width is needed.
        let mut plain = TextContext::default();
        request(&mut plain, SENTENCE, at, &style, &Wrap::default());
        assert!(plain.char_widths.is_empty());
        // A size that is no number is not kept, and the list stays bounded.
        for size in [f32::NAN, f32::INFINITY, f32::NAN] {
            assert_eq!(char_width(&mut plain, size, &style), 0.0);
        }
        assert!(plain.char_widths.is_empty());
        for size in 0..40 {
            char_width(&mut plain, 8.0 + size as f32, &style);
            assert!(plain.char_widths.len() <= CHAR_WIDTHS);
        }
    }

    #[test]
    fn a_line_limit_ends_the_last_line_with_an_ellipsis_at_the_last_cluster_that_fits() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        for rtl in [false, true] {
            for limit in [1, 2] {
                let (text, layout) = shown(&mut cx, SENTENCE, rtl, limit, 160.0);
                assert_eq!(layout.len(), limit, "rtl {rtl}");
                assert!(inked_within(&layout, 160.0, rtl), "rtl {rtl}");
                let kept = kept(&text, SENTENCE);
                // The cut falls past the start of the last line shown of the whole text: in it,
                // or in the rest of the text, which that line holds up to the cut.
                let full = cx.layout_at(SENTENCE, 15.0, rtl, &style, Some(160.0));
                let last = full
                    .get(limit - 1)
                    .expect("the last line shown")
                    .text_range();
                assert!(last.start < kept.len(), "rtl {rtl}");
                // The next letter, after the spaces, does not fit: the cut is the last that does.
                let rest = &SENTENCE[kept.len()..];
                let next = kept.len() + rest.len() - rest.trim_start().len() + 1;
                let more = format!("{}{ELLIPSIS}", &SENTENCE[..next]);
                let more = cx.layout_at(&more, 15.0, rtl, &style, Some(160.0));
                assert!(
                    more.len() > limit || more.width() > 160.5,
                    "rtl {rtl}: {kept:?}"
                );
            }
        }
        let (fits, layout) = shown(&mut cx, "Fits.", false, 1, 160.0);
        assert_eq!((fits.as_str(), layout.len()), ("Fits.", 1));
    }

    /// The byte offsets strictly inside each of `graphemes` where it occurs in `text`: where no
    /// cut may fall.
    fn inside(text: &str, graphemes: &[&str]) -> Vec<usize> {
        let mut inside = Vec::new();
        for g in graphemes {
            for (at, _) in text.match_indices(g) {
                inside.extend(g.char_indices().skip(1).map(|(i, _)| at + i));
            }
        }
        inside
    }

    #[test]
    fn no_cut_parts_a_grapheme_cluster() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        // A letter and its combining acute, a flag of two regional indicators, and a family of
        // three emoji joined by ZWJ: each one cluster of several characters.
        let graphemes = [
            "e\u{301}",
            "\u{1F1EE}\u{1F1F9}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        ];
        let text = graphemes
            .iter()
            .map(|g| format!("Caf{g}{g} {g}cafe{g} "))
            .collect::<String>()
            .repeat(2);
        let inside = inside(&text, &graphemes);
        assert!(inside.len() > 20);
        // Every cut each line offers, at every width.
        for width in (20..400).step_by(10).map(|w| w as f32) {
            let layout = cx.layout_at(&text, 15.0, false, &style, Some(width));
            for line in layout.lines() {
                let cuts = cuts([line], &text);
                assert!(
                    cuts.iter().all(|c| !inside.contains(c)),
                    "{width}: {cuts:?}"
                );
            }
            // And the cut the line limit chooses, its trailing whitespace trimmed.
            for limit in [1, 2] {
                let (shown, _) = shown(&mut cx, &text, false, limit, width);
                if let Some(kept) = shown.strip_suffix(ELLIPSIS) {
                    assert!(!inside.contains(&kept.len()), "{width}: {kept:?}");
                }
            }
        }
    }

    #[test]
    fn edge_cases_keep_the_line_limit() {
        let mut cx = TextContext::default();
        // Empty text: no ellipsis.
        let (empty, layout) = shown(&mut cx, "", false, 1, 100.0);
        assert_eq!(empty, "");
        assert!(layout.len() <= 1);
        let (ui, label) = window("", Some(wrap(0, 2)), false);
        assert!(shaped(&ui, label).len() <= 1);
        // One word wider than the label, in one line: cut to fit, with the ellipsis.
        let word = "Supercalifragilisticexpialidocious";
        let (text, layout) = shown(&mut cx, word, false, 1, 80.0);
        assert!(kept(&text, word).len() < word.len());
        assert_eq!(layout.len(), 1);
        assert!(layout.width() <= 80.5, "{}", layout.width());
        // A width of 0: no cut fits, so the lines before the last one shown stay, one word each,
        // and the ellipsis follows them: alone in one line, after the first word in two.
        for (limit, expected) in [(1, "\u{2026}"), (2, "Your\u{2026}")] {
            let (text, layout) = shown(&mut cx, SENTENCE, false, limit, 0.0);
            assert_eq!(text, expected);
            assert!(layout.len() <= limit, "{limit}: {}", layout.len());
        }
        // A final newline is no further line: no ellipsis.
        let (text, layout) = shown(&mut cx, "Short.\n", false, 1, 160.0);
        assert_eq!((text.trim_end(), layout.len()), ("Short.", 1));
        // A hard newline before the cut is trimmed with the whitespace before the ellipsis.
        let lines = "First line.\nSecond line.\nThird line.";
        let (text, layout) = shown(&mut cx, lines, false, 2, 300.0);
        assert_eq!(
            (text.as_str(), layout.len()),
            ("First line.\nSecond line.\u{2026}", 2)
        );
    }

    #[test]
    fn a_line_limit_cuts_arabic_and_hebrew_at_the_last_cluster_that_fits_ellipsis_left() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        // Vowelled throughout, so that a mark sits next to every cut.
        let texts = [
            "تَعَذَّرَتْ قِرَاءَةُ مَلَفِّ التَّخْطِيطِ وَبَقِيَ دُونَ تَغْيِيرٍ. التَّخْطِيطُ الْأَقْرَبُ مُسْتَخْدَمٌ الْآنَ.",
            "לֹא נִתָּן הָיָה לִקְרֹא אֶת קֹבֶץ הַפְּרִיסָה וְהוּא נִשְׁאַר לְלֹא שִׁנּוּי. הַפְּרִיסָה הַקְּרוֹבָה בְּשִׁמּוּשׁ.",
        ];
        for text in texts {
            for limit in [1, 2] {
                let (shown, layout) = shown(&mut cx, text, true, limit, 140.0);
                assert_eq!(layout.len(), limit, "{text}");
                assert!(inked_within(&layout, 140.0, true), "{text}");
                let kept = kept(&shown, text);
                // Maximal: the next cut of the last line shown, past the whitespace, does not fit.
                let full = cx.layout_at(text, 15.0, true, &style, Some(140.0));
                let next = cuts(full.lines().skip(limit - 1), text)
                    .into_iter()
                    .map(|c| text[..c].trim_end())
                    .find(|k| k.len() > kept.len());
                if let Some(next) = next {
                    let more = format!("{next}{ELLIPSIS}");
                    let more = cx.layout_at(&more, 15.0, true, &style, Some(140.0));
                    assert!(!fits(&more, limit, 140.0), "{kept:?} is not the last fit");
                }
                // The ellipsis ends the line in reading order: at its visual left.
                let line = layout.get(limit - 1).expect("the last line");
                let at = shown.len() - ELLIPSIS.len_utf8();
                let offsets: Vec<(usize, f32)> = line
                    .runs()
                    .flat_map(|run| run.clusters())
                    .filter_map(|c| Some((c.text_range().start, c.visual_offset()?)))
                    .collect();
                let ellipsis = offsets.iter().find(|(start, _)| *start == at);
                let (_, x) = ellipsis.expect("the ellipsis drawn");
                assert!(offsets.iter().all(|(_, o)| x <= o), "{text}: {offsets:?}");
            }
        }
    }

    #[test]
    fn the_last_line_shown_cuts_the_word_that_broke_past_it_as_pango_does() {
        let mut cx = TextContext::default();
        let style = TextStyle::default();
        let mut wide = |text: &str| cx.layout_at(text, 15.0, false, &style, None).width();
        // Room for "Not connec…" and not for "Not connect…", nor for "Not connected" or
        // "connected Not", so that both texts below break before their last word.
        let room = wide("Not connec\u{2026}") + 0.25;
        assert!(wide("Not connect\u{2026}") > room + 0.5);
        assert!(wide("Not connected") > room && wide("connected Not") > room);
        assert!(wide("connected") <= room);
        for rtl in [false, true] {
            // GTK 4.20 with lines 1 and PANGO_ELLIPSIZE_END: "Not connec…", where a cut at the
            // last word boundary of the first line gave "Not…".
            let (text, layout) = shown(&mut cx, "Not connected", rtl, 1, room);
            assert_eq!(
                (text.as_str(), layout.len()),
                ("Not connec\u{2026}", 1),
                "rtl {rtl}"
            );
            assert!(inked_within(&layout, room, rtl), "rtl {rtl}");
            // In two lines, the first keeps its word and the second is cut in the word that
            // broke past it.
            let words = "connected Not connected";
            let (text, layout) = shown(&mut cx, words, rtl, 2, room);
            assert_eq!(
                (text.as_str(), layout.len()),
                ("connected Not connec\u{2026}", 2),
                "rtl {rtl}"
            );
            assert!(inked_within(&layout, room, rtl), "rtl {rtl}");
        }
    }

    #[test]
    fn a_cut_label_keeps_its_whole_text() {
        let (ui, label) = window(SENTENCE, Some(wrap(0, 2)), false);
        assert_eq!(shaped(&ui, label).len(), 2);
        assert!(matches!(&ui.widget(label).kind, Kind::Label(t) if t == SENTENCE));
        // Assistive technologies read the whole text.
        let node = crate::a11y::node(&ui, label, 1.0);
        assert_eq!(node.value(), Some(SENTENCE));
    }

    #[test]
    fn a_ui_without_a_wrapping_label_lays_out_in_one_pass() {
        let mut ui = Ui::new("Plain", Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Label(SENTENCE.into()));
        ui.add(Ui::ROOT, Kind::Button("Go".into()));
        let size = ui.layout();
        assert!(!ui.rewrap(), "nothing to break again");
        assert_eq!(ui.layout(), size);
    }

    #[test]
    fn a_label_placed_at_zero_width_breaks_its_lines_there() {
        let mut ui = Ui::new("Zero", Theme::default(), false);
        let label = ui.add(Ui::ROOT, Kind::Label(SENTENCE.into()));
        assert!(ui.set_wrap(label, Some(Wrap::default())));
        ui.layout();
        let placed = shaped(&ui, label).len();
        // A place of no width is a place, recorded and broken at, not "not placed yet": one
        // word a line.
        ui.widgets[label].rect.w = 0.0;
        assert!(ui.rewrap());
        assert!(!ui.rewrap(), "recorded once");
        ui.measure(label);
        let words = SENTENCE.split_whitespace().count();
        assert_eq!(
            shaped(&ui, label).len(),
            words,
            "{placed} lines when placed"
        );
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
        assert!(
            ui.layout().0 > 300.0,
            "on one line it widens the window again"
        );
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
        assert!(
            ui.widget(row).rect.h >= cell.h,
            "the row is as tall as the wrapped text"
        );
    }
}
