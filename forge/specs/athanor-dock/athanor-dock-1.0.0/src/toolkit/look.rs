//! The toolkit dock's colours and measures: the toolkit's theme (`athanor_style::sal`,
//! doc_visual_language.md VL10) for the appearance's variant and accent, and the dock's
//! tokens (`athanor_style::dock`), which it shares with the GTK dock.

use athanor_style::appearance::Appearance;
use athanor_style::{dock, sal};

use athanor_sal::{Rgba, Theme};

/// The themes of the dock's surfaces and of its menus.
pub struct Look {
    pub dock: Theme,
    pub menu: Theme,
}

/// The look of `appearance`: its variant's colours, with its accent when it has one.
pub fn look(appearance: &Appearance) -> Look {
    let palette = sal::palette(appearance);
    let mut dock = Theme::default();
    dock.background = Rgba([0, 0, 0, 0]);
    dock.surface = Rgba(palette.chip);
    dock.text = Rgba(palette.ink);
    dock.accent = Rgba(palette.acc_fg);
    dock.on_accent = Rgba(palette.acc_ink);
    dock.border = Rgba(palette.line);
    dock.title_bar = Rgba(palette.bar);
    dock.focus = Rgba(palette.acc_fg);
    dock.font_size = 13.0;
    dock.padding = 8.0;
    dock.margin = 0.0;
    dock.gap = 2.0;
    dock.island = Rgba(palette.bar);
    dock.island_radius = sal::radius::DOCK;
    dock.island_padding = dock::PADDING as f32;
    // The highlights are concentric with the island (VL7): its radius less their inset of
    // `space.s`, 22 - 6 = 16 (doc_visual_language.md VL7, VL10). The island keeps today's
    // 4 px padding and 32 px buttons until the dock's geometry of VL12 step 8 (#1008).
    dock.radius = sal::radius::DOCK - sal::space::S;
    dock.button_min = dock::BUTTON as f32;

    let mut menu = dock.clone();
    menu.background = Rgba(palette.surf);
    menu.margin = 4.0;
    menu.radius = 8.0;
    Look { dock, menu }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_island_is_the_bar_colour_and_the_indicator_the_accent_foreground() {
        let appearance = Appearance::default();
        let palette = sal::palette(&appearance);
        let look = look(&appearance);
        assert_eq!(look.dock.island, Rgba(palette.bar));
        assert_eq!(look.dock.text, Rgba(palette.ink));
        assert_eq!(look.dock.accent, Rgba(palette.acc_fg));
        assert_eq!(
            look.dock.background.0[3], 0,
            "the dock's surface is transparent"
        );
        assert_eq!(look.menu.background, Rgba(palette.surf));
        assert_eq!(look.dock.button_min, athanor_style::dock::BUTTON as f32);
    }

    #[test]
    fn the_highlights_are_concentric_with_the_island() {
        let look = look(&Appearance::default());
        assert_eq!(look.dock.island_radius, sal::radius::DOCK);
        assert_eq!(look.dock.radius + sal::space::S, look.dock.island_radius);
        assert_eq!(look.dock.radius, 16.0, "VL7: highlights of 16 px at a 6 px inset");
    }
}
