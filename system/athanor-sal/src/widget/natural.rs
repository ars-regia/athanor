//! A scroll that is as tall as its content up to its height, as a GTK scrolled window with
//! `propagate-natural-height`: a short list leaves no empty viewport under it.

use super::{Kind, Ui, WidgetId};

impl Ui {
    /// Whether scroll `id` is as tall as its content, up to the height it was added with, or
    /// always that height. Whether it changed: `false` for a widget that is not a scroll. Call
    /// [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_natural_height(&mut self, id: WidgetId, natural: bool) -> bool {
        let widget = &mut self.widgets[id];
        if !matches!(widget.kind, Kind::Scroll { .. }) {
            return false;
        }
        let changed = widget.natural != natural;
        widget.natural = natural;
        changed
    }
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Theme, Ui};

    #[test]
    fn a_scroll_of_natural_height_is_as_tall_as_a_short_content() {
        let mut ui = Ui::new("Page", Theme::default(), false);
        let scroll = ui.scroll(Ui::ROOT, 320.0);
        let rows = ui.add(scroll, Kind::Column);
        let row = ui.add(rows, Kind::Button("Corner Caf\u{e9}".into()));
        ui.layout();
        assert_eq!(ui.widget(scroll).rect.h, 320.0, "its height by itself");

        assert!(ui.set_natural_height(scroll, true));
        assert!(!ui.set_natural_height(scroll, true), "unchanged");
        ui.layout();
        assert_eq!(ui.widget(scroll).rect.h, ui.widget(row).rect.h, "the content's");

        // Content past the height scrolls in a viewport of that height.
        for _ in 0..20 {
            ui.add(rows, Kind::Button("Home Network".into()));
        }
        ui.layout();
        assert_eq!(ui.widget(scroll).rect.h, 320.0, "at most its height");
    }

    #[test]
    fn only_a_scroll_takes_a_natural_height() {
        let mut ui = Ui::new("Page", Theme::default(), false);
        let column = ui.add(Ui::ROOT, Kind::Column);
        assert!(!ui.set_natural_height(column, true));
    }
}
