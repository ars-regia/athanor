//! A layout as cosmic-panel 1.8.0 configuration (doc_shell.md, SH7).
//!
//! cosmic-panel reads the list of its entries from `com.system76.CosmicPanel/v1/entries`
//! and each entry from `com.system76.CosmicPanel.<name>/v1/`, one file per key. Every
//! entry here carries all 22 keys: the per-key fallback to /usr/share/cosmic works only
//! for an entry COSMIC ships, and `Dock-<connector>` is not one. The values start from
//! COSMIC's shipped files, which `fixtures/cosmic-panel-1.8.0` holds and the tests
//! compare against.
//!
//! This is the throwaway part of stage 1c: it goes when our own panel reads the layout
//! document itself.

use std::collections::{BTreeMap, BTreeSet};

use crate::placement::{dock_edge, DockEdge, Output, Shape};
use crate::preset::{DockKnob, Layout, PanelEdge, Preset};

pub const CLOCK: &str = "com.system76.CosmicAppletTime";
pub const WORKSPACES: &str = "com.system76.CosmicPanelWorkspacesButton";
pub const APP_LIBRARY: &str = "com.system76.CosmicPanelAppButton";
pub const LAUNCHER: &str = "com.system76.CosmicPanelLauncherButton";
pub const APP_LIST: &str = "com.system76.CosmicAppList";
pub const MINIMIZE: &str = "com.system76.CosmicAppletMinimize";

/// The status applets, in COSMIC's order: the right wing of every panel. The shield of
/// package 1b-shield (SH9.1) is added here and nowhere else.
pub const TRAY: [&str; 10] = [
    "com.system76.CosmicAppletInputSources",
    "com.system76.CosmicAppletA11y",
    "com.system76.CosmicAppletStatusArea",
    "com.system76.CosmicAppletTiling",
    "com.system76.CosmicAppletAudio",
    "com.system76.CosmicAppletBluetooth",
    "com.system76.CosmicAppletNetwork",
    "com.system76.CosmicAppletBattery",
    "com.system76.CosmicAppletNotifications",
    "com.system76.CosmicAppletPower",
];

const AUTOHIDE_BEHAVIOR: &str =
    "(\n    wait_time: 1000,\n    transition_time: 200,\n    handle_size: 4,\n    unhide_delay: 200,\n)";

/// One cosmic-panel entry: its name and its key files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub keys: BTreeMap<&'static str, String>,
}

/// Everything cosmic-panel reads for one layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub entries: Vec<Entry>,
}

impl Plan {
    /// The value of `com.system76.CosmicPanel/v1/entries`, in COSMIC's own format.
    pub fn entries_value(&self) -> String {
        let names: Vec<String> = self
            .entries
            .iter()
            .map(|entry| format!("\"{}\"", entry.name))
            .collect();
        format!("[{}]", names.join(","))
    }

    /// Whether an entry is pinned to a named output. cosmic-panel 1.8.0 binds such an
    /// entry only when it starts, so adding one needs a panel restart.
    pub fn pins_outputs(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry
                .keys
                .get("output")
                .is_some_and(|output| output != "All")
        })
    }

    /// The plan as text, one line per key: two plans are equal when their records are.
    pub fn to_record(&self) -> String {
        let mut record = format!("entries={}\n", self.entries_value());
        for entry in &self.entries {
            for (key, value) in &entry.keys {
                record.push_str(&format!("{}/{key}={value:?}\n", entry.name));
            }
        }
        record
    }
}

/// The plan for `layout` on `outputs`. Outputs without a size yet are left out; with no
/// sized output at all, the dock is placed for landscape.
pub fn render(layout: &Layout, outputs: &[Output]) -> Plan {
    let mut entries = vec![panel(layout)];
    if layout.dock() != DockKnob::Off {
        entries.extend(docks(layout, outputs));
    }
    Plan { entries }
}

/// One dock for every output while all of them share a shape, one per output otherwise.
fn docks(layout: &Layout, outputs: &[Output]) -> Vec<Entry> {
    let sized: Vec<&Output> = outputs.iter().filter(|output| output.is_sized()).collect();
    let shapes: BTreeSet<Shape> = sized.iter().map(|output| output.shape()).collect();
    if shapes.len() <= 1 {
        let shape = shapes.into_iter().next().unwrap_or(Shape::Landscape);
        return vec![dock(layout, shape, "Dock", "All".into())];
    }
    let named: Option<Vec<(&str, Shape)>> = sized
        .iter()
        .map(|output| {
            output
                .connector
                .as_deref()
                .filter(|connector| is_entry_safe(connector))
                .map(|connector| (connector, output.shape()))
        })
        .collect();
    match named {
        Some(named) => named
            .into_iter()
            .map(|(connector, shape)| {
                dock(
                    layout,
                    shape,
                    &format!("Dock-{connector}"),
                    format!("Name(\"{connector}\")"),
                )
            })
            .collect(),
        None => {
            tracing::warn!(
                "outputs of both shapes, one without a usable connector name: one dock for all of them, placed for landscape"
            );
            vec![dock(layout, Shape::Landscape, "Dock", "All".into())]
        }
    }
}

/// A connector name becomes a directory name and a RON string: letters, digits, '-' and
/// '_' only, which every DRM connector name is made of.
fn is_entry_safe(connector: &str) -> bool {
    !connector.is_empty()
        && connector.len() <= 64
        && connector
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn panel(layout: &Layout) -> Entry {
    let mut keys = cosmic_panel();
    let anchor = match layout.panel() {
        PanelEdge::Top => "Top",
        PanelEdge::Bottom => "Bottom",
    };
    keys.insert("anchor", anchor.into());
    match layout.preset() {
        Preset::Minimal => {}
        Preset::Float => {
            keys.insert("anchor_gap", "true".into());
            keys.insert("margin", "4".into());
            keys.insert("border_radius", "12".into());
        }
        Preset::Bar => {
            let mut right = TRAY.to_vec();
            right.push(CLOCK);
            keys.insert("size", "M".into());
            keys.insert("plugins_center", "None".into());
            keys.insert(
                "plugins_wings",
                format!(
                    "Some(({}, {}))",
                    ron_list(&[APP_LIBRARY, APP_LIST, MINIMIZE]),
                    ron_list(&right)
                ),
            );
        }
    }
    Entry {
        name: "Panel".into(),
        keys,
    }
}

fn dock(layout: &Layout, shape: Shape, name: &str, output: String) -> Entry {
    let mut keys = cosmic_dock();
    let anchor = match dock_edge(layout.panel(), shape) {
        DockEdge::Bottom => "Bottom",
        DockEdge::Left => "Left",
    };
    keys.insert("anchor", anchor.into());
    keys.insert("name", format!("\"{name}\""));
    keys.insert("output", output);
    if layout.dock() == DockKnob::AutoHide {
        keys.insert("autohide", "Always".into());
        keys.insert("exclusive_zone", "false".into());
    }
    Entry {
        name: name.into(),
        keys,
    }
}

fn ron_list(ids: &[&str]) -> String {
    let quoted: Vec<String> = ids.iter().map(|id| format!("\"{id}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// COSMIC's shipped panel, key for key.
fn cosmic_panel() -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("anchor", "Top".to_string()),
        ("anchor_gap", "false".into()),
        ("autohide", "Never".into()),
        ("autohide_behavior", AUTOHIDE_BEHAVIOR.into()),
        ("autohover_delay_ms", "Some(500)".into()),
        ("background", "ThemeDefault".into()),
        ("border_radius", "0".into()),
        ("exclusive_zone", "true".into()),
        ("expand_to_edges", "true".into()),
        ("keyboard_interactivity", "OnDemand".into()),
        ("layer", "Top".into()),
        ("margin", "0".into()),
        ("name", "\"Panel\"".into()),
        ("opacity", "1.0".into()),
        ("output", "All".into()),
        ("padding", "0".into()),
        ("plugins_center", format!("Some({})", ron_list(&[CLOCK]))),
        (
            "plugins_wings",
            format!(
                "Some(({}, {}))",
                ron_list(&[WORKSPACES, APP_LIBRARY]),
                ron_list(&TRAY)
            ),
        ),
        ("size", "XS".into()),
        ("size_center", "None".into()),
        ("size_wings", "None".into()),
        ("spacing", "0".into()),
    ])
}

/// COSMIC's shipped dock, key for key: the panel with these differences.
fn cosmic_dock() -> BTreeMap<&'static str, String> {
    let mut keys = cosmic_panel();
    for (key, value) in [
        ("anchor", "Bottom"),
        ("anchor_gap", "true"),
        ("border_radius", "160"),
        ("expand_to_edges", "false"),
        ("margin", "4"),
        ("name", "\"Dock\""),
        ("padding", "4"),
        ("plugins_wings", "None"),
        ("size", "L"),
    ] {
        keys.insert(key, value.to_string());
    }
    keys.insert(
        "plugins_center",
        format!(
            "Some({})",
            ron_list(&[LAUNCHER, WORKSPACES, APP_LIBRARY, APP_LIST, MINIMIZE])
        ),
    );
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn out(connector: Option<&str>, width: i32, height: i32) -> Output {
        Output {
            connector: connector.map(str::to_string),
            width,
            height,
        }
    }

    fn landscape() -> Vec<Output> {
        vec![out(Some("HDMI-A-1"), 1920, 1080)]
    }

    /// Whitespace carries no meaning in RON, and COSMIC's own files are not consistent
    /// about it; compare without it.
    fn squash(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn fixture_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/cosmic-panel-1.8.0")
    }

    fn fixture(entry: &str) -> BTreeMap<String, String> {
        let dir = fixture_dir().join(format!("com.system76.CosmicPanel.{entry}/v1"));
        fs::read_dir(dir)
            .expect("fixture directory")
            .map(|file| {
                let file = file.expect("fixture entry");
                let text = fs::read_to_string(file.path()).expect("fixture file");
                (
                    file.file_name().to_string_lossy().into_owned(),
                    squash(&text),
                )
            })
            .collect()
    }

    fn squashed(entry: &Entry) -> BTreeMap<String, String> {
        entry
            .keys
            .iter()
            .map(|(key, value)| (key.to_string(), squash(value)))
            .collect()
    }

    fn entry<'a>(plan: &'a Plan, name: &str) -> &'a Entry {
        plan.entries.iter().find(|e| e.name == name).expect("entry")
    }

    #[test]
    fn essential_at_the_top_is_cosmics_shipped_panel() {
        let plan = render(&Preset::Minimal.factory(), &landscape());
        assert_eq!(squashed(entry(&plan, "Panel")), fixture("Panel"));
        assert_eq!(plan.entries_value(), "[\"Panel\"]");
    }

    #[test]
    fn the_islands_dock_is_cosmics_shipped_dock() {
        let plan = render(&Preset::Float.factory(), &landscape());
        assert_eq!(squashed(entry(&plan, "Dock")), fixture("Dock"));
        let entries = fs::read_to_string(fixture_dir().join("com.system76.CosmicPanel/v1/entries"))
            .expect("entries");
        assert_eq!(plan.entries_value(), squash(&entries));
    }

    #[test]
    fn every_entry_of_every_layout_carries_all_22_keys() {
        let screens = [
            landscape(),
            vec![out(Some("DP-1"), 1080, 1920)],
            vec![
                out(Some("HDMI-A-1"), 1920, 1080),
                out(Some("DP-1"), 1080, 1920),
            ],
        ];
        for layout in Layout::all() {
            for outputs in &screens {
                for e in render(&layout, outputs).entries {
                    assert_eq!(e.keys.len(), 22, "{layout:?} {}", e.name);
                    assert_eq!(e.keys["name"], format!("\"{}\"", e.name));
                }
            }
        }
    }

    #[test]
    fn the_island_floats_its_panel() {
        let plan = render(&Preset::Float.factory(), &landscape());
        let panel = &entry(&plan, "Panel").keys;
        assert_eq!(
            (
                panel["anchor_gap"].as_str(),
                panel["margin"].as_str(),
                panel["border_radius"].as_str()
            ),
            ("true", "4", "12")
        );
        assert_eq!(panel["size"], "XS");
    }

    #[test]
    fn the_bar_holds_the_running_applications_and_ends_with_the_clock() {
        let plan = render(&Preset::Bar.factory(), &landscape());
        assert_eq!(plan.entries_value(), "[\"Panel\"]");
        let panel = &entry(&plan, "Panel").keys;
        assert_eq!(panel["anchor"], "Bottom");
        assert_eq!(panel["size"], "M");
        assert_eq!(panel["plugins_center"], "None");
        let wings = &panel["plugins_wings"];
        assert!(
            wings.starts_with(&format!(
                "Some(([\"{APP_LIBRARY}\", \"{APP_LIST}\", \"{MINIMIZE}\"]"
            )),
            "{wings}"
        );
        assert!(wings.ends_with(&format!("\"{CLOCK}\"]))")), "{wings}");
    }

    #[test]
    fn a_bottom_panel_moves_the_landscape_dock_left_and_stacks_the_portrait_dock_above_it() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(
            entry(&render(&layout, &landscape()), "Dock").keys["anchor"],
            "Left"
        );
        let portrait = render(&layout, &[out(Some("DP-1"), 1080, 1920)]);
        assert_eq!(entry(&portrait, "Dock").keys["anchor"], "Bottom");
        assert_eq!(entry(&portrait, "Panel").keys["anchor"], "Bottom");
    }

    #[test]
    fn auto_hide_hides_the_dock_and_gives_back_its_space() {
        let layout = Layout::new(Preset::Minimal, PanelEdge::Top, DockKnob::AutoHide);
        let plan = render(&layout, &landscape());
        let dock = &entry(&plan, "Dock").keys;
        assert_eq!(
            (dock["autohide"].as_str(), dock["exclusive_zone"].as_str()),
            ("Always", "false")
        );
    }

    #[test]
    fn outputs_of_both_shapes_get_one_dock_each() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        let plan = render(
            &layout,
            &[
                out(Some("HDMI-A-1"), 1920, 1080),
                out(Some("DP-1"), 1080, 1920),
            ],
        );
        assert_eq!(
            plan.entries_value(),
            "[\"Panel\",\"Dock-HDMI-A-1\",\"Dock-DP-1\"]"
        );
        let wide = &entry(&plan, "Dock-HDMI-A-1").keys;
        assert_eq!(
            (wide["output"].as_str(), wide["anchor"].as_str()),
            ("Name(\"HDMI-A-1\")", "Left")
        );
        let tall = &entry(&plan, "Dock-DP-1").keys;
        assert_eq!(
            (tall["output"].as_str(), tall["anchor"].as_str()),
            ("Name(\"DP-1\")", "Bottom")
        );
        assert!(plan.pins_outputs());
        assert_eq!(entry(&plan, "Panel").keys["output"], "All");
    }

    #[test]
    fn an_output_without_a_usable_name_makes_the_docks_shared() {
        let layout = Preset::Float.factory();
        for bad in [None, Some("../etc"), Some("")] {
            let plan = render(
                &layout,
                &[out(Some("HDMI-A-1"), 1920, 1080), out(bad, 1080, 1920)],
            );
            assert_eq!(plan.entries_value(), "[\"Panel\",\"Dock\"]", "{bad:?}");
            assert!(!plan.pins_outputs());
        }
    }

    #[test]
    fn an_output_without_geometry_yet_changes_nothing() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        let hot_plugged = render(
            &layout,
            &[out(Some("HDMI-A-1"), 1920, 1080), out(None, 0, 0)],
        );
        assert_eq!(hot_plugged, render(&layout, &landscape()));
        assert!(!hot_plugged.pins_outputs());
    }

    #[test]
    fn a_resolution_or_scale_change_that_keeps_the_shape_renders_the_same_plan() {
        let layout = Preset::Float.factory();
        let plan = render(&layout, &landscape());
        assert_eq!(render(&layout, &[out(Some("HDMI-A-1"), 2560, 1440)]), plan);
        assert_eq!(render(&layout, &[out(Some("HDMI-A-1"), 1280, 720)]), plan);
        assert_eq!(
            render(&layout, &[out(Some("HDMI-A-1"), 2560, 1440)]).to_record(),
            plan.to_record()
        );
    }

    #[test]
    fn different_plans_have_different_records() {
        let float = render(&Preset::Float.factory(), &landscape());
        let hidden = render(
            &Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide),
            &landscape(),
        );
        assert_ne!(float.to_record(), hidden.to_record());
    }
}
