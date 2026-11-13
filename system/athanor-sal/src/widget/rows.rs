//! A list's rows at run time, for a list that a field drives, as the launcher's results
//! (doc_launcher.md, LA5 and LA7): headings among the rows, a row chosen while the focus stays
//! in the field, the rows removed to fill the list anew, and the selected row as the field's
//! active descendant for assistive technologies.

use super::{Clip, Event, Kind, Ui, WidgetId};

impl Ui {
    /// Adds a heading showing `text` at the end of the list `list`, over the rows that follow
    /// it. See [`Kind::ListHeading`].
    ///
    /// # Panics
    ///
    /// When `list` names no widget of this `Ui`.
    pub fn list_heading(&mut self, list: WidgetId, text: &str) -> WidgetId {
        self.add(list, Kind::ListHeading(text.to_owned()))
    }

    /// Makes `row` its list's selected row without moving the focus, and scrolls it into view
    /// in the scrolls that hold it: what the arrows do in a field that drives a list. Returns
    /// [`Event::Selected`] when the selection changed. `None`, and nothing changes, for a widget
    /// that is not a list's row, and for a hidden or insensitive row. Call it after
    /// [`Ui::layout`], so that the scrolls know where the row is.
    ///
    /// # Panics
    ///
    /// When `row` names no widget of this `Ui`.
    pub fn select_row(&mut self, row: WidgetId) -> Option<Event> {
        let list = self.list_of(row)?;
        if !self.is_visible(row) || !self.is_sensitive(row) {
            return None;
        }
        let Kind::List { selected, .. } = &mut self.widgets[list].kind else {
            return None;
        };
        let changed = selected.replace(row) != Some(row);
        self.reveal(row);
        changed.then_some(Event::Selected(row))
    }

    /// Removes every child of `parent` and everything under them, as GTK's `remove_all`: to
    /// fill a list anew, say. The title bar stays. The focus on a removed widget moves as it
    /// would off a hidden one, a list loses the selected row removed from it, and a removed
    /// widget's slot is filled again by a later [`Ui::add`], so that a list filled anew at each
    /// key does not grow the tree. An id of a removed widget may therefore name a widget added
    /// later: forget the ids of the removed ones. Call [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn remove_children(&mut self, parent: WidgetId) {
        let bar = self.title_bar;
        let mut stack: Vec<WidgetId> = self.widgets[parent].children.clone();
        stack.retain(|&c| Some(c) != bar);
        let mut gone = Vec::with_capacity(stack.len());
        while let Some(id) = stack.pop() {
            stack.extend_from_slice(&self.widgets[id].children);
            gone.push(id);
        }
        if gone.is_empty() {
            return;
        }
        gone.sort_unstable();
        let is_gone = |id: WidgetId| gone.binary_search(&id).is_ok();
        // Hidden while the tree still holds them, so that the focus moves as off a hidden widget.
        for &id in &gone {
            self.widgets[id].visible = false;
        }
        self.keep_focus();
        self.widgets[parent].children.retain(|&c| !is_gone(c));
        if let Kind::List { selected, .. } = &mut self.widgets[parent].kind {
            if selected.is_some_and(is_gone) {
                *selected = None;
            }
        }
        if self.hover.is_some_and(is_gone) {
            self.hover = None;
        }
        if self.press.is_some_and(|press| is_gone(press.widget)) {
            self.press = None;
        }
        if self.offered.as_ref().is_some_and(|(id, _)| is_gone(*id)) {
            self.offered = None;
            self.clips.push(Clip::Unselect);
        }
        // Served after the event, a cut or a paste of a removed field would reach the widget
        // that takes its slot.
        self.clips.retain(|clip| match clip {
            Clip::Cut(id, _) | Clip::Paste(id) | Clip::PastePrimary(id) => !is_gone(*id),
            Clip::Copy(_) | Clip::Select(_) | Clip::Unselect => true,
        });
        self.descendants
            .retain(|&(owner, list)| !is_gone(owner) && !is_gone(list));
        // Each id removed is stale from now on, the driver's and the application's included.
        for &id in &gone {
            let next = self.widgets.free(id);
            self.free.insert(next);
        }
    }

    /// The id of a widget added under `parent` in a removed widget's slot, the lowest: any
    /// serves, since drawing and the pointer follow the order of the tree, not of the arena
    /// ([`Ui::ids`]). `None` when there is none, or when `parent` is past the arena, for
    /// [`Ui::add`] to panic on.
    pub(super) fn reuse(&mut self, parent: WidgetId) -> Option<WidgetId> {
        if parent.index() >= self.widgets.len() {
            return None;
        }
        self.free.pop_first()
    }

    /// Makes the selected row of `list` the active descendant of `owner` for assistive
    /// technologies, or ends it with `None`. While `owner` has the focus a screen reader follows
    /// the selected row as if it had it, as in a combo box whose entry keeps the focus while the
    /// arrows move through its list: a field that moves its results with [`Ui::select_row`].
    /// The keyboard focus stays on `owner`. Removing either ends it.
    ///
    /// # Panics
    ///
    /// When `owner`, or `list` when it is `Some`, names no widget of this `Ui`.
    pub fn set_active_descendant(&mut self, owner: WidgetId, list: Option<WidgetId>) {
        // Past the arena panics; a stale id is logged, and kept nowhere.
        let _ = &self.widgets[owner];
        if let Some(list) = list {
            let _ = &self.widgets[list];
        }
        if !self.contains(owner) || list.is_some_and(|list| !self.contains(list)) {
            return;
        }
        self.descendants.retain(|&(o, _)| o != owner);
        if let Some(list) = list {
            self.descendants.push((owner, list));
        }
    }

    /// The active descendant of `id`: the selected row of the list set for it, while that row
    /// is shown.
    pub(crate) fn active_descendant(&self, id: WidgetId) -> Option<WidgetId> {
        let &(_, list) = self.descendants.iter().find(|&&(owner, _)| owner == id)?;
        self.selected(list).filter(|&row| self.is_visible(row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::{node_id, row_label_id, tree_update};
    use crate::{Image, Key, Theme};
    use accesskit::{Action, Role};

    /// A field over a scroll that holds a list: a heading, then `rows` rows.
    fn results(rows: usize, rtl: bool) -> (Ui, [WidgetId; 4], Vec<WidgetId>) {
        let mut ui = Ui::new("Launcher", Theme::default(), rtl);
        let field = ui.field(Ui::ROOT, "Search", "");
        let scroll = ui.scroll(Ui::ROOT, 100.0);
        let list = ui.list(scroll, "Results");
        let heading = ui.list_heading(list, "Applications");
        let rows = (1..=rows)
            .map(|n| ui.list_row(list, &format!("Row {n}")))
            .collect();
        ui.layout();
        ui.set_focus(Some(field));
        (ui, [field, scroll, list, heading], rows)
    }

    #[test]
    fn a_heading_takes_neither_the_focus_nor_the_selection() {
        let (mut ui, [field, _, list, heading], rows) = results(3, false);
        assert!(!ui.widget(heading).is_focusable());
        assert_eq!(ui.select_row(heading), None);
        assert_eq!(crate::focus::list_stop(&ui, list), Some(rows[0]));
        // Tab reaches the first row, past the heading; Up leaves the list from it.
        ui.key(Key::Tab);
        assert_eq!(ui.focus(), Some(rows[0]));
        assert_eq!(ui.key(Key::Home), Some(Event::Selected(rows[0])));
        ui.key(Key::Up);
        assert_eq!(ui.focus(), Some(field), "Up passes over the heading");
        let r = ui.widget(heading).rect;
        assert_eq!(
            ui.hit(r.x + 1.0, r.y + 1.0),
            None,
            "the pointer passes over it too"
        );
    }

    #[test]
    fn a_heading_reads_as_gtks_row_that_is_not_selectable() {
        let (mut ui, [_, _, _, heading], rows) = results(2, false);
        ui.select_row(rows[0]);
        let update = tree_update(&ui, 1.0);
        let node = |id| {
            update
                .nodes
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, node)| node)
        };
        let Some(n) = node(node_id(heading)) else {
            panic!("the heading has no node");
        };
        assert_eq!(
            (n.role(), n.label(), n.is_selected()),
            (Role::ListBoxOption, None, None)
        );
        assert!(!n.supports_action(Action::Click) && !n.supports_action(Action::Focus));
        assert_eq!(n.children(), [row_label_id(heading)]);
        let label = node(row_label_id(heading)).map(|l| (l.role(), l.value()));
        assert_eq!(label, Some((Role::Label, Some("Applications"))));
    }

    #[test]
    fn a_row_lays_its_widgets_side_by_side_from_the_start_edge() {
        for rtl in [false, true] {
            let mut ui = Ui::new("Rows", Theme::default(), rtl);
            let list = ui.list(Ui::ROOT, "Results");
            let row = ui.list_row(list, "Files, Application");
            let Ok(icon) = Image::from_rgba8(1, 1, vec![0, 0, 0, 255]) else {
                panic!("a 1 by 1 image");
            };
            let image = ui.image(row, icon, 40.0, "");
            let lines = ui.add(row, Kind::Group(String::new()));
            let title = ui.add(lines, Kind::Label("Files".into()));
            ui.add(lines, Kind::Label("Browse the files".into()));
            ui.layout();
            let (r, i, l) = (
                ui.widget(row).rect,
                ui.widget(image).rect,
                ui.widget(lines).rect,
            );
            let pad = ui.theme.padding;
            assert_eq!((i.w, i.h), (40.0, 40.0));
            assert!(
                r.h >= 40.0 + pad,
                "the row is as tall as its tallest widget"
            );
            assert!(
                ui.widget(row).text.is_none(),
                "the row's own text is not drawn"
            );
            if rtl {
                assert_eq!(i.x + i.w, r.x + r.w - pad, "{rtl}");
                assert!(l.x + l.w <= i.x - ui.theme.gap + 0.01, "{rtl}");
            } else {
                assert_eq!(i.x, r.x + pad, "{rtl}");
                assert!((l.x - (i.x + i.w + ui.theme.gap)).abs() < 0.01, "{rtl}");
            }
            assert!(
                (i.y + i.h / 2.0 - (r.y + r.h / 2.0)).abs() < 0.01,
                "centred across"
            );
            // The pointer chooses the row through its widgets.
            assert_eq!(ui.hit(i.x + 1.0, i.y + 1.0), Some(row));
            let t = ui.widget(title).rect;
            assert_eq!(ui.click(t.x + 1.0, t.y + 1.0), Some(Event::Selected(row)));

            // It reads as one option named by its text, its widgets inside it in place of a label.
            let update = tree_update(&ui, 1.0);
            let Some((_, n)) = update.nodes.iter().find(|(id, _)| *id == node_id(row)) else {
                panic!("the row has no node");
            };
            assert_eq!(
                (n.role(), n.label()),
                (Role::ListBoxOption, Some("Files, Application"))
            );
            assert_eq!(n.children(), [node_id(image), node_id(lines)]);
            assert!(!update.nodes.iter().any(|(id, _)| *id == row_label_id(row)));
        }
    }

    #[test]
    fn select_row_leaves_the_focus_in_the_field_and_scrolls_the_row_into_view() {
        let (mut ui, [field, scroll, list, _], rows) = results(50, false);
        let last = rows[49];
        assert_eq!(ui.select_row(last), Some(Event::Selected(last)));
        assert_eq!((ui.focus(), ui.selected(list)), (Some(field), Some(last)));
        let (view, r) = (ui.widget(scroll).rect, ui.widget(last).rect);
        assert!(
            r.y >= view.y && r.bottom() <= view.bottom(),
            "scrolled into view"
        );
        assert_eq!(ui.select_row(last), None, "already selected");
        assert_eq!(ui.select_row(field), None, "not a row");
        ui.set_sensitive(rows[3], false);
        assert_eq!(ui.select_row(rows[3]), None, "insensitive");
        assert_eq!(ui.selected(list), Some(last));
    }

    #[test]
    fn the_selected_row_is_the_fields_active_descendant() {
        let (mut ui, [field, _, list, _], rows) = results(3, false);
        let descendant = |ui: &Ui| {
            let update = tree_update(ui, 1.0);
            let found = update.nodes.iter().find(|(id, _)| *id == node_id(field));
            (found.and_then(|(_, n)| n.active_descendant()), update.focus)
        };
        ui.set_active_descendant(field, Some(list));
        assert_eq!(
            descendant(&ui),
            (None, node_id(field)),
            "no row selected yet"
        );
        ui.select_row(rows[1]);
        assert_eq!(descendant(&ui), (Some(node_id(rows[1])), node_id(field)));
        ui.select_row(rows[2]);
        assert_eq!(
            descendant(&ui).0,
            Some(node_id(rows[2])),
            "it follows the selection"
        );
        ui.set_active_descendant(field, None);
        assert_eq!(descendant(&ui).0, None);
        ui.set_active_descendant(field, Some(list));
        ui.remove_children(list);
        assert_eq!(descendant(&ui).0, None, "no rows, no descendant");
    }

    #[test]
    fn rows_removed_and_added_again_reuse_their_slots() {
        let (mut ui, [field, _, list, _], rows) = results(5, false);
        ui.set_active_descendant(field, Some(list));
        ui.select_row(rows[2]);
        let size = ui.len();
        for round in 0..20 {
            ui.remove_children(list);
            assert_eq!(ui.selected(list), None, "{round}");
            assert!(ui.widget(list).children.is_empty());
            ui.list_heading(list, "Files");
            let row = ui.list_row(list, "File");
            let Ok(icon) = Image::from_rgba8(1, 1, vec![0, 0, 0, 255]) else {
                panic!("a 1 by 1 image");
            };
            ui.image(row, icon, 28.0, "");
            ui.list_row(list, "Other");
            ui.layout();
            ui.select_row(row);
            assert_eq!(ui.len(), size, "the tree does not grow");
            // The tree draws and publishes with the slots reused.
            assert!(crate::a11y::missing(&ui, &tree_update(&ui, 1.0)).is_empty());
        }
        assert_eq!(ui.focus(), Some(field), "the field kept the focus");
    }

    #[test]
    fn the_focus_on_a_removed_row_moves_as_off_a_hidden_one() {
        let (mut ui, [_, _, list, _], rows) = results(3, false);
        let after = ui.add(Ui::ROOT, Kind::Button("After".into()));
        ui.layout();
        ui.set_focus(Some(rows[1]));
        ui.remove_children(list);
        assert_eq!(ui.focus(), Some(after), "the next stop after the list");
        assert_eq!(ui.hit(1.0, ui.widget(list).rect.y + 1.0), None);
    }

    #[test]
    fn a_scroll_past_the_rows_that_replace_its_content_comes_back_within_them() {
        let (mut ui, [_, scroll, list, _], rows) = results(60, false);
        let offset = |ui: &Ui| match ui.widget(scroll).kind {
            Kind::Scroll { offset, .. } => offset,
            _ => f32::NAN,
        };
        ui.select_row(rows[49]);
        assert!(offset(&ui) > 0.0, "the row scrolled into view");
        ui.remove_children(list);
        let row = ui.list_row(list, "File");
        ui.list_row(list, "Other");
        // Drawing lays out first, and the layout clamps the offset to the content left.
        ui.layout();
        assert_eq!(offset(&ui), 0.0, "two rows fit the viewport");
        let (r, view) = (ui.widget(row).rect, ui.widget(scroll).rect);
        assert!(r.y >= view.y && r.bottom() <= view.bottom(), "the first row shows");
        let update = tree_update(&ui, 1.0);
        let Some((_, node)) = update.nodes.iter().find(|(n, _)| *n == node_id(scroll)) else {
            panic!("the scroll has no node");
        };
        assert_eq!((node.scroll_y(), node.scroll_y_max()), (Some(0.0), Some(0.0)));
    }

    #[test]
    fn rows_refilled_past_a_later_sibling_draw_and_take_the_pointer_in_the_order_of_the_tree() {
        let (mut ui, [field, scroll, list, _], _) = results(2, false);
        let after = ui.add(Ui::ROOT, Kind::Button("After".into()));
        ui.remove_children(list);
        // Four rows in the three slots freed, and the fourth past the button in the arena.
        let rows: Vec<_> = (1..=4)
            .map(|n| ui.list_row(list, &format!("Row {n}")))
            .collect();
        assert!(rows[3] > after, "a slot past the button");
        ui.layout();
        let order: Vec<_> = ui.ids().collect();
        let mut tree = vec![Ui::ROOT, field, scroll, list];
        tree.extend(&rows);
        tree.push(after);
        assert_eq!(order, tree, "the order of the tree, without the freed slots");
        // The pointer finds the first row and the button below the scroll where they are drawn.
        for widget in [rows[0], after] {
            let r = ui.widget(widget).rect;
            assert_eq!(ui.hit(r.x + 1.0, r.y + 1.0), Some(widget));
        }
    }

    #[test]
    fn a_removed_field_leaves_no_cut_or_paste_for_the_widget_in_its_slot() {
        let mut ui = Ui::new("Form", Theme::default(), false);
        let form = ui.add(Ui::ROOT, Kind::Column);
        let field = ui.field(form, "Name", "abc");
        ui.layout();
        ui.set_focus(Some(field));
        for key in [Key::SelectAll, Key::Cut, Key::Paste] {
            ui.key(key);
        }
        ui.remove_children(form);
        let next = ui.field(form, "Place", "");
        assert_eq!(next.index(), field.index(), "the slot is reused");
        assert_ne!(next, field, "under another id");
        let clips = ui.take_clips();
        assert!(
            !clips.iter().any(|c| matches!(
                c,
                Clip::Cut(..) | Clip::Paste(_) | Clip::PastePrimary(_)
            )),
            "{clips:?}"
        );
        assert!(clips.iter().any(|c| matches!(c, Clip::Unselect)), "{clips:?}");
    }

    #[test]
    fn nothing_is_added_under_a_removed_widget_and_its_id_never_reaches_the_next() {
        let (mut ui, [_, _, list, heading], _) = results(2, false);
        let size = ui.len();
        ui.remove_children(list);
        // An icon loaded late for the heading, whose slot is the lowest free one.
        let Ok(image) = Image::from_rgba8(1, 1, vec![0, 0, 0, 255]) else {
            panic!("a 1 by 1 image");
        };
        let icon = ui.image(heading, image, 28.0, "");
        assert_eq!(icon, heading, "refused, with the stale id");
        assert!(ui.widget(heading).children.is_empty());
        assert_eq!(ui.widget(heading).parent, None, "never its own parent");
        assert!(!ui.is_visible(heading) && ui.is_sensitive(heading));
        ui.layout();
        // A new row takes the heading's slot, under another id.
        let row = ui.list_row(list, "File");
        assert_eq!(row.index(), heading.index());
        assert_ne!(row, icon);
        assert_eq!(ui.len(), size, "the tree does not grow");
        // What the late loader does with its id reaches nothing.
        ui.set_visible(icon, false);
        ui.set_sensitive(icon, false);
        assert_eq!(ui.add(icon, Kind::Label("Late".into())), icon);
        assert_eq!(ui.select_row(icon), None);
        assert_eq!(ui.widget(row).parent, Some(list));
        assert!(ui.widget(row).children.is_empty());
        assert!(ui.is_visible(row) && ui.is_sensitive(row));
        assert_eq!(ui.select_row(row), Some(Event::Selected(row)));
    }

    #[test]
    fn a_paste_served_after_its_field_was_removed_mid_turn_reaches_no_new_field() {
        let mut ui = Ui::new("Form", Theme::default(), false);
        let form = ui.add(Ui::ROOT, Kind::Column);
        let name = ui.field(form, "Name", "");
        let place = ui.field(form, "Place", "");
        ui.layout();
        // The driver serves its snapshot in order, and the application answers the first
        // paste's edit by building the form again.
        let mut rebuilt = Vec::new();
        for clip in [Clip::Paste(name), Clip::Paste(place)] {
            let Clip::Paste(id) = clip else {
                continue;
            };
            if ui.paste(id, "text") == Some(Event::Edited(name)) {
                ui.remove_children(form);
                rebuilt = vec![ui.field(form, "Name", ""), ui.field(form, "Place", "")];
                ui.layout();
            }
        }
        let slots: Vec<usize> = rebuilt.iter().map(|id| id.index()).collect();
        assert_eq!(slots, [name.index(), place.index()], "the slots are reused");
        for id in rebuilt {
            assert_eq!(ui.field_text(id).as_deref(), Some(""), "{id:?}");
        }
    }

    #[test]
    fn the_title_bar_stays_when_the_roots_children_go() {
        let mut ui = Ui::new("Window", Theme::default(), false);
        ui.add_title_bar();
        ui.add(Ui::ROOT, Kind::Label("Body".into()));
        let bar = ui.widget(Ui::ROOT).children[0];
        ui.remove_children(Ui::ROOT);
        assert_eq!(ui.widget(Ui::ROOT).children, [bar]);
    }
}
