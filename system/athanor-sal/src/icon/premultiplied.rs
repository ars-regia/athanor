//! Pixels whose colour is already premultiplied by their alpha, as a compositor's window capture
//! (`wl_shm`'s ARGB8888) and the launcher's preview pictures hold them (doc_launcher.md, LA5,
//! need 4): taken as they are, never premultiplied a second time.

use vello_cpu::peniko::ImageAlphaType;

use super::Image;
use crate::render::Error;

impl Image {
    /// [`Image::from_rgba8`] for pixels whose red, green and blue are already multiplied by
    /// their alpha: they are kept as they are. The same sizes are refused.
    pub fn from_premultiplied_rgba8(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<Self, Error> {
        Self::from_pixels(width, height, pixels, ImageAlphaType::AlphaPremultiplied)
    }
}

#[cfg(test)]
mod tests {
    use crate::Image;

    fn first(image: &Image) -> [u8; 4] {
        let p = image.pixmap.data()[0];
        [p.r, p.g, p.b, p.a]
    }

    #[test]
    fn premultiplied_pixels_are_kept_and_straight_ones_multiplied() {
        let half = vec![64, 32, 0, 128];
        let kept = Image::from_premultiplied_rgba8(1, 1, half.clone());
        assert_eq!(kept.as_ref().map(first).ok(), Some([64, 32, 0, 128]));
        let multiplied = Image::from_rgba8(1, 1, half);
        assert_eq!(multiplied.as_ref().map(first).ok(), Some([32, 16, 0, 128]));
    }

    #[test]
    fn the_sizes_from_rgba8_refuses_are_refused() {
        assert!(Image::from_premultiplied_rgba8(0, 1, Vec::new()).is_err());
        assert!(Image::from_premultiplied_rgba8(2, 1, vec![0; 4]).is_err());
        assert!(Image::from_premultiplied_rgba8(65536, 1, vec![0; 65536 * 4]).is_err());
        assert!(Image::from_premultiplied_rgba8(2, 1, vec![0; 8]).is_ok());
    }
}
