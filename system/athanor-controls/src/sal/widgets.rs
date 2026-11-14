//! The pieces the pages and the surfaces that hold them share, drawn with athanor-sal.
//!
//! The GTK pages' `expose_choose_action` has no counterpart: every toggle and radio of
//! athanor-sal carries AT-SPI's Click action, which chooses it as a click would.

use athanor_sal::{Image, Rgba, TextStyle, Ui, WidgetId};

use crate::i18n::tr;

/// A labelled switch, as a row of a page: athanor-sal's switch draws its label at the start edge
/// and the track at the end, and is named by its label for assistive technologies, as the GTK
/// row's `LabelledBy` relation names its switch. `Event::Toggled` carries the id it returns.
pub fn switch_row(ui: &mut Ui, parent: WidgetId, text: &str, active: bool) -> WidgetId {
    ui.switch(parent, text, active)
}

/// What the surface holding a page draws the GTK pages' styles with: the icons of its theme,
/// and the size and ink of calmo's `bar-popover-note`, a page's secondary line.
pub struct Look<'a> {
    /// The icon `name` at [`Look::icon_size`]; `None` when the theme lacks it, and the row then
    /// shows its text alone.
    pub icon: Box<dyn FnMut(&'static str) -> Option<Image> + 'a>,
    /// The side of a row's icon, in logical pixels, as GTK's image in a row.
    pub icon_size: f32,
    /// A secondary line's size and weight.
    pub caption: TextStyle,
    /// A secondary line's ink.
    pub secondary: Rgba,
}

impl Look<'_> {
    /// Draws `label` as a secondary line, as GTK's `bar-popover-note`.
    pub fn note(&self, ui: &mut Ui, label: WidgetId) {
        ui.set_text_style(label, self.caption.clone());
        ui.set_ink(label, Some(self.secondary));
    }

    /// A look with no icons and smaller secondary lines, for the pages' tests.
    #[cfg(test)]
    pub(crate) fn plain() -> Look<'static> {
        Look {
            icon: Box::new(|_| None),
            icon_size: 16.0,
            caption: TextStyle {
                scale: 0.82,
                ..TextStyle::default()
            },
            secondary: Rgba([0, 0, 0, 255]),
        }
    }
}

/// The weight of calmo's `bar-popover-title`, a page's heading.
pub fn heading() -> TextStyle {
    TextStyle {
        weight: 600,
        ..TextStyle::default()
    }
}

/// A column of a page, as wide as its parent, as a GTK box fills its parent across: its
/// switches reach the end edge and its sliders the whole width.
pub fn column(ui: &mut Ui, parent: WidgetId) -> WidgetId {
    let column = ui.add(parent, athanor_sal::Kind::Column);
    ui.set_fill(column, true);
    column
}

/// The note under a page's content that says an action the person took did not complete. It
/// shows until the page is hidden, and assistive technologies announce it.
pub struct Failure {
    page: WidgetId,
    pub(crate) note: WidgetId,
}

impl Failure {
    /// Adds the note, hidden, as the last child of `page`, the page's column.
    pub fn new(ui: &mut Ui, page: WidgetId) -> Failure {
        let note = ui.add(
            page,
            athanor_sal::Kind::Label(tr("The action did not complete.")),
        );
        ui.set_visible(note, false);
        Failure { page, note }
    }

    /// Says that an action did not complete, if the page is on screen.
    pub fn show(&self, ui: &mut Ui) {
        if !ui.is_visible(self.page) {
            return;
        }
        ui.set_visible(self.note, true);
        ui.announce(&tr("The action did not complete."));
    }

    /// Hides the note: the page was hidden, and would show a stale note when shown again.
    pub fn hide(&self, ui: &mut Ui) {
        ui.set_visible(self.note, false);
    }
}

#[cfg(test)]
mod tests {
    use athanor_sal::{Theme, Ui};

    use super::*;

    #[test]
    fn the_note_shows_only_on_a_page_on_screen_and_hides_with_it() {
        let mut ui = Ui::new("Test", Theme::default(), false);
        let page = ui.add(Ui::ROOT, athanor_sal::Kind::Column);
        let failure = Failure::new(&mut ui, page);
        assert!(!ui.is_visible(failure.note));

        ui.set_visible(page, false);
        failure.show(&mut ui);
        ui.set_visible(page, true);
        assert!(!ui.is_visible(failure.note), "a hidden page shows no note");

        failure.show(&mut ui);
        assert!(ui.is_visible(failure.note));
        failure.hide(&mut ui);
        assert!(!ui.is_visible(failure.note));
    }

    #[test]
    fn a_page_column_is_as_wide_as_its_parent_and_its_switch_reaches_the_end_edge() {
        let mut ui = Ui::new("Test", Theme::default(), false);
        ui.min_width = 400.0;
        let page = column(&mut ui, Ui::ROOT);
        let section = column(&mut ui, page);
        let switch = switch_row(&mut ui, section, "Wi-Fi", true);
        ui.layout();
        let width = ui.widget(Ui::ROOT).rect.w - 2.0 * Theme::default().margin;
        for id in [page, section, switch] {
            assert_eq!(ui.widget(id).rect.w, width);
        }
    }

    #[test]
    fn a_switch_row_is_one_switch_named_by_its_text() {
        let mut ui = Ui::new("Test", Theme::default(), false);
        let switch = switch_row(&mut ui, Ui::ROOT, "Wi-Fi", true);
        assert!(ui.is_active(switch));
        assert!(matches!(
            &ui.widget(switch).kind,
            athanor_sal::Kind::Switch { label, .. } if label == "Wi-Fi"
        ));
    }
}
