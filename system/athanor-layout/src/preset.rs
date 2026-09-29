//! The three presets of doc_shell.md, SH7, and the two knobs. Identifiers are permanent
//! and English: they are written into users' documents. Display names are the chooser's.

/// A preset: a whole arrangement of panel and dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Preset {
    /// "Isola": a floating panel and COSMIC's dock.
    Float,
    /// "Barra": one bar holding the running applications; no dock.
    Bar,
    /// "Essenziale": COSMIC's own panel and no dock.
    Minimal,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Float, Preset::Bar, Preset::Minimal];

    pub fn id(self) -> &'static str {
        match self {
            Preset::Float => "float",
            Preset::Bar => "bar",
            Preset::Minimal => "minimal",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|preset| preset.id() == id)
    }

    /// Whether the preset has a dock knob at all. The bar holds the running applications
    /// itself, and the schema rejects a dock value under it.
    pub fn has_dock(self) -> bool {
        self != Preset::Bar
    }

    /// The preset with its factory knobs.
    pub fn factory(self) -> Layout {
        match self {
            Preset::Float => Layout::new(self, PanelEdge::Top, DockKnob::Visible),
            Preset::Bar => Layout::new(self, PanelEdge::Bottom, DockKnob::Off),
            Preset::Minimal => Layout::new(self, PanelEdge::Top, DockKnob::Off),
        }
    }
}

/// The edge the panel sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanelEdge {
    Top,
    Bottom,
}

impl PanelEdge {
    pub const ALL: [PanelEdge; 2] = [PanelEdge::Top, PanelEdge::Bottom];

    pub fn id(self) -> &'static str {
        match self {
            PanelEdge::Top => "top",
            PanelEdge::Bottom => "bottom",
        }
    }

    pub fn from_id(id: &str) -> Option<PanelEdge> {
        PanelEdge::ALL.into_iter().find(|edge| edge.id() == id)
    }
}

/// The dock knob.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockKnob {
    Visible,
    AutoHide,
    /// No dock. Its identifier is "none".
    Off,
}

impl DockKnob {
    pub const ALL: [DockKnob; 3] = [DockKnob::Visible, DockKnob::AutoHide, DockKnob::Off];

    pub fn id(self) -> &'static str {
        match self {
            DockKnob::Visible => "visible",
            DockKnob::AutoHide => "auto-hide",
            DockKnob::Off => "none",
        }
    }

    pub fn from_id(id: &str) -> Option<DockKnob> {
        DockKnob::ALL.into_iter().find(|knob| knob.id() == id)
    }
}

/// A complete layout: a preset and both knobs, always consistent with each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Layout {
    preset: Preset,
    panel: PanelEdge,
    dock: DockKnob,
}

impl Layout {
    /// A layout; under `bar` the dock is off whatever `dock` says.
    pub fn new(preset: Preset, panel: PanelEdge, dock: DockKnob) -> Layout {
        let dock = if preset.has_dock() {
            dock
        } else {
            DockKnob::Off
        };
        Layout {
            preset,
            panel,
            dock,
        }
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    pub fn panel(&self) -> PanelEdge {
        self.panel
    }

    pub fn dock(&self) -> DockKnob {
        self.dock
    }

    /// Every layout the presets and knobs allow: 14 (SH7).
    pub fn all() -> Vec<Layout> {
        let mut all = Vec::new();
        for preset in Preset::ALL {
            let docks: &[DockKnob] = if preset.has_dock() {
                &DockKnob::ALL
            } else {
                &[DockKnob::Off]
            };
            for panel in PanelEdge::ALL {
                for &dock in docks {
                    all.push(Layout::new(preset, panel, dock));
                }
            }
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn identifiers_are_the_permanent_english_ones_of_sh7() {
        let ids: Vec<_> = Preset::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["float", "bar", "minimal"]);
        assert_eq!(PanelEdge::ALL.map(PanelEdge::id), ["top", "bottom"]);
        assert_eq!(
            DockKnob::ALL.map(DockKnob::id),
            ["visible", "auto-hide", "none"]
        );
        for preset in Preset::ALL {
            assert_eq!(Preset::from_id(preset.id()), Some(preset));
        }
        assert_eq!(Preset::from_id("Float"), None);
        assert_eq!(DockKnob::from_id("hidden"), None);
    }

    #[test]
    fn factory_values_are_those_of_sh7() {
        let float = Preset::Float.factory();
        assert_eq!(
            (float.panel(), float.dock()),
            (PanelEdge::Top, DockKnob::Visible)
        );
        let bar = Preset::Bar.factory();
        assert_eq!(
            (bar.panel(), bar.dock()),
            (PanelEdge::Bottom, DockKnob::Off)
        );
        let minimal = Preset::Minimal.factory();
        assert_eq!(
            (minimal.panel(), minimal.dock()),
            (PanelEdge::Top, DockKnob::Off)
        );
    }

    #[test]
    fn the_bar_has_no_dock_whatever_is_asked() {
        let layout = Layout::new(Preset::Bar, PanelEdge::Top, DockKnob::Visible);
        assert_eq!(layout.dock(), DockKnob::Off);
        assert!(!Preset::Bar.has_dock());
    }

    #[test]
    fn there_are_fourteen_layouts() {
        let all = Layout::all();
        assert_eq!(all.len(), 14);
        let unique: BTreeSet<_> = all
            .iter()
            .map(|l| (l.preset(), l.panel(), l.dock()))
            .collect();
        assert_eq!(unique.len(), 14);
    }
}
