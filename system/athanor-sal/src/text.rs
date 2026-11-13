//! Text on Parley: the shaped layout of a label, and the editable text of a field.

use crate::widget::Rect;
use parley::fontique::FamilyId;
use parley::setting::{FontFeature, Tag};
use parley::{
    Alignment, AlignmentOptions, BaseDirection, FontContext, FontFeatures, FontWeight,
    GenericFamily, Layout, LayoutContext, PlainEditor, StyleProperty,
};

mod fontconfig;

/// How a widget's text is set: a step of the type scale and OpenType features, as the toolkit's
/// `size.*` and `font.features` tokens give them (athanor-style's `sal` module), set with
/// [`crate::Ui::set_text_style`]. The default is body text: the theme's font size, regular, with
/// the font's own features.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// The size, as a multiple of the theme's `font_size`: a step's `scale`.
    pub scale: f32,
    /// The weight, from 1 to 1000 as CSS's `font-weight`: a step's `weight`.
    pub weight: u16,
    /// The OpenType features turned on, by tag: `*b"tnum"` for figures of one width, so that a
    /// number that changes does not move its neighbours.
    pub features: Vec<[u8; 4]>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            scale: 1.0,
            weight: 400,
            features: Vec::new(),
        }
    }
}

/// Parley's brush type for our layouts: the colour is chosen when drawing, not when shaping.
pub type Brush = [u8; 4];

/// The font database and the shaping scratch space, shared by every widget of a [`crate::Ui`].
pub struct TextContext {
    /// The fonts the system offers.
    pub fonts: FontContext,
    /// The scratch space shaping uses.
    pub layouts: LayoutContext<Brush>,
    /// The approximate character width of a wrapping label's `max_width_chars`, by font size and
    /// style: each pair is shaped once, for the family set.
    pub(crate) char_widths: Vec<(f32, TextStyle, f32)>,
    /// The Latin font that leads the sans-serif family: see [`TextContext::new`].
    latin: Option<FamilyId>,
    /// The family set ahead of it: see [`TextContext::set_family`].
    family: Option<String>,
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new(FontContext::default())
    }
}

impl TextContext {
    /// A context on `fonts`, whose sans-serif family starts with the font GTK draws Latin text
    /// with. Fontconfig orders sans-serif by the locale's language, so under Arabic or Hebrew it
    /// starts with a font of that script without Latin letters (Noto Sans Arabic, Droid Sans
    /// Hebrew). Parley sizes every line by at least its family's first font, the strut of CSS 2
    /// § 10.8, so a Latin line took that font's ascent, descent and leading. Pango gives a Latin
    /// run the locale's language when that language is written in Latin, and `en` when it is
    /// not, and draws it with the first font of sans-serif sorted for that language that has the
    /// letters: Noto Sans under ar_EG, he_IL and ja_JP on the image, Vazirmatn under fa_IR. That
    /// font now leads. Glyphs from the fonts after it still grow the line, so Arabic text keeps
    /// its own height.
    fn new(mut fonts: FontContext) -> Self {
        let collection = &mut fonts.collection;
        let sorted = fontconfig::sans_serif_for_english();
        let latin = sorted
            .as_deref()
            .and_then(|fonts| fontconfig::first_latin(fonts.iter().map(|(f, l)| (f.as_str(), *l))))
            .and_then(|family| collection.family_id(family));
        // Fontique serves a collection's own generic families before the system's, which follow
        // in fontconfig's order for the locale; without a Latin font that order stays as it is.
        match latin {
            Some(id) => collection.set_generic_families(GenericFamily::SansSerif, [id].into_iter()),
            None => eprintln!(
                "athanor-sal: fontconfig named no sans-serif font for Latin text; lines keep the locale's order"
            ),
        }
        Self {
            fonts,
            layouts: LayoutContext::new(),
            char_widths: Vec::new(),
            latin,
            family: None,
        }
    }

    /// Puts `family` ahead of the Latin font in the sans-serif family every layout is set in, as
    /// a theme's [`crate::Theme::font_family`]; `None` leaves the Latin font first again. A family
    /// the system lacks is skipped, with a message.
    pub(crate) fn set_family(&mut self, family: Option<&str>) {
        if self.family.as_deref() == family {
            return;
        }
        self.family = family.map(str::to_owned);
        // The character widths were measured in the family before.
        self.char_widths.clear();
        let collection = &mut self.fonts.collection;
        let first = family.and_then(|name| {
            let id = collection.family_id(name);
            if id.is_none() {
                eprintln!("athanor-sal: no font family {name:?}; text keeps the system's sans-serif");
            }
            id
        });
        let families: Vec<FamilyId> = first.into_iter().chain(self.latin).collect();
        collection.set_generic_families(GenericFamily::SansSerif, families.into_iter());
    }

    /// Shapes one paragraph of `text` in the system's sans-serif font, aligned to its start, at
    /// `size` times the style's scale.
    pub fn layout(&mut self, text: &str, size: f32, rtl: bool, style: &TextStyle) -> Layout<Brush> {
        self.layout_at(text, size, rtl, style, None)
    }

    /// [`TextContext::layout`], its lines broken at `width` logical pixels when it is given and
    /// aligned to the start edge of that width.
    pub(crate) fn layout_at(
        &mut self,
        text: &str,
        size: f32,
        rtl: bool,
        style: &TextStyle,
        width: Option<f32>,
    ) -> Layout<Brush> {
        let features: Vec<FontFeature> = style
            .features
            .iter()
            .map(|tag| FontFeature::new(Tag::new(tag), 1))
            .collect();
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(GenericFamily::SansSerif);
        builder.push_default(StyleProperty::FontSize(size * style.scale));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
            style.weight,
        ))));
        builder.push_default(StyleProperty::FontFeatures(FontFeatures::from(
            &features[..],
        )));
        builder.set_base_direction(base_direction(rtl));
        let mut layout = builder.build(text);
        layout.break_all_lines(width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }
}

fn base_direction(rtl: bool) -> BaseDirection {
    if rtl {
        BaseDirection::Rtl
    } else {
        BaseDirection::Ltr
    }
}

/// Whether a field keeps `c` from text that comes from outside the keyboard: no control
/// character, but the newline in a `multiline` one, and neither of Parley's other line breaks,
/// the line and paragraph separators, since `\n` is a text area's only newline.
pub(crate) fn keeps(c: char, multiline: bool) -> bool {
    (!c.is_control() || (multiline && c == '\n')) && !matches!(c, '\u{2028}' | '\u{2029}')
}

/// `text` with only the characters a field [`keeps`].
pub(crate) fn printable(text: &str, multiline: bool) -> String {
    text.chars().filter(|&c| keeps(c, multiline)).collect()
}

/// A text field: Parley's editor, with the input method's preedit kept apart from the committed
/// text until the input method commits it. A single-line field shows one line; a text area
/// wraps its lines at its width and shows a number of them, scrolled to keep the caret in view.
pub struct TextField {
    pub(crate) name: String,
    editor: PlainEditor<Brush>,
    /// The lines a text area shows; 1 for a single-line field.
    lines: usize,
    /// How far a text area is scrolled, in logical pixels from the top of its text.
    pub(crate) offset: f32,
    /// The horizontal position moves up and down keep, from the first of them to the next other
    /// change of the caret or the text.
    column: Option<f32>,
    /// Whether the text keeps its newlines, as a text area's does.
    multiline: bool,
}

impl TextField {
    /// A field named `name` for assistive technologies, holding the characters of `text` a field
    /// keeps at `size` logical pixels; `rtl` sets the base reading direction.
    pub fn new(name: &str, text: &str, size: f32, rtl: bool) -> Self {
        let mut editor = PlainEditor::new(size);
        editor.edit_styles().insert(GenericFamily::SansSerif.into());
        editor.set_base_direction(base_direction(rtl));
        editor.set_text(&printable(text, false));
        Self {
            name: name.to_owned(),
            editor,
            lines: 1,
            offset: 0.0,
            column: None,
            multiline: false,
        }
    }

    /// A text area: a field whose text wraps at `width` logical pixels, `lines` of it shown.
    pub(crate) fn area(
        name: &str,
        text: &str,
        (size, rtl): (f32, bool),
        width: f32,
        lines: usize,
    ) -> Self {
        let mut area = Self::new(name, "", size, rtl);
        area.multiline = true;
        area.set_text(text);
        area.editor.set_width(Some(width));
        area.lines = lines.max(1);
        area
    }

    /// The height of one line of text: the first line's, or 1.25 times the font size when
    /// Parley has none.
    pub(crate) fn line_height(&mut self, cx: &mut TextContext) -> f32 {
        let size = self.editor.get_font_size();
        let layout = self.layout(cx);
        let first = layout.lines().next().map(|l| l.metrics().line_height);
        first.filter(|h| *h > 0.0).unwrap_or(size * 1.25)
    }

    /// The height of the text a text area shows: its lines.
    pub(crate) fn view_height(&mut self, cx: &mut TextContext) -> f32 {
        self.lines as f32 * self.line_height(cx)
    }

    /// Scrolls by `dy` logical pixels, down when positive, within the text; returns whether
    /// the text moved.
    pub(crate) fn scroll_by(&mut self, cx: &mut TextContext, dy: f32) -> bool {
        let most = (self.layout(cx).height() - self.view_height(cx)).max(0.0);
        let to = (self.offset + dy).min(most).max(0.0);
        let moved = to != self.offset;
        self.offset = to;
        moved
    }

    /// Scrolls the least that brings the caret's line into view, and no further than the text;
    /// while the input method hides the caret, the line of its preedit's end.
    pub(crate) fn scroll_to_caret(&mut self, cx: &mut TextContext) {
        let view = self.view_height(cx);
        let caret = match self.caret(cx, 1.5) {
            Some(c) => Some(c),
            None => self.editor.raw_compose().clone().map(|preedit| {
                let layout = self.layout(cx);
                let end = parley::Cursor::from_byte_index(
                    layout,
                    preedit.end,
                    parley::Affinity::Upstream,
                );
                end.geometry(layout, 1.5)
            }),
        };
        if let Some(c) = caret {
            let (top, bottom) = (c.y0 as f32, c.y1 as f32);
            if top < self.offset {
                self.offset = top;
            } else if bottom > self.offset + view {
                self.offset = bottom - view;
            }
        }
        self.scroll_by(cx, 0.0);
    }

    /// The byte ranges of the committed text's lines as they are seen, wrapped, in order, which
    /// together cover all of it; the empty line after a final newline is an empty range. Before
    /// the layout is made again after an edit, and while the input method composes, the whole
    /// text is one range.
    pub(crate) fn line_ranges(&self) -> Vec<std::ops::Range<usize>> {
        let len = self.text().len();
        let layout = self.editor.try_layout().filter(|_| !self.is_composing());
        let mut starts: Vec<usize> = layout.map_or_else(Vec::new, |l| {
            l.lines().map(|line| line.text_range().start).collect()
        });
        // A text that ends in a newline ends in an empty line, which starts at its end.
        starts.retain(|&s| s > 0 && s <= len);
        starts.dedup();
        starts.insert(0, 0);
        let ends = starts.iter().skip(1).copied().chain([len]);
        starts.iter().zip(ends).map(|(&s, e)| s..e).collect()
    }

    /// The committed text, without the preedit.
    pub fn text(&self) -> String {
        self.editor.text().to_string()
    }

    /// The text as displayed, with the preedit in place.
    pub fn shown(&self) -> &str {
        self.editor.raw_text()
    }

    /// The selection's anchor and focus, the caret when they are one, as byte offsets in the
    /// committed text; none while the input method composes.
    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        let selection = self.editor.raw_selection();
        let ends = (selection.anchor().index(), selection.focus().index());
        (!self.is_composing()).then_some(ends)
    }

    /// Whether an input method's composition is in progress.
    pub fn is_composing(&self) -> bool {
        self.editor.is_composing()
    }

    /// Replaces the whole text, as an assistive technology's SetValue does, with the characters
    /// of `text` the field keeps: a `\r` of a `\r\n` goes, so the text's only newline is `\n`.
    pub fn set_text(&mut self, text: &str) {
        self.column = None;
        self.editor.set_text(&printable(text, self.multiline));
    }

    /// Moves the caret to the end of the text.
    pub(crate) fn move_to_end(&mut self, cx: &mut TextContext) {
        self.driver(cx).move_to_text_end();
    }

    /// Types `text` at the caret, replacing the selection.
    pub fn insert(&mut self, cx: &mut TextContext, text: &str) {
        self.driver(cx).insert_or_replace_selection(text);
    }

    /// Deletes the selection, or the cluster before the caret.
    pub fn backspace(&mut self, cx: &mut TextContext) {
        self.driver(cx).backdelete();
    }

    /// Deletes the selection, or the cluster after the caret.
    pub fn delete(&mut self, cx: &mut TextContext) {
        self.driver(cx).delete();
    }

    /// Moves the caret one cluster to the left on screen, which is backwards in left-to-right
    /// text and forwards in right-to-left text.
    pub fn left(&mut self, cx: &mut TextContext) {
        self.driver(cx).move_left();
    }

    /// Moves the caret one cluster to the right on screen, which is forwards in left-to-right text and backwards in right-to-left text.
    pub fn right(&mut self, cx: &mut TextContext) {
        self.driver(cx).move_right();
    }

    /// Moves the caret to the line above as it is seen, keeping its horizontal position; on the
    /// first line, to its start.
    pub fn up(&mut self, cx: &mut TextContext) {
        self.vertical(cx, false);
    }

    /// Moves the caret to the line below as it is seen, keeping its horizontal position; on the
    /// last line, to its end.
    pub fn down(&mut self, cx: &mut TextContext) {
        self.vertical(cx, true);
    }

    /// Up or down a line, to the point of the line's middle at the column the caret had before
    /// the first of these moves, as GTK's text view keeps it, an empty line between included; to
    /// an empty line, to its start. Parley's own moves start from the line its caret is drawn
    /// on, which Parley 0.12 gets wrong for a right-to-left caret on an empty line (see `caret`),
    /// and forget the column at a move to a byte.
    fn vertical(&mut self, cx: &mut TextContext, down: bool) {
        let (line, ranges) = self.caret_line(cx);
        let x = match self.column {
            Some(x) => x,
            None => self.caret(cx, 0.0).map_or(0.0, |c| c.x0 as f32),
        };
        let target = if down {
            line.checked_add(1)
        } else {
            line.checked_sub(1)
        };
        let layout = self.layout(cx);
        let middle = target.and_then(|t| layout.get(t)).map(|l| {
            let m = l.metrics();
            (m.block_min_coord + m.block_max_coord) / 2.0
        });
        let range = target.and_then(|t| ranges.get(t));
        let empty = self.empty_line(range);
        // The target line's newline, which the caret stays before: from the leading side of a
        // right-to-left newline Parley puts it after, on the line below.
        let text = self.editor.raw_text();
        let ends_in_newline =
            |r: &&std::ops::Range<usize>| text.get((*r).clone()).is_some_and(|t| t.ends_with('\n'));
        let newline = range.filter(ends_in_newline).map(|r| r.end - 1);
        let mut driver = self.driver(cx);
        match (middle, range) {
            (Some(_), Some(r)) if empty => driver.move_to_byte(r.start),
            (Some(y), _) => {
                driver.move_to_point(x, y);
                let at = driver.editor.raw_selection().focus().index();
                if let Some(newline) = newline.filter(|&n| at > n) {
                    driver.move_to_byte(newline);
                }
            }
            // Past the last line, to its end; before the first, to its start.
            (None, _) if down => driver.move_to_text_end(),
            (None, _) => driver.move_to_text_start(),
        }
        self.column = Some(x);
    }

    /// The line the caret is on, by its byte offset, and the byte ranges of the layout's lines: a
    /// line holds the offsets from its start to before its end, the last line also the end. A
    /// caret at the start of a line that a wrap began, drawn at the end of the line before (its
    /// affinity upstream), is on the line before.
    fn caret_line(&mut self, cx: &mut TextContext) -> (usize, Vec<std::ops::Range<usize>>) {
        let focus = self.editor.raw_selection().focus();
        let index = focus.index();
        let ranges: Vec<_> = self.layout(cx).lines().map(|l| l.text_range()).collect();
        let last = ranges.len().saturating_sub(1);
        let line = ranges.iter().position(|r| r.contains(&index));
        let line = line.unwrap_or(last);
        let before_caret = self.editor.raw_text().get(..index);
        let wrapped = before_caret.is_some_and(|t| !t.ends_with('\n'));
        let upstream = focus.affinity() == parley::Affinity::Upstream;
        let starts = ranges.get(line).is_some_and(|r| r.start == index);
        let before = line > 0 && starts && wrapped && upstream;
        (line - usize::from(before), ranges)
    }

    /// Whether the line of `range` is empty, or holds only its newline, while nothing composes.
    fn empty_line(&self, range: Option<&std::ops::Range<usize>>) -> bool {
        let text = range.and_then(|r| self.editor.raw_text().get(r.clone()));
        let empty = text.is_some_and(|t| t.trim_end_matches('\n').is_empty());
        empty && !self.is_composing()
    }

    /// Shows the input method's preedit; an empty preedit ends the composition without text.
    /// `cursor` is a byte range in the preedit; one that does not fall on its characters'
    /// boundaries hides the caret instead.
    pub fn compose(&mut self, cx: &mut TextContext, preedit: &str, cursor: Option<(usize, usize)>) {
        let cursor = cursor.filter(|&(start, end)| {
            start <= end && preedit.is_char_boundary(start) && preedit.is_char_boundary(end)
        });
        let mut driver = self.driver(cx);
        if preedit.is_empty() {
            driver.clear_compose();
        } else {
            driver.set_compose(preedit, cursor);
        }
    }

    /// Commits `text` from the input method, replacing the preedit.
    pub fn commit(&mut self, cx: &mut TextContext, text: &str) {
        let mut driver = self.driver(cx);
        driver.clear_compose();
        driver.insert_or_replace_selection(text);
    }

    /// The selected text, if the selection is not the caret; none while the input method
    /// composes.
    pub(crate) fn selected_text(&self) -> Option<String> {
        self.editor.selected_text().map(str::to_owned)
    }

    /// Moves the selection's focus one cluster to the left on screen, as Shift+Left does.
    pub(crate) fn select_left(&mut self, cx: &mut TextContext) {
        self.driver(cx).select_left();
    }

    /// Moves the selection's focus one cluster to the right on screen, as Shift+Right does.
    pub(crate) fn select_right(&mut self, cx: &mut TextContext) {
        self.driver(cx).select_right();
    }

    /// Selects the whole text.
    pub(crate) fn select_all(&mut self, cx: &mut TextContext) {
        self.driver(cx).select_all();
    }

    /// Puts the caret at the point of the layout nearest `(x, y)`, as a press does.
    pub(crate) fn press_at(&mut self, cx: &mut TextContext, x: f32, y: f32) {
        self.driver(cx).move_to_point(x, y);
    }

    /// Moves the selection's focus to the point of the layout nearest `(x, y)`, as a drag does.
    pub(crate) fn drag_to(&mut self, cx: &mut TextContext, x: f32, y: f32) {
        self.driver(cx).extend_selection_to_point(x, y);
    }

    /// The rectangles of the selected text in the layout; none for a caret.
    pub(crate) fn selection_boxes(&self) -> Vec<parley::BoundingBox> {
        let boxes = self.editor.selection_geometry();
        boxes.into_iter().map(|(b, _)| b).collect()
    }

    /// Where the layout's origin is in `r`, the field's rectangle, with `padding` inside its
    /// edges: a text area's lines from its scroll offset down, a single line centred and at the
    /// end edge in right-to-left text.
    pub(crate) fn origin(
        &mut self,
        cx: &mut TextContext,
        r: Rect,
        padding: f32,
        rtl: bool,
    ) -> (f32, f32) {
        let (offset, multiline) = (self.offset, self.multiline);
        let layout = self.layout(cx);
        let x = if rtl && !multiline {
            r.x + r.w - padding - layout.width()
        } else {
            r.x + padding
        };
        let y = if multiline {
            r.y + padding / 2.0 - offset
        } else {
            r.y + (r.h - layout.height()) / 2.0
        };
        (x, y)
    }

    /// Parley's editing driver, for a change of the caret or the text; one but a move up or down
    /// forgets the column those keep.
    fn driver<'a>(&'a mut self, cx: &'a mut TextContext) -> parley::PlainEditorDriver<'a, Brush> {
        self.column = None;
        self.editor.driver(&mut cx.fonts, &mut cx.layouts)
    }

    pub(crate) fn layout(&mut self, cx: &mut TextContext) -> &Layout<Brush> {
        self.editor.layout(&mut cx.fonts, &mut cx.layouts)
    }

    /// The caret's rectangle in the field's layout, `width` wide.
    pub(crate) fn caret(
        &mut self,
        cx: &mut TextContext,
        width: f32,
    ) -> Option<parley::BoundingBox> {
        let (line, ranges) = self.caret_line(cx);
        // On an empty line (one that holds no more than its newline) the caret is at its start
        // edge: Parley 0.12 draws a right-to-left caret there on the line below.
        let collapsed = self.editor.raw_selection().is_collapsed();
        let empty = collapsed && self.empty_line(ranges.get(line));
        let layout = self.editor.layout(&mut cx.fonts, &mut cx.layouts);
        let metrics = layout.get(line).map(|l| *l.metrics());
        match metrics.filter(|_| empty) {
            Some(m) => Some(parley::BoundingBox::new(
                f64::from(m.offset),
                f64::from(m.block_min_coord),
                f64::from(m.offset + width),
                f64::from(m.block_max_coord),
            )),
            None => self.editor.cursor_geometry(width),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caret's byte offset, its line, and its horizontal position.
    fn caret_at(cx: &mut TextContext, field: &mut TextField) -> (usize, usize, f64) {
        let index = field.editor.raw_selection().focus().index();
        let line = field.caret_line(cx).0;
        let x = field.caret(cx, 1.0).map_or(f64::NAN, |c| c.x0);
        (index, line, x)
    }

    /// Run by `a_latin_line_is_as_tall_under_arabic_and_hebrew_as_under_english` in a process of
    /// its own per locale: fontconfig reads the locale once per process.
    const IN_THIS_LOCALE: &str = "text::tests::a_latin_line_in_this_locale";

    #[test]
    #[ignore = "run by a_latin_line_is_as_tall_under_arabic_and_hebrew_as_under_english"]
    fn a_latin_line_in_this_locale() {
        let mut cx = TextContext::default();
        let collection = &mut cx.fonts.collection;
        let first = collection.generic_families(GenericFamily::SansSerif).next();
        let family = first
            .and_then(|id| collection.family_name(id))
            .map(str::to_owned);
        // GTK's Latin font in the rig and on the image, and its line at the theme's 15 px.
        assert_eq!(family.as_deref(), Some("Noto Sans"));
        let body = TextStyle::default();
        assert_eq!(
            cx.layout("Keyboard layout", 15.0, true, &body).height(),
            20.0
        );
    }

    #[test]
    fn a_style_sets_the_size_the_weight_and_the_features() {
        let mut cx = TextContext::default();
        let width = |cx: &mut TextContext, text: &str, style: &TextStyle| {
            cx.layout(text, 15.0, false, style).width()
        };
        let body = TextStyle::default();
        let display = TextStyle {
            scale: 5.0,
            weight: 300,
            features: vec![*b"tnum"],
        };
        let (small, large) = (
            cx.layout("45", 15.0, false, &body),
            cx.layout("45", 15.0, false, &display),
        );
        assert!(
            large.height() > 4.0 * small.height(),
            "{} at 5 times {}",
            large.height(),
            small.height()
        );
        let bold = TextStyle {
            weight: 700,
            ..TextStyle::default()
        };
        assert!(width(&mut cx, "Volume", &bold) > width(&mut cx, "Volume", &body));
        // Noto Sans's figures are of one width already; its proportional ones show that a
        // feature reaches the shaping, and tnum keeps them of one width.
        let proportional = TextStyle {
            features: vec![*b"pnum"],
            ..TextStyle::default()
        };
        let tabular = TextStyle {
            features: vec![*b"tnum"],
            ..TextStyle::default()
        };
        assert_eq!(width(&mut cx, "111", &body), width(&mut cx, "000", &body));
        assert!(width(&mut cx, "111", &proportional) < width(&mut cx, "000", &proportional));
        assert_eq!(
            width(&mut cx, "111", &tabular),
            width(&mut cx, "000", &tabular)
        );
    }

    #[test]
    fn a_latin_line_is_as_tall_under_arabic_and_hebrew_as_under_english() {
        let test = std::env::current_exe().expect("the test binary");
        for locale in ["en_US.UTF-8", "ar_EG.UTF-8", "he_IL.UTF-8"] {
            let run = std::process::Command::new(&test)
                .args(["--exact", IN_THIS_LOCALE, "--ignored", "--test-threads=1"])
                .env("LANG", locale)
                .env_remove("LC_ALL")
                .env_remove("LC_CTYPE")
                .env_remove("FC_LANG")
                .output()
                .expect("the test binary runs");
            let stdout = String::from_utf8_lossy(&run.stdout);
            assert!(run.status.success(), "{locale}: {stdout}");
            assert!(stdout.contains("1 passed"), "{locale}: {stdout}");
        }
    }

    #[test]
    fn up_and_down_keep_the_column_across_an_empty_line() {
        let mut cx = TextContext::default();
        for (rtl, top, bottom) in [(false, "abcdef", "xyz"), (true, "אבגדהו", "זחט")] {
            let text = format!("{top}\n\n{bottom}");
            let mut area = TextField::area("Notes", &text, (15.0, rtl), 300.0, 3);
            let column5 = top.char_indices().nth(5).map(|(i, _)| i).unwrap();
            area.editor
                .driver(&mut cx.fonts, &mut cx.layouts)
                .move_to_byte(column5);
            area.down(&mut cx);
            assert_eq!(caret_at(&mut cx, &mut area).1, 1, "rtl {rtl}");
            // Clamped to the end of the shorter line, as in a GTK text view.
            area.down(&mut cx);
            assert_eq!(caret_at(&mut cx, &mut area).0, text.len(), "rtl {rtl}");
            // And back at column 5, through the empty line.
            area.up(&mut cx);
            area.up(&mut cx);
            assert_eq!(caret_at(&mut cx, &mut area).0, column5, "rtl {rtl}");
        }
    }

    #[test]
    fn up_and_down_keep_the_column_across_wrapped_lines() {
        let mut cx = TextContext::default();
        for (rtl, word) in [(false, "word "), (true, "שלום ")] {
            let text = word.repeat(30);
            let mut area = TextField::area("Notes", &text, (15.0, rtl), 100.0, 3);
            let column2 = text.char_indices().nth(2).map(|(i, _)| i).unwrap();
            area.editor
                .driver(&mut cx.fonts, &mut cx.layouts)
                .move_to_byte(column2);
            let (_, line, x) = caret_at(&mut cx, &mut area);
            assert_eq!(line, 0, "rtl {rtl}");
            for n in 1..=3 {
                area.down(&mut cx);
                let (_, line, at) = caret_at(&mut cx, &mut area);
                assert_eq!(line, n, "rtl {rtl}");
                assert!((at - x).abs() < 1.0, "rtl {rtl}: line {n} at {at}, not {x}");
            }
            for _ in 0..3 {
                area.up(&mut cx);
            }
            assert_eq!(caret_at(&mut cx, &mut area).0, column2, "rtl {rtl}");
        }
    }

    #[test]
    fn a_move_that_is_not_up_or_down_forgets_the_column() {
        let mut cx = TextContext::default();
        let text = "abcdef\nxy\nabcdef";
        let mut area = TextField::area("Notes", text, (15.0, false), 300.0, 3);
        area.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .move_to_byte(5);
        area.down(&mut cx);
        assert_eq!(caret_at(&mut cx, &mut area).0, 9);
        // Back at the end of "xy", but by Left and Right: Down starts from there, column 2.
        area.left(&mut cx);
        area.right(&mut cx);
        area.down(&mut cx);
        assert_eq!(caret_at(&mut cx, &mut area).0, 12);
    }

    #[test]
    fn down_onto_a_shorter_wrapped_line_stays_on_it() {
        let mut cx = TextContext::default();
        let word = "mmmmmm";
        let width = cx.layout(word, 15.0, false, &TextStyle::default()).width() + 2.0;
        let text = format!("{word} i {word}");
        let mut area = TextField::area("Notes", &text, (15.0, false), width, 3);
        let ranges: Vec<_> = area
            .layout(&mut cx)
            .lines()
            .map(|l| l.text_range())
            .collect();
        assert_eq!(ranges, [0..7, 7..9, 9..15], "the wraps the test needs");
        area.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .move_to_byte(6);
        // Past the end of "i ": at the wrap, drawn at the end of the second line.
        area.down(&mut cx);
        assert_eq!(caret_at(&mut cx, &mut area).1, 1);
        area.down(&mut cx);
        assert_eq!(caret_at(&mut cx, &mut area).1, 2);
    }

    #[test]
    fn down_onto_a_shorter_right_to_left_line_ends_before_its_newline() {
        let mut cx = TextContext::default();
        let (long, short) = ("אבגדהוזח", "אב");
        let text = format!("{long}\n{short}\n{long}");
        let mut area = TextField::area("Notes", &text, (15.0, true), 300.0, 3);
        let column6 = long.char_indices().nth(6).map(|(i, _)| i).unwrap();
        area.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .move_to_byte(column6);
        // At the end of the short line, as in GTK, not past its newline on the line below.
        area.down(&mut cx);
        let end = long.len() + 1 + short.len();
        assert_eq!(caret_at(&mut cx, &mut area).0, end);
        assert_eq!(caret_at(&mut cx, &mut area).1, 1);
        area.down(&mut cx);
        assert_eq!(caret_at(&mut cx, &mut area).0, end + 1 + column6);
    }

    #[test]
    fn a_field_holds_only_the_newlines_it_keeps() {
        let mut area = TextField::area("Notes", "a\r\nb", (15.0, false), 300.0, 3);
        assert_eq!(area.text(), "a\nb");
        area.set_text("c\r\nd");
        assert_eq!(area.text(), "c\nd");
        let field = TextField::new("Name", "a\r\nb", 15.0, false);
        assert_eq!(field.text(), "ab");
    }

    #[test]
    fn the_field_edits_and_composes() {
        let mut cx = TextContext::default();
        let mut field = TextField::new("Name", "", 15.0, false);
        field.insert(&mut cx, "abc");
        field.backspace(&mut cx);
        assert_eq!(field.text(), "ab");
        field.left(&mut cx);
        field.insert(&mut cx, "x");
        assert_eq!(field.text(), "axb");

        // A dead key's preedit is shown but not committed; the commit replaces it.
        field.compose(&mut cx, "´", Some((0, 2)));
        assert!(field.is_composing());
        assert_eq!(field.text(), "axb");
        assert_eq!(field.shown(), "ax´b");
        field.commit(&mut cx, "é");
        assert!(!field.is_composing());
        assert_eq!(field.text(), "axéb");

        // An empty preedit cancels the composition without inserting anything.
        field.compose(&mut cx, "~", None);
        field.compose(&mut cx, "", None);
        assert!(!field.is_composing());
        assert_eq!(field.text(), "axéb");

        // A cursor from outside that splits a character, or runs backwards, is not trusted.
        for cursor in [(1, 1), (0, 9), (2, 0)] {
            field.compose(&mut cx, "é", Some(cursor));
            assert_eq!(field.shown(), "axééb", "{cursor:?}");
        }
        field.compose(&mut cx, "", None);

        field.set_text("שלום");
        assert_eq!(field.text(), "שלום");
    }

    /// The first family of the sans-serif family layouts are set in.
    fn first_sans(cx: &mut TextContext) -> Option<String> {
        let collection = &mut cx.fonts.collection;
        let first = collection.generic_families(GenericFamily::SansSerif).next();
        first.and_then(|id| collection.family_name(id)).map(str::to_owned)
    }

    #[test]
    fn a_family_leads_the_sans_serif_until_it_is_taken_away() {
        let mut cx = TextContext::default();
        let latin = first_sans(&mut cx);
        // Any family of the system other than the Latin one, by the name its id gives back: a
        // name such as "Noto Sans Arabic SemCond ExtLt" finds the id of "Noto Sans Arabic".
        let collection = &mut cx.fonts.collection;
        let names: Vec<String> = collection.family_names().map(str::to_owned).collect();
        let ids: Vec<_> = names.iter().filter_map(|name| collection.family_id(name)).collect();
        let other = ids
            .into_iter()
            .filter_map(|id| collection.family_name(id).map(str::to_owned))
            .find(|name| Some(name) != latin.as_ref())
            .expect("a second font family");
        cx.set_family(Some(&other));
        assert_eq!(first_sans(&mut cx), Some(other));
        cx.set_family(None);
        assert_eq!(first_sans(&mut cx), latin);
        cx.set_family(Some("No Such Family Anywhere"));
        assert_eq!(first_sans(&mut cx), latin, "a missing family is skipped");
    }
}
