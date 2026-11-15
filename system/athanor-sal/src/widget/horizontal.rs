//! A group whose children the application lays out as one row, an [`Ui::inline`] in it, says it
//! is horizontal to assistive technologies, as a GTK box of horizontal orientation does: a
//! [`Kind::Group`] stacks its children as a column, and says vertical, by itself.

use super::{Kind, Ui, WidgetId};

impl Ui {
    /// Whether group or alert `id` says it is horizontal, for a group the application lays out
    /// as a row, or vertical, as a column, its own layout, does. It changes what assistive
    /// technologies hear, not the layout. Whether it changed: `false` for a widget that is not
    /// a group or an alert.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_horizontal(&mut self, id: WidgetId, horizontal: bool) -> bool {
        let widget = &mut self.widgets[id];
        if !matches!(widget.kind, Kind::Group(_) | Kind::Alert(_)) {
            return false;
        }
        let changed = widget.horizontal != horizontal;
        widget.horizontal = horizontal;
        changed
    }
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Theme, Ui};
    use accesskit::Orientation;

    #[test]
    fn a_group_laid_out_as_a_row_says_it_is_horizontal() {
        let mut ui = Ui::new("Panel", Theme::default(), false);
        let battery = ui.add(Ui::ROOT, Kind::Group("Battery 72%".into()));
        let level = ui.inline(battery, [0.0; 2]);
        ui.add(level, Kind::Label("72%".into()));
        ui.layout();
        let orientation = |ui: &Ui| crate::a11y::node(ui, battery, 1.0).orientation();
        assert_eq!(orientation(&ui), Some(Orientation::Vertical), "a group by itself");
        assert!(ui.set_horizontal(battery, true));
        assert!(!ui.set_horizontal(battery, true), "no change");
        assert_eq!(orientation(&ui), Some(Orientation::Horizontal));
        assert!(ui.set_horizontal(battery, false));
        assert_eq!(orientation(&ui), Some(Orientation::Vertical));
    }

    #[test]
    fn only_a_group_or_an_alert_takes_an_orientation() {
        let mut ui = Ui::new("Panel", Theme::default(), false);
        let alert = ui.alert(Ui::ROOT, "Battery low");
        let row = ui.inline(Ui::ROOT, [0.0; 2]);
        assert!(ui.set_horizontal(alert, true));
        assert!(!ui.set_horizontal(row, true));
        ui.layout();
        let node = crate::a11y::node(&ui, alert, 1.0);
        assert_eq!(node.orientation(), Some(Orientation::Horizontal));
    }
}
