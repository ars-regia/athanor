//! The strips a bar is made of: [`Kind::Inline`], children side by side at their own widths
//! as a GTK box that is not homogeneous, and [`Kind::CenterBox`], three children at the
//! start, the centre and the end of a line, as GTK's `CenterBox`.

use super::{length, Kind, Ui, WidgetId};

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

/// The x and the width of a child that runs from `start` to `end` along a strip at `x`,
/// `width` wide, in reading order: mirrored in right to left, each edge measured from its own
/// side of the strip, and an edge at 0 or at `width` placed on the strip's edge itself.
pub(super) fn mirrored(x: f32, width: f32, start: f32, end: f32, rtl: bool) -> (f32, f32) {
    let w = end - start;
    if !rtl {
        return (x + start, w);
    }
    if end == width {
        // The far end in reading order is the left edge.
        (x, width - start)
    } else {
        (x + (width - end), w)
    }
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
        let gap = self.widgets[id].spacing.unwrap_or(self.theme.gap);
        let outer = self.widgets[id].rect;
        let h = (outer.h - 2.0 * pad[1]).max(0.0);
        // The children that fill split what the others leave of the width, as GTK's hexpand.
        // Each from the width its measure gave it, so that placing again without a measure,
        // as a scroll does, gives the same widths.
        let shown = self.visible_children(id);
        let inner = (outer.w - 2.0 * pad[0]).max(0.0);
        let fills = shown.iter().filter(|&&c| self.widgets[c].fill).count();
        let gaps = gap * shown.len().saturating_sub(1) as f32;
        let used = shown.iter().map(|&c| self.widgets[c].natural_w).sum::<f32>() + gaps;
        let spare = inner - used;
        let share = if fills > 0 && spare > 0.0 {
            spare / fills as f32
        } else {
            0.0
        };
        let last = shown.len().saturating_sub(1);
        let mut start = 0.0;
        for (at, &c) in shown.iter().enumerate() {
            let widget = &self.widgets[c];
            let w = widget.natural_w + if widget.fill { share } else { 0.0 };
            // With a share given out, the last child ends on the inner edge itself.
            let end = if share > 0.0 && at == last {
                inner
            } else {
                start + w
            };
            let (cx, cw) = mirrored(x + pad[0], inner, start, end, self.rtl);
            self.widgets[c].rect.w = cw;
            self.widgets[c].rect.h = h;
            self.place(c, cx, y + pad[1]);
            start += w + gap;
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
    /// `padding` logical pixels kept free left and right, then above and below; a padding that
    /// is not a finite length of at least 0 counts as 0, and one past 2^64 times less than the
    /// largest `f32` as that length, so that the inline and any container of it add up to a
    /// finite size. See [`Kind::Inline`].
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn inline(&mut self, parent: WidgetId, padding: [f32; 2]) -> WidgetId {
        let padding = padding.map(length);
        self.add(parent, Kind::Inline { padding })
    }

    /// Makes widget `id` take a share of the width its inline has to spare, as GTK's `hexpand`:
    /// the children of an inline that fill split equally what the others leave of its width, and
    /// an inline with a child shown that fills takes the width of its column or group. In a
    /// column or a group it takes their width, as a GTK box's child of any kind fills its box
    /// across, unless aligned ([`Ui::set_align`]). Elsewhere it changes nothing. Whether it
    /// changed; call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_fill(&mut self, id: WidgetId, fill: bool) -> bool {
        let widget = &mut self.widgets[id];
        let changed = widget.fill != fill;
        widget.fill = fill;
        changed
    }

    /// Splits row `id` into at least `slots` equal cells, as the columns of a homogeneous GTK
    /// grid: with fewer children shown, the cells past them stay empty at the end edge. 0 gives
    /// each child shown a cell. Whether it changed: `false` for a widget that is not a row. Call
    /// [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_slots(&mut self, id: WidgetId, slots: usize) -> bool {
        let widget = &mut self.widgets[id];
        if !matches!(widget.kind, Kind::Row) || widget.slots == slots {
            return false;
        }
        widget.slots = slots;
        true
    }

    /// Whether `id` is an inline with a child shown that fills, which takes its column's width.
    pub(super) fn inline_fills(&self, id: WidgetId) -> bool {
        matches!(self.widgets[id].kind, Kind::Inline { .. })
            && self.widgets[id]
                .children
                .iter()
                .any(|&c| self.widgets[c].fill && self.widgets[c].visible)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Rect, Theme, Ui, WidgetId};

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
    fn a_widget_set_to_fill_takes_the_width_of_its_column() {
        let mut ui = Ui::new("Strip", theme(), false);
        ui.min_width = 400.0;
        let page = ui.add(Ui::ROOT, Kind::Column);
        let list = ui.add(page, Kind::Column);
        let wifi = ui.switch(list, "Wi-Fi", false);
        let row = ui.add(page, Kind::Button("Speakers".into()));
        let narrow = ui.add(page, Kind::Column);
        ui.add(narrow, Kind::Label("Short".into()));
        for id in [page, list, row] {
            assert!(ui.set_fill(id, true));
        }
        ui.layout();
        assert_eq!(ui.widget(page).rect.w, 400.0, "the root's width");
        for id in [list, wifi, row] {
            assert_eq!(ui.widget(id).rect.w, 400.0, "filled across, the switch to the end edge");
        }
        assert!(ui.widget(narrow).rect.w < 400.0, "a column not set to fill keeps its width");
    }

    #[test]
    fn a_padding_that_is_not_a_finite_length_of_at_least_0_counts_as_0() {
        let rect = |padding| {
            let mut ui = Ui::new("Strip", theme(), false);
            let inline = ui.inline(Ui::ROOT, padding);
            ui.add(inline, Kind::Button("A".into()));
            ui.layout();
            ui.widget(inline).rect
        };
        let none = rect([0.0, 0.0]);
        for padding in [[f32::NAN, -4.0], [f32::INFINITY, f32::NEG_INFINITY], [-1.0, f32::NAN]] {
            assert_eq!(rect(padding), none, "{padding:?}");
        }
        assert_eq!(rect([3.0, 0.0]).w, none.w + 6.0, "a valid padding is kept");
    }

    #[test]
    fn a_child_of_an_inline_or_a_centre_box_keeps_its_least_width() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Strip", theme(), rtl);
            ui.min_width = 800.0;
            let inline = ui.inline(Ui::ROOT, [0.0, 0.0]);
            let a = ui.add(inline, Kind::Button("A".into()));
            let center = ui.add(Ui::ROOT, Kind::CenterBox);
            let [_, middle, _] =
                ["Start", "Middle", "End"].map(|t| ui.add(center, Kind::Button(t.into())));
            for id in [a, middle] {
                assert!(ui.set_min_width(id, 120.0));
            }
            ui.layout();
            for id in [a, middle] {
                assert_eq!(ui.widget(id).rect.w, 120.0, "{rtl}: {id:?}");
            }
            let (b, m) = (ui.widget(center).rect, ui.widget(middle).rect);
            assert!((m.x + m.w / 2.0 - (b.x + b.w / 2.0)).abs() < 0.01, "{rtl}: still centred");
        }
    }

    #[test]
    fn a_child_that_fills_takes_the_spare_width_and_its_inline_the_columns() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Strip", theme(), rtl);
            ui.min_width = 400.0;
            let plain = ui.inline(Ui::ROOT, [0.0, 0.0]);
            let alone = ui.add(plain, Kind::Button("Alone".into()));
            let inline = ui.inline(Ui::ROOT, [4.0, 0.0]);
            let mute = ui.add(inline, Kind::Button("Mute".into()));
            let volume = ui.slider(inline, "Volume", 0.0, 100.0, 1.0, 50.0);
            let open = ui.add(inline, Kind::Button("Open".into()));
            let Some(volume) = volume else {
                unreachable!("a valid range");
            };
            assert!(ui.set_fill(volume, true));
            assert!(!ui.set_fill(volume, true), "already filling");
            ui.layout();
            let (p, a) = (ui.widget(plain).rect, ui.widget(alone).rect);
            assert_eq!(p.w, a.w, "no child fills: the inline keeps its width");
            let (i, m, v, o) = (
                ui.widget(inline).rect,
                ui.widget(mute).rect,
                ui.widget(volume).rect,
                ui.widget(open).rect,
            );
            assert_eq!(i.w, 400.0, "it takes the column's width");
            assert_eq!(m.w + v.w + o.w + 2.0 * 2.0 + 8.0, 400.0, "no width left");
            if rtl {
                assert_eq!((m.x + m.w, o.x), (396.0, 4.0));
            } else {
                assert_eq!((m.x, o.x + o.w), (4.0, 396.0));
            }
        }
    }

    #[test]
    fn a_huge_padding_still_gives_a_finite_size() {
        for padding in [[2e38, 0.0], [0.0, 2e38], [f32::MAX, f32::MAX]] {
            let mut ui = Ui::new("Strip", theme(), false);
            let inline = ui.inline(Ui::ROOT, padding);
            let child = ui.add(inline, Kind::Button("A".into()));
            let (w, h) = ui.layout();
            let finite = |r: Rect| [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite());
            assert!(finite(ui.widget(inline).rect), "{padding:?}");
            assert!(finite(ui.widget(child).rect), "{padding:?}");
            assert!(w.is_finite() && h.is_finite(), "{padding:?}");
        }
    }

    #[test]
    fn a_row_with_slots_keeps_a_cell_for_each_slot() {
        let mut ui = Ui::new("Strip", theme(), false);
        ui.min_width = 202.0;
        let row = ui.add(Ui::ROOT, Kind::Row);
        let only = ui.add(row, Kind::Button("Wi-Fi".into()));
        assert!(ui.set_slots(row, 2));
        assert!(!ui.set_slots(row, 2), "already two");
        assert!(!ui.set_slots(only, 2), "not a row");
        ui.layout();
        assert_eq!(ui.widget(only).rect.w, 100.0, "half the row, less the gap");
        assert_eq!(ui.widget(only).rect.x, 0.0);
        assert!(ui.set_slots(row, 0));
        ui.layout();
        assert_eq!(ui.widget(only).rect.w, 202.0, "one cell for its one child");
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
        assert!(
            (m.x + m.w / 2.0 - 400.0).abs() < 0.01,
            "centred on the line, not between its neighbours: {m:?}"
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

    #[test]
    fn a_centre_box_has_no_place_for_a_fourth_child_and_never_shows_one() {
        let mut ui = Ui::new("Strip", theme(), false);
        let center = ui.add(Ui::ROOT, Kind::CenterBox);
        let three = ["Start", "Middle", "End"].map(|t| ui.add(center, Kind::Button(t.into())));
        ui.layout();
        let width = ui.widget(center).rect.w;
        // Room in the window below the box, for a rectangle of the fourth child's own.
        ui.min_height = ui.widget(center).rect.h + 40.0;
        let fourth = ui.add(center, Kind::Button("A fourth child, wider than the rest".into()));
        let (w, h) = ui.layout();
        assert_eq!(ui.widget(center).rect.w, width, "measured as three children");
        assert!(!ui.is_visible(fourth), "neither drawn nor read");
        // Even with a rectangle of its own, in the room below the others, no point of it hits it.
        let below = ui.widget(center).rect;
        let y = below.y + below.h + 10.0;
        ui.widgets[fourth].rect = Rect { x: 0.0, y, w: 50.0, h: 20.0 };
        let probe = (10.0, y + 10.0);
        let window = Rect { x: 0.0, y: 0.0, w, h };
        assert!(window.contains(probe.0, probe.1), "{probe:?} inside the window {window:?}");
        assert_eq!(ui.hit(probe.0, probe.1), None, "never hit");
        assert!(!crate::focus::chain(&ui).contains(&fourth), "never focused");
        assert!(three.iter().all(|&c| ui.is_visible(c)));
    }

    /// Whether two sums of floats agree: an edge computed as x + w carries their rounding.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// An inline of `labels` under a root `width` wide, the ones `fill` names filling.
    fn filled(width: f32, theme: Theme, rtl: bool, labels: &[(&str, bool)]) -> (Ui, WidgetId, Vec<WidgetId>) {
        let mut ui = Ui::new("Strip", theme, rtl);
        ui.min_width = width;
        let inline = ui.inline(Ui::ROOT, [4.0, 0.0]);
        let children = labels
            .iter()
            .map(|&(label, fill)| {
                let child = ui.add(inline, Kind::Button(label.into()));
                ui.set_fill(child, fill);
                child
            })
            .collect();
        ui.layout();
        (ui, inline, children)
    }

    #[test]
    fn several_children_that_fill_split_the_spare_equally() {
        for rtl in [false, true] {
            let (ui, inline, c) = filled(
                400.0,
                theme(),
                rtl,
                &[("A", true), ("Plain", false), ("A longer one", true)],
            );
            let mut alone = Ui::new("Strip", theme(), rtl);
            let natural: Vec<f32> = ["A", "A longer one"]
                .iter()
                .map(|label| {
                    let id = alone.add(Ui::ROOT, Kind::Button((*label).into()));
                    alone.layout();
                    alone.widget(id).rect.w
                })
                .collect();
            let (a, p, b) = (ui.widget(c[0]).rect, ui.widget(c[1]).rect, ui.widget(c[2]).rect);
            assert!((a.w - natural[0] - (b.w - natural[1])).abs() < 1e-3, "equal shares");
            assert!((a.w + p.w + b.w + 2.0 * 2.0 + 8.0 - 400.0).abs() < 1e-3);
            let i = ui.widget(inline).rect;
            if rtl {
                assert!(close(a.x + a.w, i.x + i.w - 4.0));
                assert_eq!(b.x, i.x + 4.0, "the last child on the left edge");
            } else {
                assert_eq!(a.x, i.x + 4.0);
                assert!(close(b.x + b.w, i.x + i.w - 4.0));
            }
        }
    }

    #[test]
    fn children_that_all_fill_end_on_both_edges() {
        let (ui, inline, c) = filled(300.0, theme(), false, &[("One", true), ("Two", true), ("Three", true)]);
        let i = ui.widget(inline).rect;
        let (first, last) = (ui.widget(c[0]).rect, ui.widget(c[2]).rect);
        assert_eq!(first.x, i.x + 4.0);
        assert!(close(last.x + last.w, i.x + i.w - 4.0));
    }

    #[test]
    fn no_spare_width_leaves_a_filling_child_at_its_own() {
        let (mut ui, inline, c) = filled(0.0, theme(), false, &[("Mute", false), ("Volume", true)]);
        let natural = ui.widget(c[1]).rect.w;
        assert_eq!(ui.widget(inline).rect.w, ui.widget(c[0]).rect.w + natural + 2.0 + 8.0, "no spare");
        // Narrower than its children: they keep their widths and overflow, never shrink.
        ui.widgets[inline].rect.w = 10.0;
        ui.place_inline(inline, 0.0, 0.0, [4.0, 0.0]);
        assert_eq!(ui.widget(c[1]).rect.w, natural);
        assert!(ui.widget(c[0]).rect.w > 0.0);
    }

    #[test]
    fn a_hidden_child_that_fills_leaves_its_share_to_the_others() {
        let (mut ui, inline, c) = filled(400.0, theme(), false, &[("A", true), ("B", true), ("C", false)]);
        ui.set_visible(c[0], false);
        ui.layout();
        let (b, last, i) = (ui.widget(c[1]).rect, ui.widget(c[2]).rect, ui.widget(inline).rect);
        assert_eq!(b.x, i.x + 4.0, "the hidden child keeps no room");
        assert!(close(last.x + last.w, i.x + i.w - 4.0));
        assert!((b.w + last.w + 2.0 + 8.0 - 400.0).abs() < 1e-3);
    }

    #[test]
    fn fractional_widths_still_end_on_the_inner_edge_in_both_directions() {
        let theme = Theme {
            gap: 2.7,
            margin: 0.0,
            ..Theme::default()
        };
        for width in [300.1, 333.3, 357.7, 400.3, 411.9, 512.7] {
            for rtl in [false, true] {
                let (ui, inline, c) = filled(
                    width,
                    theme.clone(),
                    rtl,
                    &[("Mute", false), ("Volume", true), ("Seek", true), ("Open the page", false)],
                );
                let i = ui.widget(inline).rect;
                let (first, last) = (ui.widget(c[0]).rect, ui.widget(c[3]).rect);
                if rtl {
                    assert!(close(first.x + first.w, i.x + i.w - 4.0), "{width}");
                    assert_eq!(last.x, i.x + 4.0, "{width}: the last child on the left edge");
                } else {
                    assert_eq!(first.x, i.x + 4.0, "{width}");
                    assert!(close(last.x + last.w, i.x + i.w - 4.0), "{width}");
                }
            }
        }
    }

    #[test]
    fn placing_again_without_a_measure_keeps_the_widths() {
        let mut ui = Ui::new("Strip", theme(), false);
        ui.min_width = 333.3;
        let scroll = ui.scroll(Ui::ROOT, 40.0);
        let inline = ui.inline(scroll, [4.0, 0.0]);
        let mute = ui.add(inline, Kind::Button("Mute".into()));
        let volume = ui.add(inline, Kind::Button("Volume".into()));
        ui.set_fill(volume, true);
        for n in 0..20 {
            ui.add(scroll, Kind::Label(format!("Line {n}")));
        }
        ui.layout();
        let before = (ui.widget(mute).rect.w, ui.widget(volume).rect.w);
        for _ in 0..50 {
            ui.scroll_by(scroll, 1.3);
        }
        assert_eq!((ui.widget(mute).rect.w, ui.widget(volume).rect.w), before);
    }

    #[test]
    fn a_row_with_more_children_than_slots_gives_each_a_cell() {
        let mut ui = Ui::new("Strip", theme(), false);
        ui.min_width = 304.0;
        let row = ui.add(Ui::ROOT, Kind::Row);
        ui.set_slots(row, 2);
        let cells: Vec<_> = ["A", "B", "C"].iter().map(|l| ui.add(row, Kind::Button((*l).into()))).collect();
        ui.layout();
        let widths: Vec<f32> = cells.iter().map(|&c| ui.widget(c).rect.w).collect();
        assert_eq!(widths, vec![100.0; 3]);
        let last = ui.widget(cells[2]).rect;
        assert_eq!(last.x + last.w, 304.0);
    }

    #[test]
    fn slots_wider_than_the_row_give_empty_cells_never_negative_ones() {
        let mut ui = Ui::new("Strip", theme(), false);
        let row = ui.add(Ui::ROOT, Kind::Row);
        ui.set_slots(row, 4);
        let only = ui.add(row, Kind::Button("A".into()));
        ui.layout();
        ui.widgets[row].rect.w = 5.0;
        ui.place(row, 0.0, 0.0);
        let r = ui.widget(only).rect;
        assert_eq!(r.w, 0.0);
        assert!(r.x.is_finite());
    }

    #[test]
    fn a_row_with_slots_in_right_to_left_starts_at_the_right_edge() {
        for width in [202.0, 333.3, 411.9] {
            let mut ui = Ui::new("Strip", Theme { gap: 2.7, margin: 0.0, ..Theme::default() }, true);
            ui.min_width = width;
            let row = ui.add(Ui::ROOT, Kind::Row);
            ui.set_slots(row, 3);
            let cells: Vec<_> = ["A", "B", "C"].iter().map(|l| ui.add(row, Kind::Button((*l).into()))).collect();
            ui.set_visible(cells[2], false);
            ui.layout();
            let (first, second) = (ui.widget(cells[0]).rect, ui.widget(cells[1]).rect);
            assert!(close(first.x + first.w, width), "the first cell at the right edge");
            assert!(second.x + second.w < first.x, "the second to its left");
            assert!(second.x > 0.0, "the third cell stays empty at the left");
            ui.set_visible(cells[2], true);
            ui.layout();
            assert_eq!(ui.widget(cells[2]).rect.x, 0.0, "{width}: the last cell on the left edge");
        }
    }
}
