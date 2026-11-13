//! A label of several lines wrapped at a width: read-only text such as the launcher's text
//! preview (doc_launcher.md, LA5, need 4), as a GTK label with `wrap` set and a width.

use super::{Kind, Ui, WidgetId};

impl Ui {
    /// Breaks the lines of the label `id` at `width` logical pixels, between words and, for a
    /// word wider than that, inside it, as a GTK label's `WordChar` wrap mode; `None` gives the
    /// label back its own lines, which only its newlines break. A width that is not finite or
    /// not above zero is `None`. A widget that is not a label keeps its text as it is. Call
    /// [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` is past the widgets of this `Ui`.
    pub fn set_wrap(&mut self, id: WidgetId, width: Option<f32>) {
        let widget = &mut self.widgets[id];
        if matches!(widget.kind, Kind::Label(_)) {
            widget.wrap = width.filter(|w| w.is_finite() && *w > 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Theme, Ui};

    const TEXT: &str = "A text preview holds several lines of words that a narrow pane must wrap";

    #[test]
    fn a_wrapped_label_keeps_to_its_width_and_grows_down() {
        let mut ui = Ui::new("Wrap", Theme::default(), false);
        let label = ui.add(Ui::ROOT, Kind::Label(TEXT.into()));
        ui.layout();
        let line = ui.widget(label).rect;
        ui.set_wrap(label, Some(120.0));
        ui.layout();
        let wrapped = ui.widget(label).rect;
        assert!(wrapped.w <= 120.0, "{wrapped:?}");
        assert!(wrapped.h >= 3.0 * line.h, "{line:?} {wrapped:?}");
        ui.set_wrap(label, None);
        ui.layout();
        assert_eq!(
            ui.widget(label).rect,
            line,
            "None gives the label its one line back"
        );
    }

    #[test]
    fn a_word_wider_than_the_width_breaks_inside() {
        let mut ui = Ui::new("Wrap", Theme::default(), false);
        let label = ui.add(Ui::ROOT, Kind::Label("x".repeat(200)));
        ui.set_wrap(label, Some(80.0));
        ui.layout();
        let r = ui.widget(label).rect;
        assert!(r.w <= 80.0, "{r:?}");
    }

    #[test]
    fn a_bad_width_and_another_kind_change_nothing() {
        let mut ui = Ui::new("Wrap", Theme::default(), false);
        let label = ui.add(Ui::ROOT, Kind::Label(TEXT.into()));
        let button = ui.add(Ui::ROOT, Kind::Button(TEXT.into()));
        ui.layout();
        let (line, wide) = (ui.widget(label).rect, ui.widget(button).rect);
        for width in [f32::NAN, f32::INFINITY, 0.0, -10.0] {
            ui.set_wrap(label, Some(width));
            ui.layout();
            assert_eq!(ui.widget(label).rect, line, "{width}");
        }
        ui.set_wrap(button, Some(80.0));
        ui.layout();
        assert_eq!(ui.widget(button).rect, wide, "a button keeps its line");
    }
}
