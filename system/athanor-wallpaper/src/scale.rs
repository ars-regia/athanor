//! The pixels an output shows (SD6): the decoded image cropped with [`zoom`] and resampled once
//! to the output's buffer size, so that Sal draws them one to one. The filter is a triangle
//! that widens with the reduction, so that every source pixel counts when the image shrinks
//! and two neighbours blend when it grows. It works on straight RGBA, which is exact for the
//! opaque images a wallpaper is; a translucent edge may darken by a fraction.

use crate::zoom;
use std::io::{self, Write};

/// The source pixels one output pixel reads along an axis, and their weights, summing to 1.
struct Taps {
    start: usize,
    weights: Vec<f32>,
}

/// The taps of each of `dst` output pixels over `src` source pixels starting at `offset`.
fn taps(offset: u32, src: u32, dst: u32) -> Vec<Taps> {
    let scale = f64::from(src) / f64::from(dst);
    let radius = scale.max(1.0);
    (0..dst)
        .map(|i| {
            let center = (f64::from(i) + 0.5) * scale;
            let lo = (center - radius).floor().max(0.0) as u32;
            let hi = ((center + radius).ceil() as u32).min(src);
            let mut weights: Vec<f32> = (lo..hi)
                .map(|j| (1.0 - ((f64::from(j) + 0.5 - center) / radius).abs()).max(0.0) as f32)
                .collect();
            let sum: f32 = weights.iter().sum();
            if sum > 0.0 {
                weights.iter_mut().for_each(|w| *w /= sum);
            } else {
                // Unreachable for src and dst of at least 1: the centre's own pixel weighs.
                weights = vec![1.0];
            }
            Taps {
                start: (offset + lo) as usize,
                weights,
            }
        })
        .collect()
}

/// Writes `image` (width, height) of straight RGBA8 `pixels`, rows `stride` bytes apart, scaled
/// with zoom to `size`, to `out` as rows of `size.0` pixels packed, one row at a time: the
/// decoder holds the decoded frame and one row of the result, never the whole result.
/// `InvalidInput` when a side is 0 or the buffer is shorter than its size says.
pub fn write_zoomed(
    pixels: &[u8],
    stride: usize,
    image: (u32, u32),
    size: (u32, u32),
    out: &mut impl Write,
) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "no picture of that size");
    let crop = zoom(image, size).ok_or_else(invalid)?;
    let row = image.0 as usize * 4;
    if stride < row || pixels.len() < stride * (image.1 as usize - 1) + row {
        return Err(invalid());
    }
    let columns = taps(crop.x, crop.width, size.0);
    let rows = taps(crop.y, crop.height, size.1);
    let first = crop.x as usize;
    let mut line = vec![0f32; crop.width as usize * 4];
    let mut scaled = Vec::with_capacity(size.0 as usize * 4);
    for tap in &rows {
        line.iter_mut().for_each(|v| *v = 0.0);
        for (k, &w) in tap.weights.iter().enumerate() {
            let at = (tap.start + k) * stride + first * 4;
            let source = &pixels[at..at + line.len()];
            for (v, &p) in line.iter_mut().zip(source) {
                *v += w * f32::from(p);
            }
        }
        scaled.clear();
        for tap in &columns {
            let mut px = [0f32; 4];
            for (k, &w) in tap.weights.iter().enumerate() {
                let at = (tap.start - first + k) * 4;
                for (c, v) in px.iter_mut().enumerate() {
                    *v += w * line[at + c];
                }
            }
            scaled.extend(px.map(|v| v.round().clamp(0.0, 255.0) as u8));
        }
        out.write_all(&scaled)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zoomed(
        pixels: &[u8],
        stride: usize,
        image: (u32, u32),
        size: (u32, u32),
    ) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        write_zoomed(pixels, stride, image, size, &mut out).ok()?;
        Some(out)
    }

    fn image(pixels: &[[u8; 4]]) -> Vec<u8> {
        pixels.concat()
    }

    #[test]
    fn a_plain_colour_stays_plain_at_any_size() {
        let px = [0x81, 0x3d, 0x9c, 255];
        let src = image(&[px; 6 * 4]);
        for size in [(1, 1), (3, 2), (12, 8), (7, 13)] {
            let out = zoomed(&src, 6 * 4, (6, 4), size).expect("scaled");
            assert_eq!(out.len(), size.0 as usize * size.1 as usize * 4);
            assert!(out.chunks(4).all(|p| p == px), "{size:?}");
        }
    }

    #[test]
    fn the_crop_keeps_the_centre_and_a_same_size_copy_is_exact() {
        let (r, g, b, w) = (
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255; 4],
        );
        let src = image(&[r, g, b, w]);
        assert_eq!(zoomed(&src, 16, (4, 1), (2, 1)), Some(image(&[g, b])));
        assert_eq!(zoomed(&src, 16, (4, 1), (4, 1)), Some(src.clone()));
    }

    #[test]
    fn a_reduction_averages_and_an_enlargement_blends() {
        let (k, w) = ([0, 0, 0, 255], [255; 4]);
        let checker = image(&[k, w, k, w, w, k, w, k, k, w, k, w, w, k, w, k]);
        let one = zoomed(&checker, 16, (4, 4), (1, 1)).expect("scaled");
        assert!((126..=129).contains(&one[0]), "{one:?}");
        assert_eq!(one[3], 255);

        let pair = image(&[k, w]);
        let wide = zoomed(&pair, 8, (2, 1), (8, 4)).expect("scaled");
        let first_row: Vec<u8> = wide[..8 * 4].chunks(4).map(|p| p[0]).collect();
        assert_eq!(first_row[0], 0);
        assert_eq!(first_row[7], 255);
        assert!(first_row.windows(2).all(|p| p[0] <= p[1]), "{first_row:?}");
    }

    #[test]
    fn a_stride_wider_than_a_row_is_followed() {
        let px = [9, 8, 7, 255];
        let mut src = image(&[px, px]);
        src.extend([0; 8]);
        src.extend(image(&[px, px]));
        let out = zoomed(&src, 16, (2, 2), (1, 1)).expect("scaled");
        assert_eq!(out, px);
    }

    #[test]
    fn a_short_buffer_or_a_zero_side_is_refused() {
        let src = vec![0; 15];
        assert_eq!(zoomed(&src, 8, (2, 2), (1, 1)), None);
        assert_eq!(zoomed(&src, 4, (2, 1), (1, 1)), None);
        assert_eq!(zoomed(&[0; 16], 8, (2, 2), (0, 1)), None);
    }
}
