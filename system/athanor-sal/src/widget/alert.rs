//! [`Kind::Alert`]: a column that assistive technologies read as an alert, as a GTK box or
//! label with the `alert` role: a notification shown over the desktop, a refusal the user
//! must hear.

use super::{Kind, Ui, WidgetId};

impl Ui {
    /// Adds an alert named `name`: a column whose children assistive technologies read as one
    /// alert. Add its children to the id it returns. See [`Kind::Alert`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn alert(&mut self, parent: WidgetId, name: &str) -> WidgetId {
        self.add(parent, Kind::Alert(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Event, Kind, Theme, Ui};
    use accesskit::{Orientation, Role};

    #[test]
    fn an_alert_stacks_its_children_as_a_group_does() {
        let mut ui = Ui::new("Alert", Theme::default(), false);
        let alert = ui.alert(Ui::ROOT, "Update refused");
        let group = ui.add(Ui::ROOT, Kind::Group("Update refused".into()));
        for parent in [alert, group] {
            ui.add(parent, Kind::Label("The image is not signed.".into()));
            ui.add(parent, Kind::Button("Details".into()));
        }
        ui.layout();
        let (a, g) = (ui.widget(alert).rect, ui.widget(group).rect);
        assert_eq!((a.w, a.h), (g.w, g.h));
        let [first, second] = ui.widget(alert).children[..] else {
            panic!("two children");
        };
        assert!(ui.widget(second).rect.y > ui.widget(first).rect.y, "a column");
        assert_eq!(ui.set_text(alert, "Restart refused"), Some(Event::Edited(alert)));
    }

    #[test]
    fn an_alert_reads_as_an_alert_named_by_its_name() {
        let mut ui = Ui::new("Alert", Theme::default(), false);
        let alert = ui.alert(Ui::ROOT, "Battery low");
        ui.layout();
        let node = crate::a11y::node(&ui, alert, 1.0);
        assert_eq!(node.role(), Role::Alert);
        assert_eq!(node.label(), Some("Battery low"));
        assert_eq!(node.orientation(), Some(Orientation::Vertical), "as a group");
    }
}
