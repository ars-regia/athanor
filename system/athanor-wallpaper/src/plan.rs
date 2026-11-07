//! What the daemon asks of the decoder: each output's buffer size (SD6: the output's mode at
//! its scale) and the requests that cover every (image, size) not yet decoded.

use crate::protocol::{MAX_SIDE, MAX_SIZES};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The buffer of an output, in physical pixels: its current `mode`, turned to the orientation
/// of its `logical` size when the two differ (a rotated output), or `logical` times its integer
/// `scale` when it reports no mode. A side over [`MAX_SIDE`] shrinks both sides by one factor:
/// Sal stretches that picture over the rest. `None` when nothing gives a positive size.
pub fn buffer_size(
    mode: Option<(i32, i32)>,
    logical: Option<(i32, i32)>,
    scale: i32,
) -> Option<(u32, u32)> {
    let positive = |(w, h): (i32, i32)| Some((u32::try_from(w).ok()?, u32::try_from(h).ok()?));
    let (w, h) = match (mode.and_then(positive), logical.and_then(positive)) {
        (Some((w, h)), Some((lw, lh))) if (w > h) != (lw > lh) && w != h && lw != lh => (h, w),
        (Some(size), _) => size,
        (None, Some((lw, lh))) => {
            let scale = u32::try_from(scale).ok().filter(|s| *s > 0).unwrap_or(1);
            (lw.saturating_mul(scale), lh.saturating_mul(scale))
        }
        (None, None) => return None,
    };
    if w == 0 || h == 0 {
        return None;
    }
    let long = w.max(h);
    if long <= MAX_SIDE {
        return Some((w, h));
    }
    let shrink = |v: u32| ((u64::from(v) * u64::from(MAX_SIDE) / u64::from(long)) as u32).max(1);
    Some((shrink(w), shrink(h)))
}

/// The requests for `wants`, (image, size) pairs: one image per request, its sizes once each
/// and at most [`MAX_SIZES`] of them, so an image shown at more sizes takes more requests.
pub fn requests(
    wants: impl IntoIterator<Item = (PathBuf, (u32, u32))>,
) -> Vec<(PathBuf, Vec<(u32, u32)>)> {
    let mut by_path: BTreeMap<PathBuf, BTreeSet<(u32, u32)>> = BTreeMap::new();
    for (path, size) in wants {
        by_path.entry(path).or_default().insert(size);
    }
    let mut out = Vec::new();
    for (path, sizes) in by_path {
        let sizes: Vec<_> = sizes.into_iter().collect();
        for chunk in sizes.chunks(MAX_SIZES) {
            out.push((path.clone(), chunk.to_vec()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_is_the_mode_turned_with_the_output() {
        assert_eq!(
            buffer_size(Some((3840, 2160)), Some((2560, 1440)), 1),
            Some((3840, 2160))
        );
        assert_eq!(
            buffer_size(Some((3840, 2160)), Some((1440, 2560)), 1),
            Some((2160, 3840))
        );
        assert_eq!(buffer_size(Some((1920, 1080)), None, 2), Some((1920, 1080)));
        assert_eq!(buffer_size(None, Some((1280, 800)), 2), Some((2560, 1600)));
        assert_eq!(buffer_size(None, Some((1280, 800)), 0), Some((1280, 800)));
        assert_eq!(buffer_size(None, None, 1), None);
        assert_eq!(buffer_size(Some((0, 1080)), None, 1), None);
        assert_eq!(buffer_size(Some((-1, 1080)), None, 1), None);
    }

    #[test]
    fn a_buffer_over_the_largest_side_keeps_its_aspect() {
        assert_eq!(
            buffer_size(Some((15360, 8640)), None, 1),
            Some((MAX_SIDE, 4608))
        );
    }

    #[test]
    fn requests_group_sizes_by_image() {
        let (a, b) = (PathBuf::from("/a"), PathBuf::from("/b"));
        let wants = [
            (a.clone(), (1, 1)),
            (b.clone(), (2, 2)),
            (a.clone(), (1, 1)),
            (a.clone(), (3, 3)),
        ];
        assert_eq!(
            requests(wants),
            vec![(a, vec![(1, 1), (3, 3)]), (b, vec![(2, 2)])]
        );
        let many = (1..=MAX_SIZES as u32 + 1).map(|i| (PathBuf::from("/a"), (i, i)));
        let split = requests(many);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].1.len(), MAX_SIZES);
        assert_eq!(split[1].1, vec![(9, 9)]);
    }
}
