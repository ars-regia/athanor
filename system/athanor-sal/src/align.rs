//! Where a widget sits in the column that holds it, as GTK's `halign` and `valign`: across the
//! column at its start or end edge, at its own width, and along it at the top or the bottom.
//! The control center puts its panel at a corner of a surface over the whole output this way
//! (doc_control_center.md CC9).

use crate::widget::{Ui, WidgetId};

/// How a widget sits along one axis of its column: see [`Ui::set_align`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// As the column places it: a row, a group, a switch, a slider, a list and the like take
    /// its width, every other widget sits at its start edge; along the column, after the
    /// widget before it.
    #[default]
    Fill,
    /// At the start edge at its own width: the left one, the right one in right to left; at
    /// the top along the column.
    Start,
    /// At the end edge at its own width: the right one, the left one in right to left; at the
    /// bottom along the column, after every widget that is not at the bottom.
    End,
}

impl Ui {
    /// Places widget `id` in its column by `align`, across it and along it, as GTK's `halign`
    /// and `valign`: [`Align::Start`] and [`Align::End`] keep its own width at that edge, and
    /// along the column [`Align::End`] puts it at the bottom, with every other widget at the
    /// bottom in order. Outside a column or group, the root included, it changes nothing.
    /// Whether it changed; call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_align(&mut self, id: WidgetId, align: [Align; 2]) -> bool {
        let widget = &mut self.widgets[id];
        let changed = widget.align != align;
        widget.align = align;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::Align;
    use crate::theme::Theme;
    use crate::widget::{Kind, Ui};

    fn theme() -> Theme {
        Theme {
            margin: 10.0,
            gap: 0.0,
            ..Theme::default()
        }
    }

    #[test]
    fn a_widget_at_the_end_sits_in_the_corner_of_the_root() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Align", theme(), rtl);
            (ui.min_width, ui.min_height) = (400.0, 300.0);
            let panel = ui.add(Ui::ROOT, Kind::Group("Panel".into()));
            ui.add(panel, Kind::Button("Wi-Fi".into()));
            ui.layout();
            assert_eq!(ui.widget(panel).rect.w, 380.0, "a group fills by default");
            assert!(ui.set_align(panel, [Align::End, Align::End]));
            assert!(!ui.set_align(panel, [Align::End, Align::End]), "unchanged");
            ui.layout();
            let r = ui.widget(panel).rect;
            assert!(r.w < 380.0, "its own width");
            assert_eq!(r.y + r.h, 290.0, "at the bottom, inside the margin");
            if rtl {
                assert_eq!(r.x, 10.0, "the end edge is the left one");
            } else {
                assert_eq!(r.x + r.w, 390.0);
            }
            assert!(ui.set_align(panel, [Align::Start, Align::Start]));
            ui.layout();
            let r = ui.widget(panel).rect;
            assert_eq!(r.y, 10.0);
            if rtl {
                assert_eq!(r.x + r.w, 390.0, "the start edge is the right one");
            } else {
                assert_eq!(r.x, 10.0);
            }
        }
    }

    #[test]
    fn an_island_aligned_along_the_root_keeps_its_own_height() {
        let mut ui = Ui::new("Align", theme(), false);
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let island = ui.island(Ui::ROOT, true, [0.0; 4]);
        ui.add(island, Kind::Button("Wi-Fi".into()));
        ui.layout();
        assert_eq!(ui.widget(island).rect.h, 280.0, "an island fills by default");
        for along in [Align::Start, Align::End] {
            ui.set_align(island, [Align::End, along]);
            ui.layout();
            let r = ui.widget(island).rect;
            assert!(r.h < 280.0, "{along:?}: its own height");
            assert_eq!(r.x + r.w, 390.0, "{along:?}");
            let y = if along == Align::End { 290.0 - r.h } else { 10.0 };
            assert_eq!(r.y, y, "{along:?}");
        }
    }

    #[test]
    fn a_nested_column_places_its_child_at_its_end_and_the_rest_in_order() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Align", theme(), rtl);
            ui.min_width = 300.0;
            let column = ui.add(Ui::ROOT, Kind::Group("Column".into()));
            let top = ui.add(column, Kind::Button("Top".into()));
            let end = ui.add(column, Kind::Button("End".into()));
            let row = ui.add(column, Kind::Row);
            ui.add(row, Kind::Button("Row".into()));
            assert!(ui.set_align(end, [Align::End, Align::Fill]));
            ui.layout();
            let (c, t, e, r) = (
                ui.widget(column).rect,
                ui.widget(top).rect,
                ui.widget(end).rect,
                ui.widget(row).rect,
            );
            assert_eq!((t.y, e.y, r.y), (c.y, t.y + t.h, e.y + e.h), "in order");
            assert_eq!(r.w, c.w, "a row still fills");
            if rtl {
                assert_eq!((e.x, t.x + t.w), (c.x, c.x + c.w));
            } else {
                assert_eq!((e.x + e.w, t.x), (c.x + c.w, c.x));
            }
            assert!(!ui.set_align(end, [Align::End, Align::Fill]));
        }
    }
}
