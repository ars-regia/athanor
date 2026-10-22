//! Text on Parley: the shaped layout of a label, and the editable text of a field.

use parley::{
    Alignment, AlignmentOptions, BaseDirection, FontContext, GenericFamily, Layout, LayoutContext,
    PlainEditor, StyleProperty,
};

/// Parley's brush type for our layouts: the colour is chosen when drawing, not when shaping.
pub type Brush = [u8; 4];

/// The font database and the shaping scratch space, shared by every widget of a [`crate::Ui`].
#[derive(Default)]
pub struct TextContext {
    /// The fonts the system offers.
    pub fonts: FontContext,
    /// The scratch space shaping uses.
    pub layouts: LayoutContext<Brush>,
}

impl TextContext {
    /// Shapes one paragraph of `text` in the system's sans-serif font, aligned to its start.
    pub fn layout(&mut self, text: &str, size: f32, rtl: bool) -> Layout<Brush> {
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(GenericFamily::SansSerif);
        builder.push_default(StyleProperty::FontSize(size));
        builder.set_base_direction(base_direction(rtl));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
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
        let width = cx.layout(word, 15.0, false).width() + 2.0;
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
}
