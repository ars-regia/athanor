//! The compositor's half of the appearance (doc_visual_language.md, VL4; spike VL-S1).
//! athanor-style's apply function ends by calling [`write`], which writes `appearance.toml` of
//! the preference store, the values the own compositor draws the focus outline with
//! (`doc_own_stack.md` decision 38), and, while cosmic-comp and COSMIC's applications still read
//! them, COSMIC's built themes. The shell reads the appearance from GSettings
//! (`athanor_style::appearance`); the themes are read back once, by the import, to start
//! `appearance.toml` from what the outline showed before it existed ([`read_cosmic_in`]).
//!
//! The file lives in a subdirectory of the store, [`DIR`], with its own lock: the bar and the
//! control center write it, so their write grants name that subdirectory and not the store's
//! directory, where the files of the other families are.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use athanor_layout::atomic::{replace, sync_dir};
use athanor_style::appearance::{Accent, Appearance};

use crate::cosmic_config;
use crate::store::{Error, Locked, Store};
use toml::{Table, Value};

/// The file, in the store's subdirectory [`DIR`].
pub const FILE: &str = "appearance.toml";

/// The subdirectory of the store that holds [`FILE`], with its own lock.
pub const DIR: &str = "appearance";

/// The store of [`FILE`]: the subdirectory [`DIR`] of the preference store.
pub fn store() -> Result<Store, Error> {
    Ok(Store::from_env()?.sub(DIR))
}

/// Creates that subdirectory if it is absent and returns its canonical path, a link
/// resolved: the one directory the bar and the control center are granted for the file.
pub fn confinement_dir() -> Result<PathBuf, Error> {
    store()?.dir_for_confinement()
}

/// The outline's width in logical pixels, `active_hint` of Calmo's built themes.
const WIDTH: i64 = 3;
/// `radius_s` of Calmo's built themes, for each corner.
const RADIUS_S: f64 = 8.0;

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

/// The divider of Calmo's built `background`, `#rrggbbaa`, by mode and contrast: the values of
/// `variants.json`, which a test compares.
const fn divider(dark: bool, high_contrast: bool) -> &'static str {
    match (dark, high_contrast) {
        (true, false) => "#393b4aff",
        (true, true) => "#767884ff",
        (false, false) => "#c3c5d1ff",
        (false, true) => "#858797ff",
    }
}

/// Writes `appearance.toml`: the mode, the accent, the width, the corners and the divider of
/// Calmo's theme for `appearance`'s mode and contrast. `window_hint` is removed, since Calmo's
/// themes set none. A key the file holds that this does not name is kept.
pub fn write_store(store: &Store, appearance: &Appearance) -> Result<(), Error> {
    store.update(FILE, |_, table| {
        fill(table, appearance);
        Ok(())
    })
}

/// The keys of [`FILE`] for `appearance`.
fn fill(table: &mut Table, appearance: &Appearance) {
    table.insert("is_dark".to_owned(), Value::Boolean(appearance.dark));
    table.insert(
        "accent".to_owned(),
        Value::String(appearance.accent.rgb().hex()),
    );
    table.remove("window_hint");
    table.insert("active_hint".to_owned(), Value::Integer(WIDTH));
    table.insert(
        "radius_s".to_owned(),
        Value::Array(vec![Value::Float(RADIUS_S); 4]),
    );
    let divider = divider(appearance.dark, appearance.high_contrast);
    table.insert("divider".to_owned(), Value::String(divider.to_owned()));
}

/// Writes [`FILE`] for `appearance` under the store's lock that the import holds: the one
/// other place the file is written, once, when it is absent. The caller checked that.
/// `active_hint` is a width the outline always had; the user's own is not imported (ADR-0150
/// point 6, question 6).
pub(crate) fn write_imported(
    locked: &Locked<'_>,
    appearance: &Appearance,
) -> Result<(), Error> {
    locked.update(FILE, |_, table| {
        fill(table, appearance);
        Ok(())
    })
}

/// The compositor's half of the apply function: `appearance.toml` of the store, then COSMIC's
/// keys (`write_cosmic`) while they are still read. Each is tried whatever the other does.
/// An error of the file is returned, so that the caller logs it; COSMIC's is logged and never
/// returned, and also logged when both fail: COSMIC's keys alone failing leaves the outline
/// right and only COSMIC's applications behind.
// ponytail: runs on the caller's thread, the interface's; the store's lock waits at most
// `LOCK_BOUND` there, once, and the rest is a few small files. Off the thread when measured.
pub fn write(appearance: &Appearance) -> io::Result<()> {
    let stored = store().and_then(|store| write_store(&store, appearance));
    combine(stored, write_cosmic(appearance))
}

/// What [`write`] returns for the outcome of each half.
fn combine(stored: Result<(), Error>, cosmic: io::Result<()>) -> io::Result<()> {
    match (stored, cosmic) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(err)) => {
            tracing::warn!(error = %err, "cannot write COSMIC's theme keys; the applications that still read them keep the previous appearance");
            Ok(())
        }
        (Err(err), cosmic) => {
            if let Err(cosmic) = cosmic {
                tracing::warn!(error = %cosmic, "cannot write COSMIC's theme keys either");
            }
            Err(io::Error::other(err))
        }
    }
}

/// The appearance COSMIC's keys hold under `dirs`, the user's first (the first copy of a key
/// answers): the mode, then the built theme of that mode: its `is_high_contrast` gives the
/// contrast, since the text of `accent` is the same for both contrasts, and that text matched
/// against `variants` (the text of [`VARIANTS`]) gives the accent. `None` when no copy has the
/// mode.
/// `InvalidData` when the mode or the theme is not one this module writes: a custom accent
/// set elsewhere has no value of ours to give.
pub fn read_cosmic_in(dirs: &[PathBuf], variants: &str) -> io::Result<Option<Appearance>> {
    let Some(mode) = cosmic_config::key_at(dirs, MODE, MODE_VERSION, "is_dark")? else {
        // No mode of the user's: none when no accent of either theme is, since then the user
        // has no theme of their own; an accent without a mode is not one of ours.
        for component in [DARK, LIGHT] {
            if cosmic_config::key_at(dirs, component, THEME_VERSION, "accent")?.is_some() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "an accent and no mode"));
            }
        }
        return Ok(None);
    };
    let invalid = |reason: String| io::Error::new(io::ErrorKind::InvalidData, reason);
    let dark = match mode.trim() {
        "true" => true,
        "false" => false,
        other => return Err(invalid(format!("is_dark is {other:?}"))),
    };
    let (component, name) = if dark { (DARK, "dark") } else { (LIGHT, "light") };
    let Some(theme) = cosmic_config::key_at(dirs, component, THEME_VERSION, "accent")? else {
        return Err(invalid(format!("no {name} theme")));
    };
    let high_contrast = match cosmic_config::key_at(dirs, component, THEME_VERSION, "is_high_contrast")? {
        Some(text) if text.trim() == "true" => true,
        Some(text) if text.trim() == "false" => false,
        other => return Err(invalid(format!("is_high_contrast of the {name} theme is {other:?}"))),
    };
    let contrast = if high_contrast { "high" } else { "normal" };
    let variants: Variants = serde_json::from_str(variants)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    for (accent, keys) in variants.get(name).and_then(|modes| modes.get(contrast)).into_iter().flatten() {
        if keys.get("accent") == Some(&theme) {
            let accent = Accent::from_name(accent)
                .ok_or_else(|| invalid(format!("{accent} is not an accent")))?;
            return Ok(Some(Appearance {
                dark,
                high_contrast,
                accent,
                ..Appearance::default()
            }));
        }
    }
    Err(invalid(format!("the {name} theme is none that Athanor ships")))
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

    /// The file belongs to its own subdirectory, so that the grant of the processes that write
    /// it names that subdirectory and not the store's directory.
    #[test]
    fn the_file_is_in_its_own_subdirectory_with_its_own_lock() {
        let dir = cosmic_dir("subdir");
        let root = Store::at(dir.join("athanor/sulphur"));
        let sub = root.sub(DIR);
        write_store(&sub, &Appearance::default()).expect("write");
        assert!(root.dir().join(DIR).join(FILE).is_file());
        assert!(root.dir().join(DIR).join(".lock").is_file());
        assert!(!root.dir().join(FILE).exists() && !root.dir().join(".lock").exists());
        let granted = sub.dir_for_confinement().expect("the grant");
        let store_dir = fs::canonicalize(root.dir()).expect("store");
        assert_eq!(granted, store_dir.join(DIR));
        assert_ne!(granted, store_dir, "never the store's directory");
    }

    /// The error of the file is the apply function's; COSMIC's is logged and never returned,
    /// alone or with the file's.
    #[test]
    fn an_error_of_the_file_is_returned_and_one_of_cosmic_alone_is_not() {
        let cosmic = || Err(io::Error::other("no themes"));
        let file = || {
            Err(Error::Refused {
                path: PathBuf::from("appearance.toml"),
                reason: "refused".to_owned(),
            })
        };
        assert!(combine(Ok(()), Ok(())).is_ok());
        assert!(combine(Ok(()), cosmic()).is_ok());
        let err = combine(file(), Ok(())).expect_err("the file");
        assert!(err.to_string().contains("appearance.toml"), "{err}");
        let err = combine(file(), cosmic()).expect_err("both");
        assert!(err.to_string().contains("appearance.toml"), "{err}");
    }

    /// COSMIC's keys read back give the choice that wrote them: every mode, contrast and accent.
    #[test]
    fn cosmic_state_reads_back_as_the_choice_that_wrote_it() {
        let dir = cosmic_dir("read-back");
        let dirs = [dir.clone()];
        assert!(read_cosmic_in(&dirs, &variants()).expect("nothing").is_none());
        for dark in [false, true] {
            for high_contrast in [false, true] {
                for accent in [Accent::Purple, Accent::Teal] {
                    let choice = Appearance {
                        dark,
                        high_contrast,
                        accent,
                        ..Appearance::default()
                    };
                    write_cosmic_in(&dir, &variants(), &choice).expect("write");
                    let read = read_cosmic_in(&dirs, &variants()).expect("read");
                    assert_eq!(read, Some(choice), "{choice:?}");
                }
            }
        }
        // A theme none of ours is refused as invalid data, not read as a default.
        let light = cosmic_config::versioned(&dir, LIGHT, THEME_VERSION);
        let dark = cosmic_config::versioned(&dir, DARK, THEME_VERSION);
        for theme in [light, dark] {
            fs::write(theme.join("accent"), "custom").expect("custom");
        }
        let err = read_cosmic_in(&dirs, &variants()).expect_err("custom");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
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

    /// The first quoted `#rrggbbaa` after `after` in `text`, lowercase.
    fn hex_after(text: &str, after: &str) -> String {
        let rest = &text[text.find(after).expect(after) + after.len()..];
        let start = rest.find('"').expect("a quote") + 1;
        rest[start..start + 9].to_ascii_lowercase()
    }

    /// Every mode, contrast and accent reaches `appearance.toml` as the values of Calmo's built
    /// theme that `variants.json` holds: the accent, the divider, the width and the corners.
    #[test]
    fn each_mode_contrast_and_accent_reaches_the_file_as_the_built_theme_has_it() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../athanor-style/calmo/generated/cosmic/variants.json");
        let shipped = fs::read_to_string(shipped).expect("variants.json");
        let variants: Variants = serde_json::from_str(&shipped).expect("variants");
        let dir = cosmic_dir("store-shipped");
        let store = Store::at(dir.clone());
        for dark in [false, true] {
            for high_contrast in [false, true] {
                for accent in Accent::ALL {
                    let choice = Appearance {
                        dark,
                        high_contrast,
                        accent,
                        ..Appearance::default()
                    };
                    write_store(&store, &choice).expect("write");
                    let file = store.lock().expect("lock").read(FILE).expect("read").expect("a file");
                    let mode = if dark { "dark" } else { "light" };
                    let contrast = if high_contrast { "high" } else { "normal" };
                    let theme = &variants[mode][contrast][accent.name()];
                    let what = format!("{mode} {contrast} {accent:?}");
                    assert_eq!(file["is_dark"].as_bool(), Some(dark), "{what}");
                    let built = hex_after(&theme["accent"], "base:");
                    assert_eq!(file["accent"].as_str(), Some(&built[..7]), "{what}");
                    assert_eq!(&built[7..], "ff", "{what}: an opaque accent");
                    let divider = hex_after(&theme["background"], "\n    divider:");
                    assert_eq!(file["divider"].as_str(), Some(divider.as_str()), "{what}");
                    assert_eq!(theme["window_hint"], "None", "{what}");
                    assert!(!file.contains_key("window_hint"), "{what}");
                    assert_eq!(
                        theme["active_hint"],
                        file["active_hint"].as_integer().expect("width").to_string(),
                        "{what}"
                    );
                    assert!(
                        theme["corner_radii"].contains("radius_s: (8.0, 8.0, 8.0, 8.0)"),
                        "{what}"
                    );
                    let radius: Vec<f64> = file["radius_s"]
                        .as_array()
                        .expect("four radii")
                        .iter()
                        .filter_map(Value::as_float)
                        .collect();
                    assert_eq!(radius, [8.0; 4], "{what}");
                }
            }
        }
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The text Sulphur's `focus_theme.rs` compares in its `WRITTEN`: the factory appearance,
    /// light, normal contrast and purple, which is the compositor's factory theme.
    #[test]
    fn the_default_is_what_the_compositor_reads_as_the_factory_theme() {
        let written = "schema = 1\nis_dark = false\naccent = \"#9141ac\"\nactive_hint = 3\n\
            radius_s = [8.0, 8.0, 8.0, 8.0]\ndivider = \"#c3c5d1ff\"\n";
        let dark = Appearance::default();
        let dir = cosmic_dir("store-golden");
        let store = Store::at(dir.clone());
        write_store(&store, &dark).expect("write");
        let file = store.lock().expect("lock").read(FILE).expect("read").expect("a file");
        assert_eq!(file, written.parse::<toml::Table>().expect("toml"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A high-contrast change rewrites the divider, and the keys the writer does not name stay.
    #[test]
    fn a_contrast_change_is_written_and_unknown_keys_are_kept() {
        let dir = cosmic_dir("store-contrast");
        fs::write(
            dir.join(FILE),
            "schema = 1\nwindow_hint = \"#ff0000\"\nextra = 5\n[more]\nkey = true\n",
        )
        .expect("seed");
        let store = Store::at(dir.clone());
        let normal = Appearance::default();
        write_store(&store, &normal).expect("write");
        let before = store.lock().expect("lock").read(FILE).expect("read").expect("a file");
        let high = Appearance {
            high_contrast: true,
            ..normal
        };
        write_store(&store, &high).expect("write");
        let after = store.lock().expect("lock").read(FILE).expect("read").expect("a file");
        assert_ne!(before["divider"], after["divider"]);
        assert_eq!(after["extra"].as_integer(), Some(5));
        assert_eq!(after["more"]["key"].as_bool(), Some(true));
        assert!(!after.contains_key("window_hint"), "Calmo sets none");
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The README lists the file's keys, which are the keys the writer writes, both ways.
    #[test]
    fn the_keys_are_the_readmes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/README.md"
        );
        let text = fs::read_to_string(path).expect("Sulphur's README");
        let listed: std::collections::BTreeSet<String> = text
            .lines()
            .skip_while(|line| !line.starts_with("| Appearance key | Accepted values |"))
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .filter_map(|line| line.split('`').nth(1))
            .map(str::to_owned)
            .collect();
        let dir = cosmic_dir("store-readme");
        let store = Store::at(dir.clone());
        write_store(&store, &Appearance::default()).expect("write");
        let file = store.lock().expect("lock").read(FILE).expect("read").expect("a file");
        let mut written: std::collections::BTreeSet<String> = file.keys().cloned().collect();
        written.insert("window_hint".to_owned());
        assert_eq!(written, listed);
        fs::remove_dir_all(dir).expect("cleanup");
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
                    // The import reads back the choice that the apply keys hold, with the
                    // shipped text, where the accent's text is the same for both contrasts.
                    let read = read_cosmic_in(std::slice::from_ref(&dir), &shipped).expect("read");
                    assert_eq!(read, Some(choice), "{choice:?}");
                }
            }
        }
    }
}
