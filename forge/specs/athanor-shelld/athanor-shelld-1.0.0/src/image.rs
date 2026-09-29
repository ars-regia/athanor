//! The `image-data` hint (doc_bar.md BR4): accepted only when its fields agree with its
//! length and stay within bounds, then scaled down to the size a popup draws. The bounds
//! keep every product below 2^23, so no arithmetic here can overflow.

pub const MAX_SIDE: i32 = 1024;
pub const SHOWN_SIDE: usize = 96;
/// Row padding a toolkit may add: GdkPixbuf aligns to 4, others to 16 or 64.
const MAX_PADDING: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA, `width * 4` bytes a row.
    pub rgba: Vec<u8>,
}

/// The fields of `(iiibiiay)`, in their order.
#[derive(Debug, Clone, Copy)]
pub struct Raw<'a> {
    pub width: i32,
    pub height: i32,
    pub rowstride: i32,
    pub has_alpha: bool,
    pub bits_per_sample: i32,
    pub channels: i32,
    pub data: &'a [u8],
}

#[must_use]
pub fn accept(raw: &Raw<'_>) -> Option<Image> {
    let channels: usize = if raw.has_alpha { 4 } else { 3 };
    let sides_ok = (1..=MAX_SIDE).contains(&raw.width) && (1..=MAX_SIDE).contains(&raw.height);
    if !sides_ok || raw.bits_per_sample != 8 || usize::try_from(raw.channels).ok() != Some(channels)
    {
        return None;
    }
    let (width, height) = (
        usize::try_from(raw.width).ok()?,
        usize::try_from(raw.height).ok()?,
    );
    let row = width * channels;
    let stride = usize::try_from(raw.rowstride).ok()?;
    if stride < row || stride > row + MAX_PADDING {
        return None;
    }
    // GdkPixbuf leaves the last row unpadded; others pad every row.
    let tight = stride * (height - 1) + row;
    if raw.data.len() != tight && raw.data.len() != stride * height {
        return None;
    }
    Some(scale(raw.data, width, height, stride, channels))
}

/// Box filter over premultiplied samples, down to `SHOWN_SIDE` on the longer side; never up.
fn scale(data: &[u8], width: usize, height: usize, stride: usize, channels: usize) -> Image {
    let longest = width.max(height);
    let (target_w, target_h) = if longest <= SHOWN_SIDE {
        (width, height)
    } else {
        (
            (width * SHOWN_SIDE / longest).max(1),
            (height * SHOWN_SIDE / longest).max(1),
        )
    };
    let mut rgba = Vec::with_capacity(target_w * target_h * 4);
    for ty in 0..target_h {
        let (y0, y1) = span(ty, target_h, height);
        for tx in 0..target_w {
            let (x0, x1) = span(tx, target_w, width);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = y * stride + x * channels;
                    let alpha = if channels == 4 {
                        u64::from(data[at + 3])
                    } else {
                        255
                    };
                    r += u64::from(data[at]) * alpha;
                    g += u64::from(data[at + 1]) * alpha;
                    b += u64::from(data[at + 2]) * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            let pixel = [
                r.checked_div(a).unwrap_or(0) as u8,
                g.checked_div(a).unwrap_or(0) as u8,
                b.checked_div(a).unwrap_or(0) as u8,
                (a / n) as u8,
            ];
            rgba.extend_from_slice(&pixel);
        }
    }
    Image {
        width: target_w as u32,
        height: target_h as u32,
        rgba,
    }
}

/// The source pixels `[start, end)` that target pixel `t` of `target` covers, on an axis of `source`.
fn span(t: usize, target: usize, source: usize) -> (usize, usize) {
    let start = t * source / target;
    (start, ((t + 1) * source / target).max(start + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(
        width: i32,
        height: i32,
        rowstride: i32,
        has_alpha: bool,
        channels: i32,
        data: &[u8],
    ) -> Raw<'_> {
        Raw {
            width,
            height,
            rowstride,
            has_alpha,
            bits_per_sample: 8,
            channels,
            data,
        }
    }

    #[test]
    fn a_padded_rgb_image_is_copied_with_opaque_alpha() {
        // 2x2 RGB, rowstride 8 (2 bytes of padding), last row unpadded: 8 + 6 = 14 bytes.
        let data = [1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12];
        let image = accept(&raw(2, 2, 8, false, 3, &data)).expect("valid");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(
            image.rgba,
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
        assert!(
            accept(&raw(2, 2, 8, false, 3, &[0; 16])).is_some(),
            "every row padded"
        );
    }

    #[test]
    fn fields_that_disagree_or_exceed_the_bounds_are_refused() {
        let data = [0u8; 14];
        assert!(
            accept(&raw(2, 2, 8, false, 3, &data[..13])).is_none(),
            "length"
        );
        assert!(
            accept(&raw(2, 2, 8, true, 3, &data)).is_none(),
            "alpha with 3 channels"
        );
        assert!(
            accept(&raw(2, 2, 5, false, 3, &data)).is_none(),
            "rowstride shorter than a row"
        );
        assert!(
            accept(&raw(2, 2, -8, false, 3, &data)).is_none(),
            "negative rowstride"
        );
        assert!(
            accept(&raw(0, 2, 8, false, 3, &data)).is_none(),
            "zero width"
        );
        assert!(
            accept(&raw(MAX_SIDE + 1, 1, 4 * 1025, true, 4, &data)).is_none(),
            "too wide"
        );
        assert!(
            accept(&raw(i32::MAX, i32::MAX, i32::MAX, true, 4, &data)).is_none(),
            "overflow bait"
        );
        assert!(
            accept(&raw(2, 2, 8 + 65, false, 3, &data)).is_none(),
            "absurd padding"
        );
        let mut deep = raw(2, 2, 8, false, 3, &data);
        deep.bits_per_sample = 16;
        assert!(accept(&deep).is_none(), "16 bits");
    }

    #[test]
    fn a_large_image_is_scaled_to_the_popup_size_keeping_its_shape() {
        let pixel = [10u8, 20, 30, 255];
        let data: Vec<u8> = pixel.iter().copied().cycle().take(1024 * 512 * 4).collect();
        let image = accept(&raw(1024, 512, 4096, true, 4, &data)).expect("valid");
        assert_eq!((image.width, image.height), (96, 48));
        assert!(image.rgba.as_chunks::<4>().0.iter().all(|p| *p == pixel));
    }

    #[test]
    fn transparent_pixels_do_not_darken_their_neighbours() {
        // 200x1: opaque red on the left half, fully transparent blue on the right.
        let mut data = Vec::new();
        for x in 0..200 {
            data.extend_from_slice(if x < 100 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 0]
            });
        }
        let image = accept(&raw(200, 1, 800, true, 4, &data)).expect("valid");
        assert_eq!(image.width, 96);
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[image.rgba.len() - 4..], &[0, 0, 0, 0]);
    }
}
