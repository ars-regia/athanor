//! The symbolic icons the panel draws, read once for each size and scale and drawn in the
//! text's colour, as GTK recolours them.

use std::collections::HashMap;

use athanor_sal::{IconTheme, Image, Rgba};
use athanor_style::sal;

/// The theme the icons come from, as the OSD's.
const THEME: &str = "Adwaita";

/// The icons of one surface's scale, in one colour and one reading direction.
pub struct Icons {
    theme: IconTheme,
    scale: u16,
    tint: Rgba,
    rtl: bool,
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
            rtl: false,
            images: HashMap::new(),
        }
    }

    /// Draws the icons at `scale` in `tint`, for a right-to-left reading when `rtl`, from now on;
    /// what was read for another one is dropped.
    pub fn set(&mut self, scale: u16, tint: Rgba, rtl: bool) {
        if (scale, tint, rtl) != (self.scale, self.tint, self.rtl) {
            self.scale = scale;
            self.tint = tint;
            self.rtl = rtl;
            self.images.clear();
        }
    }

    /// The icon `name` at `size` logical pixels; `None`, with a warning, when the theme has
    /// neither it nor its fallback.
    pub fn get(&mut self, name: &'static str, size: f32) -> Option<Image> {
        let (theme, scale, tint) = (&self.theme, self.scale, self.tint);
        // The sizes are the tokens' few whole numbers.
        let size = size as u16;
        // In a right-to-left reading the theme's `-rtl` variant comes first, as GTK's lookup
        // takes it: a back arrow points to the reading's start.
        let mirrored = self.rtl.then(|| format!("{name}-rtl"));
        self.images
            .entry((name, size))
            .or_insert_with(|| match mirrored
                .and_then(|name| theme.image(&name, size, scale).ok())
                .map_or_else(|| theme.image(name, size, scale), Ok)
            {
                Ok(image) => Some(image.tinted(tint)),
                Err(error) => {
                    tracing::warn!(icon = name, %error, "the panel's icon is unavailable");
                    None
                }
            })
            .clone()
    }
}
