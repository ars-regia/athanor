//! calmo-cosmic-theme <cosmic-inputs.json> <out-dir>
//!
//! Reads the five Builder inputs the Calmo tokens set for each mode, applies them to
//! COSMIC's stock builders, derives the themes with `ThemeBuilder::build()` and writes
//! complete cosmic-config directories under `<out-dir>/cosmic/`, ready to be served as
//! system defaults from a directory placed first in `XDG_DATA_DIRS`.
//!
//! It also writes `<out-dir>/variants.json`, what the appearance apply function of
//! athanor-compositor-client writes to the user's built themes (doc_visual_language.md,
//! VL4, spike VL-S1): for each mode, each contrast and each of libadwaita's nine accent
//! presets, every key of the built theme. Whole themes, not differences from the default:
//! cosmic-config falls back to a user's `v1` key for one missing in `v2`, and earlier
//! Athanor releases wrote `is_high_contrast` there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use cosmic_config::{Config, CosmicConfigEntry};
use cosmic_theme::palette::{Srgb, Srgba};
use cosmic_theme::{CosmicPalette, Theme, ThemeBuilder, ThemeMode, DARK_THEME_BUILDER_ID, DARK_THEME_ID, LIGHT_THEME_BUILDER_ID, LIGHT_THEME_ID, THEME_MODE_ID};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Inputs {
    light: Mode,
    dark: Mode,
}

#[derive(Deserialize)]
struct Mode {
    accent: [f32; 3],
    bg_color: [f32; 3],
    primary_container_bg: [f32; 3],
    /// Optional: leaving a tint out keeps COSMIC's neutral steps.
    #[serde(default)]
    neutral_tint: Option<[f32; 3]>,
    #[serde(default)]
    text_tint: Option<[f32; 3]>,
}

fn rgb(c: [f32; 3]) -> Srgb {
    Srgb::new(c[0], c[1], c[2])
}

fn rgba(c: [f32; 3]) -> Srgba {
    Srgba::new(c[0], c[1], c[2], 1.0)
}

fn write<T: CosmicConfigEntry>(entry: &T, id: &str, out: &Path) -> Result<()> {
    let config = Config::with_custom_path(id, T::VERSION, out.to_path_buf())
        .map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
    entry.write_entry(&config).map_err(|e| anyhow::anyhow!("{id}: {e}"))
}

/// libadwaita's nine accent presets (`AdwAccentColor`), the values of
/// org.gnome.desktop.interface accent-color.
const ACCENTS: [(&str, [u8; 3]); 9] = [
    ("blue", [0x35, 0x84, 0xe4]),
    ("teal", [0x21, 0x90, 0xa4]),
    ("green", [0x3a, 0x94, 0x4a]),
    ("yellow", [0xc8, 0x88, 0x00]),
    ("orange", [0xed, 0x5b, 0x00]),
    ("red", [0xe6, 0x2d, 0x42]),
    ("pink", [0xd5, 0x61, 0x99]),
    ("purple", [0x91, 0x41, 0xac]),
    ("slate", [0x6f, 0x83, 0x96]),
];

/// Key name to RON text: one built theme as cosmic-config lays it out on disk.
type Files = BTreeMap<String, String>;

fn read_files(dir: &Path) -> Result<Files> {
    let mut files = Files::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("a key name that is not UTF-8 in {}", dir.display()))?
            .to_owned();
        files.insert(name, std::fs::read_to_string(&path)?);
    }
    Ok(files)
}

/// The files of `theme` as cosmic-config writes them, through a scratch directory.
fn theme_files(theme: &Theme, theme_id: &str, scratch: &Path) -> Result<Files> {
    if scratch.exists() {
        std::fs::remove_dir_all(scratch)?;
    }
    write(theme, theme_id, scratch)?;
    read_files(&scratch.join("cosmic").join(theme_id).join(format!("v{}", Theme::VERSION)))
}

fn builder(stock: ThemeBuilder, mode: &Mode) -> ThemeBuilder {
    let mut builder = stock
        .accent(rgb(mode.accent))
        .bg_color(rgba(mode.bg_color))
        .primary_container_bg(rgba(mode.primary_container_bg));
    if let Some(tint) = mode.neutral_tint {
        builder = builder.neutral_tint(rgb(tint));
    }
    if let Some(tint) = mode.text_tint {
        builder = builder.text_tint(rgb(tint));
    }
    builder
}

fn write_mode(builder: &ThemeBuilder, builder_id: &str, theme_id: &str, out: &Path) -> Result<()> {
    write(builder, builder_id, out)?;
    let theme: Theme = builder.clone().build();
    write(&theme, theme_id, out)
}

/// contrast name -> accent name -> the built theme, with the keys of `default`. A high-contrast
/// variant is what cosmic-settings' accessibility page builds: the same builder with the
/// high-contrast palette.
fn variants(builder: &ThemeBuilder, theme_id: &str, default: &Files, scratch: &Path) -> Result<BTreeMap<&'static str, BTreeMap<&'static str, Files>>> {
    let mut by_contrast = BTreeMap::new();
    for contrast in ["normal", "high"] {
        let mut base = builder.clone();
        if contrast == "high" {
            base.palette = if base.palette.is_dark() {
                CosmicPalette::HighContrastDark(base.palette.inner())
            } else {
                CosmicPalette::HighContrastLight(base.palette.inner())
            };
        }
        let mut by_accent = BTreeMap::new();
        for (name, [r, g, b]) in ACCENTS {
            let accent = Srgb::new(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
            let files = theme_files(&base.clone().accent(accent).build(), theme_id, scratch)?;
            if files.keys().ne(default.keys()) {
                bail!("{theme_id} {contrast} {name}: the keys differ from the default's");
            }
            by_accent.insert(name, files);
        }
        by_contrast.insert(contrast, by_accent);
    }
    Ok(by_contrast)
}

/// cosmic-config creates the directory of the previous schema version as a side effect.
/// An empty `v1` served from the overlay would shadow COSMIC's own `v1` defaults.
fn remove_empty_dirs(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            remove_empty_dirs(&path)?;
            if std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(&path)?;
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let [_, inputs_path, out] = args.as_slice() else {
        bail!("usage: calmo-cosmic-theme <cosmic-inputs.json> <out-dir>");
    };
    let out = PathBuf::from(out);
    let raw = std::fs::read(inputs_path).with_context(|| format!("reading {inputs_path}"))?;
    let inputs: Inputs = serde_json::from_slice(&raw).context("parsing the inputs")?;

    if out.join("cosmic").exists() {
        std::fs::remove_dir_all(out.join("cosmic"))?;
    }
    let light = builder(ThemeBuilder::light(), &inputs.light);
    let dark = builder(ThemeBuilder::dark(), &inputs.dark);
    write_mode(&light, LIGHT_THEME_BUILDER_ID, LIGHT_THEME_ID, &out)?;
    write_mode(&dark, DARK_THEME_BUILDER_ID, DARK_THEME_ID, &out)?;
    // Calmo is light by default (doc_shell.md, SH5); COSMIC's stock default is dark.
    write(&ThemeMode { is_dark: false, auto_switch: false }, THEME_MODE_ID, &out)?;
    remove_empty_dirs(&out.join("cosmic"))?;

    let scratch = std::env::temp_dir().join(format!("calmo-cosmic-theme-{}", std::process::id()));
    let mut all = BTreeMap::new();
    for (mode, builder, theme_id) in [("light", &light, LIGHT_THEME_ID), ("dark", &dark, DARK_THEME_ID)] {
        let default = read_files(&out.join("cosmic").join(theme_id).join(format!("v{}", Theme::VERSION)))?;
        all.insert(mode, variants(builder, theme_id, &default, &scratch)?);
    }
    std::fs::remove_dir_all(&scratch)?;
    let mut json = serde_json::to_string_pretty(&all)?;
    json.push('\n');
    std::fs::write(out.join("variants.json"), json)?;

    std::fs::write(out.join("STAMP"), format!("{}\n", hex::encode(Sha256::digest(&raw))))?;
    Ok(())
}
