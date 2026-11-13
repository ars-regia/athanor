//! A window's picture from the compositor as the preview's picture (LA6).

use athanor_preview::Picture;
use athanor_sulphur_client::{Picture as Capture, PixelFormat};

/// Where red, green and blue sit in a pixel of `format`, and alpha when it has one.
fn channels(format: PixelFormat) -> ([usize; 3], Option<usize>) {
    match format {
        PixelFormat::Argb8888 => ([2, 1, 0], Some(3)),
        PixelFormat::Xrgb8888 => ([2, 1, 0], None),
        PixelFormat::Abgr8888 => ([0, 1, 2], Some(3)),
        PixelFormat::Xbgr8888 => ([0, 1, 2], None),
    }
}

/// The pixels of `capture` as red, green and blue premultiplied by alpha, and alpha, the alpha
/// opaque for a format without one, at most `side` pixels on either side: a larger capture is
/// shrunk by a whole factor, each pixel the mean of four samples of the block it stands for, so
/// that a 4K window is not copied whole for a pane a few hundred pixels wide. `None` for a
/// capture with no pixel, or whose bytes do not hold its rows.
pub fn picture(capture: &Capture, side: u32) -> Option<Picture> {
    let (width, height, stride) = (
        capture.width as usize,
        capture.height as usize,
        capture.stride as usize,
    );
    let row = width.checked_mul(4)?;
    if row == 0
        || height == 0
        || row > stride
        || capture.bytes.len() < stride.checked_mul(height)?
    {
        return None;
    }
    // The smallest whole factor that brings the longer side within `side`.
    let factor = width.max(height).div_ceil((side as usize).max(1));
    let (out_w, out_h) = (width / factor, height / factor);
    if out_w == 0 || out_h == 0 {
        return None;
    }
    let ([r, g, b], a) = channels(capture.format);
    // Four taps per output pixel, at the quarters of its block, as a linear filter samples a
    // shrunk texture: reading every pixel of a 4K window costs more than it shows at this size.
    let taps = [factor / 4, factor * 3 / 4];
    let mut rgba = Vec::with_capacity(out_w * out_h * 4);
    for out_y in 0..out_h {
        for out_x in 0..out_w {
            let mut sum = [0u32; 4];
            for y in taps.map(|tap| out_y * factor + tap) {
                for x in taps.map(|tap| out_x * factor + tap) {
                    let at = y * stride + x * 4;
                    for (total, byte) in sum.iter_mut().zip(capture.bytes.get(at..at + 4)?) {
                        *total += u32::from(*byte);
                    }
                }
            }
            // The mean of four bytes is a byte.
            let mean = |channel: usize| (sum[channel] / 4) as u8;
            rgba.extend([mean(r), mean(g), mean(b), a.map_or(255, mean)]);
        }
    }
    Some(Picture {
        width: out_w as u32,
        height: out_h as u32,
        rgba: glib::Bytes::from_owned(rgba),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(
        format: PixelFormat,
        (width, height, stride): (u32, u32, u32),
        bytes: Vec<u8>,
    ) -> Capture {
        Capture {
            width,
            height,
            stride,
            format,
            bytes,
        }
    }

    fn rgba(capture: &Capture, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        picture(capture, side).map(|p| (p.width, p.height, p.rgba.to_vec()))
    }

    #[test]
    fn the_bytes_of_a_little_endian_word_become_red_green_blue_alpha() {
        // Two rows of one pixel, each row padded to 8 bytes.
        let bytes = vec![1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0];
        let rgba = |format| rgba(&capture(format, (1, 2, 8), bytes.clone()), 512).map(|p| p.2);
        assert_eq!(
            rgba(PixelFormat::Argb8888),
            Some(vec![3, 2, 1, 4, 7, 6, 5, 8])
        );
        assert_eq!(
            rgba(PixelFormat::Xrgb8888),
            Some(vec![3, 2, 1, 255, 7, 6, 5, 255])
        );
        assert_eq!(
            rgba(PixelFormat::Abgr8888),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert_eq!(
            rgba(PixelFormat::Xbgr8888),
            Some(vec![1, 2, 3, 255, 5, 6, 7, 255])
        );
    }

    #[test]
    fn a_capture_too_short_for_its_rows_or_without_a_pixel_is_refused() {
        let refused = |shape, len| rgba(&capture(PixelFormat::Argb8888, shape, vec![0; len]), 512);
        assert_eq!(refused((1, 2, 4), 7), None);
        assert_eq!(refused((1, 2, 2), 8), None, "a stride below a row");
        assert_eq!(refused((0, 2, 0), 8), None, "no column, and a stride of 0");
        assert_eq!(refused((0, 2, 4), 8), None, "no column");
        assert_eq!(refused((1, 0, 4), 8), None, "no row");
        assert_eq!(refused((1, 2, 0), 8), None, "a stride of 0");
    }

    #[test]
    fn a_capture_shrunk_by_two_is_the_mean_of_its_blocks() {
        // 4 by 2 pixels shrunk by 2 into 2 by 1: each pixel the mean of a 2 by 2 block.
        let mut bytes = Vec::new();
        for line in [[0, 10, 20, 30], [40, 50, 60, 70]] {
            for v in line {
                bytes.extend([v, v, v, 255]);
            }
        }
        let shrunk = rgba(&capture(PixelFormat::Argb8888, (4, 2, 16), bytes), 2);
        assert_eq!(shrunk, Some((2, 1, vec![25, 25, 25, 255, 45, 45, 45, 255])));
    }

    #[test]
    fn a_4k_capture_fits_the_side() {
        let (width, height) = (3840u32, 2160u32);
        let bytes = vec![128; (width * height * 4) as usize];
        let shrunk = picture(
            &capture(PixelFormat::Xrgb8888, (width, height, width * 4), bytes),
            768,
        );
        let size = shrunk.as_ref().map(|p| (p.width, p.height));
        assert_eq!(size, Some((768, 432)));
        assert_eq!(shrunk.map(|p| p.rgba.len()), Some(768 * 432 * 4));
    }
}
