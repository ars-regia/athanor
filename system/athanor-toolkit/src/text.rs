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
    pub fonts: FontContext,
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

/// A single-line text field: Parley's editor, with the input method's preedit kept apart from the
/// committed text until the input method commits it.
pub struct TextField {
    pub(crate) name: String,
    editor: PlainEditor<Brush>,
}

impl TextField {
    pub fn new(name: &str, text: &str, size: f32, rtl: bool) -> Self {
        let mut editor = PlainEditor::new(size);
        editor.edit_styles().insert(GenericFamily::SansSerif.into());
        editor.set_base_direction(base_direction(rtl));
        editor.set_text(text);
        Self {
            name: name.to_owned(),
            editor,
        }
    }

    /// The committed text, without the preedit.
    pub fn text(&self) -> String {
        self.editor.text().to_string()
    }

    /// The text as displayed, with the preedit in place.
    pub fn shown(&self) -> &str {
        self.editor.raw_text()
    }

    pub fn is_composing(&self) -> bool {
        self.editor.is_composing()
    }

    /// Replaces the whole text, as an assistive technology's SetValue does.
    pub fn set_text(&mut self, text: &str) {
        self.editor.set_text(text);
    }

    pub fn insert(&mut self, cx: &mut TextContext, text: &str) {
        self.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .insert_or_replace_selection(text);
    }

    pub fn backspace(&mut self, cx: &mut TextContext) {
        self.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .backdelete();
    }

    pub fn delete(&mut self, cx: &mut TextContext) {
        self.editor.driver(&mut cx.fonts, &mut cx.layouts).delete();
    }

    /// Moves the caret one cluster to the left on screen, which is backwards in left-to-right
    /// text and forwards in right-to-left text.
    pub fn left(&mut self, cx: &mut TextContext) {
        self.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .move_left();
    }

    pub fn right(&mut self, cx: &mut TextContext) {
        self.editor
            .driver(&mut cx.fonts, &mut cx.layouts)
            .move_right();
    }

    /// Shows the input method's preedit; an empty preedit ends the composition without text.
    /// `cursor` is a byte range in the preedit; one that does not fall on its characters'
    /// boundaries hides the caret instead.
    pub fn compose(&mut self, cx: &mut TextContext, preedit: &str, cursor: Option<(usize, usize)>) {
        let cursor = cursor.filter(|&(start, end)| {
            start <= end && preedit.is_char_boundary(start) && preedit.is_char_boundary(end)
        });
        let mut driver = self.editor.driver(&mut cx.fonts, &mut cx.layouts);
        if preedit.is_empty() {
            driver.clear_compose();
        } else {
            driver.set_compose(preedit, cursor);
        }
    }

    /// Commits `text` from the input method, replacing the preedit.
    pub fn commit(&mut self, cx: &mut TextContext, text: &str) {
        let mut driver = self.editor.driver(&mut cx.fonts, &mut cx.layouts);
        driver.clear_compose();
        driver.insert_or_replace_selection(text);
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
        self.editor.layout(&mut cx.fonts, &mut cx.layouts);
        self.editor.cursor_geometry(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
