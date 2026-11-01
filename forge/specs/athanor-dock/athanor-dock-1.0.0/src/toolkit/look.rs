//! The toolkit dock's colours and measures: Calmo's named colours for the appearance's
//! variant and accent, read from the stylesheets the GTK dock loads, and the dock's tokens
//! (`athanor_style::dock`), so that both docks draw one palette.

use std::collections::HashMap;

use athanor_style::appearance::Appearance;
use athanor_style::dock;
use athanor_toolkit::{Rgba, Theme};

/// The themes of the dock's surfaces and of its menus.
pub struct Look {
    pub dock: Theme,
    pub menu: Theme,
}

/// The `@define-color` lines of `css` whose value is `#rrggbb`, by name.
pub fn colours(css: &str) -> HashMap<String, Rgba> {
    css.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("@define-color ")?;
            let (name, value) = rest.split_once(' ')?;
            let hex = value.trim().strip_suffix(';')?.strip_prefix('#')?;
            if hex.len() != 6 {
                return None;
            }
            let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
            Some((
                name.to_owned(),
                Rgba::rgb(channel(0)?, channel(2)?, channel(4)?),
            ))
        })
        .collect()
}

/// The look of `appearance`: its variant's colours, with its accent when it has one. A
/// colour the stylesheet lacks keeps the toolkit's default; a test holds every variant to
/// naming them all.
pub fn look(appearance: &Appearance) -> Look {
    let mut named = colours(appearance.variant().css());
    if let Some(accent) = appearance.accent_css() {
        named.extend(colours(&accent));
    }
    let base = Theme::default();
    let colour = |name: &str, default: Rgba| named.get(name).copied().unwrap_or(default);
    let dock = Theme {
        background: Rgba([0, 0, 0, 0]),
        surface: colour("ath_chip", base.surface),
        text: colour("ath_ink", base.text),
        accent: colour("ath_acc_fg", base.accent),
        on_accent: colour("ath_acc_ink", base.on_accent),
        border: colour("ath_line", base.border),
        title_bar: colour("ath_bar", base.title_bar),
        focus: colour("ath_acc_fg", base.focus),
        font_size: 13.0,
        padding: 8.0,
        margin: 0.0,
        gap: 2.0,
        radius: 8.0,
        island: colour("ath_bar", base.island),
        island_radius: dock::RADIUS as f32,
        island_padding: dock::PADDING as f32,
        button_min: dock::BUTTON as f32,
        ..base
    };
    let menu = Theme {
        background: colour("ath_surf", base.background),
        margin: 4.0,
        ..dock.clone()
    };
    Look { dock, menu }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_style::variant::Variant;

    /// Every colour `look` reads.
    const NAMES: [&str; 8] = [
        "ath_bar",
        "ath_surf",
        "ath_chip",
        "ath_ink",
        "ath_line",
        "ath_acc",
        "ath_acc_ink",
        "ath_acc_fg",
    ];

    #[test]
    fn a_define_color_line_with_a_hex_value_reads_as_its_colour() {
        let css = "@define-color ath_bar #fbfcfd;\n  @define-color ath_d1 rgba(140, 156, 248, 0.5);\nwindow { color: #000000; }\n";
        let named = colours(css);
        assert_eq!(named.get("ath_bar"), Some(&Rgba::rgb(0xfb, 0xfc, 0xfd)));
        assert_eq!(named.len(), 1, "{named:?}");
    }

    #[test]
    fn every_variant_names_every_colour_the_dock_draws_with() {
        for variant in [
            Variant::Light,
            Variant::Dark,
            Variant::LightHc,
            Variant::DarkHc,
        ] {
            let named = colours(variant.css());
            for name in NAMES {
                assert!(named.contains_key(name), "{} lacks {name}", variant.name());
            }
        }
    }

    #[test]
    fn the_island_is_the_bar_colour_and_the_indicator_the_accent_foreground() {
        let appearance = Appearance::default();
        let mut named = colours(appearance.variant().css());
        if let Some(accent) = appearance.accent_css() {
            named.extend(colours(&accent));
        }
        let look = look(&appearance);
        assert_eq!(look.dock.island, named["ath_bar"]);
        assert_eq!(look.dock.text, named["ath_ink"]);
        assert_eq!(look.dock.accent, named["ath_acc_fg"]);
        assert_eq!(
            look.dock.background.0[3], 0,
            "the dock's surface is transparent"
        );
        assert_eq!(look.menu.background, named["ath_surf"]);
        assert_eq!(look.dock.button_min, athanor_style::dock::BUTTON as f32);
    }
}
