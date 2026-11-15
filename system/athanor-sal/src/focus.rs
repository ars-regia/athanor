//! Keyboard focus. Tab and Shift+Tab walk the logical order, which is the tree's order and does
//! not depend on the reading direction; the arrows walk the visual order, from the laid-out
//! rectangles, so they follow the mirrored layout in right-to-left.

use crate::widget::{Key, Kind, Ui, WidgetId};

/// The visible, sensitive, focusable widgets in logical order: the tree, depth first. A list is
/// one stop, on its focused row, else its selected row, else its first; its insensitive rows
/// are none. A group of radios is one stop too, as [`radio_stop`] says.
pub fn chain(ui: &Ui) -> Vec<WidgetId> {
    let mut out = Vec::new();
    let mut stack = vec![Ui::ROOT];
    while let Some(id) = stack.pop() {
        if !ui.shown(id) || !ui.widget(id).sensitive {
            continue;
        }
        if let Kind::List { .. } = ui.widget(id).kind {
            out.extend(list_stop(ui, id));
            continue;
        }
        let stop = radio_stop(ui, id).is_none_or(|stop| stop == id);
        if ui.widget(id).is_focusable() && ui.is_calendar_stop(id) && stop {
            out.push(id);
        }
        stack.extend(ui.widget(id).children.iter().rev());
    }
    out
}

/// The row of the list `id` that takes the focus: its focused row, else its selected row, else
/// its first; none of its hidden or insensitive rows, nor a heading, and none when `id` is not a
/// list.
pub fn list_stop(ui: &Ui, id: WidgetId) -> Option<WidgetId> {
    let Kind::List { selected, .. } = ui.widget(id).kind else {
        return None;
    };
    let mut rows = ui.visible_children(id);
    rows.retain(|&r| ui.widget(r).is_focusable());
    let stop = [ui.focus(), selected]
        .into_iter()
        .flatten()
        .find(|r| rows.contains(r));
    stop.or(rows.first().copied())
}

/// The radio of the group of `id` that takes the focus: its focused radio, else its chosen one,
/// else its first, as GTK's grouped check buttons and the WAI-ARIA radio group; none when `id` is
/// not a radio.
pub fn radio_stop(ui: &Ui, id: WidgetId) -> Option<WidgetId> {
    let group = radio_group(ui, id);
    let chosen = group.iter().copied().find(|&r| ui.is_active(r));
    let stop = [ui.focus(), chosen]
        .into_iter()
        .flatten()
        .find(|r| group.contains(r));
    stop.or(group.first().copied())
}

/// The group of the radio `id`: the shown, sensitive radios that share its parent, in the tree's
/// order; empty when `id` is not a radio, or is a radio menu item, a drop-down's say, which the
/// menu walks item by item.
pub fn radio_group(ui: &Ui, id: WidgetId) -> Vec<WidgetId> {
    let is_radio = |r: WidgetId| matches!(ui.widget(r).kind, Kind::Toggle { radio: true, .. });
    if !is_radio(id) {
        return Vec::new();
    }
    let Some(parent) = ui.widget(id).parent else {
        return vec![id];
    };
    if matches!(ui.widget(parent).kind, Kind::Menu) {
        return Vec::new();
    }
    let mut group = ui.visible_children(parent);
    group.retain(|&r| is_radio(r) && ui.widget(r).sensitive);
    group
}

/// The next focusable widget after the focused one, or before it when `backwards`, wrapping
/// around.
pub fn step(ui: &Ui, backwards: bool) -> Option<WidgetId> {
    wrap(ui, chain(ui), backwards)
}

/// The next item of `menu` after the focused one, or before it when `backwards`, wrapping
/// around: what the arrows do in a menu.
pub fn cycle(ui: &Ui, menu: WidgetId, backwards: bool) -> Option<WidgetId> {
    wrap(ui, items(ui, menu), backwards)
}

/// The items of `menu` that take the focus, in order: past its separators and insensitive items.
pub fn items(ui: &Ui, menu: WidgetId) -> Vec<WidgetId> {
    let items = chain(ui).into_iter();
    items.filter(|&id| ui.widget(id).parent == Some(menu)).collect()
}

fn wrap(ui: &Ui, chain: Vec<WidgetId>, backwards: bool) -> Option<WidgetId> {
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

/// Where the arrows find `id`, and start from it: a list's stop, wherever its row is scrolled, at
/// the part of its list in view.
fn place(ui: &Ui, id: WidgetId) -> crate::widget::Rect {
    match ui.widget(id).parent {
        Some(list) if matches!(ui.widget(list).kind, Kind::List { .. }) => {
            let r = ui.widget(list).rect;
            ui.clip(list).map_or(r, |c| r.intersect(c))
        }
        _ => ui.widget(id).rect,
    }
}

/// The nearest focusable widget on screen in the arrow's direction, if any.
pub fn arrow(ui: &Ui, key: &Key) -> Option<WidgetId> {
    let chain = chain(ui);
    let Some(from) = ui.focus() else {
        return chain.first().copied();
    };
    let r = place(ui, from);
    let candidates = chain
        .into_iter()
        .filter(|&id| id != from)
        .map(|id| (id, place(ui, id)));
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
                .map(|(id, c)| (id, ((r.y - c.y).round(), dx(&c))))
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use crate::widget::Event;

    /// A row of three toggles, a field under it and a button under that.
    fn form(rtl: bool) -> (Ui, [WidgetId; 5]) {
        let mut ui = Ui::new("Form", Theme::default(), rtl);
        let row = ui.add(Ui::ROOT, Kind::Row);
        let toggle = |label: &str| Kind::Toggle {
            label: label.into(),
            active: label == "A",
            radio: false,
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
                ui.focus().unwrap_or(WidgetId(u32::MAX, 0))
            })
            .collect()
    }

    /// A button, a group of three radios, the one at `chosen` chosen, and a field.
    fn choices(rtl: bool, chosen: Option<usize>) -> (Ui, WidgetId, [WidgetId; 3], WidgetId) {
        let mut ui = Ui::new("Choices", Theme::default(), rtl);
        let before = ui.add(Ui::ROOT, Kind::Button("Before".into()));
        let group = ui.add(Ui::ROOT, Kind::Column);
        let a = ui.radio(group, "A", chosen == Some(0));
        let b = ui.radio(group, "B", chosen == Some(1));
        let c = ui.radio(group, "C", chosen == Some(2));
        let field = ui.field(Ui::ROOT, "Name", "");
        ui.layout();
        (ui, before, [a, b, c], field)
    }

    #[test]
    fn a_radio_group_is_one_tab_stop_on_its_chosen_radio() {
        let (mut ui, before, [_, b, _], field) = choices(false, Some(1));
        assert_eq!(walk(&mut ui, Key::Tab, 4), [before, b, field, before]);
        assert_eq!(walk(&mut ui, Key::BackTab, 2), [field, b]);

        let (mut ui, before, [a, ..], field) = choices(false, None);
        assert_eq!(
            walk(&mut ui, Key::Tab, 3),
            [before, a, field],
            "none chosen: the first"
        );
    }

    #[test]
    fn the_arrows_choose_along_a_radio_group_and_wrap_as_read() {
        let chosen = |ui: &Ui, radios: [WidgetId; 3]| radios.map(|r| ui.is_active(r));
        let (mut ui, _, radios @ [a, b, c], field) = choices(false, Some(1));
        ui.set_focus(Some(b));
        assert_eq!(ui.key(Key::Down), Some(Event::Selected(c)));
        assert_eq!(chosen(&ui, radios), [false, false, true]);
        assert_eq!(walk(&mut ui, Key::Down, 2), [a, b], "wraps round");
        assert_eq!(walk(&mut ui, Key::Up, 2), [a, c], "wraps round");
        assert_eq!(walk(&mut ui, Key::Right, 1), [a]);
        assert_eq!(walk(&mut ui, Key::Left, 1), [c]);
        assert_eq!(
            chosen(&ui, radios),
            [false, false, true],
            "the focus is the choice"
        );
        assert_eq!(walk(&mut ui, Key::Tab, 1), [field], "Tab leaves the group");

        let (mut ui, _, radios @ [a, b, c], _) = choices(true, Some(1));
        ui.set_focus(Some(b));
        assert_eq!(
            walk(&mut ui, Key::Left, 1),
            [c],
            "left is forwards in right-to-left"
        );
        assert_eq!(walk(&mut ui, Key::Right, 2), [b, a]);
        assert_eq!(chosen(&ui, radios), [true, false, false]);
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
        assert!([a, b, c].contains(&ui.focus().unwrap_or(WidgetId(u32::MAX, 0))));

        let (mut ui, [_, b, c, ..]) = form(false);
        ui.set_focus(Some(b));
        assert_eq!(
            walk(&mut ui, Key::Right, 1),
            [c],
            "left-to-right is not mirrored"
        );
    }
}
