//! Where a widget sits in the column that holds it, as GTK's `halign` and `valign`: across the
//! column at its start or end edge, at its own width, and along it at the top or the bottom.
//! The control center puts its panel at a corner of a surface over the whole output this way
//! (doc_control_center.md CC9).

use crate::widget::{Kind, Ui, WidgetId};

/// How a widget sits along one axis of its column: see [`Ui::set_align`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// As the column places it: a row, a group, a switch, a slider, a list, a wrapping label
    /// and the like take its width, every other widget sits at its start edge; along the
    /// column, after the widget before it.
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
    /// bottom in order. A widget wider than its column sits at the start edge and overflows at
    /// the end, and the widgets at the bottom never rise over the ones before them. A wrapping
    /// label aligned across keeps its own width, up to the column's, and in a narrower column
    /// breaks into more lines, down to its longest word. Call [`Ui::layout`] afterwards.
    ///
    /// - `Some(true)`: the alignment changed.
    /// - `Some(false)`: the widget already has this alignment.
    /// - `None`: ignored, and nothing changes: only a [`Kind::Column`], the root included, a
    ///   [`Kind::Group`] and a [`Kind::Alert`], laid out as a group, read their children's
    ///   alignment, so the root itself, the title bar, a child of any other widget and a
    ///   [`Kind::Picture`], whose size comes from its column alone, take none.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_align(&mut self, id: WidgetId, align: [Align; 2]) -> Option<bool> {
        let widget = &self.widgets[id];
        let parent = &self.widgets[widget.parent?].kind;
        if !matches!(parent, Kind::Column | Kind::Group(_) | Kind::Alert(_))
            || matches!(widget.kind, Kind::Picture { .. })
            || self.title_bar == Some(id)
        {
            return None;
        }
        let widget = &mut self.widgets[id];
        let changed = widget.align != align;
        widget.align = align;
        Some(changed)
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
            assert_eq!(ui.set_align(panel, [Align::End, Align::End]), Some(true));
            assert_eq!(ui.set_align(panel, [Align::End, Align::End]), Some(false), "unchanged");
            ui.layout();
            let r = ui.widget(panel).rect;
            assert!(r.w < 380.0, "its own width");
            assert_eq!(r.y + r.h, 290.0, "at the bottom, inside the margin");
            if rtl {
                assert_eq!(r.x, 10.0, "the end edge is the left one");
            } else {
                assert_eq!(r.x + r.w, 390.0);
            }
            assert_eq!(ui.set_align(panel, [Align::Start, Align::Start]), Some(true));
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
            assert_eq!(ui.set_align(island, [Align::End, along]), Some(true));
            ui.layout();
            let r = ui.widget(island).rect;
            assert!(r.h < 280.0, "{along:?}: its own height");
            assert_eq!(r.x + r.w, 390.0, "{along:?}");
            let y = if along == Align::End { 290.0 - r.h } else { 10.0 };
            assert_eq!(r.y, y, "{along:?}");
        }
    }

    #[test]
    fn a_child_of_an_alert_takes_its_alignment_as_in_a_group() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Align", theme(), rtl);
            ui.min_width = 300.0;
            let alert = ui.alert(Ui::ROOT, "Battery low");
            let text = ui.add(alert, Kind::Label("Plug in the charger".into()));
            let end = ui.add(alert, Kind::Button("Dismiss".into()));
            assert_eq!(ui.set_align(end, [Align::End, Align::Fill]), Some(true));
            ui.layout();
            let (a, t, e) = (ui.widget(alert).rect, ui.widget(text).rect, ui.widget(end).rect);
            assert_eq!(e.y, t.y + t.h, "in order");
            assert!(e.w < a.w, "its own width");
            if rtl {
                assert_eq!(e.x, a.x, "rtl {rtl}");
            } else {
                assert_eq!(e.x + e.w, a.x + a.w, "rtl {rtl}");
            }
            assert_eq!(ui.set_align(end, [Align::End, Align::Fill]), Some(false));
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
            assert_eq!(ui.set_align(end, [Align::End, Align::Fill]), Some(true));
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
            assert_eq!(ui.set_align(end, [Align::End, Align::Fill]), Some(false));
        }
    }

    /// A theme whose gap is not zero, for the tests of widgets stacked at the bottom.
    fn gapped() -> Theme {
        Theme {
            margin: 10.0,
            gap: 6.0,
            section_gap: 6.0,
            ..Theme::default()
        }
    }

    #[test]
    fn only_a_column_or_a_group_reads_the_alignment_and_never_a_pictures() -> Result<(), crate::render::Error> {
        let mut ui = Ui::new("Align", theme(), false);
        ui.add_title_bar();
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let picture = ui.picture(Ui::ROOT, crate::Image::from_rgba8(1, 1, vec![0, 0, 0, 255])?, "");
        let row = ui.add(Ui::ROOT, Kind::Row);
        let in_row = ui.add(row, Kind::Button("Cell".into()));
        let group = ui.add(Ui::ROOT, Kind::Group("Group".into()));
        let in_group = ui.add(group, Kind::Button("Item".into()));
        let ignored = [Align::End, Align::End];
        assert_eq!(ui.set_align(Ui::ROOT, ignored), None, "the root has no column");
        assert_eq!(ui.set_align(picture, ignored), None, "a picture");
        assert_eq!(ui.set_align(in_row, ignored), None, "a row ignores it");
        let Some(bar) = ui.widget(Ui::ROOT).children.first().copied() else {
            unreachable!("the title bar is the root's first child");
        };
        assert_eq!(ui.set_align(bar, ignored), None, "the title bar");
        assert_eq!(ui.set_align(in_group, ignored), Some(true));
        assert_eq!(ui.set_align(row, ignored), Some(true), "the root is a column");
        ui.layout();
        let r = ui.widget(picture).rect;
        assert_eq!(r.w, 380.0, "a picture still fills its column");
        Ok(())
    }

    #[test]
    fn an_island_that_fills_stops_above_the_widgets_at_the_bottom() {
        let mut ui = Ui::new("Align", gapped(), false);
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let island = ui.island(Ui::ROOT, true, [0.0; 4]);
        ui.add(island, Kind::Button("Wi-Fi".into()));
        let done = ui.add(Ui::ROOT, Kind::Button("Done".into()));
        assert_eq!(ui.set_align(done, [Align::End, Align::End]), Some(true));
        ui.layout();
        let (i, d) = (ui.widget(island).rect, ui.widget(done).rect);
        assert_eq!(d.y + d.h, 290.0, "the button at the bottom");
        assert_eq!(i.y + i.h, d.y - 6.0, "the island down to it, a gap apart");
    }

    #[test]
    fn an_island_that_fills_leaves_room_for_the_flow_after_it_above_the_bottom() {
        let mut ui = Ui::new("Align", gapped(), false);
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let island = ui.island(Ui::ROOT, true, [0.0; 4]);
        ui.add(island, Kind::Button("Wi-Fi".into()));
        let label = ui.add(Ui::ROOT, Kind::Label("Connected".into()));
        let done = ui.add(Ui::ROOT, Kind::Button("Done".into()));
        assert_eq!(ui.set_align(done, [Align::End, Align::End]), Some(true));
        ui.layout();
        let (i, l, d) = (ui.widget(island).rect, ui.widget(label).rect, ui.widget(done).rect);
        assert_eq!(d.y + d.h, 290.0, "the button at the bottom");
        assert_eq!(l.y, i.y + i.h + 6.0, "the label after the island");
        assert_eq!(l.y + l.h, d.y - 6.0, "the label a gap above the button");

        // Without a widget at the bottom, the flow after the island overflows the column as it
        // always has: the island takes the whole rest.
        let mut ui = Ui::new("Align", gapped(), false);
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let island = ui.island(Ui::ROOT, true, [0.0; 4]);
        ui.add(island, Kind::Button("Wi-Fi".into()));
        let label = ui.add(Ui::ROOT, Kind::Label("Connected".into()));
        ui.layout();
        let (i, l) = (ui.widget(island).rect, ui.widget(label).rect);
        assert_eq!(i.y + i.h, 290.0, "down to the margin");
        assert_eq!(l.y, 296.0, "below it");
    }

    #[test]
    fn a_wrapping_label_fills_its_column_unless_aligned_across_it() {
        let mut ui = Ui::new("Align", gapped(), false);
        ui.min_width = 400.0;
        let wrap = crate::Wrap {
            max_width_chars: std::num::NonZeroU32::new(10),
            lines: None,
        };
        let filled = ui.add(Ui::ROOT, Kind::Label("A sentence long enough to wrap".into()));
        let start = ui.add(Ui::ROOT, Kind::Label("A sentence long enough to wrap".into()));
        assert!(ui.set_wrap(filled, Some(wrap.clone())));
        assert!(ui.set_wrap(start, Some(wrap)));
        assert_eq!(ui.set_align(start, [Align::Start, Align::Start]), Some(true));
        ui.layout();
        let (f, s) = (ui.widget(filled).rect, ui.widget(start).rect);
        assert_eq!(f.w, 380.0, "the column's width inside its margins");
        assert!(s.w < f.w, "{} keeps the width it asks for", s.w);
        assert_eq!(s.x, f.x, "at the start");
    }

    #[test]
    fn widgets_at_the_bottom_follow_one_another_a_gap_apart_and_skip_the_hidden() {
        let mut ui = Ui::new("Align", gapped(), false);
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let ends: Vec<_> = ["One", "Two", "Three"]
            .into_iter()
            .map(|name| ui.add(Ui::ROOT, Kind::Button(name.into())))
            .collect();
        for &id in &ends {
            assert_eq!(ui.set_align(id, [Align::Fill, Align::End]), Some(true));
        }
        ui.layout();
        let r: Vec<_> = ends.iter().map(|&id| ui.widget(id).rect).collect();
        assert_eq!(r[2].y + r[2].h, 290.0);
        assert_eq!((r[1].y, r[2].y), (r[0].y + r[0].h + 6.0, r[1].y + r[1].h + 6.0));
        ui.set_visible(ends[1], false);
        ui.layout();
        let (one, three) = (ui.widget(ends[0]).rect, ui.widget(ends[2]).rect);
        assert_eq!(three.y + three.h, 290.0);
        assert_eq!(three.y, one.y + one.h + 6.0, "the hidden one takes no room");
    }

    #[test]
    fn a_title_bar_stays_on_top_of_a_widget_at_the_bottom_of_the_root() {
        let mut ui = Ui::new("Align", theme(), false);
        ui.add_title_bar();
        (ui.min_width, ui.min_height) = (400.0, 300.0);
        let first = ui.add(Ui::ROOT, Kind::Button("First".into()));
        let done = ui.add(Ui::ROOT, Kind::Button("Done".into()));
        assert_eq!(ui.set_align(done, [Align::End, Align::End]), Some(true));
        ui.layout();
        let Some(bar) = ui.widget(Ui::ROOT).children.first().copied() else {
            unreachable!("the title bar is the root's first child");
        };
        let (b, f, d) = (ui.widget(bar).rect, ui.widget(first).rect, ui.widget(done).rect);
        assert_eq!((b.x, b.y, b.w), (0.0, 0.0, 400.0), "the bar across the top");
        assert_eq!(f.y, b.h + 10.0, "the flow below it");
        assert_eq!((d.y + d.h, d.x + d.w), (290.0, 390.0), "the button in the corner");
    }
}
