//! Athanor's style crate. `calmo` is the identity; the three modules below are the
//! pre-Calmo glass theme, kept only because athanor-recovery still loads it. They
//! predate the lint gate and keep their warnings silenced until recovery is re-skinned
//! and they are deleted (doc_shell.md, SH4).
//!
//! Without the `gtk` feature only `appearance`, `variant`, `dock` and `sal` remain: what a surface
//! drawn without GTK reads.

pub mod appearance;
#[cfg(feature = "gtk")]
pub mod calmo;
pub mod variant;

/// The dock's measures in pixels, from `calmo/tokens.toml`: the GTK dock reads them through
/// the stylesheet, a dock drawn without GTK from here.
pub mod dock {
    include!("../calmo/generated/dock.rs");
}

/// The theme of Athanor's toolkit, athanor-sal, from the `[sal.*]` tables of
/// `calmo/tokens.toml` (doc_visual_language.md VL10): its radii, type scale, spacing, motion,
/// materials, icon sizes and the colours of the four variants. The GTK shell keeps the tables
/// it reads today until it is removed (VL1).
pub mod sal {
    include!("../calmo/generated/sal.rs");

    use crate::appearance::{Appearance, Rgb};
    use crate::variant::Variant;

    /// The toolkit's colours for `appearance`: its variant's palette, with the user's accent
    /// in place of the factory one outside high contrast, derived by the rules of the GTK
    /// shell (#571) against the toolkit's own surfaces and dark ink.
    pub fn palette(appearance: &Appearance) -> Palette {
        let mut palette = match appearance.variant() {
            Variant::Light => LIGHT,
            Variant::Dark => DARK,
            Variant::LightHc => LIGHT_HC,
            Variant::DarkHc => DARK_HC,
        };
        let rgb = |[red, green, blue, _]: [u8; 4]| Rgb { red, green, blue };
        let rgba = |colour: Rgb| [colour.red, colour.green, colour.blue, 255];
        let surfaces = [palette.surf, palette.surf2, palette.bar, palette.chip].map(rgb);
        if let Some(accent) = appearance.accent_colours(&surfaces, rgb(DARK_INK)) {
            palette.acc = rgba(accent.acc);
            palette.acc_ink = rgba(accent.acc_ink);
            palette.acc_fg = rgba(accent.acc_fg);
        }
        palette
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::appearance::Accent;

        #[test]
        fn the_factory_accent_derives_the_palette_the_generator_wrote() {
            for (dark, expected) in [(false, LIGHT), (true, DARK)] {
                let appearance = Appearance { dark, ..Appearance::default() };
                assert_eq!(appearance.accent, Accent::Purple);
                assert_eq!(palette(&appearance), expected, "dark {dark}");
            }
        }

        #[test]
        fn high_contrast_keeps_the_checked_accent() {
            let appearance = Appearance {
                high_contrast: true,
                accent: Accent::Yellow,
                ..Appearance::default()
            };
            assert_eq!(palette(&appearance), LIGHT_HC);
        }

        #[test]
        fn a_users_accent_replaces_the_factory_one() {
            let appearance = Appearance { accent: Accent::Blue, ..Appearance::default() };
            let blue = Accent::Blue.rgb();
            let palette = palette(&appearance);
            assert_eq!(palette.acc, [blue.red, blue.green, blue.blue, 255]);
            assert_eq!(palette.surf, LIGHT.surf, "the neutrals keep the factory tint");
        }
    }
}

#[cfg(feature = "gtk")]
#[allow(clippy::all, warnings)]
pub mod accent_engine;
#[cfg(feature = "gtk")]
#[allow(clippy::all, warnings)]
pub mod appearance_engine;
#[cfg(feature = "gtk")]
#[allow(clippy::all, warnings)]
pub mod glass;

#[cfg(feature = "gtk")]
pub use accent_engine::*;
#[cfg(feature = "gtk")]
pub use appearance_engine::*;
#[cfg(feature = "gtk")]
pub use glass::*;
