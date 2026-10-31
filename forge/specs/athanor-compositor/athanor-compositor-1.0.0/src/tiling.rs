//! The layout of a tiled workspace (doc_own_stack.md OS12): a binary tree of what it tiles, whose
//! tiles the area shares. The rules are cosmic-comp's, and the plan names them
//! (`docs/superpowers/plans/2026-10-31-compositor-tiling.md`, D1 to D4): a new tile splits the
//! focused one along its longer side and takes the second half, an end gives its sibling the
//! whole space, and a gap of 8 stands around and between the tiles. Nothing is kept in pixels:
//! each layout starts from the area.

use smithay::utils::{Logical, Rectangle};

/// Around the tiled area and between two tiles (D4).
const GAP: i32 = 8;

/// The side a split divides.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Along {
    Width,
    Height,
}

impl Along {
    /// The longer side of `area`: a tile as wide as it is high divides its height, as the
    /// reference does.
    fn longer(area: Rectangle<i32, Logical>) -> Self {
        if area.size.w > area.size.h {
            Self::Width
        } else {
            Self::Height
        }
    }
}

enum Node<T> {
    Tile(T),
    Split {
        along: Along,
        /// What the first child takes of what the gap leaves.
        share: f64,
        first: Box<Node<T>>,
        second: Box<Node<T>>,
    },
}

impl<T: PartialEq> Node<T> {
    fn split(first: Self, second: Self, along: Along) -> Self {
        Self::Split {
            along,
            share: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// This node without the tile of `window`; none if that was all of it. Its sibling takes the
    /// place of a split it leaves.
    fn without(self, window: &T) -> Option<Self> {
        match self {
            Self::Tile(tile) => (tile != *window).then_some(Self::Tile(tile)),
            Self::Split {
                along,
                share,
                first,
                second,
            } => match (first.without(window), second.without(window)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    along,
                    share,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (kept, None) | (None, kept) => kept,
            },
        }
    }

    /// Splits the tile of `near` for `window`, which `along` is asked of by the tile's area.
    fn split_tile(self, near: &T, window: T, along: Along) -> Self {
        match self {
            Self::Tile(tile) if tile == *near => {
                Self::split(Self::Tile(tile), Self::Tile(window), along)
            }
            Self::Tile(tile) => Self::Tile(tile),
            Self::Split {
                along: own,
                share,
                first,
                second,
            } if first.holds(near) => Self::Split {
                along: own,
                share,
                first: Box::new(first.split_tile(near, window, along)),
                second,
            },
            Self::Split {
                along: own,
                share,
                first,
                second,
            } => Self::Split {
                along: own,
                share,
                first,
                second: Box::new(second.split_tile(near, window, along)),
            },
        }
    }

    fn holds(&self, window: &T) -> bool {
        match self {
            Self::Tile(tile) => tile == window,
            Self::Split { first, second, .. } => first.holds(window) || second.holds(window),
        }
    }

    fn lay_out<'a>(
        &'a self,
        area: Rectangle<i32, Logical>,
        tiles: &mut Vec<(&'a T, Rectangle<i32, Logical>)>,
    ) {
        match self {
            Self::Tile(tile) => tiles.push((tile, area)),
            Self::Split {
                along,
                share,
                first,
                second,
            } => {
                let (one, other) = divided(area, *along, *share);
                first.lay_out(one, tiles);
                second.lay_out(other, tiles);
            }
        }
    }
}

/// `area` divided `along` a side, the gap between the two, no pixel lost to rounding. Each part
/// keeps a pixel however small the area: a size of nothing would ask the client to choose.
fn divided(
    area: Rectangle<i32, Logical>,
    along: Along,
    share: f64,
) -> (Rectangle<i32, Logical>, Rectangle<i32, Logical>) {
    let part = |room: i32| ((f64::from(room) * share).round() as i32).clamp(1, room - 1);
    let (mut one, mut other) = (area, area);
    match along {
        Along::Width => {
            let room = (area.size.w - GAP).max(2);
            one.size.w = part(room);
            other.size.w = room - one.size.w;
            other.loc.x += one.size.w + GAP;
        }
        Along::Height => {
            let room = (area.size.h - GAP).max(2);
            one.size.h = part(room);
            other.size.h = room - one.size.h;
            other.loc.y += one.size.h + GAP;
        }
    }
    (one, other)
}

/// The area with the gap taken off every side.
fn inset(mut area: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    area.loc += (GAP, GAP).into();
    area.size = (
        (area.size.w - GAP * 2).max(1),
        (area.size.h - GAP * 2).max(1),
    )
        .into();
    area
}

/// What a workspace tiles.
pub struct Tree<T> {
    root: Option<Node<T>>,
}

impl<T> Default for Tree<T> {
    fn default() -> Self {
        Self { root: None }
    }
}

impl<T: PartialEq> Tree<T> {
    /// Adds `window` beside `near`, the focused window (D2). With no such tile it splits the
    /// root along the longer side of `area`.
    pub fn insert(&mut self, window: T, near: Option<&T>, area: Rectangle<i32, Logical>) {
        let beside = near.and_then(|near| {
            let tiles = self.layout(area);
            let (_, tile) = tiles.into_iter().find(|(tile, _)| *tile == near)?;
            Some((near, Along::longer(tile)))
        });
        self.root = Some(match (self.root.take(), beside) {
            (None, _) => Node::Tile(window),
            (Some(root), Some((near, along))) => root.split_tile(near, window, along),
            (Some(root), None) => Node::split(root, Node::Tile(window), Along::longer(area)),
        });
    }

    /// Takes `window` out; its sibling takes the space (D3). Another window changes nothing.
    pub fn remove(&mut self, window: &T) {
        self.root = self.root.take().and_then(|root| root.without(window));
    }

    pub fn holds(&self, window: &T) -> bool {
        self.root.as_ref().is_some_and(|root| root.holds(window))
    }

    /// Each window with its tile in `area`, the gaps taken off (D4), in the order of the tree.
    pub fn layout(&self, area: Rectangle<i32, Logical>) -> Vec<(&T, Rectangle<i32, Logical>)> {
        let mut tiles = Vec::new();
        if let Some(root) = &self.root {
            root.lay_out(inset(area), &mut tiles);
        }
        tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (w, h).into())
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn tiles(tree: &Tree<u8>, area: Rectangle<i32, Logical>) -> Vec<(u8, Rectangle<i32, Logical>)> {
        let shown = tree.layout(area);
        shown.into_iter().map(|(tile, at)| (*tile, at)).collect()
    }

    #[test]
    fn a_first_window_fills_the_area() {
        let mut tree = Tree::default();
        tree.insert(1, None, area(1000, 600));
        assert_eq!(tiles(&tree, area(1000, 600)), [(1, rect(8, 8, 984, 584))]);
    }

    #[test]
    fn a_new_window_splits_the_longer_side() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, None, whole);
        tree.insert(2, Some(&1), whole);
        assert_eq!(
            tiles(&tree, whole),
            [(1, rect(8, 8, 496, 600)), (2, rect(512, 8, 496, 600))],
        );
        tree.insert(3, Some(&2), whole);
        assert_eq!(
            tiles(&tree, whole),
            [
                (1, rect(8, 8, 496, 600)),
                (2, rect(512, 8, 496, 296)),
                (3, rect(512, 312, 496, 296)),
            ],
        );
    }

    #[test]
    fn no_focus_splits_the_root() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, None, whole);
        tree.insert(2, Some(&1), whole);
        tree.insert(3, Some(&9), whole);
        assert_eq!(
            tiles(&tree, whole),
            [
                (1, rect(8, 8, 244, 600)),
                (2, rect(260, 8, 244, 600)),
                (3, rect(512, 8, 496, 600)),
            ],
        );
    }

    #[test]
    fn an_end_gives_the_sibling_its_space() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, None, whole);
        tree.insert(2, Some(&1), whole);
        tree.insert(3, Some(&2), whole);
        tree.remove(&2);
        assert_eq!(
            tiles(&tree, whole),
            [(1, rect(8, 8, 496, 600)), (3, rect(512, 8, 496, 600))],
        );
        tree.remove(&1);
        tree.remove(&3);
        assert!(tiles(&tree, whole).is_empty());
    }

    #[test]
    fn the_gaps_are_even() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, None, whole);
        tree.insert(2, Some(&1), whole);
        let shown = tiles(&tree, whole);
        let (one, other) = (shown[0].1, shown[1].1);
        assert_eq!(one.loc.x, 8);
        assert_eq!(other.loc.x - (one.loc.x + one.size.w), 8);
        assert_eq!(1016 - (other.loc.x + other.size.w), 8);
    }

    #[test]
    fn no_pixel_is_lost_to_rounding() {
        let whole = area(1017, 601);
        let mut tree = Tree::default();
        tree.insert(1, None, whole);
        tree.insert(2, Some(&1), whole);
        let shown = tiles(&tree, whole);
        let (one, other) = (shown[0].1, shown[1].1);
        assert_eq!(one.size.w + 8 + other.size.w, 1017 - 16);
    }

    #[test]
    fn a_small_area_leaves_every_tile_a_size() {
        let whole = area(20, 20);
        let mut tree = Tree::default();
        tree.insert(0, None, whole);
        for window in 1..6 {
            tree.insert(window, Some(&(window - 1)), whole);
        }
        for (_, tile) in tiles(&tree, whole) {
            assert!(tile.size.w >= 1 && tile.size.h >= 1, "{tile:?}");
        }
    }

    #[test]
    fn a_window_it_does_not_hold_changes_nothing() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, None, whole);
        tree.insert(2, Some(&1), whole);
        let before = tiles(&tree, whole);
        tree.remove(&7);
        assert_eq!(tiles(&tree, whole), before);
        assert!(tree.holds(&2));
        assert!(!tree.holds(&7));
    }
}
