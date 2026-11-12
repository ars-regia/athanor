//! The strips a bar is made of: [`Kind::Inline`], children side by side at their own widths
//! as a GTK box that is not homogeneous, and [`Kind::CenterBox`], three children at the
//! start, the centre and the end of a line, as GTK's `CenterBox`.

use super::{Kind, Ui, WidgetId};

/// The size of an inline whose children measure `sizes`: their widths and the gaps between
/// them, the tallest of them, and its padding, horizontal then vertical, on both sides.
pub(super) fn measure_inline(sizes: &[(f32, f32)], gap: f32, pad: [f32; 2]) -> (f32, f32) {
    let gaps = gap * sizes.len().saturating_sub(1) as f32;
    let w = sizes.iter().map(|s| s.0).sum::<f32>() + gaps;
    let h = sizes.iter().map(|s| s.1).fold(0.0, f32::max);
    (w + 2.0 * pad[0], h + 2.0 * pad[1])
}

/// The size of a centre box whose children measure `sizes`: room for all three side by
/// side, and the tallest of them.
pub(super) fn measure_center(sizes: &[(f32, f32)]) -> (f32, f32) {
    let w = sizes.iter().map(|s| s.0).sum::<f32>();
    (w, sizes.iter().map(|s| s.1).fold(0.0, f32::max))
}

/// Where the middle child of a centre box `width` wide starts, `w` wide between a start
/// child `start` wide and an end child `end` wide: centred on the whole box, and moved aside
/// only as far as the start and end children need, as GTK's `CenterBox`.
fn centre_x(width: f32, start: f32, w: f32, end: f32) -> f32 {
    let centred = (width - w) / 2.0;
    centred.max(start).min((width - end - w).max(start))
}

impl Ui {
    /// Places the children of the inline `id` at `x`, `y` in reading order, each at its own
    /// width and the inline's height inside its padding.
    pub(super) fn place_inline(&mut self, id: WidgetId, x: f32, y: f32, pad: [f32; 2]) {
        let gap = self.theme.gap;
        let outer = self.widgets[id].rect;
        let h = (outer.h - 2.0 * pad[1]).max(0.0);
        let mut cx = if self.rtl {
            x + outer.w - pad[0]
        } else {
            x + pad[0]
        };
        for c in self.visible_children(id) {
            let w = self.widgets[c].rect.w;
            self.widgets[c].rect.h = h;
            if self.rtl {
                cx -= w;
                self.place(c, cx, y + pad[1]);
                cx -= gap;
            } else {
                self.place(c, cx, y + pad[1]);
                cx += w + gap;
            }
        }
    }

    /// Places the children of the centre box `id` at `x`, `y`: the first at the start edge,
    /// the second in the middle, the third at the end edge, each at its own width and the
    /// box's height. A child that is not shown keeps its place empty.
    pub(super) fn place_center(&mut self, id: WidgetId, x: f32, y: f32) {
        let outer = self.widgets[id].rect;
        let slots: Vec<Option<WidgetId>> = self.widgets[id]
            .children
            .iter()
            .take(3)
            .map(|&c| self.widgets[c].visible.then_some(c))
            .collect();
        let width = |slot: Option<&Option<WidgetId>>| {
            slot.copied()
                .flatten()
                .map_or(0.0, |c| self.widgets[c].rect.w)
        };
        let (start, middle, end) = (
            width(slots.first()),
            width(slots.get(1)),
            width(slots.get(2)),
        );
        let centre = centre_x(outer.w, start, middle, end);
        // Mirrored in right to left: the start child at the right edge. Each offset is
        // computed from its own edge, so an edge child sits exactly on the box's edge.
        let offsets = if self.rtl {
            [outer.w - start, outer.w - centre - middle, 0.0]
        } else {
            [0.0, centre, outer.w - end]
        };
        for (slot, offset) in slots.iter().zip(offsets) {
            let Some(c) = *slot else { continue };
            self.widgets[c].rect.h = outer.h;
            self.place(c, x + offset, y);
        }
    }

    /// Adds an inline: its children side by side in reading order at their own widths, with
    /// `padding` logical pixels kept free left and right, then above and below. See
    /// [`Kind::Inline`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn inline(&mut self, parent: WidgetId, padding: [f32; 2]) -> WidgetId {
        self.add(parent, Kind::Inline { padding })
    }
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Theme, Ui};

    fn theme() -> Theme {
        Theme {
            gap: 2.0,
            margin: 0.0,
            ..Theme::default()
        }
    }

    #[test]
    fn an_inline_keeps_each_childs_width_and_gives_them_its_height() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Strip", theme(), rtl);
            let inline = ui.inline(Ui::ROOT, [4.0, 2.0]);
            let short = ui.add(inline, Kind::Button("A".into()));
            let long = ui.add(inline, Kind::Button("A much longer label".into()));
            let tall = ui.add(inline, Kind::Separator);
            ui.layout();
            let (s, l, i) = (
                ui.widget(short).rect,
                ui.widget(long).rect,
                ui.widget(inline).rect,
            );
            assert!(l.w > s.w, "not homogeneous");
            assert_eq!(s.h, l.h);
            assert_eq!(s.h, ui.widget(tall).rect.h);
            assert_eq!(i.h, s.h + 4.0);
            assert_eq!(i.w, s.w + l.w + ui.widget(tall).rect.w + 2.0 * 2.0 + 8.0);
            if rtl {
                assert_eq!(
                    s.x + s.w,
                    i.x + i.w - 4.0,
                    "the first child at the right edge"
                );
                assert_eq!(l.x + l.w + 2.0, s.x);
            } else {
                assert_eq!(s.x, i.x + 4.0);
                assert_eq!(l.x, s.x + s.w + 2.0);
            }
            assert_eq!(s.y, i.y + 2.0);
        }
    }

    #[test]
    fn a_centre_box_centres_its_middle_child_on_the_whole_line() {
        let mut ui = Ui::new("Strip", theme(), false);
        ui.min_width = 800.0;
        let center = ui.add(Ui::ROOT, Kind::CenterBox);
        let start = ui.inline(center, [0.0, 0.0]);
        ui.add(
            start,
            Kind::Button("A long start group, longer than the end".into()),
        );
        let middle = ui.inline(center, [0.0, 0.0]);
        ui.add(middle, Kind::Button("Clock".into()));
        let end = ui.inline(center, [0.0, 0.0]);
        ui.add(end, Kind::Button("End".into()));
        ui.layout();
        let (b, s, m, e) = (
            ui.widget(center).rect,
            ui.widget(start).rect,
            ui.widget(middle).rect,
            ui.widget(end).rect,
        );
        assert_eq!(b.w, 800.0, "it fills the column's width");
        assert_eq!(s.x, 0.0);
        assert_eq!(e.x + e.w, 800.0);
        assert_eq!(
            m.x + m.w / 2.0,
            400.0,
            "centred on the line, not between its neighbours"
        );
        assert_eq!(s.h, b.h);
    }

    #[test]
    fn a_crowded_centre_moves_aside_and_a_hidden_child_keeps_its_place() {
        assert_eq!(
            super::centre_x(100.0, 60.0, 20.0, 0.0),
            60.0,
            "past the start child"
        );
        assert_eq!(
            super::centre_x(100.0, 0.0, 20.0, 60.0),
            20.0,
            "before the end child"
        );
        assert_eq!(super::centre_x(100.0, 10.0, 20.0, 10.0), 40.0);
        let mut ui = Ui::new("Strip", theme(), true);
        ui.min_width = 300.0;
        let center = ui.add(Ui::ROOT, Kind::CenterBox);
        let start = ui.add(center, Kind::Button("Start".into()));
        let middle = ui.add(center, Kind::Button("Middle".into()));
        let end = ui.add(center, Kind::Button("End".into()));
        ui.set_visible(middle, false);
        ui.layout();
        let (s, e) = (ui.widget(start).rect, ui.widget(end).rect);
        assert_eq!(
            s.x + s.w,
            300.0,
            "right to left: the start child at the right edge"
        );
        assert_eq!(e.x, 0.0);
    }
}
