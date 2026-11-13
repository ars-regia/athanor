//! Accessibility on AccessKit: every widget is one node, and assistive technologies act through
//! the same paths as the keyboard and the pointer.

use crate::focus::list_stop;
use crate::widget::{Event, Kind, RootRole, Ui, WidgetId};
use accesskit::{
    Action, ActionData, ActionRequest, HasPopup, Live, Node, NodeId, Orientation, Rect, Role,
    TextPosition, TextSelection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
use std::ops::Range;
use std::sync::Mutex;

/// The bits of a node id below which its widget's slot lies.
const SLOT_BITS: u32 = 24;
/// The bits of a node id below which its widget's slot and generation lie: the line of a text
/// run lies above, and the announcement's bit 63 above that.
const WIDGET_BITS: u32 = 40;

/// The AccessKit node id of a widget: its slot, and the low 16 bits of its generation above,
/// so that a node an assistive technology cached for a widget since removed names no widget,
/// also once another widget takes the slot: the slot is retired before those bits come back
/// (`Widgets::free`).
pub fn node_id(id: WidgetId) -> NodeId {
    NodeId(u64::from(id.0) | u64::from(id.1 & 0xffff) << SLOT_BITS)
}

/// The node of line `line` of a field's text, beside the widgets' ids: a single-line field has
/// one line, a text area one per line as it is seen.
// ponytail: 23 bits of line, past 8 million lines a run would reach the announcement's bit.
pub fn text_run_id(id: WidgetId, line: usize) -> NodeId {
    NodeId(node_id(id).0 | (line as u64 + 1) << WIDGET_BITS)
}

/// The widget `node` names, while it is the one the node was published for: `None` for a node
/// of a text run or of the announcement, past the arena, or once the widget was removed,
/// whatever took its slot since.
fn widget_of(ui: &Ui, node: NodeId) -> Option<WidgetId> {
    if node.0 >> WIDGET_BITS != 0 {
        return None;
    }
    let index = (node.0 & ((1 << SLOT_BITS) - 1)) as u32;
    let generation = ui.widgets.generation(index as usize)?;
    let id = WidgetId(index, generation);
    (node_id(id) == node).then_some(id)
}

/// The node of a list row's label, beside the widgets' ids: a row has no text runs, so it takes
/// the id of a first one.
pub fn row_label_id(id: WidgetId) -> NodeId {
    text_run_id(id, 0)
}

/// The node of the root's announcement ([`Ui::announce`]), beside the widgets' ids and their
/// text runs, which never reach it.
const ANNOUNCEMENT: NodeId = NodeId(u64::MAX);

/// The node of widget `id`, with its bounds in physical pixels at `scale`. The match is
/// exhaustive, so a new kind of widget cannot be added without its node.
pub fn node(ui: &Ui, id: WidgetId, scale: f64) -> Node {
    let widget = ui.widget(id);
    let mut node = match &widget.kind {
        Kind::Column if id == Ui::ROOT => {
            let mut node = Node::new(match ui.root_role {
                RootRole::Window => Role::Window,
                RootRole::Status => Role::Status,
                RootRole::AlertDialog => Role::AlertDialog,
                RootRole::Dialog => Role::Dialog,
            });
            node.set_label(ui.title.as_str());
            if ui.modal {
                node.set_modal();
            }
            node
        }
        Kind::Column | Kind::Row | Kind::TitleBar | Kind::Island { .. } => {
            Node::new(Role::GenericContainer)
        }
        Kind::Group(name) => {
            // A group stacks its children as a column, and says so as GTK's vertical box does.
            let mut node = Node::new(Role::Group);
            node.set_label(name.as_str());
            node.set_orientation(Orientation::Vertical);
            node
        }
        Kind::Label(text) => {
            let mut node = Node::new(Role::Label);
            node.set_value(text.as_str());
            node
        }
        Kind::WindowButton(button) => {
            let mut node = Node::new(Role::Button);
            node.set_label(button.name());
            node.add_action(Action::Click);
            node
        }
        Kind::Button(label) | Kind::IconButton { label, .. } => {
            let mut node = Node::new(Role::Button);
            node.set_label(label.as_str());
            node.add_action(Action::Click);
            node
        }
        Kind::Menu => Node::new(Role::Menu),
        Kind::MenuItem(label) => {
            let mut node = Node::new(Role::MenuItem);
            node.set_label(label.as_str());
            node.add_action(Action::Click);
            node
        }
        // AccessKit's splitter is AT-SPI's separator.
        Kind::Separator => Node::new(Role::Splitter),
        // An image or a picture without a description only decorates: its node is hidden, so
        // assistive technologies skip it.
        Kind::Image { description, .. } | Kind::Picture { description, .. } => {
            let mut node = Node::new(Role::Image);
            if description.is_empty() {
                node.set_hidden();
            } else {
                node.set_label(description.as_str());
            }
            node
        }
        Kind::Tooltip(text) => {
            let mut node = Node::new(Role::Tooltip);
            node.set_label(text.as_str());
            node
        }
        // A button with a toggled state: AT-SPI shows it as a toggle button, pressed when
        // active, as GTK's ToggleButton; a radio as a radio button, checked when active, and as
        // a radio menu item in a menu, as a drop-down's items.
        Kind::Toggle {
            label,
            active,
            radio,
        } => {
            let in_menu = widget
                .parent
                .is_some_and(|p| matches!(ui.widget(p).kind, Kind::Menu));
            let mut node = Node::new(match (*radio, in_menu) {
                (true, true) => Role::MenuItemRadio,
                (true, false) => Role::RadioButton,
                (false, _) => Role::Button,
            });
            node.set_label(label.as_str());
            node.set_toggled(if *active {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        // A tile: a toggle button, as GTK's ToggleButton, pressed while on, and described by its
        // second line, as the GTK panel's tiles are.
        Kind::Tile(crate::tile::Tile {
            title: label,
            subtitle,
            active,
            ..
        }) => {
            let mut node = Node::new(Role::Button);
            node.set_label(label.as_str());
            if !subtitle.is_empty() {
                node.set_description(subtitle.as_str());
            }
            node.set_toggled(if *active {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        // A drop-down: a combo box, as GTK's DropDown, which names it by its chosen item and
        // gives the name an application sets as its description, through the tooltip GTK's
        // panels set to it; without an item it has no name, as GTK's, and Orca reads the
        // description. The value is the chosen item too, though AT-SPI reads no value of a combo
        // box (AccessKit#824). Its menu is announced by the popup state every widget with one
        // carries; a description or a tooltip set on the drop-down takes the place of its name
        // below.
        Kind::DropDown(drop) => {
            let mut node = Node::new(Role::ComboBox);
            let chosen = drop.chosen_text();
            if !chosen.is_empty() {
                node.set_label(chosen);
            }
            node.set_description(drop.name.as_str());
            node.set_value(chosen);
            node.add_action(Action::Click);
            node
        }
        // An icon toggle: a toggle button, as GTK's ToggleButton, pressed while on.
        Kind::IconToggle(crate::icon_toggle::IconToggle { label, active, .. }) => {
            let mut node = Node::new(Role::Button);
            node.set_label(label.as_str());
            node.set_toggled(if *active {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        // A check box or a switch with a toggled state, named by its label.
        Kind::Check { label, active } | Kind::Switch { label, active } => {
            let role = if matches!(widget.kind, Kind::Switch { .. }) {
                Role::Switch
            } else {
                Role::CheckBox
            };
            let mut node = Node::new(role);
            node.set_label(label.as_str());
            node.set_toggled(if *active {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        Kind::Slider(slider) => {
            let mut node = Node::new(Role::Slider);
            node.set_label(slider.name.as_str());
            node.set_numeric_value(slider.value);
            node.set_min_numeric_value(slider.min);
            node.set_max_numeric_value(slider.max);
            node.set_numeric_value_step(slider.step);
            node.set_orientation(Orientation::Horizontal);
            for action in [Action::Increment, Action::Decrement, Action::SetValue] {
                node.add_action(action);
            }
            node
        }
        // GTK's LevelBar: a level bar to AT-SPI, with a value and no action.
        Kind::Level(level) => {
            let mut node = Node::new(Role::Meter);
            node.set_label(level.name.as_str());
            node.set_numeric_value(level.value);
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(level.max);
            node.set_orientation(Orientation::Horizontal);
            node
        }
        // A text area reaches AT-SPI as GTK's TextView does: a text input of several lines.
        Kind::Field(field) | Kind::TextArea(field) => {
            let mut node = Node::new(if matches!(widget.kind, Kind::TextArea(_)) {
                Role::MultilineTextInput
            } else {
                Role::TextInput
            });
            node.set_label(field.name.as_str());
            node.set_value(field.text());
            node.add_action(Action::SetValue);
            // Its edit menu, as GTK 4.24's entry says; AT-SPI receives it through the patched
            // accesskit_atspi_common (vendor/accesskit_atspi_common/PATCHES.md, #638).
            node.set_has_popup(HasPopup::Menu);
            node
        }
        // How far the content is scrolled, in logical pixels, as GTK's ScrolledWindow.
        Kind::Scroll { height, offset } => {
            let mut node = Node::new(Role::ScrollView);
            // A hit test finds nothing of the content outside the viewport.
            node.set_clips_children();
            node.set_scroll_y(f64::from(*offset));
            node.set_scroll_y_min(0.0);
            node.set_scroll_y_max(f64::from((ui.content_height(id) - height).max(0.0)));
            node
        }
        Kind::List { name, .. } => {
            let mut node = Node::new(Role::ListBox);
            node.set_label(name.as_str());
            node
        }
        Kind::ListRow(text) => {
            let mut node = Node::new(Role::ListBoxOption);
            node.set_label(text.as_str());
            // As GTK's rows, selected or not once the list has a selection, and neither before.
            if let Some(selected) = widget.parent.and_then(|list| ui.selected(list)) {
                node.set_selected(selected == id);
            }
            node.add_action(Action::Click);
            node
        }
        // As GTK's row that is neither selectable nor activatable: an option with no name, no
        // selected state and no action, its text a label inside it.
        Kind::ListHeading(_) => Node::new(Role::ListBoxOption),
    };
    // The selected row of a list stands for the widget while it has the focus: a field that
    // moves the selection of the list under it, as a combo box's entry.
    if let Some(row) = ui.active_descendant(id) {
        node.set_active_descendant(node_id(row));
    }
    let sensitive = ui.is_sensitive(id);
    // A list is focusable as GTK's ListBox is, while it has a row to take the focus.
    if (widget.is_focusable() || list_stop(ui, id).is_some()) && sensitive {
        node.add_action(Action::Focus);
    }
    if !sensitive {
        node.set_disabled();
    }
    if let Some(expanded) = widget.expanded {
        node.set_has_popup(HasPopup::Menu);
        node.set_expanded(expanded);
    }
    // A tile's second line stays its description over a tooltip, as the GTK panel sets the
    // description after the tooltip its name gives.
    let second_line = matches!(&widget.kind, Kind::Tile(tile) if !tile.subtitle.is_empty());
    if let Some(description) = ui.description(id).filter(|_| !second_line) {
        node.set_description(description);
    }
    node.set_bounds(bounds(ui, id, scale));
    let shown = widget.children.iter().filter(|&&c| ui.widget(c).visible);
    let mut children: Vec<NodeId> = shown.map(|&c| node_id(c)).collect();
    if has_row_label(ui, id) {
        children.push(row_label_id(id));
    }
    if id == Ui::ROOT && ui.announcement.is_some() {
        children.push(ANNOUNCEMENT);
    }
    if let Kind::Field(field) | Kind::TextArea(field) = &widget.kind {
        let lines = field.line_ranges();
        children.extend((0..lines.len()).map(|n| text_run_id(id, n)));
        // The caret, which AT-SPI's caret offset gives and Orca reads the line of.
        if let Some((anchor, focus)) = field.selection() {
            let text = field.text();
            let at = |index| text_position(id, &text, &lines, index);
            node.set_text_selection(TextSelection {
                anchor: at(anchor),
                focus: at(focus),
            });
        }
    }
    node.set_children(children);
    node
}

/// A field's committed text as one run per line as it is seen, which AT-SPI's Text and
/// EditableText interfaces read: AccessKit offers them only for a text input with runs. A line
/// that ends in a newline holds it.
fn text_runs(ui: &Ui, id: WidgetId, scale: f64) -> Vec<(NodeId, Node)> {
    let (Kind::Field(field) | Kind::TextArea(field)) = &ui.widget(id).kind else {
        return Vec::new();
    };
    let text = field.text();
    let lines = field.line_ranges().into_iter().enumerate();
    lines
        .map(|(n, range)| {
            let line = text.get(range).unwrap_or_default();
            let mut run = Node::new(Role::TextRun);
            let lengths: Vec<u8> = line.chars().map(|c| c.len_utf8() as u8).collect();
            run.set_value(line);
            run.set_character_lengths(lengths);
            // ponytail: every run has the field's bounds; the line's own, when a screen reader
            // asks where a character is.
            run.set_bounds(bounds(ui, id, scale));
            (text_run_id(id, n), run)
        })
        .collect()
}

/// A list row's text as a label inside the row, as GTK's ListBox holds a GtkLabel in each of its
/// rows: AT-SPI reads the row as a list item named by its text, and the text as that label.
fn row_label(ui: &Ui, id: WidgetId, scale: f64) -> Option<(NodeId, Node)> {
    if !has_row_label(ui, id) {
        return None;
    }
    let (Kind::ListRow(text) | Kind::ListHeading(text)) = &ui.widget(id).kind else {
        return None;
    };
    let mut label = Node::new(Role::Label);
    label.set_value(text.as_str());
    label.set_bounds(bounds(ui, id, scale));
    Some((row_label_id(id), label))
}

/// Whether row or heading `id` holds its text as a label: a row that shows widgets of its own
/// has them in its place.
fn has_row_label(ui: &Ui, id: WidgetId) -> bool {
    let widget = ui.widget(id);
    match widget.kind {
        Kind::ListHeading(_) => true,
        Kind::ListRow(_) => ui.visible_children(id).is_empty(),
        _ => false,
    }
}

/// Byte `index` of field `id`'s `text`, split in `lines` as its runs, as a run and a character
/// in it: the line that holds the index, or the last line for the end of the text.
fn text_position(id: WidgetId, text: &str, lines: &[Range<usize>], index: usize) -> TextPosition {
    let line = lines.iter().position(|r| r.contains(&index));
    let line = line.unwrap_or(lines.len().saturating_sub(1));
    let start = lines.get(line).map_or(0, |r| r.start);
    TextPosition {
        node: text_run_id(id, line),
        character_index: text.get(start..index).map_or(0, |s| s.chars().count()),
    }
}

/// Widget `id`'s rectangle in physical pixels at `scale`.
fn bounds(ui: &Ui, id: WidgetId, scale: f64) -> Rect {
    let r = ui.widget(id).rect;
    Rect {
        x0: f64::from(r.x) * scale,
        y0: f64::from(r.y) * scale,
        x1: f64::from(r.x + r.w) * scale,
        y1: f64::from(r.y + r.h) * scale,
    }
}

/// The announcement of [`Ui::announce`]: a polite live region, which the AT-SPI bridge announces
/// as the node is added (`accesskit_atspi_common`, `adapter.rs`, `add_node`).
fn announcement(ui: &Ui, scale: f64) -> Option<(NodeId, Node)> {
    let text = ui.announcement.as_deref()?;
    let mut node = Node::new(Role::Label);
    node.set_value(text);
    node.set_live(Live::Polite);
    node.set_bounds(bounds(ui, Ui::ROOT, scale));
    Some((ANNOUNCEMENT, node))
}

/// The updates that publish `ui`, in order. With an announcement asked for, the first holds it
/// and the second no longer does: the bridge announces the text once and keeps no stale copy of
/// it in the tree. The announcement is taken.
pub fn tree_updates(ui: &mut Ui, scale: f64) -> Vec<TreeUpdate> {
    let mut updates = Vec::with_capacity(2);
    if ui.announcement.is_some() {
        updates.push(tree_update(ui, scale));
        ui.announcement = None;
    }
    updates.push(tree_update(ui, scale));
    updates
}

/// The last update of a surface leaving the accessibility tree: the tree as it stands with its
/// root hidden, so the bridge removes the root and every node under it, each defunct, out of
/// the cache and its objects unregistered from the bus (`accesskit_atspi_common`, `adapter.rs`,
/// `node_updated`). Dropping the adapter alone unregisters none of them.
pub fn withdrawn(ui: &Ui, scale: f64) -> TreeUpdate {
    let mut update = tree_update(ui, scale);
    let root = node_id(Ui::ROOT);
    for (_, node) in update.nodes.iter_mut().filter(|(id, _)| *id == root) {
        node.set_hidden();
    }
    update
}

/// What a surface leaving the accessibility tree asks of its adapter: `accesskit_unix`'s, and
/// in the tests the AT-SPI bridge's under it.
pub trait Bridge {
    /// Tells the adapter whether its window is the focused one.
    fn set_focused(&mut self, focused: bool);
    /// Applies `update`, if the adapter is active.
    fn apply(&mut self, update: TreeUpdate);
}

impl Bridge for accesskit_unix::Adapter {
    fn set_focused(&mut self, focused: bool) {
        self.update_window_focus_state(focused);
    }

    fn apply(&mut self, update: TreeUpdate) {
        self.update_if_active(|| update);
    }
}

/// Takes a surface out of the accessibility tree before its adapter is dropped, which alone
/// unregisters none of its nodes: its window is deactivated if it is the `focused` one, then
/// its nodes are withdrawn. An adapter not active yet, its activation still queued on the bus
/// thread, would register the whole of `initial` there and leave it: it gets no tree instead,
/// and stays pending until it is dropped (`accesskit_unix` 0.24, `activate_adapter`). An
/// announcement not yet published is dropped: out of the tree, a surface announces nothing.
pub fn leave(
    adapter: &mut impl Bridge,
    initial: &Mutex<Option<TreeUpdate>>,
    ui: &mut Ui,
    scale: f64,
    focused: bool,
) {
    ui.announcement = None;
    // Before the update, so that an activation from here on finds no tree.
    // ponytail: an activation between this and `apply` goes pending, and `apply` activates it
    // with the withdrawn tree, whose hidden root alone the bridge registers and keeps; that
    // window closes only if accesskit_unix unregisters a dropped adapter's nodes.
    if let Ok(mut tree) = initial.lock() {
        *tree = None;
    }
    if focused {
        adapter.set_focused(false);
    }
    adapter.apply(withdrawn(ui, scale));
}

/// The first tree of a surface coming back into the accessibility tree. An announcement asked
/// for while it was out is dropped, not heard as it comes back.
pub fn rejoining(ui: &mut Ui, scale: f64) -> TreeUpdate {
    ui.announcement = None;
    tree_update(ui, scale)
}

/// The whole tree of the visible widgets, as the adapter's first update and after every change.
pub fn tree_update(ui: &Ui, scale: f64) -> TreeUpdate {
    let mut tree = TreeInfo::new(node_id(Ui::ROOT));
    tree.toolkit_name = Some("athanor-sal".into());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    let shown = || ui.ids().filter(|&id| ui.is_visible(id));
    TreeUpdate {
        nodes: shown()
            .map(|id| (node_id(id), node(ui, id, scale)))
            .chain(shown().flat_map(|id| text_runs(ui, id, scale)))
            .chain(shown().filter_map(|id| row_label(ui, id, scale)))
            .chain(announcement(ui, scale))
            .collect(),
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus: node_id(ui.focus().unwrap_or(Ui::ROOT)),
    }
}

/// The visible widgets that `update` leaves without a usable node: missing, of an unknown role,
/// or focusable without a name or with a blank one.
#[cfg(test)]
pub fn missing(ui: &Ui, update: &TreeUpdate) -> Vec<WidgetId> {
    ui.ids()
        .filter(|&id| ui.is_visible(id))
        .filter(|&id| {
            let Some((_, node)) = update.nodes.iter().find(|(n, _)| *n == node_id(id)) else {
                return true;
            };
            node.role() == Role::Unknown
                || (ui.widget(id).is_focusable()
                    && node.label().is_none_or(|l| l.trim().is_empty()))
        })
        .collect()
}

impl Ui {
    /// Carries out an assistive technology's request on the widget it names, unless it is
    /// hidden or insensitive. A request for a node of a widget since removed is refused, and
    /// logged: it never reaches the widget that took the slot ([`node_id`]).
    pub(crate) fn do_action(&mut self, request: &ActionRequest) -> Option<Event> {
        let Some(id) = widget_of(self, request.target_node) else {
            let slot = request.target_node.0 & ((1 << SLOT_BITS) - 1);
            if request.target_node.0 >> WIDGET_BITS == 0 && (slot as usize) < self.len() {
                self.widgets
                    .warn(format_args!("no action on slot {slot}: its widget was removed"));
            }
            return None;
        };
        if !self.is_visible(id) || !self.is_sensitive(id) {
            return None;
        }
        match (request.action, &request.data) {
            (Action::Click, _) => self.activate(id),
            // A list's focus goes to its stop, as the Tab key's does.
            (Action::Focus, _) => {
                self.set_focus(Some(list_stop(self, id).unwrap_or(id)));
                None
            }
            (Action::Increment, _) => self.step_value(id, 1),
            (Action::Decrement, _) => self.step_value(id, -1),
            (Action::SetValue, Some(ActionData::NumericValue(value))) => self.set_value(id, *value),
            (Action::SetValue, Some(ActionData::Value(value))) => self.set_field_text(id, value),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chooser;
    use crate::theme::Theme;

    #[test]
    fn every_widget_has_a_node() {
        let mut ui = Ui::new("Every kind", Theme::default(), false);
        let group = ui.add(Ui::ROOT, Kind::Group("Group".into()));
        ui.add(group, Kind::Label("Label".into()));
        let row = ui.add(group, Kind::Row);
        ui.add(row, Kind::Button("Button".into()));
        ui.add(
            row,
            Kind::Toggle {
                label: "Toggle".into(),
                active: true,
                radio: false,
            },
        );
        let field = ui.field(Ui::ROOT, "Field", "text");
        ui.add_title_bar();
        ui.layout();

        let hidden = ui.add(group, Kind::Button("Hidden".into()));
        ui.set_visible(hidden, false);
        ui.layout();
        let mut update = tree_update(&ui, 1.0);
        // Every visible widget, and the field's text run.
        let visible = ui.ids().filter(|&id| ui.is_visible(id)).count();
        assert_eq!(update.nodes.len(), visible + 1);
        assert!(update.nodes.iter().all(|(n, _)| *n != node_id(hidden)));
        assert_eq!(missing(&ui, &update), Vec::<WidgetId>::new());

        // The check notices a widget whose node is gone or nameless.
        update.nodes.retain(|(n, _)| *n != node_id(row));
        if let Some((_, node)) = update.nodes.iter_mut().find(|(n, _)| *n == node_id(field)) {
            node.clear_label();
        }
        assert_eq!(missing(&ui, &update), vec![row, field]);
        if let Some((_, node)) = update.nodes.iter_mut().find(|(n, _)| *n == node_id(field)) {
            node.set_label(" ");
        }
        assert_eq!(missing(&ui, &update), vec![row, field]);
    }

    #[test]
    fn a_picture_that_only_decorates_is_hidden() -> Result<(), crate::Error> {
        let mut ui = Ui::new("Wallpaper", Theme::default(), false);
        let pixel = || crate::Image::from_rgba8(1, 1, vec![0, 0, 0, 255]);
        let decorates = ui.picture(Ui::ROOT, pixel()?, "");
        let named = ui.picture(Ui::ROOT, pixel()?, "Hearth");
        ui.layout();
        let (decorates, named) = (node(&ui, decorates, 1.0), node(&ui, named, 1.0));
        assert_eq!((decorates.role(), decorates.is_hidden()), (Role::Image, true));
        assert_eq!((named.role(), named.is_hidden()), (Role::Image, false));
        assert_eq!(named.label(), Some("Hearth"));
        Ok(())
    }

    #[test]
    fn the_chooser_tree_has_roles_and_names() {
        let chooser = chooser::build(&athanor_i18n::Catalog::empty(), false);
        assert_eq!(chooser.choices.len(), 8);
        let ui = &chooser.ui;
        let update = tree_update(ui, 1.0);
        let node = |id: WidgetId| &update.nodes[id.index()].1;
        let named = |role: Role| {
            update
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == role)
                .map(|(_, n)| n.label().unwrap_or_default().to_owned())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            update.tree.as_ref().map(|t| t.root),
            Some(node_id(Ui::ROOT))
        );
        assert_eq!(node(Ui::ROOT).role(), Role::Window);
        assert_eq!(node(Ui::ROOT).label(), Some("Layout"));
        assert_eq!(named(Role::Group), ["Style", "Panel", "Dock"]);
        assert_eq!(
            named(Role::Button),
            [
                "Island",
                "Bar",
                "Essential",
                "Top",
                "Bottom",
                "Visible",
                "Auto-hide",
                "None"
            ]
        );
        for group in ui
            .widget(Ui::ROOT)
            .children
            .iter()
            .filter(|&&g| node(g).role() == Role::Group)
        {
            assert_eq!(node(*group).orientation(), Some(Orientation::Vertical));
            let toggles: Vec<_> = descendants(ui, *group)
                .into_iter()
                .filter_map(|id| node(id).toggled())
                .collect();
            assert_eq!(
                toggles.iter().filter(|&&t| t == Toggled::True).count(),
                1,
                "one choice is pressed in {:?}",
                node(*group).label()
            );
        }
        assert_eq!(missing(ui, &update), Vec::<WidgetId>::new());
    }

    #[test]
    fn a_check_and_a_switch_read_as_gtks() {
        let mut ui = Ui::new("Choices", Theme::default(), false);
        let choice = |switch, label: &str, active| {
            let label = label.into();
            if switch {
                Kind::Switch { label, active }
            } else {
                Kind::Check { label, active }
            }
        };
        let check = ui.add(Ui::ROOT, choice(false, "Check", true));
        let switch = ui.add(Ui::ROOT, choice(true, "Switch", false));
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let read = |id: WidgetId| {
            let n = &update.nodes[id.index()].1;
            (
                n.role(),
                n.label().map(str::to_owned),
                n.toggled(),
                n.supports_action(Action::Click),
            )
        };
        let (checked, off) = (Some(Toggled::True), Some(Toggled::False));
        assert_eq!(read(check), (Role::CheckBox, Some("Check".into()), checked, true));
        assert_eq!(read(switch), (Role::Switch, Some("Switch".into()), off, true));
        assert_eq!(missing(&ui, &update), Vec::<WidgetId>::new());
        let click = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: node_id(switch),
            data: None,
        };
        assert_eq!(ui.do_action(&click), Some(Event::Toggled(switch)));
        assert!(ui.is_active(switch));
    }

    #[test]
    fn a_node_cached_for_a_removed_widget_never_reaches_the_one_in_its_slot() {
        let mut ui = Ui::new("Requests", Theme::default(), false);
        let column = ui.add(Ui::ROOT, Kind::Column);
        let old = ui.add(column, Kind::Button("Old".into()));
        ui.layout();
        let cached = node_id(old);
        assert!(tree_update(&ui, 1.0).nodes.iter().any(|(n, _)| *n == cached));
        let click = |node| ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: node,
            data: None,
        };
        // The application replaces the button, and the tree is published again; the assistive
        // technology still acts on the node it cached.
        ui.remove_children(column);
        let new = ui.add(column, Kind::Button("New".into()));
        ui.layout();
        let published = tree_update(&ui, 1.0);
        assert_eq!(new.index(), old.index(), "the slot is reused");
        assert_ne!(node_id(new), cached, "under another node");
        assert!(!published.nodes.iter().any(|(n, _)| *n == cached));
        assert_eq!(ui.do_action(&click(cached)), None);
        assert_ne!(ui.focus(), Some(new));
        // A request on the node published for the new button acts on it.
        assert_eq!(ui.do_action(&click(node_id(new))), Some(Event::Clicked(new)));
        // A text run's node names no widget, whatever its slot.
        assert_eq!(ui.do_action(&click(text_run_id(new, 0))), None);
    }

    #[test]
    fn a_slot_is_retired_before_a_cached_node_could_name_its_next_widget() {
        let mut ui = Ui::new("Requests", Theme::default(), false);
        let column = ui.add(Ui::ROOT, Kind::Column);
        let first = ui.add(column, Kind::Button("First".into()));
        let cached = node_id(first);
        // 65,535 removals bring the slot's generation to the last whose low 16 bits are its own.
        let mut last = first;
        for _ in 0..u16::MAX {
            ui.remove_children(column);
            last = ui.add(column, Kind::Button("Again".into()));
            assert_eq!(last.index(), first.index(), "the slot is reused");
        }
        assert_eq!(last.1, u32::from(u16::MAX));
        // The next removal would bring them back to the first widget's: the slot is retired.
        ui.remove_children(column);
        let next = ui.add(column, Kind::Button("Next".into()));
        assert_ne!(next.index(), first.index(), "a retired slot holds no widget again");
        assert_eq!(widget_of(&ui, cached), None);
        ui.layout();
        let click = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: cached,
            data: None,
        };
        assert_eq!(ui.do_action(&click), None);
        assert_eq!(widget_of(&ui, node_id(next)), Some(next));
    }

    #[test]
    fn requests_from_assistive_technologies_are_bounded() {
        let mut ui = Ui::new("Requests", Theme::default(), false);
        let label = ui.add(Ui::ROOT, Kind::Label("Label".into()));
        let button = ui.add(Ui::ROOT, Kind::Button("Button".into()));
        let field = ui.field(Ui::ROOT, "Field", "");
        ui.layout();
        let request = |target: WidgetId, action, data| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node_id(target),
            data,
        };

        // A widget that takes no focus, or a node that does not exist, leaves the focus alone.
        ui.set_focus(Some(button));
        assert_eq!(ui.do_action(&request(label, Action::Click, None)), None);
        assert_eq!(ui.do_action(&request(label, Action::Focus, None)), None);
        assert_eq!(
            ui.do_action(&request(WidgetId(ui.len() as u32, 0), Action::Click, None)),
            None
        );
        assert_eq!(ui.focus(), Some(button));

        // A value set from outside loses its control characters and its excess length.
        let value = format!("a\nb\u{7}\u{202E}{}", "x".repeat(2 * Ui::FIELD_MAX_CHARS));
        let set = request(
            field,
            Action::SetValue,
            Some(ActionData::Value(value.into())),
        );
        assert_eq!(ui.do_action(&set), Some(Event::Edited(field)));
        let text = ui.field_text(field).unwrap_or_default();
        assert!(text.starts_with("abx"), "{text:.8}");
        assert_eq!(text.chars().count(), Ui::FIELD_MAX_CHARS);
        assert!(crate::render::render(&mut ui, 1.0).is_ok());
    }

    #[test]
    fn the_title_bar_reads_as_gtks() {
        let mut ui = chooser::build(&athanor_i18n::Catalog::empty(), false).ui;
        ui.add_title_bar();
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let bar = ui.widget(Ui::ROOT).children[0];
        let parts: Vec<_> = ui
            .widget(bar)
            .children
            .iter()
            .filter_map(|&id| update.nodes.iter().find(|(n, _)| *n == node_id(id)))
            .map(|(_, node)| node)
            .collect();
        let read: Vec<_> = parts
            .iter()
            .map(|n| (n.role(), n.label().or(n.value()).unwrap_or_default()))
            .collect();
        assert_eq!(
            read,
            [
                (Role::Label, "Layout"),
                (Role::Button, "Minimize"),
                (Role::Button, "Close")
            ]
        );
        assert!(parts[1..].iter().all(|n| n.supports_action(Action::Click)));
    }

    #[test]
    fn a_field_reports_its_committed_text() {
        let mut ui = Ui::new("Field", Theme::default(), false);
        let field = ui.field(Ui::ROOT, "Name", "");
        ui.set_focus(Some(field));
        let run = |ui: &Ui| {
            let update = tree_update(ui, 1.0);
            let value = |id| {
                update
                    .nodes
                    .iter()
                    .find(|(n, _)| *n == id)
                    .and_then(|(_, node)| node.value().map(str::to_owned))
            };
            (value(node_id(field)), value(text_run_id(field, 0)))
        };

        // A preedit is not text yet; the commit is.
        ui.compose("ni", Some((2, 2)));
        assert_eq!(run(&ui), (Some(String::new()), Some(String::new())));
        ui.commit("你");
        assert_eq!(run(&ui), (Some("你".into()), Some("你".into())));
        let update = tree_update(&ui, 1.0);
        let lengths = update
            .nodes
            .iter()
            .find(|(n, _)| *n == text_run_id(field, 0))
            .map(|(_, node)| node.character_lengths().to_vec());
        assert_eq!(lengths, Some(vec![3]));
    }

    #[test]
    fn a_menu_its_button_and_a_tooltip_read_as_gtks() {
        use accesskit::HasPopup;
        let mut ui = Ui::new("Dock", Theme::default(), false);
        let app = ui.add(Ui::ROOT, Kind::Button("Files".into()));
        ui.set_has_popup(app, true);
        ui.set_tooltip(app, Some("Files"));
        ui.set_description(app, Some("Running"));
        ui.layout();
        let read = |ui: &Ui, id: WidgetId| node(ui, id, 1.0);
        let button = read(&ui, app);
        assert_eq!(button.has_popup(), Some(HasPopup::Menu));
        assert_eq!(button.is_expanded(), Some(false));
        assert_eq!(button.description(), Some("Running"));
        ui.set_expanded(app, true);
        assert_eq!(read(&ui, app).is_expanded(), Some(true));

        let mut menu = Ui::new("Files", Theme::default(), false);
        let list = menu.add(Ui::ROOT, Kind::Menu);
        let item = menu.add(list, Kind::MenuItem("New Window".into()));
        let line = menu.add(list, Kind::Separator);
        menu.layout();
        assert_eq!(read(&menu, list).role(), Role::Menu);
        let item = read(&menu, item);
        assert_eq!(
            (item.role(), item.label()),
            (Role::MenuItem, Some("New Window"))
        );
        assert!(item.supports_action(Action::Click) && item.supports_action(Action::Focus));
        // AccessKit's splitter is AT-SPI's separator.
        assert_eq!(read(&menu, line).role(), Role::Splitter);
        assert_eq!(
            missing(&menu, &tree_update(&menu, 1.0)),
            Vec::<WidgetId>::new()
        );

        let mut tip = Ui::new("Files", Theme::default(), false);
        let text = tip.add(Ui::ROOT, Kind::Tooltip("Files".into()));
        tip.layout();
        let text = read(&tip, text);
        assert_eq!((text.role(), text.label()), (Role::Tooltip, Some("Files")));
    }

    #[test]
    fn a_slider_reads_as_gtks_and_obeys_assistive_technologies() {
        let mut ui = Ui::new("Values", Theme::default(), false);
        let id = ui
            .slider(Ui::ROOT, "Level", 0.0, 10.0, 3.0, 9.0)
            .unwrap_or(Ui::ROOT);
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let n = &update.nodes[id.index()].1;
        assert_eq!((n.role(), n.label()), (Role::Slider, Some("Level")));
        assert_eq!(n.orientation(), Some(Orientation::Horizontal));
        assert_eq!(
            (
                n.numeric_value(),
                n.min_numeric_value(),
                n.max_numeric_value(),
                n.numeric_value_step()
            ),
            (Some(9.0), Some(0.0), Some(10.0), Some(3.0))
        );
        for action in [
            Action::Increment,
            Action::Decrement,
            Action::SetValue,
            Action::Focus,
        ] {
            assert!(n.supports_action(action), "{action:?}");
        }
        assert_eq!(missing(&ui, &update), Vec::<WidgetId>::new());

        let request = |action, data| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node_id(id),
            data,
        };
        let changed = Some(Event::Changed(id));
        assert_eq!(ui.do_action(&request(Action::Increment, None)), changed);
        assert_eq!(ui.value(id), Some(10.0));
        assert_eq!(ui.do_action(&request(Action::Increment, None)), None);
        assert_eq!(ui.do_action(&request(Action::Decrement, None)), changed);
        assert_eq!(ui.value(id), Some(9.0));
        let set = |v| request(Action::SetValue, Some(ActionData::NumericValue(v)));
        assert_eq!(ui.do_action(&set(1.4)), changed);
        assert_eq!(ui.value(id), Some(0.0));
        assert_eq!(ui.do_action(&set(99.0)), changed);
        assert_eq!(ui.value(id), Some(10.0));
        assert_eq!(ui.do_action(&set(f64::NAN)), None);
        assert_eq!(ui.value(id), Some(10.0));
        // The text form of SetValue is for a field.
        let text = request(Action::SetValue, Some(ActionData::Value("3".into())));
        assert_eq!(ui.do_action(&text), None);
        assert_eq!(ui.value(id), Some(10.0));
    }

    #[test]
    fn a_level_reads_as_gtks_and_takes_no_action() {
        let mut ui = Ui::new("Values", Theme::default(), false);
        let id = ui
            .level(Ui::ROOT, "Volume", 120.0, 100.0, 150.0)
            .unwrap_or(Ui::ROOT);
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let n = &update.nodes[id.index()].1;
        assert_eq!((n.role(), n.label()), (Role::Meter, Some("Volume")));
        assert_eq!(
            (
                n.numeric_value(),
                n.min_numeric_value(),
                n.max_numeric_value()
            ),
            (Some(120.0), Some(0.0), Some(150.0))
        );
        for action in [
            Action::Increment,
            Action::Decrement,
            Action::SetValue,
            Action::Focus,
            Action::Click,
        ] {
            assert!(!n.supports_action(action), "{action:?}");
            let request = ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: node_id(id),
                data: Some(ActionData::NumericValue(10.0)),
            };
            assert_eq!(ui.do_action(&request), None, "{action:?}");
            assert_eq!(ui.value(id), Some(120.0), "{action:?}");
        }
        assert_eq!(missing(&ui, &update), Vec::<WidgetId>::new());
    }

    #[test]
    fn a_list_in_a_scroll_reads_as_gtks() {
        let mut ui = Ui::new("Lists", Theme::default(), false);
        let scroll = ui.scroll(Ui::ROOT, 100.0);
        let list = ui.list(scroll, "Fruit");
        let rows: Vec<WidgetId> = ["Apple", "Pear", "Plum", "Fig", "Lime", "Kiwi"]
            .iter()
            .map(|name| ui.list_row(list, name))
            .collect();
        ui.layout();
        let unselected = tree_update(&ui, 1.0);
        assert_eq!(unselected.nodes[rows[0].index()].1.is_selected(), None);
        assert_eq!(ui.activate(rows[1]), Some(Event::Selected(rows[1])));
        ui.key(crate::Key::End);
        let update = tree_update(&ui, 1.0);
        let n = |id: WidgetId| &update.nodes[id.index()].1;
        assert_eq!(
            (n(list).role(), n(list).label()),
            (Role::ListBox, Some("Fruit"))
        );
        assert!(n(list).supports_action(Action::Focus), "as GTK's");
        assert_eq!(
            n(list).children(),
            rows.iter().map(|&r| node_id(r)).collect::<Vec<_>>()
        );
        // Each row holds its text as a label, as GTK's rows hold a GtkLabel.
        assert_eq!(n(rows[0]).children(), [row_label_id(rows[0])]);
        let label = update
            .nodes
            .iter()
            .find(|(id, _)| *id == row_label_id(rows[0]))
            .map(|(_, node)| (node.role(), node.value()));
        assert_eq!(label, Some((Role::Label, Some("Apple"))));
        let read = |id: WidgetId| (n(id).role(), n(id).label(), n(id).is_selected());
        assert_eq!(
            read(rows[0]),
            (Role::ListBoxOption, Some("Apple"), Some(false))
        );
        assert_eq!(
            read(rows[5]),
            (Role::ListBoxOption, Some("Kiwi"), Some(true))
        );
        assert!(
            n(rows[0]).supports_action(Action::Click) && n(rows[0]).supports_action(Action::Focus)
        );
        assert_eq!(
            update.focus,
            node_id(rows[5]),
            "the focus is the selected row"
        );

        // The scroll says how far it is scrolled; the rows above its viewport stay in the tree.
        let content: f32 = rows.iter().map(|&r| ui.widget(r).rect.h).sum();
        let offset = f64::from(content - 100.0);
        assert_eq!(n(scroll).role(), Role::ScrollView);
        assert!(n(scroll).clips_children(), "a hit test finds no row out of view");
        let scrolled = (
            n(scroll).scroll_y(),
            n(scroll).scroll_y_min(),
            n(scroll).scroll_y_max(),
        );
        assert_eq!(scrolled, (Some(offset), Some(0.0), Some(offset)));
        assert!(ui.widget(rows[0]).rect.bottom() < ui.widget(scroll).rect.y);
        assert!(update.nodes.iter().any(|(id, _)| *id == node_id(rows[0])));
        assert_eq!(missing(&ui, &update), Vec::<WidgetId>::new());

        let request = |target: WidgetId, action| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node_id(target),
            data: None,
        };
        assert_eq!(
            ui.do_action(&request(rows[0], Action::Click)),
            Some(Event::Selected(rows[0]))
        );
        assert_eq!(ui.selected(list), Some(rows[0]));
        assert_eq!(ui.do_action(&request(rows[3], Action::Focus)), None);
        assert_eq!(
            (ui.focus(), ui.selected(list)),
            (Some(rows[3]), Some(rows[0]))
        );
        // The list's focus goes to its stop, the selected row once the focus has left it.
        ui.set_focus(None);
        assert_eq!(ui.do_action(&request(list, Action::Focus)), None);
        assert_eq!(ui.focus(), Some(rows[0]));
    }

    #[test]
    fn a_list_without_a_row_to_focus_is_not_focusable() {
        let mut ui = Ui::new("Lists", Theme::default(), false);
        let empty = ui.list(Ui::ROOT, "Empty");
        let list = ui.list(Ui::ROOT, "Off");
        let row = ui.list_row(list, "Row");
        ui.set_sensitive(row, false);
        ui.layout();
        let update = tree_update(&ui, 1.0);
        for id in [empty, list] {
            let node = &update.nodes[id.index()].1;
            assert!(!node.supports_action(Action::Focus), "{id:?}");
        }
    }

    fn descendants(ui: &Ui, id: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        for &c in &ui.widget(id).children {
            out.push(c);
            out.extend(descendants(ui, c));
        }
        out
    }

    #[test]
    fn a_text_area_reads_as_gtks_text_view() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let area = ui.text_area(Ui::ROOT, "Notes", "", 4);
        let field = ui.field(Ui::ROOT, "Name", "one");
        ui.layout();
        // Two hard lines, the second longer than the field is wide.
        let long = "word ".repeat(30);
        ui.set_field_text(area, &format!("first\n{long}"));
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let find = |id: NodeId| update.nodes.iter().find(|(n, _)| *n == id).map(|(_, n)| n);

        let node = find(node_id(area));
        assert_eq!(node.map(Node::role), Some(Role::MultilineTextInput));
        assert_eq!(node.and_then(Node::label), Some("Notes"));
        let value = format!("first\n{long}");
        assert_eq!(node.and_then(Node::value), Some(value.as_str()));

        // One run per line as it is seen, in order, their text the whole value.
        let runs: Vec<&Node> = (0..)
            .map_while(|n| find(text_run_id(area, n)))
            .collect();
        assert!(runs.len() > 2, "{} runs: the long line wraps", runs.len());
        assert_eq!(runs[0].value(), Some("first\n"));
        let joined: String = runs.iter().filter_map(|r| r.value()).collect();
        assert_eq!(joined, value);
        assert!(runs.iter().all(|r| r.role() == Role::TextRun));
        let children: Vec<NodeId> = (0..runs.len()).map(|n| text_run_id(area, n)).collect();
        assert_eq!(node.map(|n| n.children().to_vec()), Some(children));
        assert_eq!(text_run_id(area, 2), NodeId(node_id(area).0 | 3 << 40));

        // A single-line field keeps its one run at `id | 1 << 40`.
        assert_eq!(text_run_id(field, 0), NodeId(node_id(field).0 | 1 << 40));
        assert_eq!(find(text_run_id(field, 0)).and_then(Node::value), Some("one"));
        assert!(find(text_run_id(field, 1)).is_none());
    }

    #[test]
    fn a_fields_caret_is_its_text_selection() {
        // Orca reads the line of the caret, as AT-SPI's caret offset gives it: the end of a text
        // area's text, the start of a field's, as GTK's.
        let mut ui = Ui::new("Text", Theme::default(), false);
        let area = ui.text_area(Ui::ROOT, "Notes", "ab\ncd", 4);
        let field = ui.field(Ui::ROOT, "Name", "one");
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let selection = |id| {
            let node = update.nodes.iter().find(|(n, _)| *n == node_id(id));
            node.and_then(|(_, n)| n.text_selection()).copied()
        };
        let at = |node, character_index| accesskit::TextPosition {
            node,
            character_index,
        };
        let caret = |position| accesskit::TextSelection {
            anchor: position,
            focus: position,
        };
        assert_eq!(selection(area), Some(caret(at(text_run_id(area, 1), 2))));
        assert_eq!(selection(field), Some(caret(at(text_run_id(field, 0), 0))));

        // On a line's start after a newline, the caret is on that line, not at the end of the
        // line before.
        ui.set_focus(Some(area));
        ui.key(crate::Key::Left);
        ui.key(crate::Key::Left);
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let node = update.nodes.iter().find(|(n, _)| *n == node_id(area));
        let selection = node.and_then(|(_, n)| n.text_selection()).copied();
        assert_eq!(selection, Some(caret(at(text_run_id(area, 1), 0))));

        // After a final newline the caret is on the empty last line, which has a run of its own.
        ui.set_field_text(area, "ab\n");
        ui.layout();
        let update = tree_update(&ui, 1.0);
        let find = |id| update.nodes.iter().find(|(n, _)| *n == id).map(|(_, n)| n);
        let selection = find(node_id(area)).and_then(|n| n.text_selection()).copied();
        assert_eq!(selection, Some(caret(at(text_run_id(area, 1), 0))));
        assert_eq!(find(text_run_id(area, 1)).and_then(Node::value), Some(""));
    }

    #[test]
    fn a_field_opens_a_menu_whose_disabled_items_read_as_unavailable() {
        use accesskit::HasPopup;
        let mut ui = Ui::new("Text", Theme::default(), false);
        let field = ui.field(Ui::ROOT, "Name", "");
        let area = ui.text_area(Ui::ROOT, "Notes", "", 3);
        ui.layout();
        for id in [field, area] {
            let n = node(&ui, id, 1.0);
            assert_eq!(n.has_popup(), Some(HasPopup::Menu));
            // GTK's entry reads as neither collapsed nor expanded.
            assert_eq!(n.is_expanded(), None);
        }
        let (mut menu, items) = ui
            .field_menu(field, true)
            .expect("a field has a context menu");
        menu.layout();
        let (cut, paste) = (node(&menu, items[0].0, 1.0), node(&menu, items[2].0, 1.0));
        assert_eq!((cut.role(), cut.label()), (Role::MenuItem, Some("Cut")));
        assert!(cut.is_disabled());
        assert!(!cut.supports_action(Action::Focus));
        assert!(!paste.is_disabled());
        assert!(paste.supports_action(Action::Focus));
        // An assistive technology's click on a disabled item does nothing.
        let click = ActionRequest {
            action: Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node_id(items[0].0),
            data: None,
        };
        assert_eq!(menu.do_action(&click), None);
    }

    #[test]
    fn an_insensitive_widget_and_its_children_read_as_unavailable_and_refuse_every_action() {
        let mut ui = Ui::new("Test", Theme::default(), false);
        let group = ui.add(Ui::ROOT, Kind::Group("Group".into()));
        let button = ui.add(group, Kind::Button("Button".into()));
        let field = ui.field(group, "Name", "a");
        let slider = ui
            .slider(group, "Volume", 0.0, 10.0, 1.0, 5.0)
            .expect("a valid range");
        ui.layout();
        ui.set_sensitive(group, false);
        for id in [group, button, field, slider] {
            let n = node(&ui, id, 1.0);
            assert!(n.is_disabled(), "{id:?} reads as unavailable");
            assert!(!n.supports_action(Action::Focus), "{id:?} offers no focus");
        }
        let request = |action, target, data| ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node_id(target),
            data,
        };
        assert_eq!(ui.do_action(&request(Action::Click, button, None)), None);
        assert_eq!(ui.do_action(&request(Action::Focus, button, None)), None);
        assert_eq!(ui.focus(), None);
        assert_eq!(
            ui.do_action(&request(Action::Increment, slider, None)),
            None
        );
        let text = Some(ActionData::Value("b".into()));
        assert_eq!(ui.do_action(&request(Action::SetValue, field, text)), None);
        assert_eq!(
            (ui.value(slider), ui.field_text(field).as_deref()),
            (Some(5.0), Some("a"))
        );

        ui.set_sensitive(group, true);
        assert!(!node(&ui, button, 1.0).is_disabled());
        assert!(node(&ui, button, 1.0).supports_action(Action::Focus));
        let click = request(Action::Click, button, None);
        assert_eq!(ui.do_action(&click), Some(Event::Clicked(button)));
    }

    #[test]
    fn a_selection_made_with_shift_and_an_arrow_is_the_fields_text_selection() {
        let mut ui = Ui::new("Text", Theme::default(), false);
        let field = ui.field(Ui::ROOT, "Name", "");
        ui.layout();
        ui.set_focus(Some(field));
        ui.commit("abc");
        let selection = |ui: &mut Ui| {
            ui.layout();
            let s = node(ui, field, 1.0).text_selection().copied()?;
            let at = |p: TextPosition| (p.node, p.character_index);
            Some((at(s.anchor), at(s.focus)))
        };
        let run = text_run_id(field, 0);
        assert_eq!(selection(&mut ui), Some(((run, 3), (run, 3))));
        ui.key(crate::Key::SelectLeft);
        ui.key(crate::Key::SelectLeft);
        assert_eq!(selection(&mut ui), Some(((run, 3), (run, 1))));
        ui.key(crate::Key::SelectRight);
        assert_eq!(selection(&mut ui), Some(((run, 3), (run, 2))));
    }

    /// Through the AT-SPI bridge itself: a menu button reads as expandable, and expanded while
    /// its menu is open, and both it and a field as opening a popup, as GTK's do and W3C
    /// Core-AAM 1.2 maps `aria-expanded` and `aria-haspopup` (#638; the patch of
    /// `vendor/accesskit_atspi_common`). Neither says collapsed: Orca reads an expandable that is
    /// not expanded as collapsed.
    #[test]
    fn a_menu_button_and_a_field_reach_at_spi_with_their_popup_states() {
        use accesskit::{ActionHandler, ActionRequest};
        use accesskit_atspi_common::{
            Adapter, AdapterCallback, AppContext, Event, FullNodeId, InterfaceSet, PlatformNode,
            State, WindowBounds,
        };
        struct NoCallback;
        impl AdapterCallback for NoCallback {
            fn register_interfaces(&self, _: &Adapter, _: FullNodeId, _: InterfaceSet) {}
            fn unregister_interfaces(&self, _: &Adapter, _: FullNodeId, _: InterfaceSet) {}
            fn emit_event(&self, _: &Adapter, _: Event) {}
        }
        struct NoActions;
        impl ActionHandler for NoActions {
            fn do_action(&mut self, _: ActionRequest) {}
        }
        fn find(node: &PlatformNode, name: &str) -> Option<PlatformNode> {
            if node.name().ok()? == name {
                return Some(node.clone());
            }
            (0..node.child_count().ok()?).find_map(|i| {
                let child = node.relative(node.child_at_index(usize::try_from(i).ok()?).ok()??);
                find(&child, name)
            })
        }
        let states = |ui: &Ui, name: &str| {
            let adapter = Adapter::new(
                &AppContext::new(None),
                NoCallback,
                tree_update(ui, 1.0),
                true,
                WindowBounds::default(),
                NoActions,
            );
            let root = adapter.platform_node(adapter.root_id());
            let node = find(&root, name).expect("the node reaches AT-SPI");
            [
                State::Expandable,
                State::Expanded,
                State::Collapsed,
                State::HasPopup,
            ]
            .map(|s| node.state().contains(s))
        };
        let mut ui = Ui::new("Dock", Theme::default(), false);
        let app = ui.add(Ui::ROOT, Kind::Button("Files".into()));
        ui.set_has_popup(app, true);
        ui.field(Ui::ROOT, "Name", "");
        ui.layout();
        // [expandable, expanded, collapsed, has popup]
        assert_eq!(states(&ui, "Files"), [true, false, false, true]);
        assert_eq!(states(&ui, "Name"), [false, false, false, true]);
        ui.set_expanded(app, true);
        assert_eq!(states(&ui, "Files"), [true, true, false, true]);
    }

    /// What an in-process AT-SPI adapter told the bus.
    #[derive(Default)]
    struct Heard {
        /// The announcements: their text and politeness.
        announcements: Vec<(String, String)>,
        /// The windows deactivated, by name.
        deactivated: Vec<String>,
        /// The nodes with interfaces registered on the bus, and their interfaces' bits.
        registered: std::collections::HashMap<accesskit_atspi_common::FullNodeId, u32>,
    }
    type Bus = std::sync::Arc<std::sync::Mutex<Heard>>;

    /// An AT-SPI adapter of `ui` in-process, unfocused, and what it tells the bus.
    fn at_spi(ui: &Ui) -> (accesskit_atspi_common::Adapter, Bus) {
        at_spi_focused(ui, false)
    }

    fn at_spi_focused(ui: &Ui, focused: bool) -> (accesskit_atspi_common::Adapter, Bus) {
        use accesskit::{ActionHandler, ActionRequest};
        use accesskit_atspi_common::{
            Adapter, AdapterCallback, AppContext, Event, FullNodeId, InterfaceSet, ObjectEvent,
            WindowBounds, WindowEvent,
        };
        struct Recorder(Bus);
        impl AdapterCallback for Recorder {
            fn register_interfaces(&self, _: &Adapter, id: FullNodeId, set: InterfaceSet) {
                let mut heard = self.0.lock().expect("not poisoned");
                *heard.registered.entry(id).or_default() |= set.bits();
            }
            fn unregister_interfaces(&self, _: &Adapter, id: FullNodeId, set: InterfaceSet) {
                let mut heard = self.0.lock().expect("not poisoned");
                if let Some(bits) = heard.registered.get_mut(&id) {
                    *bits &= !set.bits();
                }
                heard.registered.retain(|_, bits| *bits != 0);
            }
            fn emit_event(&self, _: &Adapter, event: Event) {
                let mut heard = self.0.lock().expect("not poisoned");
                match event {
                    Event::Object {
                        event: ObjectEvent::Announcement(text, politeness),
                        ..
                    } => heard
                        .announcements
                        .push((text, format!("{politeness:?}"))),
                    Event::Window {
                        name,
                        event: WindowEvent::Deactivated,
                        ..
                    } => heard.deactivated.push(name),
                    _ => {}
                }
            }
        }
        struct NoActions;
        impl ActionHandler for NoActions {
            fn do_action(&mut self, _: ActionRequest) {}
        }
        let heard = Bus::default();
        let adapter = Adapter::new(
            &AppContext::new(None),
            Recorder(heard.clone()),
            tree_update(ui, 1.0),
            focused,
            WindowBounds::default(),
            NoActions,
        );
        (adapter, heard)
    }

    /// The root reads as the role the application sets, named by the title, as an on-screen
    /// display (`status`) and an end-of-session dialog (`alertdialog`) need them (doc_osd.md
    /// OD11, OD18, requirement S2); a window by default.
    #[test]
    fn the_root_takes_the_role_the_application_sets() {
        use accesskit_atspi_common::Role as AtspiRole;
        let root = |ui: &Ui| {
            let (adapter, _) = at_spi(ui);
            let root = adapter.platform_node(adapter.root_id());
            (root.role().expect("a role"), root.name().expect("a name"))
        };
        let mut ui = Ui::new("Volume 45 percent", Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Label("45".into()));
        ui.layout();
        assert_eq!(root(&ui), (AtspiRole::Frame, "Volume 45 percent".into()));
        ui.root_role = RootRole::Status;
        assert_eq!(root(&ui), (AtspiRole::StatusBar, "Volume 45 percent".into()));
        ui.root_role = RootRole::AlertDialog;
        ui.title = "Shut down now?".into();
        assert_eq!(root(&ui), (AtspiRole::Alert, "Shut down now?".into()));
    }

    /// A dialog's root reads as a modal dialog, and an alert dialog's keeps its role, modal too,
    /// as GTK's modal windows and alert dialogs do.
    #[test]
    fn a_dialog_reads_as_a_modal_dialog() {
        use accesskit_atspi_common::{Role as AtspiRole, State};
        let root = |ui: &Ui| {
            let (adapter, _) = at_spi(ui);
            let root = adapter.platform_node(adapter.root_id());
            let modal = root.state().contains(State::Modal);
            (root.role().expect("a role"), modal)
        };
        let mut ui = Ui::new("A dialog", Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Button("Close".into()));
        ui.layout();
        assert_eq!(root(&ui), (AtspiRole::Frame, false));
        ui.set_dialog();
        assert_eq!(root(&ui), (AtspiRole::Dialog, true));
        let mut alert = Ui::new("Replace the newer layout?", Theme::default(), false);
        alert.root_role = RootRole::AlertDialog;
        alert.add(Ui::ROOT, Kind::Button("Replace".into()));
        alert.layout();
        alert.set_dialog();
        assert_eq!(root(&alert), (AtspiRole::Alert, true));
    }

    /// The radios of a group read as radio buttons, the active one checked, where the toggles
    /// read as GTK's toggle buttons, pressed (doc_osd.md OD19, requirement S2). Activating
    /// another moves the check, as for the toggles.
    #[test]
    fn radios_read_as_radio_buttons_and_toggles_as_toggle_buttons() {
        use accesskit_atspi_common::{Role as AtspiRole, State};
        let mut ui = Ui::new("What did you plug in?", Theme::default(), false);
        let choices = ui.add(Ui::ROOT, Kind::Column);
        let headphones = ui.radio(choices, "Headphones", true);
        let headset = ui.radio(choices, "Headset with microphone", false);
        let row = ui.add(Ui::ROOT, Kind::Row);
        ui.toggle(row, "Island", true);
        ui.layout();
        let read = |ui: &Ui, name: &str| {
            let (adapter, _) = at_spi(ui);
            let root = adapter.platform_node(adapter.root_id());
            let node = (0..root.child_count().expect("children"))
                .filter_map(|i| root.child_at_index(usize::try_from(i).ok()?).ok()?)
                .map(|child| root.relative(child))
                .find(|child| child.name().ok().as_deref() == Some(name))
                .expect("the node reaches AT-SPI");
            let state = node.state();
            let states = [State::Checkable, State::Checked, State::Pressed];
            (node.role().expect("a role"), states.map(|s| state.contains(s)))
        };
        let radio = AtspiRole::RadioButton;
        assert_eq!(read(&ui, "Headphones"), (radio, [true, true, false]));
        assert_eq!(read(&ui, "Headset with microphone"), (radio, [true, false, false]));
        let toggle = (AtspiRole::ToggleButton, [false, false, true]);
        assert_eq!(read(&ui, "Island"), toggle);
        assert_eq!(ui.activate(headset), Some(Event::Selected(headset)));
        assert!(!ui.is_active(headphones));
        assert_eq!(read(&ui, "Headphones"), (radio, [true, false, false]));
        assert_eq!(read(&ui, "Headset with microphone"), (radio, [true, true, false]));
    }

    /// A drop-down reaches AT-SPI as GTK's DropDown with the name and the tooltip the panels set:
    /// a combo box named by its chosen item, with its own name as its description, renamed on
    /// every choice through the updates a live adapter takes; a description or a tooltip set on
    /// it describes it in place of its name; without items it has no name, as GTK's.
    #[test]
    fn a_drop_down_reads_as_gtks_named_by_its_chosen_item() {
        use accesskit_atspi_common::{Adapter, Role as AtspiRole};
        let read = |adapter: &Adapter| {
            let root = adapter.platform_node(adapter.root_id());
            let combo = (0..root.child_count().expect("children"))
                .filter_map(|i| root.child_at_index(usize::try_from(i).ok()?).ok()?)
                .map(|child| root.relative(child))
                .find(|child| child.role().ok() == Some(AtspiRole::ComboBox))
                .expect("the combo box reaches AT-SPI");
            let read = (combo.name(), combo.description());
            (read.0.expect("a name"), read.1.expect("a description"))
        };
        let publish = |ui: &mut Ui, adapter: &mut Adapter| {
            ui.layout();
            for update in tree_updates(ui, 1.0) {
                adapter.update(update);
            }
        };
        let mut ui = Ui::new("Drop-down", Theme::default(), false);
        let players = ui.drop_down(Ui::ROOT, "Media player", &["Spotify", "Firefox"], 0);
        ui.layout();
        let (mut adapter, _) = at_spi(&ui);
        assert_eq!(read(&adapter), ("Spotify".into(), "Media player".into()));
        assert_eq!(ui.choose(players, 1), Some(Event::Changed(players)));
        publish(&mut ui, &mut adapter);
        assert_eq!(read(&adapter), ("Firefox".into(), "Media player".into()));
        ui.set_tooltip(players, Some("Choose a player"));
        publish(&mut ui, &mut adapter);
        assert_eq!(read(&adapter), ("Firefox".into(), "Choose a player".into()));
        ui.set_description(players, Some("The player the keys control"));
        publish(&mut ui, &mut adapter);
        assert_eq!(
            read(&adapter),
            ("Firefox".into(), "The player the keys control".into())
        );
        let mut empty = Ui::new("Drop-down", Theme::default(), false);
        empty.drop_down(Ui::ROOT, "Media player", &[], 0);
        empty.layout();
        let (adapter, _) = at_spi(&empty);
        assert_eq!(read(&adapter), (String::new(), "Media player".into()));
    }

    /// An announcement reaches AT-SPI once, at polite priority, from a live region that leaves
    /// the tree with the next update, so the tree holds no stale text; the same text announced
    /// again is heard again (doc_osd.md OD11, requirement S2).
    #[test]
    fn an_announcement_is_heard_once_and_leaves_no_node() {
        let mut ui = Ui::new("Volume", Theme::default(), false);
        ui.root_role = RootRole::Status;
        ui.add(Ui::ROOT, Kind::Label("45".into()));
        ui.layout();
        let (mut adapter, heard) = at_spi(&ui);
        let children = |adapter: &accesskit_atspi_common::Adapter| {
            let root = adapter.platform_node(adapter.root_id());
            root.child_count().expect("children")
        };
        let before = children(&adapter);
        for _ in 0..2 {
            ui.announce("Volume 45 percent");
            let updates = tree_updates(&mut ui, 1.0);
            assert_eq!(updates.len(), 2);
            for update in updates {
                adapter.update(update);
            }
            assert_eq!(children(&adapter), before);
        }
        // Nothing more is announced once it was.
        for update in tree_updates(&mut ui, 1.0) {
            adapter.update(update);
        }
        let polite = ("Volume 45 percent".to_owned(), "Polite".to_owned());
        let heard = heard.lock().expect("not poisoned");
        assert_eq!(heard.announcements, [polite.clone(), polite]);
    }

    impl Bridge for accesskit_atspi_common::Adapter {
        fn set_focused(&mut self, focused: bool) {
            self.update_window_focus_state(focused);
        }

        fn apply(&mut self, update: TreeUpdate) {
            self.update(update);
        }
    }

    /// An announcement asked for before a surface leaves the accessibility tree and not yet
    /// published, or while it is out, is never heard (`Ui::announce`).
    #[test]
    fn a_surface_out_of_the_tree_announces_nothing() {
        let mut ui = Ui::new("Volume", Theme::default(), false);
        ui.add(Ui::ROOT, Kind::Button("Mute".into()));
        ui.layout();
        let (mut adapter, heard) = at_spi(&ui);
        ui.announce("Volume 45 percent");
        leave(&mut adapter, &Mutex::new(None), &mut ui, 1.0, false);
        assert!(ui.announcement.is_none(), "the next adapter would announce it");
        ui.announce("Volume 50 percent");
        let tree = rejoining(&mut ui, 1.0);
        assert!(tree.nodes.iter().all(|(id, _)| *id != ANNOUNCEMENT));
        let (mut adapter, again) = at_spi(&ui);
        for update in tree_updates(&mut ui, 1.0) {
            adapter.update(update);
        }
        assert!(heard.lock().expect("not poisoned").announcements.is_empty());
        assert!(again.lock().expect("not poisoned").announcements.is_empty());
    }

    /// A surface leaving the accessibility tree first deactivates its window when it was the
    /// active one, then unregisters every node of it, the root included, from the bus, and
    /// leaves an adapter not active yet no tree to register: what `Surface::withdraw` does
    /// before it drops the adapter, which unregisters none (requirement S2).
    #[test]
    fn a_surface_leaving_deactivates_its_window_and_leaves_nothing_on_the_bus() {
        let mut ui = Ui::new("What did you plug in?", Theme::default(), false);
        ui.root_role = RootRole::AlertDialog;
        let choices = ui.add(Ui::ROOT, Kind::Column);
        ui.radio(choices, "Headphones", true);
        ui.radio(choices, "Headset with microphone", false);
        ui.add(Ui::ROOT, Kind::Button("Hide status".into()));
        ui.layout();
        let (mut adapter, heard) = at_spi_focused(&ui, true);
        assert!(heard.lock().expect("not poisoned").registered.len() > 1);
        let initial = Mutex::new(Some(tree_update(&ui, 1.0)));
        leave(&mut adapter, &initial, &mut ui, 1.0, true);
        let heard = heard.lock().expect("not poisoned");
        assert_eq!(heard.deactivated, ["What did you plug in?"]);
        assert!(heard.registered.is_empty(), "{:?}", heard.registered);
        let initial = initial.lock().expect("not poisoned");
        assert!(initial.is_none(), "an adapter not active yet would register the whole tree");
    }
}
