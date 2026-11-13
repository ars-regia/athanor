//! A drop-down, as GTK's `DropDown`: a raised button that shows the chosen one of its items, with
//! an arrow at its end edge, and opens them in a menu. AT-SPI reads it as GTK's: a combo box named
//! by its chosen item, with its own name as its description, and the menu's items as radio menu
//! items, the chosen one checked. The driver opens the menu and does the choice: the application learns
//! of it through [`crate::Event::Changed`].

use crate::widget::{Event, Kind, Rect, Ui, WidgetId};

/// A drop-down: see [`Ui::drop_down`]. Its fields are closed to applications so that the chosen
/// index always names one of its items, or is 0 without items: only [`Ui::drop_down`] and
/// [`Ui::set_drop_down`] make or change one.
pub struct DropDown {
    /// What it is for: its description for assistive technologies, which name it by its chosen
    /// item.
    pub(crate) name: String,
    /// What it offers, in order.
    pub(crate) items: Vec<String>,
    /// The index of the chosen item; 0 when there is none.
    pub(crate) selected: usize,
}

impl DropDown {
    /// What it is for: its description for assistive technologies, which name it by its chosen
    /// item.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What it offers, in order.
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// The index of the chosen item; 0 when there is none.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The chosen item's text, empty when there is none.
    pub fn chosen_text(&self) -> &str {
        self.items.get(self.selected).map_or("", String::as_str)
    }
}

/// The side of the square a drop-down's arrow is drawn in, against the theme's font size.
pub(crate) const ARROW: f32 = 0.75;

/// Where a drop-down drawn in `r` puts its text, `text_w` wide, and its arrow, `arrow` wide:
/// the left edge of each. The text sits at the start edge and the arrow at the end one, both
/// `padding` in, so in right to left the text is at the right whatever the drop-down's width.
pub(crate) fn spots(r: Rect, text_w: f32, arrow: f32, padding: f32, rtl: bool) -> (f32, f32) {
    let (left, right) = (r.x + padding, r.x + r.w - padding);
    if rtl {
        (right - text_w, left)
    } else {
        (left, right - arrow)
    }
}

impl Ui {
    /// Adds a drop-down for `name`, offering `items` with the one at `selected` chosen, or the
    /// last one when `selected` is past the end. Activating it opens the items in a menu.
    /// Assistive technologies read the chosen item as its name, none without items, and `name`
    /// as its description, as GTK's `DropDown` with the name the panels give it.
    ///
    /// # Panics
    ///
    /// When `parent` names no widget of this `Ui`.
    pub fn drop_down(
        &mut self,
        parent: WidgetId,
        name: &str,
        items: &[&str],
        selected: usize,
    ) -> WidgetId {
        let drop = DropDown {
            name: name.to_owned(),
            items: items.iter().map(|&item| item.to_owned()).collect(),
            selected: selected.min(items.len().saturating_sub(1)),
        };
        let id = self.add(parent, Kind::DropDown(drop));
        self.set_has_popup(id, true);
        id
    }

    /// Replaces the items of drop-down `id` with `items` and chooses the one at `selected`, as
    /// [`Ui::drop_down`] does; a change closes its menu if it is open. Whether anything
    /// changed: `false` for a widget that is not a drop-down. Raises no event; call
    /// [`Ui::layout`] afterwards.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn set_drop_down(&mut self, id: WidgetId, items: &[&str], selected: usize) -> bool {
        let Kind::DropDown(drop) = &mut self.widgets[id].kind else {
            return false;
        };
        let selected = selected.min(items.len().saturating_sub(1));
        if drop.selected == selected && drop.items.iter().eq(items.iter().copied()) {
            return false;
        }
        drop.items = items.iter().map(|&item| item.to_owned()).collect();
        drop.selected = selected;
        self.stale_popup |= self.is_expanded(id);
        true
    }

    /// The index of the item chosen in drop-down `id`: `None` for a drop-down without items and
    /// for a widget that is not a drop-down.
    ///
    /// # Panics
    ///
    /// When `id` names no widget of this `Ui`.
    pub fn chosen(&self, id: WidgetId) -> Option<usize> {
        match &self.widgets[id].kind {
            Kind::DropDown(drop) if !drop.items.is_empty() => Some(drop.selected),
            _ => None,
        }
    }

    /// The menu drop-down `id` opens: its items as radios, the chosen one checked, each with
    /// the index it chooses. A menu taller than `max_height`, the height of the output, holds
    /// its items in a scroll that fits it. The scroll always shows one item, and that floor
    /// wins over the bound: on an output about one item high, the menu is one item and its
    /// margins high, taller than `max_height`, which the compositor's slide then keeps on the
    /// output as far as it can. `None` for a drop-down without items, or a widget that is not
    /// one.
    pub(crate) fn drop_down_menu(
        &self,
        id: WidgetId,
        max_height: Option<f32>,
    ) -> Option<(Ui, Vec<(WidgetId, usize)>)> {
        let Kind::DropDown(drop) = &self.widgets.get(id.index())?.kind else {
            return None;
        };
        if drop.items.is_empty() {
            return None;
        }
        let build = |viewport: Option<f32>| {
            let mut menu = Ui::new(&self.title, self.theme.clone(), self.rtl);
            let parent = viewport.map_or(Ui::ROOT, |height| menu.scroll(Ui::ROOT, height));
            let list = menu.add(parent, Kind::Menu);
            let items: Vec<_> = drop
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| (menu.radio(list, item, index == drop.selected), index))
                .collect();
            (menu, items)
        };
        let (mut menu, items) = build(None);
        let (_, height) = menu.layout();
        // The root's margin stays around the scroll, which shows one item at least however short
        // the bound, even past it; a bound of 0 or less, or not finite, is none.
        let margins = 2.0 * self.theme.margin;
        let item = items.first().map_or(0.0, |&(first, _)| menu.widget(first).rect.h);
        match max_height.filter(|&max| max.is_finite() && max > 0.0 && height > max) {
            Some(max) => Some(build(Some((max - margins).max(item)))),
            None => Some((menu, items)),
        }
    }

    /// Chooses the item at `index` of drop-down `id`, as its menu does: [`Event::Changed`] when
    /// the choice changed, `None` when it did not or `index` is past the end.
    pub(crate) fn choose(&mut self, id: WidgetId, index: usize) -> Option<Event> {
        if id.index() >= self.widgets.len() {
            return None;
        }
        let Kind::DropDown(drop) = &mut self.widgets[id].kind else {
            return None;
        };
        if index >= drop.items.len() || index == drop.selected {
            return None;
        }
        drop.selected = index;
        Some(Event::Changed(id))
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;
    use crate::widget::{Event, Kind, Ui};

    #[test]
    fn a_drop_down_offers_its_items_as_radios_and_reports_a_new_choice() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let players = ui.drop_down(Ui::ROOT, "Media player", &["Firefox", "Spotify"], 1);
        assert!(ui.widget(players).is_focusable());
        assert_eq!(ui.activate(players), Some(Event::Clicked(players)));
        assert_eq!(ui.chosen(players), Some(1));
        let Some((menu, items)) = ui.drop_down_menu(players, None) else {
            unreachable!("a drop-down with items opens a menu");
        };
        let checked: Vec<bool> = items
            .iter()
            .map(|&(item, _)| menu.is_active(item))
            .collect();
        assert_eq!(checked, [false, true]);
        assert!(items
            .iter()
            .all(|&(item, _)| matches!(menu.widget(item).kind, Kind::Toggle { radio: true, .. })));
        assert_eq!(ui.choose(players, 1), None, "already chosen");
        assert_eq!(ui.choose(players, 2), None, "past the end");
        assert_eq!(ui.choose(players, 0), Some(Event::Changed(players)));
        assert_eq!(ui.chosen(players), Some(0));
        ui.layout();
        assert!(ui.widget(players).rect.w >= ui.theme.button_min);
    }

    #[test]
    fn assistive_technologies_read_a_combo_box_and_radio_menu_items() {
        use accesskit::{HasPopup, Role, Toggled};
        let mut ui = Ui::new("t", Theme::default(), false);
        let players = ui.drop_down(Ui::ROOT, "Media player", &["Firefox", "Spotify"], 1);
        let node = crate::a11y::node(&ui, players, 1.0);
        assert_eq!(node.role(), Role::ComboBox);
        assert_eq!(
            (node.label(), node.description(), node.value()),
            (Some("Spotify"), Some("Media player"), Some("Spotify"))
        );
        let empty = ui.drop_down(Ui::ROOT, "Media player", &[], 0);
        let node_empty = crate::a11y::node(&ui, empty, 1.0);
        assert_eq!(
            (node_empty.label(), node_empty.description()),
            (None, Some("Media player")),
            "no item: no name, as GTK's, and the description"
        );
        assert_eq!(node.has_popup(), Some(HasPopup::Menu));
        let Some((menu, items)) = ui.drop_down_menu(players, None) else {
            unreachable!("a drop-down with items opens a menu");
        };
        let read: Vec<_> = items
            .iter()
            .map(|&(item, _)| {
                let node = crate::a11y::node(&menu, item, 1.0);
                (node.role(), node.toggled())
            })
            .collect();
        assert_eq!(
            read,
            [
                (Role::MenuItemRadio, Some(Toggled::False)),
                (Role::MenuItemRadio, Some(Toggled::True))
            ]
        );
    }

    #[test]
    fn the_text_sits_at_the_start_edge_and_the_arrow_at_the_end_one() {
        use crate::widget::Rect;
        let r = Rect {
            x: 10.0,
            y: 0.0,
            w: 200.0,
            h: 30.0,
        };
        // A drop-down wider than its 50-pixel text and its 12-pixel arrow.
        assert_eq!(super::spots(r, 50.0, 12.0, 8.0, false), (18.0, 190.0));
        assert_eq!(
            super::spots(r, 50.0, 12.0, 8.0, true),
            (152.0, 18.0),
            "the text at the right, the arrow at the left"
        );
    }

    #[test]
    fn a_menu_taller_than_its_output_scrolls_and_shows_the_chosen_item() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let names: Vec<String> = (1..=40).map(|n| format!("Player {n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let players = ui.drop_down(Ui::ROOT, "Media player", &names, 39);
        let Some((mut whole, _)) = ui.drop_down_menu(players, Some(10_000.0)) else {
            unreachable!("a drop-down with items opens a menu");
        };
        let (_, tall) = whole.layout();
        assert!(tall > 300.0, "forty items do not fit in 300 pixels");
        let Some((mut menu, items)) = ui.drop_down_menu(players, Some(300.0)) else {
            unreachable!("a drop-down with items opens a menu");
        };
        let (_, high) = menu.layout();
        assert!(high <= 300.0, "{high} fits the output");
        assert_eq!(items.len(), 40);
        let (last, _) = items[39];
        menu.set_focus(Some(last));
        menu.layout();
        let scroll = menu.widget(Ui::ROOT).children[0];
        assert!(matches!(menu.widget(scroll).kind, Kind::Scroll { .. }));
        assert_eq!(
            menu.clip(last).map(|view| view.contains(
                menu.widget(last).rect.x + 1.0,
                menu.widget(last).rect.y + 1.0
            )),
            Some(true),
            "the focused item is scrolled into view"
        );
    }

    #[test]
    fn a_bound_of_nothing_is_none_and_a_tiny_one_still_shows_an_item() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let names: Vec<String> = (1..=40).map(|n| format!("Player {n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let players = ui.drop_down(Ui::ROOT, "Media player", &names, 0);
        for none in [0.0, -5.0, f32::NAN] {
            let Some((menu, _)) = ui.drop_down_menu(players, Some(none)) else {
                unreachable!("a drop-down with items opens a menu");
            };
            let first = menu.widget(Ui::ROOT).children[0];
            assert!(matches!(menu.widget(first).kind, Kind::Menu), "{none}: no scroll");
        }
        let Some((mut menu, items)) = ui.drop_down_menu(players, Some(1.0)) else {
            unreachable!("a drop-down with items opens a menu");
        };
        menu.layout();
        let scroll = menu.widget(Ui::ROOT).children[0];
        let (view, item) = (menu.widget(scroll).rect.h, menu.widget(items[0].0).rect.h);
        assert!(item > 0.0);
        assert!(view >= item, "a viewport of {view} shows an item {item} high");
        let margins = 2.0 * menu.theme.margin;
        let (_, high) = menu.layout();
        assert!(high >= item + margins, "{high}: the one-item floor wins over the bound of 1");
    }

    #[test]
    fn a_drop_down_takes_new_items_and_keeps_its_choice_in_range() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let players = ui.drop_down(Ui::ROOT, "Media player", &[], 3);
        assert_eq!(ui.chosen(players), None);
        assert!(ui.drop_down_menu(players, None).is_none(), "nothing to offer");
        assert!(ui.set_drop_down(players, &["Firefox", "Spotify"], 7));
        assert_eq!(ui.chosen(players), Some(1), "the last one");
        assert!(!ui.set_drop_down(players, &["Firefox", "Spotify"], 1));
        let label = ui.add(Ui::ROOT, Kind::Label("Media".into()));
        assert!(!ui.set_drop_down(label, &["Firefox"], 0));
        assert_eq!(ui.chosen(label), None);
        assert!(!ui.stale_popup, "no menu is open");
        ui.set_expanded(players, true);
        assert!(!ui.set_drop_down(players, &["Firefox", "Spotify"], 1));
        assert!(!ui.stale_popup, "the same items");
        assert!(ui.set_drop_down(players, &["Spotify"], 0));
        assert!(ui.stale_popup, "the open menu shows the old items");
    }
}
