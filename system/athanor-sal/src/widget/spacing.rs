//! The room one column keeps between its children, as a GTK box's spacing: every other column
//! keeps the theme's section gap.

use super::{length, Kind, Ui, WidgetId};

impl Ui {
    /// Sets the logical pixels column `id` keeps between its children, `None` for the theme's
    /// section gap; a spacing that is not a finite length of at least 0 counts as 0, and one
    /// past 2^64 times less than the largest `f32` as that length. Whether it changed: `false`
    /// for a widget that is not a column. Call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_spacing(&mut self, id: WidgetId, spacing: Option<f32>) -> bool {
        let spacing = spacing.map(length);
        let widget = &mut self.widgets[id];
        if !matches!(widget.kind, Kind::Column) {
            return false;
        }
        let changed = widget.spacing != spacing;
        widget.spacing = spacing;
        changed
    }
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Theme, Ui};

    #[test]
    fn a_column_keeps_its_own_spacing_and_another_the_themes() {
        let mut ui = Ui::new("Page", Theme::default(), false);
        let column = ui.add(Ui::ROOT, Kind::Column);
        let [first, second] = ["Balanced", "Power saver"].map(|t| ui.radio(column, t, false));
        let pitch = |ui: &Ui| ui.widget(second).rect.y - ui.widget(first).rect.y;
        ui.layout();
        let height = ui.widget(first).rect.h;
        assert_eq!(pitch(&ui), height + ui.theme.section_gap, "the theme's gap");

        assert!(ui.set_spacing(column, Some(2.0)));
        assert!(!ui.set_spacing(column, Some(2.0)), "unchanged");
        ui.layout();
        assert_eq!(pitch(&ui), height + 2.0);
        assert_eq!(ui.widget(column).rect.h, 2.0 * height + 2.0, "measured with it");

        ui.set_spacing(column, Some(f32::NAN));
        ui.layout();
        assert_eq!(pitch(&ui), height, "not a length: none");
        ui.set_spacing(column, None);
        ui.layout();
        assert_eq!(pitch(&ui), height + ui.theme.section_gap, "the theme's again");
    }

    #[test]
    fn only_a_column_takes_a_spacing() {
        let mut ui = Ui::new("Page", Theme::default(), false);
        let row = ui.add(Ui::ROOT, Kind::Row);
        assert!(!ui.set_spacing(row, Some(2.0)));
    }
}
