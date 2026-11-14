//! The launcher's colours and measures: the toolkit's theme (`athanor_style::sal`,
//! doc_visual_language.md VL10) for the appearance's variant and accent, as the dock's look.

use athanor_sal::{Rgba, Theme};
use athanor_style::appearance::Appearance;
use athanor_style::sal;

/// The themes of the panel and of its menu of actions.
pub struct Look {
    pub panel: Theme,
    pub menu: Theme,
}

/// The look of `appearance`: its variant's colours, with its accent when it has one, and the
/// system font's size.
pub fn look(appearance: &Appearance) -> Look {
    let palette = sal::palette(appearance);
    let mut panel = Theme::default();
    // The surface covers its output; only the panel, the island, is drawn.
    panel.background = Rgba([0, 0, 0, 0]);
    panel.surface = Rgba(palette.chip);
    panel.text = Rgba(palette.ink);
    panel.accent = Rgba(palette.acc_fg);
    panel.on_accent = Rgba(palette.acc_ink);
    // The query and the results lie on a sheet a step into the panel, dark in the dark variant.
    panel.view = Rgba(palette.surf2);
    panel.border = Rgba(palette.line);
    panel.title_bar = Rgba(palette.bar);
    panel.focus = Rgba(palette.acc_fg);
    // The system font's size: the text follows the user's font, as the GTK launcher's (VL6).
    panel.font_size = appearance.font_size;
    panel.island = Rgba(palette.surf);
    // The launcher is a panel of VL7, 30 px, with the GTK launcher's 12 px padding.
    panel.island_radius = sal::radius::PANEL;
    panel.island_padding = sal::space::M;
    panel.radius = sal::radius::CONTROL;
    // The island places the panel on the output by its own margins.
    panel.margin = 0.0;

    let mut menu = panel.clone();
    menu.background = Rgba(palette.surf);
    menu.margin = 4.0;
    menu.radius = 8.0;
    Look { panel, menu }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_the_surface_colour_on_a_transparent_surface() {
        let appearance = Appearance::default();
        let palette = sal::palette(&appearance);
        let look = look(&appearance);
        assert_eq!(look.panel.island, Rgba(palette.surf));
        assert_eq!(look.panel.background.0[3], 0);
        assert_eq!(look.panel.island_radius, sal::radius::PANEL);
        assert_eq!(look.menu.background, Rgba(palette.surf));
        assert_eq!(look.panel.view, Rgba(palette.surf2));
    }

    #[test]
    fn the_text_is_the_system_fonts_size() {
        let large = Appearance {
            font_size: 22.0,
            ..Appearance::default()
        };
        let look = look(&large);
        assert_eq!((look.panel.font_size, look.menu.font_size), (22.0, 22.0));
    }
}
