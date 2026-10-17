//! The layout chooser's widgets, as the GTK chooser's `ui.rs` lays them out: in a window 420
//! pixels wide, a group per setting, each a heading over a row of toggle buttons of one width.
//! Every string is a message of the GTK chooser, translated by its catalogue: nothing here is
//! translated again.

use athanor_i18n::Catalog;
use athanor_toolkit::{Kind, Theme, Ui, WidgetId};

pub const TITLE: &str = "Layout";

/// Each group's heading and its choices, as message ids of the GTK chooser.
pub const GROUPS: [(&str, &[&str]); 3] = [
    ("Style", &["Island", "Bar", "Essential"]),
    ("Panel", &["Top", "Bottom"]),
    ("Dock", &["Visible", "Auto-hide", "None"]),
];

pub struct Chooser {
    pub ui: Ui,
    /// Each toggle with its group's and its own message id.
    pub choices: Vec<(WidgetId, &'static str, &'static str)>,
}

/// The chooser in the catalogue's language, with the first choice of every group active: the
/// factory layout, as the GTK chooser shows it without a user document.
pub fn build(catalog: &Catalog, rtl: bool) -> Chooser {
    // The GTK chooser's margins and spacings: 24 around, 18 between the groups, 8 inside one.
    let theme = Theme {
        margin: 24.0,
        section_gap: 18.0,
        gap: 8.0,
        ..Theme::default()
    };
    let mut ui = Ui::new(catalog.tr(TITLE), theme, rtl);
    ui.min_width = 420.0;
    let mut choices = Vec::new();
    for (group, names) in GROUPS {
        let heading = catalog.tr(group);
        let container = ui.add(Ui::ROOT, Kind::Group(heading.into()));
        ui.add(container, Kind::Label(heading.into()));
        let row = ui.add(container, Kind::Row);
        for (i, &name) in names.iter().enumerate() {
            let label = catalog.tr(name).into();
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
    ui.layout();
    Chooser { ui, choices }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_string_is_a_message_of_the_gtk_chooser() {
        let template = include_str!(
            "../../../../forge/specs/athanor-layout-chooser/athanor-layout-chooser-1.0.0/po/athanor-layout-chooser.pot"
        );
        let messages = GROUPS
            .iter()
            .flat_map(|(group, names)| std::iter::once(*group).chain(names.iter().copied()))
            .chain([TITLE]);
        for msgid in messages {
            assert!(
                template.contains(&format!("\nmsgid \"{msgid}\"\n")),
                "{msgid:?} is not in the GTK chooser's template"
            );
        }
    }

    #[test]
    fn the_window_is_as_wide_as_the_gtk_choosers() {
        for rtl in [false, true] {
            assert_eq!(
                build(&Catalog::empty(), rtl).ui.size().0,
                420.0,
                "rtl {rtl}"
            );
        }
    }
}
