//! Accessibility on AccessKit: every widget is one node, and assistive technologies act through
//! the same paths as the keyboard and the pointer.

use crate::widget::{Event, Kind, Ui, WidgetId};
use accesskit::{
    Action, ActionData, ActionRequest, HasPopup, Node, NodeId, Orientation, Rect, Role, Toggled,
    TreeId, TreeInfo, TreeUpdate,
};

/// The AccessKit node id of a widget, for an application that adds nodes of its own to the tree.
pub fn node_id(id: WidgetId) -> NodeId {
    NodeId(id as u64)
}

/// The node of a field's text, beside the widgets' ids.
pub fn text_run_id(id: WidgetId) -> NodeId {
    NodeId(id as u64 | 1 << 32)
}

/// The node of widget `id`, with its bounds in physical pixels at `scale`. The match is
/// exhaustive, so a new kind of widget cannot be added without its node.
pub fn node(ui: &Ui, id: WidgetId, scale: f64) -> Node {
    let widget = ui.widget(id);
    let mut node = match &widget.kind {
        Kind::Column if id == Ui::ROOT => {
            let mut node = Node::new(Role::Window);
            node.set_label(ui.title.as_str());
            node
        }
        Kind::Column | Kind::Row | Kind::TitleBar => Node::new(Role::GenericContainer),
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
        Kind::Button(label) => {
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
        Kind::Tooltip(text) => {
            let mut node = Node::new(Role::Tooltip);
            node.set_label(text.as_str());
            node
        }
        // A button with a toggled state: AT-SPI shows it as a toggle button, pressed when
        // active, as GTK's ToggleButton.
        Kind::Toggle { label, active } => {
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
        Kind::Field(field) => {
            let mut node = Node::new(Role::TextInput);
            node.set_label(field.name.as_str());
            node.set_value(field.text());
            node.add_action(Action::SetValue);
            node
        }
    };
    if widget.is_focusable() {
        node.add_action(Action::Focus);
    }
    if let Some(expanded) = widget.expanded {
        node.set_has_popup(HasPopup::Menu);
        node.set_expanded(expanded);
    }
    if let Some(description) = ui.description(id) {
        node.set_description(description);
    }
    node.set_bounds(bounds(ui, id, scale));
    let shown = widget.children.iter().filter(|&&c| ui.widget(c).visible);
    let mut children: Vec<NodeId> = shown.map(|&c| node_id(c)).collect();
    if matches!(widget.kind, Kind::Field(_)) {
        children.push(text_run_id(id));
    }
    node.set_children(children);
    node
}

/// A field's committed text as one run, which AT-SPI's Text and EditableText interfaces read:
/// AccessKit offers them only for a text input with a run.
fn text_run(ui: &Ui, id: WidgetId, scale: f64) -> Option<(NodeId, Node)> {
    let text = ui.field_text(id)?;
    let mut run = Node::new(Role::TextRun);
    let lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
    run.set_value(text);
    run.set_character_lengths(lengths);
    run.set_bounds(bounds(ui, id, scale));
    Some((text_run_id(id), run))
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

/// The whole tree of the visible widgets, as the adapter's first update and after every change.
pub fn tree_update(ui: &Ui, scale: f64) -> TreeUpdate {
    let mut tree = TreeInfo::new(node_id(Ui::ROOT));
    tree.toolkit_name = Some("athanor-toolkit".into());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    let shown = || (0..ui.len()).filter(|&id| ui.is_visible(id));
    TreeUpdate {
        nodes: shown()
            .map(|id| (node_id(id), node(ui, id, scale)))
            .chain(shown().filter_map(|id| text_run(ui, id, scale)))
            .collect(),
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus: node_id(ui.focus().unwrap_or(Ui::ROOT)),
    }
}

/// The visible widgets that `update` leaves without a usable node: missing, of an unknown role,
/// or focusable without a name.
pub fn missing(ui: &Ui, update: &TreeUpdate) -> Vec<WidgetId> {
    (0..ui.len())
        .filter(|&id| ui.is_visible(id))
        .filter(|&id| {
            let Some((_, node)) = update.nodes.iter().find(|(n, _)| *n == node_id(id)) else {
                return true;
            };
            node.role() == Role::Unknown || (ui.widget(id).is_focusable() && node.label().is_none())
        })
        .collect()
}

impl Ui {
    /// Carries out an assistive technology's request on the widget it names.
    pub fn do_action(&mut self, request: &ActionRequest) -> Option<Event> {
        let id = usize::try_from(request.target_node.0)
            .ok()
            .filter(|&id| id < self.len() && self.is_visible(id))?;
        match (request.action, &request.data) {
            (Action::Click, _) => self.activate(id),
            (Action::Focus, _) => {
                self.set_focus(Some(id));
                None
            }
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
        let visible = (0..ui.len()).filter(|&id| ui.is_visible(id)).count();
        assert_eq!(update.nodes.len(), visible + 1);
        assert!(update.nodes.iter().all(|(n, _)| *n != node_id(hidden)));
        assert_eq!(missing(&ui, &update), Vec::<WidgetId>::new());

        // The check notices a widget whose node is gone or nameless.
        update.nodes.retain(|(n, _)| *n != node_id(row));
        if let Some((_, node)) = update.nodes.iter_mut().find(|(n, _)| *n == node_id(field)) {
            node.clear_label();
        }
        assert_eq!(missing(&ui, &update), vec![row, field]);
    }

    #[test]
    fn the_chooser_tree_has_roles_and_names() {
        let chooser = chooser::build(&athanor_i18n::Catalog::empty(), false);
        assert_eq!(chooser.choices.len(), 8);
        let ui = &chooser.ui;
        let update = tree_update(ui, 1.0);
        let node = |id: WidgetId| &update.nodes[id].1;
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
            let n = &update.nodes[id].1;
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
        assert_eq!(ui.do_action(&request(ui.len(), Action::Click, None)), None);
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
            .map(|&id| &update.nodes[id].1)
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
            (value(node_id(field)), value(text_run_id(field)))
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
            .find(|(n, _)| *n == text_run_id(field))
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

    fn descendants(ui: &Ui, id: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        for &c in &ui.widget(id).children {
            out.push(c);
            out.extend(descendants(ui, c));
        }
        out
    }
}
