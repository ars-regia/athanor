//! A layout of large but finite lengths stays finite: every size and every offset, however
//! many containers add them up (#1153).

use athanor_sal::{Kind, Theme, Ui, WidgetId};

/// Lays `ui` out and checks that its size and the rect of each of `ids` are finite.
fn assert_finite(ui: &mut Ui, ids: &[WidgetId], case: &str) {
    let (w, h) = ui.layout();
    assert!(
        w.is_finite() && h.is_finite(),
        "{case}: the window is {w} by {h}"
    );
    for &id in ids {
        let r = ui.widget(id).rect;
        let all = [r.x, r.y, r.w, r.h];
        assert!(all.iter().all(|v| v.is_finite()), "{case}: {id:?} at {r:?}");
    }
}

/// An inline with `padding` holding a button, in `parent`: the inline, then the button.
fn padded(ui: &mut Ui, parent: WidgetId, padding: [f32; 2]) -> [WidgetId; 2] {
    let inline = ui.inline(parent, padding);
    [inline, ui.add(inline, Kind::Button("A".into()))]
}

#[test]
fn three_inlines_with_a_huge_padding_in_a_column() {
    let mut ui = Ui::new("Bounds", Theme::default(), false);
    let column = ui.add(Ui::ROOT, Kind::Column);
    let mut ids = vec![column];
    for _ in 0..3 {
        ids.extend(padded(&mut ui, column, [0.0, f32::MAX]));
    }
    assert_finite(&mut ui, &ids, "column");
}

#[test]
fn a_centre_box_of_three_inlines_with_a_huge_padding() {
    for rtl in [false, true] {
        let mut ui = Ui::new("Bounds", Theme::default(), rtl);
        let centre = ui.add(Ui::ROOT, Kind::CenterBox);
        let mut ids = vec![centre];
        for _ in 0..3 {
            ids.extend(padded(&mut ui, centre, [f32::MAX, 0.0]));
        }
        assert_finite(&mut ui, &ids, "centre box");
    }
}

#[test]
fn two_children_of_an_inline_at_the_largest_least_width() {
    let mut ui = Ui::new("Bounds", Theme::default(), false);
    let inline = ui.inline(Ui::ROOT, [0.0, 0.0]);
    let children = ["A", "B"].map(|t| ui.add(inline, Kind::Button(t.into())));
    for &c in &children {
        assert!(ui.set_min_width(c, f32::MAX));
    }
    let mut ids = vec![inline];
    ids.extend(children);
    assert_finite(&mut ui, &ids, "least width");
}

#[test]
fn an_inline_and_a_row_with_a_huge_gap() {
    let mut theme = Theme::default();
    theme.gap = f32::MAX;
    let mut ui = Ui::new("Bounds", theme, false);
    let inline = ui.inline(Ui::ROOT, [0.0, 0.0]);
    let row = ui.add(Ui::ROOT, Kind::Row);
    let mut ids = vec![inline, row];
    for parent in [inline, row] {
        for t in ["A", "B", "C"] {
            ids.push(ui.add(parent, Kind::Button(t.into())));
        }
    }
    assert_finite(&mut ui, &ids, "gap");
}

#[test]
fn a_scroll_of_the_largest_height_over_content_that_adds_up_past_it() {
    let mut ui = Ui::new("Bounds", Theme::default(), false);
    let scroll = ui.scroll(Ui::ROOT, f32::MAX);
    let column = ui.add(scroll, Kind::Column);
    let mut ids = vec![scroll, column];
    for _ in 0..3 {
        ids.extend(padded(&mut ui, column, [0.0, f32::MAX]));
    }
    assert_finite(&mut ui, &ids, "scroll");
    // To the end of its content, then laid out again from that offset.
    ui.pointer_axis(1.0, 1.0, f32::MAX);
    assert_finite(&mut ui, &ids, "scrolled");
}

#[test]
fn inlines_nested_with_a_huge_padding() {
    let mut ui = Ui::new("Bounds", Theme::default(), false);
    let mut ids = Vec::new();
    let mut parent = ui.add(Ui::ROOT, Kind::Column);
    for _ in 0..6 {
        let [inline, button] = padded(&mut ui, parent, [f32::MAX, f32::MAX]);
        ids.extend([inline, button]);
        parent = inline;
    }
    assert_finite(&mut ui, &ids, "nested");
}

#[test]
fn nested_islands_with_a_huge_or_unset_margin() {
    for margin in [[0.0, 0.0, 0.0, f32::MAX], [f32::MAX; 4], [f32::NAN; 4]] {
        let mut ui = Ui::new("Bounds", Theme::default(), false);
        let mut parent = Ui::ROOT;
        let mut ids = Vec::new();
        for _ in 0..3 {
            parent = ui.island(parent, true, margin);
            ids.push(parent);
        }
        ids.push(ui.add(parent, Kind::Button("A".into())));
        assert_finite(&mut ui, &ids, &format!("islands {margin:?}"));
    }
}
