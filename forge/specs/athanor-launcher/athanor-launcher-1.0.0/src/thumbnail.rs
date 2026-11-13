//! A window's picture from the compositor as the preview's picture (LA6).

use athanor_preview::Picture;
use athanor_sulphur_client::{Picture as Capture, PixelFormat};

/// The pixels of `capture` as red, green, blue and alpha, the alpha opaque for a format
/// without one. `None` for a capture whose bytes do not hold its rows.
pub fn picture(capture: &Capture) -> Option<Picture> {
    let (width, height, stride) = (
        capture.width as usize,
        capture.height as usize,
        capture.stride as usize,
    );
    let row = width.checked_mul(4)?;
    if row > stride || capture.bytes.len() < stride.checked_mul(height)? {
        return None;
    }
    let mut rgba = Vec::with_capacity(row * height);
    for line in capture.bytes.chunks_exact(stride).take(height) {
        for &[b0, b1, b2, b3] in line[..row].as_chunks::<4>().0 {
            rgba.extend_from_slice(&match capture.format {
                PixelFormat::Argb8888 => [b2, b1, b0, b3],
                PixelFormat::Xrgb8888 => [b2, b1, b0, 255],
                PixelFormat::Abgr8888 => [b0, b1, b2, b3],
                PixelFormat::Xbgr8888 => [b0, b1, b2, 255],
            });
        }
    }
    Some(Picture {
        width: capture.width,
        height: capture.height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(format: PixelFormat, stride: u32, bytes: Vec<u8>) -> Capture {
        Capture {
            width: 1,
            height: 2,
            stride,
            format,
            bytes,
        }
    }

    #[test]
    fn the_bytes_of_a_little_endian_word_become_red_green_blue_alpha() {
        // Two rows of one pixel, each row padded to 8 bytes.
        let bytes = vec![1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0];
        let rgba = |format| picture(&capture(format, 8, bytes.clone())).map(|p| p.rgba);
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
    fn a_capture_too_short_for_its_rows_is_refused() {
        assert_eq!(
            picture(&capture(PixelFormat::Argb8888, 4, vec![0; 7])),
            None
        );
        assert_eq!(
            picture(&capture(PixelFormat::Argb8888, 2, vec![0; 8])),
            None,
            "a stride below a row"
        );
    }
}
