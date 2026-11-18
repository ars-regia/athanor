//! "Zoom" (fill and crop), the only scaling mode (SD3): the image is scaled by one factor
//! until it covers the buffer, and the overflow is cut equally on both sides.

/// The part of the image, in image pixels, that fills the whole buffer once scaled to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The centred crop of an `image` (width, height) that has the aspect of the `buffer`.
/// `None` when either has a zero side. The size is rounded down, so scaling the crop to the
/// buffer may stretch it by less than one image pixel.
pub fn zoom(image: (u32, u32), buffer: (u32, u32)) -> Option<Crop> {
    let ((iw, ih), (bw, bh)) = (image, buffer);
    if iw == 0 || ih == 0 || bw == 0 || bh == 0 {
        return None;
    }
    let (iw, ih, bw, bh) = (u64::from(iw), u64::from(ih), u64::from(bw), u64::from(bh));
    // The image is wider than the buffer when iw/ih > bw/bh: cut the sides, else top and bottom.
    let (width, height) = if iw * bh > ih * bw {
        ((ih * bw / bh).max(1), ih)
    } else {
        (iw, (iw * bh / bw).max(1))
    };
    // Each value is at most the image's side, so it fits in u32.
    let narrow = |v: u64| u32::try_from(v).ok();
    Some(Crop {
        x: narrow((iw - width) / 2)?,
        y: narrow((ih - height) / 2)?,
        width: narrow(width)?,
        height: narrow(height)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crop(x: u32, y: u32, width: u32, height: u32) -> Option<Crop> {
        Some(Crop {
            x,
            y,
            width,
            height,
        })
    }

    #[test]
    fn same_aspect_keeps_everything() {
        assert_eq!(zoom((1920, 1080), (960, 540)), crop(0, 0, 1920, 1080));
    }

    #[test]
    fn wide_image_loses_its_sides() {
        assert_eq!(zoom((4000, 1000), (1000, 1000)), crop(1500, 0, 1000, 1000));
    }

    #[test]
    fn tall_image_loses_top_and_bottom() {
        assert_eq!(zoom((1000, 4000), (2000, 1000)), crop(0, 1750, 1000, 500));
    }

    #[test]
    fn odd_overflow_rounds_down_and_stays_inside() {
        let c = zoom((101, 100), (100, 100)).unwrap_or(Crop {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        assert_eq!((c.x, c.width, c.height), (0, 100, 100));
    }

    #[test]
    fn zero_sides_have_no_crop() {
        assert_eq!(zoom((0, 10), (10, 10)), None);
        assert_eq!(zoom((10, 10), (10, 0)), None);
    }

    #[test]
    fn extreme_sizes_do_not_overflow() {
        assert!(zoom((u32::MAX, u32::MAX), (u32::MAX, 1)).is_some());
        assert!(zoom((1, u32::MAX), (u32::MAX, 1)).is_some());
    }
}
