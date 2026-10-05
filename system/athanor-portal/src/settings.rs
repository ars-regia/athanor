//! The values of the Settings portal's `org.freedesktop.appearance` namespace (doc_portal.md,
//! PT5; doc_visual_language.md, VL5), and which namespaces a `ReadAll` asks for.
//!
//! Where each value is stored (maintainer decision A2-20): the colour scheme in
//! `org.gnome.desktop.interface color-scheme`, the contrast in
//! `org.gnome.desktop.a11y.interface high-contrast`, the fixed accent in
//! `org.gnome.desktop.interface accent-color`; `org.athanor.desktop.appearance` holds only
//! `accent-mode` and `accent-computed`.

/// The namespace the portal specification defines for the appearance.
pub const APPEARANCE: &str = "org.freedesktop.appearance";

/// An sRGB colour, each component from 0 to 1: the portal's `(ddd)`.
pub type Rgb = (f64, f64, f64);

/// The nine accents of `org.gnome.desktop.interface accent-color`, with the colours
/// libadwaita 1.8 draws for them (doc_visual_language.md, section 1).
pub const PRESETS: [(&str, &str); 9] = [
    ("blue", "#3584e4"),
    ("teal", "#2190a4"),
    ("green", "#3a944a"),
    ("yellow", "#c88800"),
    ("orange", "#ed5b00"),
    ("red", "#e62d42"),
    ("pink", "#d56199"),
    ("purple", "#9141ac"),
    ("slate", "#6f8396"),
];

/// The accent GNOME's schema falls back to.
const DEFAULT_PRESET: &str = "blue";

/// `color-scheme` from GNOME's key: `default` is no preference (0), the light variant
/// (VL5); `prefer-dark` is 1; `prefer-light` is 2, served as stored because only a
/// deliberate choice outside Athanor's own writers sets it. Anything else is 0, as the
/// portal specification asks of an unknown value.
pub fn color_scheme(gnome: &str) -> u32 {
    match gnome {
        "prefer-dark" => 1,
        "prefer-light" => 2,
        _ => 0,
    }
}

/// `contrast` from `org.gnome.desktop.a11y.interface high-contrast`: 1 high, 0 normal.
pub fn contrast(high_contrast: bool) -> u32 {
    u32::from(high_contrast)
}

/// Why the served accent is not the one the stored keys ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// GNOME's `accent-color` named no preset: the schema's default, blue, is served.
    UnknownPreset,
    /// The wallpaper mode's `accent-computed` is not `#rrggbb`: the fixed preset is served.
    MalformedComputed,
}

/// `accent-color`. `mode` is `org.athanor.desktop.appearance accent-mode`, `None` while
/// that schema is not installed; `preset` is GNOME's `accent-color`; `computed` is
/// `accent-computed`. In the wallpaper mode the computed colour is served, otherwise the
/// preset; a fallback says which stored value could not be used.
pub fn accent_color(mode: Option<&str>, preset: &str, computed: &str) -> (Rgb, Option<Fallback>) {
    let (fixed, unknown) = match preset_rgb(preset) {
        Some(rgb) => (rgb, None),
        None => (default_preset(), Some(Fallback::UnknownPreset)),
    };
    if mode != Some("wallpaper") {
        return (fixed, unknown);
    }
    match parse_hex(computed) {
        Some(rgb) => (rgb, None),
        None => (fixed, Some(Fallback::MalformedComputed)),
    }
}

fn preset_rgb(name: &str) -> Option<Rgb> {
    PRESETS
        .iter()
        .find(|(preset, _)| *preset == name)
        .and_then(|(_, hex)| parse_hex(hex))
}

fn default_preset() -> Rgb {
    // Every entry of PRESETS parses: the test `every_preset_is_served_exactly` holds it.
    preset_rgb(DEFAULT_PRESET).unwrap_or((0.0, 0.0, 0.0))
}

/// `#rrggbb`, exactly, as components from 0 to 1.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let component = |at: usize| {
        u8::from_str_radix(&digits[at..at + 2], 16)
            .ok()
            .map(|value| f64::from(value) / 255.0)
    };
    Some((component(0)?, component(2)?, component(4)?))
}

/// Whether `namespace` is one a `ReadAll` with `patterns` asks for: every namespace when the
/// list is empty or holds an empty string; otherwise an exact name, or a prefix ending in
/// `*` (org.freedesktop.impl.portal.Settings, `ReadAll`).
pub fn namespace_matches(namespace: &str, patterns: &[String]) -> bool {
    patterns.is_empty()
        || patterns.iter().any(|pattern| {
            pattern.is_empty()
                || pattern == namespace
                || pattern
                    .strip_suffix('*')
                    .is_some_and(|prefix| namespace.starts_with(prefix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scheme_follows_the_gnome_key() {
        assert_eq!(color_scheme("default"), 0);
        assert_eq!(color_scheme("prefer-dark"), 1);
        assert_eq!(color_scheme("prefer-light"), 2);
        assert_eq!(color_scheme(""), 0);
        assert_eq!(color_scheme("dark"), 0);
    }

    #[test]
    fn contrast_follows_high_contrast() {
        assert_eq!(contrast(false), 0);
        assert_eq!(contrast(true), 1);
    }

    #[test]
    fn every_preset_is_served_exactly() {
        let expected: [(&str, (u8, u8, u8)); 9] = [
            ("blue", (0x35, 0x84, 0xe4)),
            ("teal", (0x21, 0x90, 0xa4)),
            ("green", (0x3a, 0x94, 0x4a)),
            ("yellow", (0xc8, 0x88, 0x00)),
            ("orange", (0xed, 0x5b, 0x00)),
            ("red", (0xe6, 0x2d, 0x42)),
            ("pink", (0xd5, 0x61, 0x99)),
            ("purple", (0x91, 0x41, 0xac)),
            ("slate", (0x6f, 0x83, 0x96)),
        ];
        for (name, (r, g, b)) in expected {
            let rgb = (
                f64::from(r) / 255.0,
                f64::from(g) / 255.0,
                f64::from(b) / 255.0,
            );
            for mode in [None, Some("fixed")] {
                assert_eq!(
                    accent_color(mode, name, ""),
                    (rgb, None),
                    "{name} in {mode:?}"
                );
            }
        }
    }

    #[test]
    fn the_wallpaper_mode_serves_the_computed_colour() {
        assert_eq!(
            accent_color(Some("wallpaper"), "purple", "#FF8000"),
            ((1.0, 128.0 / 255.0, 0.0), None)
        );
        // The fixed mode ignores a computed colour left from an earlier wallpaper.
        assert_eq!(
            accent_color(Some("fixed"), "purple", "#ff8000").0,
            accent_color(None, "purple", "").0
        );
    }

    #[test]
    fn a_malformed_computed_colour_serves_the_preset() {
        let purple = accent_color(None, "purple", "").0;
        for computed in [
            "",
            "#",
            "ff8000",
            "#ff800",
            "#ff80000",
            "#gg8000",
            "#ff 800",
            "#ff8000\n",
            "＃ff8000",
        ] {
            assert_eq!(
                accent_color(Some("wallpaper"), "purple", computed),
                (purple, Some(Fallback::MalformedComputed)),
                "{computed:?}"
            );
        }
    }

    #[test]
    fn an_unknown_preset_serves_blue() {
        let blue = accent_color(None, "blue", "").0;
        assert_eq!(
            accent_color(None, "indigo", ""),
            (blue, Some(Fallback::UnknownPreset))
        );
        assert_eq!(
            accent_color(Some("wallpaper"), "", "#000000"),
            ((0.0, 0.0, 0.0), None),
            "a valid computed colour does not need the preset"
        );
    }

    #[test]
    fn namespaces_match_exactly_by_prefix_or_all() {
        let patterns = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(namespace_matches(APPEARANCE, &[]));
        assert!(namespace_matches(APPEARANCE, &patterns(&[""])));
        assert!(namespace_matches(APPEARANCE, &patterns(&[APPEARANCE])));
        assert!(namespace_matches(
            "org.gnome.desktop.a11y.interface",
            &patterns(&["org.gnome.*"])
        ));
        assert!(namespace_matches(
            "org.gnome.desktop.interface",
            &patterns(&["x", "org.gnome.desktop.*"])
        ));
        assert!(!namespace_matches(
            "org.gnome.desktop.interface",
            &patterns(&["org.gnome.desktop"])
        ));
        assert!(!namespace_matches(APPEARANCE, &patterns(&["org.gnome.*"])));
    }
}
