//! A drop-down, as GTK's `DropDown`: a raised button that shows the chosen one of its items, with
//! an arrow at its end edge, and opens them in a menu. AT-SPI reads it as a combo box named for
//! assistive technologies, its chosen item as its value, and the menu's items as radio menu items,
//! the chosen one checked. The driver opens the menu and does the choice: the application learns
//! of it through [`crate::Event::Changed`].

use crate::widget::{Event, Kind, Ui, WidgetId};

/// A drop-down: see [`Ui::drop_down`].
pub struct DropDown {
    /// Its name for assistive technologies.
    pub name: String,
    /// What it offers, in order.
    pub items: Vec<String>,
    /// The index of the chosen item; 0 when there is none.
    pub selected: usize,
}

impl DropDown {
    /// The chosen item's text, empty when there is none.
    pub fn chosen_text(&self) -> &str {
        self.items.get(self.selected).map_or("", String::as_str)
    }
}

/// The side of the square a drop-down's arrow is drawn in, against the theme's font size.
pub(crate) const ARROW: f32 = 0.75;

impl Ui {
    /// Adds a drop-down named `name`, offering `items` with the one at `selected` chosen, or the
    /// last one when `selected` is past the end. Activating it opens the items in a menu.
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
    /// the index it chooses. `None` for a drop-down without items, or a widget that is not one.
    pub(crate) fn drop_down_menu(&self, id: WidgetId) -> Option<(Ui, Vec<(WidgetId, usize)>)> {
        let Kind::DropDown(drop) = &self.widgets.get(id.index())?.kind else {
            return None;
        };
        if drop.items.is_empty() {
            return None;
        }
        let mut menu = Ui::new(&self.title, self.theme.clone(), self.rtl);
        let list = menu.add(Ui::ROOT, Kind::Menu);
        let items = drop
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| (menu.radio(list, item, index == drop.selected), index))
            .collect();
        Some((menu, items))
    }

    /// Chooses the item at `index` of drop-down `id`, as its menu does: [`Event::Changed`] when
    /// the choice changed, `None` when it did not or `index` is past the end.
    pub(crate) fn choose(&mut self, id: WidgetId, index: usize) -> Option<Event> {
        let Kind::DropDown(drop) = &mut self.widgets.get_mut(id.index())?.kind else {
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
        let Some((menu, items)) = ui.drop_down_menu(players) else {
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
            (node.label(), node.value()),
            (Some("Media player"), Some("Spotify"))
        );
        assert_eq!(node.has_popup(), Some(HasPopup::Menu));
        let Some((menu, items)) = ui.drop_down_menu(players) else {
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
    fn a_drop_down_takes_new_items_and_keeps_its_choice_in_range() {
        let mut ui = Ui::new("t", Theme::default(), false);
        let players = ui.drop_down(Ui::ROOT, "Media player", &[], 3);
        assert_eq!(ui.chosen(players), None);
        assert!(ui.drop_down_menu(players).is_none(), "nothing to offer");
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
