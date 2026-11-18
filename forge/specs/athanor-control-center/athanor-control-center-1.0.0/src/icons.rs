//! The symbolic icons the panel draws, read once for each size and scale and drawn in the
//! text's colour, as GTK recolours them.

use std::borrow::Cow;
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
        let rtl = self.rtl;
        self.images
            .entry((name, size))
            .or_insert_with(|| match theme.image(&reading(theme, name, size, scale, rtl), size, scale) {
                Ok(image) => Some(image.tinted(tint)),
                Err(error) => {
                    tracing::warn!(icon = name, %error, "the panel's icon is unavailable");
                    None
                }
            })
            .clone()
    }
}

/// The name `name` is read under: in a right-to-left reading the theme's `-rtl` variant when
/// it has one, as GTK's lookup takes it, so that a back arrow points to the reading's start.
/// The theme's image draws a fallback for a name it lacks, so the variant is looked up first.
fn reading<'a>(theme: &IconTheme, name: &'a str, size: u16, scale: u16, rtl: bool) -> Cow<'a, str> {
    let mirrored = format!("{name}-rtl");
    if rtl && theme.lookup(&mirrored, size, scale).is_some() {
        Cow::Owned(mirrored)
    } else {
        Cow::Borrowed(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A right-to-left reading takes the `-rtl` variant the theme has, and the plain name of an
    /// icon that has none: the theme's fallback is no variant.
    #[test]
    fn a_right_to_left_reading_takes_the_variant_the_theme_has() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("athanor-cc-icons-{}", std::process::id()));
        let apps = root.join("t/apps");
        std::fs::create_dir_all(&apps)?;
        std::fs::write(
            root.join("t/index.theme"),
            "[Icon Theme]\nDirectories=apps\n[apps]\nSize=16\nType=Fixed\n",
        )?;
        for name in ["back", "back-rtl", "radio"] {
            std::fs::write(apps.join(format!("{name}.svg")), "<svg/>")?;
        }
        let theme = IconTheme::new(vec![root.clone()], "t");
        let names = [
            reading(&theme, "back", 16, 1, true),
            reading(&theme, "radio", 16, 1, true),
            reading(&theme, "back", 16, 1, false),
        ];
        std::fs::remove_dir_all(&root)?;
        assert_eq!(names, ["back-rtl", "radio", "back"]);
        Ok(())
    }
}
