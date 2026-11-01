//! Where the toolkit dock's surface sits for a placement, as `ui/surface.rs` puts the GTK
//! dock's: the layer surface's anchors and zone, the island's margin from Calmo's dock
//! rules, and the sides its menus and indicators take.

use athanor_dock::placement::{Anchor, Placement};
use athanor_style::dock;
use athanor_toolkit::driver::{Anchor as Edges, KeyboardInteractivity, Layer, LayerOptions, Side};

/// The layer shell namespace cosmic-comp and the rig know the dock by.
pub const NAMESPACE: &str = "athanor-dock";

/// The layer surface of `placement`, `depth` logical pixels deep across its edge. An output
/// without a dock keeps an empty surface that takes no input: cosmic-comp closes the
/// connection of a client that unmaps a layer surface and maps it again (`ui/surface.rs`).
pub fn layer(placement: Option<Placement>, depth: i32) -> LayerOptions {
    let mut options = LayerOptions {
        layer: Layer::Top,
        namespace: NAMESPACE.to_owned(),
        anchor: Edges::BOTTOM,
        margin: [0; 4],
        exclusive_zone: 0,
        keyboard: KeyboardInteractivity::OnDemand,
        input_region: Some(Vec::new()),
    };
    let Some(placement) = placement else {
        return options;
    };
    let (edge, across) = match placement.anchor {
        Anchor::Bottom => (Edges::BOTTOM, Edges::LEFT | Edges::RIGHT),
        Anchor::Left => (Edges::LEFT, Edges::TOP | Edges::BOTTOM),
        Anchor::Right => (Edges::RIGHT, Edges::TOP | Edges::BOTTOM),
    };
    // Under auto-hide the surface spans its whole edge for its whole life, so the strip is
    // found anywhere along it and nothing is resized when it hides or shows; its input
    // region is set by the hider.
    if placement.auto_hide {
        options.anchor = edge | across;
    } else {
        options.anchor = edge;
        options.exclusive_zone = depth;
        options.input_region = None;
    }
    options
}

/// The island's margin on `anchor`: top, right, bottom, left, as Calmo's dock rules.
pub fn margin(anchor: Anchor) -> [f32; 4] {
    let edge = dock::MARGIN_EDGE as f32;
    let inner = dock::MARGIN_INNER as f32;
    match anchor {
        Anchor::Bottom => {
            let along = dock::MARGIN_ALONG_BOTTOM as f32;
            [inner, along, edge, along]
        }
        Anchor::Left => {
            let along = dock::MARGIN_ALONG_SIDE as f32;
            [along, inner, along, edge]
        }
        Anchor::Right => {
            let along = dock::MARGIN_ALONG_SIDE as f32;
            [along, edge, along, inner]
        }
    }
}

/// The side an entry's menu opens on: towards the inside of the screen.
pub fn menu_side(anchor: Anchor) -> Side {
    match anchor {
        Anchor::Bottom => Side::Top,
        Anchor::Left => Side::Right,
        Anchor::Right => Side::Left,
    }
}

/// The side of a button its running indicator lines: the dock's edge.
pub fn indicator_side(anchor: Anchor) -> Side {
    match anchor {
        Anchor::Bottom => Side::Bottom,
        Anchor::Left => Side::Left,
        Anchor::Right => Side::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(anchor: Anchor, auto_hide: bool) -> Option<Placement> {
        Some(Placement { anchor, auto_hide })
    }

    #[test]
    fn a_dock_that_stays_shown_sits_on_its_edge_and_reserves_its_depth() {
        let options = layer(placed(Anchor::Bottom, false), 60);
        assert_eq!(options.anchor, Edges::BOTTOM);
        assert_eq!(options.exclusive_zone, 60);
        assert_eq!(options.namespace, NAMESPACE);
        assert_eq!(options.layer, Layer::Top);
        assert_eq!(options.keyboard, KeyboardInteractivity::OnDemand);
        assert_eq!(options.input_region, None);
    }

    #[test]
    fn an_auto_hiding_dock_spans_its_edge_and_reserves_nothing() {
        let options = layer(placed(Anchor::Left, true), 76);
        assert_eq!(options.anchor, Edges::LEFT | Edges::TOP | Edges::BOTTOM);
        assert_eq!(options.exclusive_zone, 0);
        let options = layer(placed(Anchor::Bottom, true), 60);
        assert_eq!(options.anchor, Edges::BOTTOM | Edges::LEFT | Edges::RIGHT);
    }

    #[test]
    fn an_output_without_a_dock_keeps_a_surface_that_takes_no_input() {
        let options = layer(None, 0);
        assert_eq!(options.exclusive_zone, 0);
        assert_eq!(options.input_region, Some(Vec::new()));
    }

    #[test]
    fn the_island_keeps_its_shadow_room_and_its_distance_from_the_edge() {
        // Calmo's dock rules: 6px from the edge, 14px inside, 18px along a bottom dock and
        // 24px along a vertical one.
        assert_eq!(margin(Anchor::Bottom), [14.0, 18.0, 6.0, 18.0]);
        assert_eq!(margin(Anchor::Left), [24.0, 14.0, 24.0, 6.0]);
        assert_eq!(margin(Anchor::Right), [24.0, 6.0, 24.0, 14.0]);
    }

    #[test]
    fn menus_open_inwards_and_indicators_line_the_edge() {
        assert_eq!(menu_side(Anchor::Bottom), Side::Top);
        assert_eq!(menu_side(Anchor::Left), Side::Right);
        assert_eq!(menu_side(Anchor::Right), Side::Left);
        assert_eq!(indicator_side(Anchor::Bottom), Side::Bottom);
        assert_eq!(indicator_side(Anchor::Left), Side::Left);
        assert_eq!(indicator_side(Anchor::Right), Side::Right);
    }
}
