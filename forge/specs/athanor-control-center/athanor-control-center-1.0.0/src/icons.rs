//! The symbolic icons the panel draws, read once for each size and scale and drawn in the
//! text's colour, as GTK recolours them.

use std::collections::HashMap;

use athanor_sal::{IconTheme, Image, Rgba};
use athanor_style::sal;

/// The theme the icons come from, as the OSD's.
const THEME: &str = "Adwaita";

/// The icons of one surface's scale, in one colour.
pub struct Icons {
    theme: IconTheme,
    scale: u16,
    tint: Rgba,
    images: HashMap<(&'static str, u16), Option<Image>>,
}

impl Icons {
    /// The side of a button's icon, as the bar's.
    pub const BUTTON: f32 = sal::size::icon::BAR;
    /// The side of a tile's glyph (doc_visual_language.md VL10).
    pub const GLYPH: f32 = sal::size::icon::CC_GLYPH;

    pub fn new(tint: Rgba) -> Icons {
        Icons {
            theme: IconTheme::system(THEME),
            scale: 1,
            tint,
            images: HashMap::new(),
        }
    }

    /// Draws the icons at `scale` in `tint` from now on; what was read at another one is dropped.
    pub fn set(&mut self, scale: u16, tint: Rgba) {
        if (scale, tint) != (self.scale, self.tint) {
            self.scale = scale;
            self.tint = tint;
            self.images.clear();
        }
    }

    /// The icon `name` at `size` logical pixels; `None`, with a warning, when the theme has
    /// neither it nor its fallback.
    pub fn get(&mut self, name: &'static str, size: f32) -> Option<Image> {
        let (theme, scale, tint) = (&self.theme, self.scale, self.tint);
        // The sizes are the tokens' few whole numbers.
        let size = size as u16;
        self.images
            .entry((name, size))
            .or_insert_with(|| match theme.image(name, size, scale) {
                Ok(image) => Some(image.tinted(tint)),
                Err(error) => {
                    tracing::warn!(icon = name, %error, "the panel's icon is unavailable");
                    None
                }
            })
            .clone()
    }
}
