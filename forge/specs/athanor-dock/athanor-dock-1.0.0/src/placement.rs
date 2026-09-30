//! Whether an output gets a dock surface, on which edge, and whether it hides
//! (doc_bar.md, BR7; doc_shell.md, SH7).

use athanor_layout::placement::{dock_edge, DockEdge, Output};
use athanor_layout::preset::{DockKnob, Layout};

/// The screen edge a dock surface is anchored to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Bottom,
    Left,
    Right,
}

impl Anchor {
    /// A vertical dock carries icons only (SH9.3).
    pub fn vertical(self) -> bool {
        self != Anchor::Bottom
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub anchor: Anchor,
    /// Auto-hide: no exclusive zone, and a strip on the edge until the pointer reaches it.
    pub auto_hide: bool,
}

/// `None` when the output gets no dock surface: the knob is `none` (always under `bar`),
/// or the output has no size yet (a hot-plugged output reported before its geometry).
pub fn place(layout: Layout, output: &Output, rtl: bool) -> Option<Placement> {
    let auto_hide = match layout.dock() {
        DockKnob::Off => return None,
        DockKnob::Visible => false,
        DockKnob::AutoHide => true,
    };
    if !output.is_sized() {
        return None;
    }
    let anchor = match dock_edge(layout.panel(), output.shape()) {
        DockEdge::Bottom => Anchor::Bottom,
        // The left edge of SH7 is the start edge: the right one under right-to-left text.
        DockEdge::Left if rtl => Anchor::Right,
        DockEdge::Left => Anchor::Left,
    };
    Some(Placement { anchor, auto_hide })
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_layout::preset::{PanelEdge, Preset};

    fn output(width: i32, height: i32) -> Output {
        Output {
            connector: Some("HDMI-A-1".into()),
            width,
            height,
        }
    }

    const LANDSCAPE: (i32, i32) = (1920, 1080);
    const PORTRAIT: (i32, i32) = (1080, 1920);

    fn at(layout: Layout, (width, height): (i32, i32), rtl: bool) -> Option<Placement> {
        place(layout, &output(width, height), rtl)
    }

    #[test]
    fn a_landscape_output_under_a_top_panel_gets_a_bottom_dock() {
        assert_eq!(
            at(Preset::Float.factory(), LANDSCAPE, false),
            Some(Placement {
                anchor: Anchor::Bottom,
                auto_hide: false
            })
        );
    }

    #[test]
    fn a_bottom_panel_puts_the_dock_on_the_left_edge() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(
            at(layout, LANDSCAPE, false).map(|p| p.anchor),
            Some(Anchor::Left)
        );
    }

    #[test]
    fn a_vertical_dock_mirrors_to_the_right_under_rtl() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(
            at(layout, LANDSCAPE, true).map(|p| p.anchor),
            Some(Anchor::Right)
        );
        // A horizontal dock has no start edge to mirror.
        assert_eq!(
            at(Preset::Float.factory(), LANDSCAPE, true).map(|p| p.anchor),
            Some(Anchor::Bottom)
        );
    }

    #[test]
    fn rotation_moves_the_dock_to_the_bottom() {
        let layout = Layout::new(Preset::Float, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(
            at(layout, PORTRAIT, false).map(|p| p.anchor),
            Some(Anchor::Bottom)
        );
        assert_eq!(
            at(layout, PORTRAIT, true).map(|p| p.anchor),
            Some(Anchor::Bottom)
        );
    }

    #[test]
    fn the_bar_preset_and_the_none_knob_get_no_dock() {
        assert_eq!(at(Preset::Bar.factory(), LANDSCAPE, false), None);
        // `bar` forces the knob off, whatever the document says.
        let bar = Layout::new(Preset::Bar, PanelEdge::Bottom, DockKnob::Visible);
        assert_eq!(at(bar, LANDSCAPE, false), None);
        let none = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::Off);
        assert_eq!(at(none, LANDSCAPE, false), None);
        assert_eq!(at(Preset::Minimal.factory(), LANDSCAPE, false), None);
    }

    #[test]
    fn minimal_with_the_dock_on_gets_one() {
        let layout = Layout::new(Preset::Minimal, PanelEdge::Top, DockKnob::Visible);
        assert_eq!(
            at(layout, LANDSCAPE, false).map(|p| p.anchor),
            Some(Anchor::Bottom)
        );
    }

    #[test]
    fn an_output_not_sized_yet_gets_no_dock() {
        let layout = Preset::Float.factory();
        assert_eq!(at(layout, (0, 0), false), None);
        assert_eq!(at(layout, (1920, 0), false), None);
        assert_eq!(at(layout, (-1, 1080), false), None);
    }

    #[test]
    fn auto_hide_is_carried() {
        let layout = Layout::new(Preset::Float, PanelEdge::Top, DockKnob::AutoHide);
        assert_eq!(
            at(layout, LANDSCAPE, false),
            Some(Placement {
                anchor: Anchor::Bottom,
                auto_hide: true
            })
        );
    }

    #[test]
    fn only_the_side_edges_are_vertical() {
        assert!(!Anchor::Bottom.vertical());
        assert!(Anchor::Left.vertical());
        assert!(Anchor::Right.vertical());
    }
}
