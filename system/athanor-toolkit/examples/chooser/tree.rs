//! The layout chooser's widgets, as the GTK chooser lays them out: a group per setting, each a
//! heading and a row of toggle buttons, and a status line.

use athanor_toolkit::{Kind, Theme, Ui, WidgetId};

pub const GROUPS: [(&str, &[&str]); 3] = [
    ("Style", &["Island", "Bar", "Essential"]),
    ("Panel", &["Top", "Bottom"]),
    ("Dock", &["Visible", "Auto-hide", "None"]),
];

pub struct Chooser {
    pub ui: Ui,
    /// Each toggle with its group's and its own name.
    pub choices: Vec<(WidgetId, &'static str, &'static str)>,
}

/// The chooser with the first choice of every group active.
pub fn build(rtl: bool) -> Chooser {
    let mut ui = Ui::new("Layout", Theme::default(), rtl);
    let mut choices = Vec::new();
    for (group, names) in GROUPS {
        let container = ui.add(Ui::ROOT, Kind::Group(group.into()));
        ui.add(container, Kind::Label(group.into()));
        let row = ui.add(container, Kind::Row);
        for (i, &name) in names.iter().enumerate() {
            let label = name.into();
            let id = ui.add(
                row,
                Kind::Toggle {
                    label,
                    active: i == 0,
                },
            );
            choices.push((id, group, name));
        }
    }
    ui.add(Ui::ROOT, Kind::Label("Changes apply at once.".into()));
    ui.layout();
    Chooser { ui, choices }
}
