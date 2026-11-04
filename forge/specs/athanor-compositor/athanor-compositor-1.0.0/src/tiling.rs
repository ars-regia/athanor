//! The layout of a tiled workspace (doc_own_stack.md OS12): a binary tree of what it tiles, whose
//! tiles the area shares. The rules are cosmic-comp's, and the plan names them
//! (`docs/superpowers/plans/2026-10-31-compositor-tiling.md`, D1 to D4): a new tile splits the
//! one focused last along its longer side and takes the second half, an end gives its sibling the
//! whole space, and a gap of 8 stands around and between the tiles. Nothing is kept in pixels:
//! each layout starts from the area.

use smithay::utils::{Logical, Point, Rectangle};

/// On a side of a tile at the edge of the area, and between two tiles, half on each (D4).
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

#[derive(Clone)]
enum Node<T> {
    Tile(T),
    Split {
        along: Along,
        /// What the first child takes of the side.
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

    /// Moves to `at` the split nearest `window`'s `side`, the one after the window if `end` and
    /// the one before it if not: whether one did. `place` is this node's.
    fn move_split(
        &mut self,
        window: &T,
        side: Along,
        end: bool,
        at: i32,
        place: Rectangle<i32, Logical>,
    ) -> bool {
        let Self::Split {
            along,
            share,
            first,
            second,
        } = self
        else {
            return false;
        };
        let (one, other) = divided(place, *along, *share);
        let in_first = first.holds(window);
        let nearer = if in_first {
            first.move_split(window, side, end, at, one)
        } else {
            second.move_split(window, side, end, at, other)
        };
        if nearer || *along != side || in_first != end {
            return nearer;
        }
        let (start, room) = match along {
            Along::Width => (place.loc.x, place.size.w.max(2)),
            Along::Height => (place.loc.y, place.size.h.max(2)),
        };
        let part = at.saturating_sub(start);
        // Each part keeps room for a tile and its gaps, so that no tile leaves its place.
        let least = (2 * GAP + 1).min(room / 2);
        let part = part.clamp(least, room - least);
        *share = f64::from(part) / f64::from(room);
        true
    }

    fn swap(&mut self, one: &T, other: &T)
    where
        T: Clone,
    {
        match self {
            Self::Tile(tile) if tile == one => *tile = other.clone(),
            Self::Tile(tile) if tile == other => *tile = one.clone(),
            Self::Tile(_) => {}
            Self::Split { first, second, .. } => {
                first.swap(one, other);
                second.swap(one, other);
            }
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
        place: Rectangle<i32, Logical>,
        area: Rectangle<i32, Logical>,
        tiles: &mut Vec<(&'a T, Rectangle<i32, Logical>)>,
    ) {
        match self {
            Self::Tile(tile) => tiles.push((tile, gapped(place, area))),
            Self::Split {
                along,
                share,
                first,
                second,
            } => {
                let (one, other) = divided(place, *along, *share);
                first.lay_out(one, area, tiles);
                second.lay_out(other, area, tiles);
            }
        }
    }
}

/// `area` divided `along` a side, no pixel lost to rounding. Each part keeps a pixel however small
/// the area: a size of nothing would ask the client to choose.
fn divided(
    area: Rectangle<i32, Logical>,
    along: Along,
    share: f64,
) -> (Rectangle<i32, Logical>, Rectangle<i32, Logical>) {
    let part = |room: i32| ((f64::from(room) * share).round() as i32).clamp(1, room - 1);
    let (mut one, mut other) = (area, area);
    match along {
        Along::Width => {
            let room = area.size.w.max(2);
            one.size.w = part(room);
            other.size.w = room - one.size.w;
            other.loc.x += one.size.w;
        }
        Along::Height => {
            let room = area.size.h.max(2);
            one.size.h = part(room);
            other.size.h = room - one.size.h;
            other.loc.y += one.size.h;
        }
    }
    (one, other)
}

/// The tile a `place` of `area` leaves: `GAP` off a side at the edge of the area, half of it off a
/// side another tile shares. A tile of a nested split can so be a few pixels narrower than its
/// sibling, as in the reference, and it keeps a pixel however small.
fn gapped(
    place: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let gap = |edge: bool| if edge { GAP } else { GAP / 2 };
    let (left, top) = (
        gap(place.loc.x == area.loc.x),
        gap(place.loc.y == area.loc.y),
    );
    let right = gap(place.loc.x + place.size.w == area.loc.x + area.size.w);
    let bottom = gap(place.loc.y + place.size.h == area.loc.y + area.size.h);
    let size = (place.size.w - left - right, place.size.h - top - bottom);
    let size = (size.0.max(1), size.1.max(1));
    Rectangle::new(place.loc + Point::from((left, top)), size.into())
}

/// What a workspace tiles.
#[derive(Clone)]
pub struct Tree<T> {
    root: Option<Node<T>>,
    /// The windows of the tree, the one focused last at the end.
    focused: Vec<T>,
}

impl<T> Default for Tree<T> {
    fn default() -> Self {
        Self {
            root: None,
            focused: Vec::new(),
        }
    }
}

impl<T: PartialEq + Clone> Tree<T> {
    /// Adds `window` beside the window focused last (D2); one it holds stays where it is.
    pub fn insert(&mut self, window: T, area: Rectangle<i32, Logical>) {
        if self.holds(&window) {
            return;
        }
        let tiles = self.layout(area);
        let near = self.focused.last();
        let tile = tiles.iter().find(|(tile, _)| Some(*tile) == near);
        let beside = tile.map(|(tile, at)| ((*tile).clone(), Along::longer(*at)));
        self.focused.push(window.clone());
        self.root = Some(match (self.root.take(), beside) {
            (Some(root), Some((near, along))) => root.split_tile(&near, window, along),
            // A last focus that names no tile of the tree: the window splits the whole.
            (Some(root), None) => Node::split(root, Node::Tile(window), Along::longer(area)),
            (None, _) => Node::Tile(window),
        });
    }

    /// `insert` for a window without the keyboard: the next tile splits what the focus left.
    pub fn insert_behind(&mut self, window: T, area: Rectangle<i32, Logical>) {
        if self.holds(&window) {
            return;
        }
        self.insert(window, area);
        if let Some(window) = self.focused.pop() {
            self.focused.insert(self.focused.len().saturating_sub(1), window);
        }
    }

    /// The tile `insert` would give `window`, or the one it has, the tree staying as it is.
    pub fn preview(
        &self,
        window: &T,
        area: Rectangle<i32, Logical>,
    ) -> Option<Rectangle<i32, Logical>> {
        let mut after = self.clone();
        after.insert(window.clone(), area);
        let tiles = after.layout(area);
        let tile = tiles.iter().find(|(tile, _)| *tile == window);
        tile.map(|(_, at)| *at)
    }

    /// Takes `window` out; its sibling takes the space (D3). Another window changes nothing.
    pub fn remove(&mut self, window: &T) {
        self.root = self.root.take().and_then(|root| root.without(window));
        self.focused.retain(|kept| kept != window);
    }

    /// Gives `one` the tile of `other` and `other` that of `one` (D11); unless it holds both,
    /// nothing changes.
    pub fn swap(&mut self, one: &T, other: &T) {
        if one == other || !self.holds(one) || !self.holds(other) {
            return;
        }
        if let Some(root) = &mut self.root {
            root.swap(one, other);
        }
    }

    /// Moves the splits by the tile of `window` in `area` so that its sides go where `to` has
    /// them (D11): each side that moved moves the split nearest it, and a side at the edge of
    /// the area has none. `to` is the client's, and counts only as far as the area. Each split
    /// goes where the side of `to` puts it, half a gap beyond, whatever another moved first.
    pub fn resize(
        &mut self,
        window: &T,
        to: Rectangle<i32, Logical>,
        area: Rectangle<i32, Logical>,
    ) {
        let tiles = self.layout(area);
        let Some(tile) = tiles
            .into_iter()
            .find(|(kept, _)| *kept == window)
            .map(|(_, tile)| tile)
        else {
            return;
        };
        let Some(root) = &mut self.root else {
            return;
        };
        let within = |start: i32, size: i32, from: i32, room: i32| {
            let end = from.saturating_add(room.max(0));
            (
                start.clamp(from, end),
                start.saturating_add(size).clamp(from, end),
            )
        };
        let (left, right) = within(to.loc.x, to.size.w, area.loc.x, area.size.w);
        let (top, bottom) = within(to.loc.y, to.size.h, area.loc.y, area.size.h);
        let half = GAP / 2;
        let sides = [
            (Along::Width, false, tile.loc.x, left, left.saturating_sub(half)),
            (Along::Width, true, tile.loc.x + tile.size.w, right, right.saturating_add(half)),
            (Along::Height, false, tile.loc.y, top, top.saturating_sub(half)),
            (Along::Height, true, tile.loc.y + tile.size.h, bottom, bottom.saturating_add(half)),
        ];
        for (side, end, was, now, at) in sides {
            if was != now {
                root.move_split(window, side, end, at, area);
            }
        }
    }

    /// Notes that the focus went to `window`, which the next tile will split.
    pub fn focus(&mut self, window: &T) {
        if let Some(index) = self.focused.iter().position(|kept| kept == window) {
            let window = self.focused.remove(index);
            self.focused.push(window);
        }
    }

    /// The windows it holds, the one focused last at the end.
    pub fn windows(&self) -> &[T] {
        &self.focused
    }

    pub fn holds(&self, window: &T) -> bool {
        self.root.as_ref().is_some_and(|root| root.holds(window))
    }

    /// Each window with its tile in `area`, the gaps taken off (D4), in the order of the tree.
    pub fn layout(&self, area: Rectangle<i32, Logical>) -> Vec<(&T, Rectangle<i32, Logical>)> {
        let mut tiles = Vec::new();
        if let Some(root) = &self.root {
            root.lay_out(area, area, &mut tiles);
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
        tree.insert(1, area(1000, 600));
        assert_eq!(tiles(&tree, area(1000, 600)), [(1, rect(8, 8, 984, 584))]);
    }

    #[test]
    fn a_new_window_splits_the_longer_side() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        assert_eq!(
            tiles(&tree, whole),
            [(1, rect(8, 8, 496, 600)), (2, rect(512, 8, 496, 600))],
        );
        tree.insert(3, whole);
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
    fn a_preview_is_the_tile_insert_gives() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        assert_eq!(tree.preview(&1, whole), Some(rect(8, 8, 1000, 600)));
        tree.insert(1, whole);
        let before = tiles(&tree, whole);

        let preview = tree.preview(&2, whole);

        assert_eq!(tiles(&tree, whole), before, "the tree stays as it is");
        assert!(!tree.holds(&2));
        tree.insert(2, whole);
        assert_eq!(preview, Some(rect(512, 8, 496, 600)));
        assert_eq!(tree.preview(&2, whole), preview, "the tile it has");
        assert_eq!(tiles(&tree, whole)[1], (2, rect(512, 8, 496, 600)));
    }

    #[test]
    fn a_window_inserted_behind_leaves_the_split_where_it_was() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert_behind(2, whole);
        tree.insert(3, whole);
        assert_eq!(
            tiles(&tree, whole),
            [
                (1, rect(8, 8, 496, 296)),
                (3, rect(8, 312, 496, 296)),
                (2, rect(512, 8, 496, 600)),
            ],
        );

        let mut alone = Tree::default();
        alone.insert_behind(1, whole);
        assert_eq!(tiles(&alone, whole), [(1, rect(8, 8, 1000, 600))]);
        alone.insert_behind(1, whole);
        assert_eq!(tiles(&alone, whole).len(), 1, "a window it holds stays");
    }

    #[test]
    fn the_last_focused_tile_is_split() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        tree.focus(&1);
        tree.focus(&9);
        tree.insert(3, whole);
        assert_eq!(
            tiles(&tree, whole),
            [
                (1, rect(8, 8, 496, 296)),
                (3, rect(8, 312, 496, 296)),
                (2, rect(512, 8, 496, 600)),
            ],
        );
    }

    #[test]
    fn the_focus_goes_back_when_a_tile_ends() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        tree.insert(3, whole);
        tree.focus(&1);
        tree.remove(&1);
        tree.insert(4, whole);
        assert_eq!(
            tiles(&tree, whole),
            [
                (2, rect(8, 8, 1000, 296)),
                (3, rect(8, 312, 496, 296)),
                (4, rect(512, 312, 496, 296)),
            ],
        );
    }

    #[test]
    fn a_nested_split_takes_the_gaps_of_the_reference() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        for window in 1..=4 {
            tree.insert(window, whole);
        }
        assert_eq!(
            tiles(&tree, whole),
            [
                (1, rect(8, 8, 496, 600)),
                (2, rect(512, 8, 496, 296)),
                (3, rect(512, 312, 246, 296)),
                (4, rect(766, 312, 242, 296)),
            ],
        );
    }

    #[test]
    fn an_end_gives_the_sibling_its_space() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        tree.insert(3, whole);
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
        tree.insert(1, whole);
        tree.insert(2, whole);
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
        tree.insert(1, whole);
        tree.insert(2, whole);
        let shown = tiles(&tree, whole);
        let (one, other) = (shown[0].1, shown[1].1);
        assert_eq!(one.size.w + 8 + other.size.w, 1017 - 16);
    }

    #[test]
    fn a_small_area_leaves_every_tile_a_size() {
        let whole = area(20, 20);
        let mut tree = Tree::default();
        for window in 0..6 {
            tree.insert(window, whole);
        }
        for (_, tile) in tiles(&tree, whole) {
            assert!(tile.size.w >= 1 && tile.size.h >= 1, "{tile:?}");
        }
    }

    #[test]
    fn a_window_it_does_not_hold_changes_nothing() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        let before = tiles(&tree, whole);
        tree.remove(&7);
        assert_eq!(tiles(&tree, whole), before);
        assert!(tree.holds(&2));
        assert!(!tree.holds(&7));
    }

    /// Should the last focus name no tile of the tree, the new window splits the whole and no
    /// tile is lost.
    #[test]
    fn a_focus_out_of_the_tree_loses_no_tile() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        tree.focused.push(7);
        tree.insert(3, whole);
        assert!(tree.holds(&1) && tree.holds(&2) && tree.holds(&3));
        assert_eq!(tiles(&tree, whole).len(), 3);
    }

    #[test]
    fn a_window_it_holds_is_not_added_twice() {
        let whole = area(1016, 616);
        let mut tree = Tree::default();
        tree.insert(1, whole);
        tree.insert(2, whole);
        let before = tiles(&tree, whole);
        tree.insert(1, whole);
        assert_eq!(tiles(&tree, whole), before);
    }
}
