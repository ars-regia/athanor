//! `com.canonical.dbusmenu` layouts as the tray's menus read them (doc_bar.md BR5). The
//! layout is a peer's tree: its depth, its node count and its labels are bounded here, and a
//! node of the wrong type is skipped (BR9). Icons and shortcuts are not read (ruling 9).

use athanor_unit::text;
use glib::prelude::*;
use glib::{Variant, VariantDict};

pub const MAX_DEPTH: usize = 8;
pub const MAX_NODES: usize = 512;
pub const LAYOUT_SIGNATURE: &str = "(u(ia{sv}av))";
const NODE_SIGNATURE: &str = "(ia{sv}av)";
const LABEL_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    None,
    Check(bool),
    Radio(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Separator,
    Item(Item),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: i32,
    /// Without mnemonics, one bounded line.
    pub label: String,
    pub enabled: bool,
    pub toggle: Toggle,
    /// The item opens a submenu, even one whose children have not arrived yet.
    pub submenu: bool,
    pub children: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub revision: u32,
    pub entries: Vec<Entry>,
    /// The ids of the items with a submenu, for `AboutToShowGroup`.
    pub submenus: Vec<i32>,
}

/// `None` when the reply is not `(u(ia{sv}av))`. Past `MAX_DEPTH` levels the children are
/// dropped; past `MAX_NODES` nodes in all, the rest of the tree is.
#[must_use]
pub fn parse_layout(reply: &Variant) -> Option<Layout> {
    if reply.type_().as_str() != LAYOUT_SIGNATURE {
        return None;
    }
    let revision = reply.try_child_value(0)?.get::<u32>()?;
    let root = reply.try_child_value(1)?;
    let mut budget = MAX_NODES;
    let mut submenus = Vec::new();
    let entries = children(&root, 0, &mut budget, &mut submenus);
    Some(Layout {
        revision,
        entries,
        submenus,
    })
}

/// The `(isvu)` arguments of `Event`: no data, and no timestamp.
#[must_use]
pub fn event(id: i32, name: &str) -> Variant {
    (id, name, "".to_variant(), 0u32).to_variant()
}

fn children(
    node: &Variant,
    depth: usize,
    budget: &mut usize,
    submenus: &mut Vec<i32>,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    if depth >= MAX_DEPTH {
        return entries;
    }
    let Some(list) = node.try_child_value(2) else {
        return entries;
    };
    for index in 0..list.n_children() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Some(child) = list
            .try_child_value(index)
            .and_then(|boxed| boxed.as_variant())
        else {
            continue;
        };
        if child.type_().as_str() != NODE_SIGNATURE {
            continue;
        }
        if let Some(entry) = entry(&child, depth + 1, budget, submenus) {
            entries.push(entry);
        }
    }
    tidy(entries)
}

/// One node, already checked to be `(ia{sv}av)`. `None` for an invisible node or an item
/// with no label.
fn entry(
    node: &Variant,
    depth: usize,
    budget: &mut usize,
    submenus: &mut Vec<i32>,
) -> Option<Entry> {
    let id = node.try_child_value(0)?.get::<i32>()?;
    let props = node.try_child_value(1)?;
    let dict = VariantDict::new(Some(&props));
    let boolean = |key: &str| dict.lookup::<bool>(key).ok().flatten();
    let string = |key: &str| dict.lookup::<String>(key).ok().flatten();
    if boolean("visible") == Some(false) {
        return None;
    }
    if string("type").as_deref() == Some("separator") {
        return Some(Entry::Separator);
    }
    let label = label(&string("label").unwrap_or_default());
    if label.is_empty() {
        return None;
    }
    let on = dict.lookup::<i32>("toggle-state").ok().flatten() == Some(1);
    let toggle = match string("toggle-type").as_deref() {
        Some("checkmark") => Toggle::Check(on),
        Some("radio") => Toggle::Radio(on),
        _ => Toggle::None,
    };
    let children = children(node, depth, budget, submenus);
    let submenu = string("children-display").as_deref() == Some("submenu") || !children.is_empty();
    if submenu {
        submenus.push(id);
    }
    Some(Entry::Item(Item {
        id,
        label,
        enabled: boolean("enabled").unwrap_or(true),
        toggle,
        submenu,
        children,
    }))
}

/// No separator first, last, or next to another.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let separator = matches!(entry, Entry::Separator);
        if separator
            && kept
                .last()
                .is_none_or(|last| matches!(last, Entry::Separator))
        {
            continue;
        }
        kept.push(entry);
    }
    if matches!(kept.last(), Some(Entry::Separator)) {
        kept.pop();
    }
    kept
}

/// `_` marks the mnemonic and `__` is a literal underscore. The raw label is cut first, so
/// a huge one costs nothing.
fn label(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().take(LABEL_CHARS * 2);
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    text::line(&out, LABEL_CHARS)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn node(id: i32, props: &[(&str, Variant)], children: Vec<Variant>) -> Variant {
        let props: HashMap<String, Variant> = props
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect();
        // A Vec<Variant> is an `av`: each child is boxed as a `v`.
        (id, props, children).to_variant()
    }

    fn item(id: i32, label: &str) -> Variant {
        node(id, &[("label", label.to_variant())], Vec::new())
    }

    fn separator(id: i32) -> Variant {
        node(id, &[("type", "separator".to_variant())], Vec::new())
    }

    fn layout(children: Vec<Variant>) -> Variant {
        // `(u32, Variant).to_variant()` would box the root as a `v`: build the tuple from
        // its members so the root stays an `(ia{sv}av)`, as in a real reply.
        Variant::tuple_from_iter([
            7u32.to_variant(),
            node(0, &[("children-display", "submenu".to_variant())], children),
        ])
    }

    fn labels(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Separator => "-",
                Entry::Item(item) => item.label.as_str(),
            })
            .collect()
    }

    #[test]
    fn a_menu_parses_with_its_toggles_and_submenus() {
        let parsed = parse_layout(&layout(vec![
            item(1, "_Open window"),
            separator(2),
            node(
                3,
                &[
                    ("label", "Mute".to_variant()),
                    ("toggle-type", "checkmark".to_variant()),
                    ("toggle-state", 1i32.to_variant()),
                ],
                Vec::new(),
            ),
            node(
                4,
                &[
                    ("label", "Low".to_variant()),
                    ("toggle-type", "radio".to_variant()),
                    ("toggle-state", 0i32.to_variant()),
                ],
                Vec::new(),
            ),
            node(
                5,
                &[
                    ("label", "Sync".to_variant()),
                    ("enabled", false.to_variant()),
                ],
                Vec::new(),
            ),
            node(6, &[("label", "More".to_variant())], vec![item(7, "About")]),
            node(
                8,
                &[
                    ("label", "Hidden".to_variant()),
                    ("visible", false.to_variant()),
                ],
                Vec::new(),
            ),
        ]))
        .unwrap();
        assert_eq!(parsed.revision, 7);
        assert_eq!(
            labels(&parsed.entries),
            ["Open window", "-", "Mute", "Low", "Sync", "More"]
        );
        let items: Vec<&Item> = parsed
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item(item) => Some(item),
                Entry::Separator => None,
            })
            .collect();
        assert_eq!(items.get(1).map(|i| i.toggle), Some(Toggle::Check(true)));
        assert_eq!(items.get(2).map(|i| i.toggle), Some(Toggle::Radio(false)));
        assert_eq!(items.get(3).map(|i| i.enabled), Some(false));
        assert_eq!(
            items.get(4).map(|i| (i.submenu, labels(&i.children))),
            Some((true, vec!["About"]))
        );
        assert_eq!(parsed.submenus, [6]);
    }

    #[test]
    fn separators_are_tidied() {
        let parsed = parse_layout(&layout(vec![
            separator(1),
            item(2, "A"),
            separator(3),
            separator(4),
            item(5, "B"),
            separator(6),
        ]))
        .unwrap();
        assert_eq!(labels(&parsed.entries), ["A", "-", "B"]);
    }

    #[test]
    fn nesting_stops_at_the_depth_bound() {
        let mut deepest = item(20, "Level");
        for id in (1..20).rev() {
            deepest = node(id, &[("label", "Level".to_variant())], vec![deepest]);
        }
        let parsed = parse_layout(&layout(vec![deepest])).unwrap();
        let mut depth = 0;
        let mut level = &parsed.entries;
        while let Some(Entry::Item(item)) = level.first() {
            depth += 1;
            level = &item.children;
        }
        assert_eq!(depth, MAX_DEPTH);
    }

    #[test]
    fn siblings_stop_at_the_node_bound() {
        let parsed = parse_layout(&layout((1..=2000).map(|id| item(id, "x")).collect())).unwrap();
        assert_eq!(parsed.entries.len(), MAX_NODES);
    }

    #[test]
    fn a_child_of_the_wrong_type_is_skipped() {
        let parsed = parse_layout(&layout(vec![
            "not a node".to_variant(),
            (1i32,).to_variant(),
            item(3, "Kept"),
        ]))
        .unwrap();
        assert_eq!(labels(&parsed.entries), ["Kept"]);
    }

    #[test]
    fn a_reply_of_another_type_is_refused() {
        assert!(parse_layout(&(1u32, "x").to_variant()).is_none());
        assert!(parse_layout(&item(1, "x")).is_none());
    }

    #[test]
    fn labels_lose_mnemonics_and_are_bounded() {
        assert_eq!(label("_File"), "File");
        assert_eq!(label("snake__case"), "snake_case");
        assert_eq!(label("trailing_"), "trailing");
        assert_eq!(label("\u{202E}evil\n"), "evil");
        assert_eq!(label(&"x".repeat(10_000)).chars().count(), LABEL_CHARS);
        let parsed = parse_layout(&layout(vec![item(1, ""), item(2, "\u{0007}")])).unwrap();
        assert!(
            parsed.entries.is_empty(),
            "an item with no label is dropped"
        );
    }

    #[test]
    fn the_event_is_isvu() {
        let value = event(4, "clicked");
        assert_eq!(value.type_().as_str(), "(isvu)");
        assert_eq!(
            value
                .try_child_value(1)
                .and_then(|v| v.get::<String>())
                .as_deref(),
            Some("clicked")
        );
    }
}
