//! The compositor's half of the appearance (doc_visual_language.md, VL4; spike VL-S1).
//! cosmic-comp draws borders, indicators and overlays from COSMIC's built themes, so
//! athanor-style's apply function ends by writing them here. Nothing in Athanor reads them
//! back: the shell reads the appearance from GSettings (`athanor_style::appearance`).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use athanor_layout::atomic::{replace, sync_dir};
use athanor_style::appearance::Appearance;

use crate::cosmic_config;

/// `is_dark`, at `v1`.
pub const MODE: &str = "com.system76.CosmicTheme.Mode";
/// The built themes, at `v2`.
pub const DARK: &str = "com.system76.CosmicTheme.Dark";
pub const LIGHT: &str = "com.system76.CosmicTheme.Light";
const MODE_VERSION: u32 = 1;
const THEME_VERSION: u32 = 2;

/// athanor-calmo's built themes, written by `calmo-cosmic-theme`: mode, then contrast,
/// then accent, then each key of the theme with its RON text.
pub const VARIANTS: &str = "/usr/share/athanor/cosmic-variants.json";

type Variants = BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>>;

/// The directories `write_cosmic` writes under the user's `cosmic` directory, for a
/// Landlock grant. Empty without a home directory.
pub fn theme_dirs() -> Vec<PathBuf> {
    cosmic_config::user_dir().map_or_else(Vec::new, |user| theme_dirs_in(&user))
}

/// The directories `write_cosmic_in` writes under `cosmic_dir`.
pub fn theme_dirs_in(cosmic_dir: &Path) -> Vec<PathBuf> {
    vec![
        cosmic_config::versioned(cosmic_dir, MODE, MODE_VERSION),
        cosmic_config::versioned(cosmic_dir, DARK, THEME_VERSION),
        cosmic_config::versioned(cosmic_dir, LIGHT, THEME_VERSION),
    ]
}

/// Writes `appearance` into the user's COSMIC theme, from the themes athanor-calmo ships.
/// `NotFound` without a home directory or without the themes.
pub fn write_cosmic(appearance: &Appearance) -> io::Result<()> {
    let user = cosmic_config::user_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no home directory for the user's cosmic configuration",
        )
    })?;
    let variants = fs::read_to_string(VARIANTS)
        .map_err(|err| io::Error::new(err.kind(), format!("{VARIANTS}: {err}")))?;
    write_cosmic_in(&user, &variants, appearance)
}

/// Writes the built theme of `appearance`'s contrast and accent in both modes, so a mode
/// switched elsewhere (cosmic-settings) keeps them, then `is_dark`. A key whose file already
/// holds the text is left alone: cosmic-comp reloads the whole theme on every change.
pub fn write_cosmic_in(
    cosmic_dir: &Path,
    variants: &str,
    appearance: &Appearance,
) -> io::Result<()> {
    let variants: Variants = serde_json::from_str(variants)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let contrast = if appearance.high_contrast {
        "high"
    } else {
        "normal"
    };
    let accent = appearance.accent.name();
    let theme = |mode: &str| {
        variants
            .get(mode)
            .and_then(|contrasts| contrasts.get(contrast))
            .and_then(|accents| accents.get(accent))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("no built theme for {mode}, {contrast} contrast, {accent}"),
                )
            })
    };
    // Both looked up before the first write, so a missing one changes nothing.
    let themes = [(DARK, theme("dark")?), (LIGHT, theme("light")?)];
    for (component, theme) in themes {
        let dir = cosmic_config::versioned(cosmic_dir, component, THEME_VERSION);
        write_keys(
            &dir,
            theme
                .iter()
                .map(|(key, text)| (key.as_str(), text.as_str())),
        )?;
    }
    let dir = cosmic_config::versioned(cosmic_dir, MODE, MODE_VERSION);
    write_keys(&dir, [("is_dark", appearance.dark.to_string().as_str())])
}

/// Replaces the keys of one component whose file does not already hold the text, then
/// fsyncs the directory once: the first apply writes some forty keys per theme.
fn write_keys<'a>(
    dir: &Path,
    keys: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut written = false;
    for (key, text) in keys {
        if Path::new(key).file_name().and_then(|name| name.to_str()) != Some(key) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{key:?} is not a key name"),
            ));
        }
        let path = dir.join(key);
        if fs::read_to_string(&path).is_ok_and(|current| current == text) {
            continue;
        }
        replace(&path, text)?;
        written = true;
    }
    if written {
        sync_dir(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_style::appearance::Accent;

    fn cosmic_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-theme-{}-{name}", std::process::id()));
        let _fresh = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// Two accents in both modes and contrasts; each theme names its variant.
    fn variants() -> String {
        let mut modes = serde_json::Map::new();
        for mode in ["dark", "light"] {
            let mut contrasts = serde_json::Map::new();
            for contrast in ["normal", "high"] {
                let mut accents = serde_json::Map::new();
                for accent in ["purple", "teal"] {
                    let theme = serde_json::json!({
                        "accent": format!("{mode} {contrast} {accent}"),
                        "is_high_contrast": (contrast == "high").to_string(),
                    });
                    accents.insert(accent.into(), theme);
                }
                contrasts.insert(contrast.into(), accents.into());
            }
            modes.insert(mode.into(), contrasts.into());
        }
        serde_json::Value::from(modes).to_string()
    }

    fn read(dir: &Path, component: &str, version: u32, key: &str) -> String {
        fs::read_to_string(cosmic_config::versioned(dir, component, version).join(key))
            .expect("key")
    }

    #[test]
    fn both_built_themes_and_the_mode_land_where_cosmic_comp_reads_them() {
        let dir = cosmic_dir("write");
        let choice = Appearance {
            dark: true,
            high_contrast: true,
            accent: Accent::Teal,
            ..Appearance::default()
        };
        write_cosmic_in(&dir, &variants(), &choice).expect("write");
        assert_eq!(read(&dir, MODE, 1, "is_dark"), "true");
        assert_eq!(read(&dir, DARK, 2, "accent"), "dark high teal");
        assert_eq!(read(&dir, LIGHT, 2, "accent"), "light high teal");
        assert_eq!(read(&dir, LIGHT, 2, "is_high_contrast"), "true");
        assert!(!dir.join(DARK).join("v1").exists(), "nothing under v1");

        let choice = Appearance {
            dark: false,
            high_contrast: false,
            ..choice
        };
        write_cosmic_in(&dir, &variants(), &choice).expect("write");
        assert_eq!(read(&dir, MODE, 1, "is_dark"), "false");
        assert_eq!(read(&dir, DARK, 2, "accent"), "dark normal teal");
        assert_eq!(read(&dir, LIGHT, 2, "is_high_contrast"), "false");
    }

    #[test]
    fn a_missing_variant_writes_nothing() {
        let dir = cosmic_dir("missing");
        let choice = Appearance {
            accent: Accent::Slate,
            ..Appearance::default()
        };
        let err = write_cosmic_in(&dir, &variants(), &choice).expect_err("no slate");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_dir(&dir).expect("dir").count(), 0);
        let err = write_cosmic_in(&dir, "{", &Appearance::default()).expect_err("not JSON");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_key_never_names_a_path() {
        let dir = cosmic_dir("escape");
        let escape =
            r#"{"dark":{"normal":{"purple":{"../is_dark":"x"}}},"light":{"normal":{"purple":{}}}}"#;
        let err = write_cosmic_in(&dir, escape, &Appearance::default()).expect_err("escape");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(!dir.join(DARK).join("is_dark").exists());
    }

    #[test]
    fn the_shipped_themes_cover_every_choice() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../athanor-style/calmo/generated/cosmic/variants.json");
        let shipped = fs::read_to_string(shipped).expect("variants.json");
        let dir = cosmic_dir("shipped");
        for dark in [false, true] {
            for high_contrast in [false, true] {
                for accent in Accent::ALL {
                    let choice = Appearance {
                        dark,
                        high_contrast,
                        accent,
                        ..Appearance::default()
                    };
                    write_cosmic_in(&dir, &shipped, &choice).expect("write");
                    assert_eq!(
                        read(&dir, LIGHT, 2, "is_high_contrast"),
                        high_contrast.to_string()
                    );
                }
            }
        }
    }
}
