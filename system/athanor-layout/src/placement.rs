//! Where the dock goes (doc_shell.md, SH7): "On an output wider than tall the dock sits
//! on the bottom edge, and on the left edge when the panel is at the bottom. On an output
//! taller than wide ... it sits on the bottom edge, stacked above the panel."

use crate::preset::PanelEdge;

/// One output as the session reports it, in logical pixels. A hot-plugged output can be
/// reported before its geometry or its connector is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub connector: Option<String>,
    pub width: i32,
    pub height: i32,
}

impl Output {
    /// Whether the output has a real size yet. An unsized output is left out of every
    /// decision until it has one.
    pub fn is_sized(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Taller than wide is portrait; a square output counts as landscape.
    pub fn shape(&self) -> Shape {
        if self.height > self.width {
            Shape::Portrait
        } else {
            Shape::Landscape
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    Landscape,
    Portrait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEdge {
    Bottom,
    Left,
}

pub fn dock_edge(panel: PanelEdge, shape: Shape) -> DockEdge {
    match (shape, panel) {
        (Shape::Landscape, PanelEdge::Bottom) => DockEdge::Left,
        _ => DockEdge::Bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(width: i32, height: i32) -> Output {
        Output {
            connector: Some("HDMI-A-1".into()),
            width,
            height,
        }
    }

    #[test]
    fn the_dock_moves_left_only_on_a_landscape_output_with_the_panel_below() {
        assert_eq!(
            dock_edge(PanelEdge::Top, Shape::Landscape),
            DockEdge::Bottom
        );
        assert_eq!(
            dock_edge(PanelEdge::Bottom, Shape::Landscape),
            DockEdge::Left
        );
        assert_eq!(dock_edge(PanelEdge::Top, Shape::Portrait), DockEdge::Bottom);
        assert_eq!(
            dock_edge(PanelEdge::Bottom, Shape::Portrait),
            DockEdge::Bottom
        );
    }

    #[test]
    fn a_square_output_counts_as_landscape() {
        assert_eq!(output(1920, 1080).shape(), Shape::Landscape);
        assert_eq!(output(1080, 1920).shape(), Shape::Portrait);
        assert_eq!(output(1200, 1200).shape(), Shape::Landscape);
    }

    #[test]
    fn an_output_without_geometry_yet_is_not_sized() {
        assert!(!Output {
            connector: None,
            width: 0,
            height: 0
        }
        .is_sized());
        assert!(output(1920, 1080).is_sized());
    }
}
