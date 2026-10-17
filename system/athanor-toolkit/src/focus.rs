//! Keyboard focus. Tab and Shift+Tab walk the logical order, which is the tree's order and does
//! not depend on the reading direction; the arrows walk the visual order, from the laid-out
//! rectangles, so they follow the mirrored layout in right-to-left.

use crate::widget::{Key, Ui, WidgetId};

/// The focusable widgets in logical order: the tree, depth first.
pub fn chain(ui: &Ui) -> Vec<WidgetId> {
    let mut out = Vec::new();
    let mut stack = vec![Ui::ROOT];
    while let Some(id) = stack.pop() {
        if ui.widget(id).is_focusable() {
            out.push(id);
        }
        stack.extend(ui.widget(id).children.iter().rev());
    }
    out
}

/// The next focusable widget after the focused one, or before it when `backwards`, wrapping
/// around.
pub fn step(ui: &Ui, backwards: bool) -> Option<WidgetId> {
    let chain = chain(ui);
    let n = chain.len();
    if n == 0 {
        return None;
    }
    let at = ui
        .focus()
        .and_then(|f| chain.iter().position(|&id| id == f));
    let i = match (at, backwards) {
        (None, false) => 0,
        (None, true) => n - 1,
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
    };
    Some(chain[i])
}

/// The nearest focusable widget on screen in the arrow's direction, if any.
pub fn arrow(ui: &Ui, key: &Key) -> Option<WidgetId> {
    let chain = chain(ui);
    let Some(from) = ui.focus() else {
        return chain.first().copied();
    };
    let r = ui.widget(from).rect;
    let candidates = chain
        .into_iter()
        .filter(|&id| id != from)
        .map(|id| (id, ui.widget(id).rect));
    let dx = |c: &crate::widget::Rect| (c.center_x() - r.center_x()).abs();
    // Each candidate gets a key that is smaller the better it fits: the gap along the arrow's
    // axis, then the distance across it.
    let best = |keyed: Vec<(WidgetId, (f32, f32))>| {
        keyed
            .into_iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id)
    };
    let same_line = |c: &crate::widget::Rect| c.y < r.bottom() && c.bottom() > r.y;
    match key {
        Key::Left => best(
            candidates
                .filter(|(_, c)| same_line(c) && c.center_x() < r.center_x())
                .map(|(id, c)| (id, (r.center_x() - c.center_x(), 0.0)))
                .collect(),
        ),
        Key::Right => best(
            candidates
                .filter(|(_, c)| same_line(c) && c.center_x() > r.center_x())
                .map(|(id, c)| (id, (c.center_x() - r.center_x(), 0.0)))
                .collect(),
        ),
        Key::Down => best(
            candidates
                .filter(|(_, c)| c.y >= r.bottom())
                .map(|(id, c)| (id, ((c.y - r.bottom()).round(), dx(&c))))
                .collect(),
        ),
        Key::Up => best(
            candidates
                .filter(|(_, c)| c.bottom() <= r.y)
                .map(|(id, c)| (id, ((r.y - c.bottom()).round(), dx(&c))))
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use crate::widget::Kind;

    /// A row of three toggles, a field under it and a button under that.
    fn form(rtl: bool) -> (Ui, [WidgetId; 5]) {
        let mut ui = Ui::new("Form", Theme::default(), rtl);
        let row = ui.add(Ui::ROOT, Kind::Row);
        let toggle = |label: &str| Kind::Toggle {
            label: label.into(),
            active: label == "A",
        };
        let a = ui.add(row, toggle("A"));
        let b = ui.add(row, toggle("B"));
        let c = ui.add(row, toggle("C"));
        let field = ui.field(Ui::ROOT, "Name", "");
        let ok = ui.add(Ui::ROOT, Kind::Button("OK".into()));
        ui.layout();
        (ui, [a, b, c, field, ok])
    }

    fn walk(ui: &mut Ui, key: Key, times: usize) -> Vec<WidgetId> {
        (0..times)
            .map(|_| {
                ui.key(key.clone());
                ui.focus().unwrap_or(usize::MAX)
            })
            .collect()
    }

    #[test]
    fn tab_order_is_logical_in_ltr_and_rtl() {
        for rtl in [false, true] {
            let (mut ui, [a, b, c, field, ok]) = form(rtl);
            assert_eq!(
                walk(&mut ui, Key::Tab, 6),
                [a, b, c, field, ok, a],
                "rtl={rtl}"
            );
            assert_eq!(walk(&mut ui, Key::BackTab, 3), [ok, field, c], "rtl={rtl}");
        }
    }

    #[test]
    fn arrows_follow_the_visual_order_in_rtl() {
        let (mut ui, [a, b, c, field, ok]) = form(true);
        let x = |ui: &Ui, id| ui.widget(id).rect.x;
        assert!(
            x(&ui, a) > x(&ui, b) && x(&ui, b) > x(&ui, c),
            "the row is mirrored"
        );

        ui.set_focus(Some(b));
        assert_eq!(
            walk(&mut ui, Key::Right, 2),
            [a, a],
            "right is backwards in right-to-left"
        );
        assert_eq!(
            walk(&mut ui, Key::Left, 3),
            [b, c, c],
            "left is forwards and stops at the end"
        );
        assert_eq!(walk(&mut ui, Key::Down, 3), [field, ok, ok]);
        ui.set_focus(Some(field));
        ui.key(Key::Up);
        assert!([a, b, c].contains(&ui.focus().unwrap_or(usize::MAX)));

        let (mut ui, [_, b, c, ..]) = form(false);
        ui.set_focus(Some(b));
        assert_eq!(
            walk(&mut ui, Key::Right, 1),
            [c],
            "left-to-right is not mirrored"
        );
    }
}
