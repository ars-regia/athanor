//! Accessibility on AccessKit: every widget is one node, and assistive technologies act through
//! the same paths as the keyboard and the pointer.

use crate::widget::{Event, Kind, Ui, WidgetId};
use accesskit::{
    Action, ActionData, ActionRequest, Node, NodeId, Rect, Role, Toggled, TreeId, TreeInfo,
    TreeUpdate,
};

pub fn node_id(id: WidgetId) -> NodeId {
    NodeId(id as u64)
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
        Kind::Column | Kind::Row => Node::new(Role::GenericContainer),
        Kind::Group(name) => {
            let mut node = Node::new(Role::Group);
            node.set_label(name.as_str());
            node
        }
        Kind::Label(text) => {
            let mut node = Node::new(Role::Label);
            node.set_value(text.as_str());
            node
        }
        Kind::Button(label) => {
            let mut node = Node::new(Role::Button);
            node.set_label(label.as_str());
            node.add_action(Action::Click);
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
    let r = widget.rect;
    node.set_bounds(Rect {
        x0: f64::from(r.x) * scale,
        y0: f64::from(r.y) * scale,
        x1: f64::from(r.x + r.w) * scale,
        y1: f64::from(r.y + r.h) * scale,
    });
    node.set_children(
        widget
            .children
            .iter()
            .map(|&c| node_id(c))
            .collect::<Vec<_>>(),
    );
    node
}

/// The whole tree, as the adapter's first update and after every change.
pub fn tree_update(ui: &Ui, scale: f64) -> TreeUpdate {
    let mut tree = TreeInfo::new(node_id(Ui::ROOT));
    tree.toolkit_name = Some("athanor-toolkit".into());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    TreeUpdate {
        nodes: (0..ui.len())
            .map(|id| (node_id(id), node(ui, id, scale)))
            .collect(),
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus: node_id(ui.focus().unwrap_or(Ui::ROOT)),
    }
}

/// The widgets that `update` leaves without a usable node: missing, of an unknown role, or
/// focusable without a name.
pub fn missing(ui: &Ui, update: &TreeUpdate) -> Vec<WidgetId> {
    (0..ui.len())
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
            .filter(|&id| id < self.len())?;
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
        ui.layout();

        let mut update = tree_update(&ui, 1.0);
        assert_eq!(update.nodes.len(), ui.len());
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
        let chooser = chooser::build(false);
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

    fn descendants(ui: &Ui, id: WidgetId) -> Vec<WidgetId> {
        let mut out = Vec::new();
        for &c in &ui.widget(id).children {
            out.push(c);
            out.extend(descendants(ui, c));
        }
        out
    }
}
