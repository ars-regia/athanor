//! The user's COSMIC appearance, for our surfaces inside the user session (doc_shell.md,
//! SH5): light or dark, high contrast, and the accent. The greeter never reads it: it
//! runs before any user exists.
//!
//! A key that cannot be read keeps Calmo's default.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use athanor_style::calmo::Variant;
use gtk4::{gdk, gio, prelude::*};

use crate::cosmic_config::{self, key};

const MODE: &str = "com.system76.CosmicTheme.Mode";
const DARK: &str = "com.system76.CosmicTheme.Dark";
const LIGHT: &str = "com.system76.CosmicTheme.Light";

/// Calmo's ink on light accents (`ath_acc_ink` of the dark variant) and plain white.
const DARK_INK: Rgb = Rgb {
    red: 0x0d,
    green: 0x11,
    blue: 0x26,
};
const WHITE: Rgb = Rgb {
    red: 0xff,
    green: 0xff,
    blue: 0xff,
};
/// WCAG 2.x, level AA, normal text.
const MIN_CONTRAST: f64 = 4.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CosmicTheme {
    pub is_dark: bool,
    pub is_high_contrast: bool,
    /// The user's accent, when COSMIC has one on disk.
    pub accent: Option<Rgb>,
}

impl Default for CosmicTheme {
    /// COSMIC's own default mode is dark.
    fn default() -> Self {
        CosmicTheme {
            is_dark: true,
            is_high_contrast: false,
            accent: None,
        }
    }
}

/// The theme from the user's COSMIC configuration, then the system's.
pub fn read() -> CosmicTheme {
    read_from(&cosmic_config::dirs())
}

/// The theme from `dirs`, each a `cosmic` configuration directory, highest first.
pub fn read_from(dirs: &[PathBuf]) -> CosmicTheme {
    let default = CosmicTheme::default();
    let is_dark = key(dirs, MODE, "is_dark")
        .and_then(|text| parse_bool(&text))
        .unwrap_or(default.is_dark);
    let theme = if is_dark { DARK } else { LIGHT };
    CosmicTheme {
        is_dark,
        is_high_contrast: key(dirs, theme, "is_high_contrast")
            .and_then(|text| parse_bool(&text))
            .unwrap_or(false),
        accent: key(dirs, theme, "accent").and_then(|text| parse_accent(&text)),
    }
}

fn parse_bool(text: &str) -> Option<bool> {
    text.trim().parse().ok()
}

/// The `base` colour of a COSMIC accent: `base: ( red: <0..1>, green: …, blue: …, … )`.
fn parse_accent(text: &str) -> Option<Rgb> {
    let start = text.find("base:")? + "base:".len();
    let body = &text[start..];
    let body = &body[body.find('(')? + 1..body.find(')')?];
    let channel = |name: &str| -> Option<u8> {
        let value = body
            .split(',')
            .find_map(|field| field.trim().strip_prefix(name)?.trim().strip_prefix(':'))?;
        let value: f64 = value.trim().parse().ok()?;
        (0.0..=1.0)
            .contains(&value)
            .then(|| (value * 255.0).round() as u8)
    };
    Some(Rgb {
        red: channel("red")?,
        green: channel("green")?,
        blue: channel("blue")?,
    })
}

fn luminance(colour: Rgb) -> f64 {
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour.red) + 0.7152 * linear(colour.green) + 0.0722 * linear(colour.blue)
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The text colour on `accent`: white or Calmo's dark ink, whichever contrasts more,
/// and only when that passes WCAG AA.
pub fn on_accent(accent: Rgb) -> Option<Rgb> {
    let ink = if contrast(accent, WHITE) >= contrast(accent, DARK_INK) {
        WHITE
    } else {
        DARK_INK
    };
    (contrast(accent, ink) >= MIN_CONTRAST).then_some(ink)
}

impl CosmicTheme {
    pub fn variant(&self) -> Variant {
        let base = if self.is_dark {
            Variant::Dark
        } else {
            Variant::Light
        };
        base.with_high_contrast(self.is_high_contrast)
    }

    /// The named colours that replace Calmo's accent, or `None` to keep it: in high
    /// contrast, whose accent the contrast gate has checked, and for an accent no text
    /// colour can be read on.
    pub fn accent_css(&self) -> Option<String> {
        if self.is_high_contrast {
            return None;
        }
        let accent = self.accent?;
        let Some(ink) = on_accent(accent) else {
            tracing::warn!(accent = %accent.hex(), "no text colour passes WCAG AA on the COSMIC accent; Calmo's accent is used");
            return None;
        };
        Some(format!(
            "@define-color ath_acc {};\n@define-color ath_acc_ink {};\n",
            accent.hex(),
            ink.hex()
        ))
    }
}

thread_local! {
    static ACCENT_PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

/// Installs the user's accent above the Calmo sheet (calmo.rs names this mechanism).
/// Called again after a change, it replaces the accent it installed, or removes it when
/// the theme has none any more.
pub fn load_accent(display: &gdk::Display, theme: &CosmicTheme) {
    let provider = theme.accent_css().map(|css| {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&css);
        provider
    });
    let previous = ACCENT_PROVIDER.with(|slot| slot.replace(provider.clone()));
    if let Some(previous) = previous {
        gtk4::style_context_remove_provider_for_display(display, &previous);
    }
    if let Some(provider) = provider {
        gtk4::style_context_add_provider_for_display(
            display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
}

/// Calls `on_change` with the new theme each time the user's theme keys change it. The
/// watch lasts as long as the returned monitors. A directory that does not exist yet is
/// watched too: GIO polls for it.
#[must_use = "the watch stops when the monitors are dropped"]
pub fn watch(on_change: impl Fn(CosmicTheme) + 'static) -> Vec<gio::FileMonitor> {
    let Some(user) = cosmic_config::user_dir() else {
        tracing::warn!("no home directory: theme changes are not followed");
        return Vec::new();
    };
    let last = std::rc::Rc::new(Cell::new(read()));
    let on_change = std::rc::Rc::new(on_change);
    [MODE, DARK, LIGHT]
        .into_iter()
        .filter_map(|component| {
            let dir = gio::File::for_path(cosmic_config::component(&user, component));
            match dir.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                Ok(monitor) => Some(monitor),
                Err(err) => {
                    tracing::warn!(component, "theme changes are not followed: {err}");
                    None
                }
            }
        })
        .inspect(|monitor| {
            let (last, on_change) = (last.clone(), on_change.clone());
            monitor.connect_changed(move |_, _, _, _| {
                let theme = read();
                if last.replace(theme) != theme {
                    on_change(theme);
                }
            });
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    const ACCENT: &str = "(\n    base: (\n        red: 0.3882353,\n        green: 0.8156863,\n        blue: 0.8745098,\n        alpha: 1.0,\n    ),\n    hover: (\n        red: 0.1,\n        green: 0.1,\n        blue: 0.1,\n        alpha: 1.0,\n    ),\n)";

    fn cosmic_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-theme-{}-{name}", std::process::id()));
        let _fresh = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn put(dir: &Path, component: &str, key: &str, text: &str) {
        let path = dir.join(component).join("v1").join(key);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    #[test]
    fn nothing_readable_is_calmo_dark_without_an_accent() {
        let theme = read_from(&[cosmic_dir("empty")]);
        assert_eq!(theme, CosmicTheme::default());
        assert_eq!(theme.variant(), Variant::Dark);
        assert_eq!(theme.accent_css(), None);
    }

    #[test]
    fn the_user_key_wins_over_the_system_key_one_key_at_a_time() {
        let (user, system) = (cosmic_dir("user"), cosmic_dir("system"));
        put(&user, MODE, "is_dark", "false\n");
        put(&system, MODE, "is_dark", "true");
        put(&system, LIGHT, "is_high_contrast", "true");
        put(&system, LIGHT, "accent", ACCENT);
        let theme = read_from(&[user, system]);
        assert!(!theme.is_dark);
        assert!(
            theme.is_high_contrast,
            "read from the system file: the user has none"
        );
        assert_eq!(
            theme.accent,
            Some(Rgb {
                red: 99,
                green: 208,
                blue: 223
            })
        );
        assert_eq!(theme.variant(), Variant::LightHc);
    }

    #[test]
    fn the_accent_is_the_base_colour_not_the_first_one_in_the_file() {
        let reordered = "(\n    hover: ( red: 0.1, green: 0.1, blue: 0.1, alpha: 1.0 ),\n    base: ( red: 1.0, green: 0.5, blue: 0.0, alpha: 1.0 ),\n)";
        assert_eq!(
            parse_accent(reordered),
            Some(Rgb {
                red: 255,
                green: 128,
                blue: 0
            })
        );
        assert_eq!(parse_accent("( hover: ( red: 0.1 ) )"), None);
        assert_eq!(
            parse_accent("( base: ( red: 2.0, green: 0.0, blue: 0.0 ) )"),
            None,
            "out of range"
        );
    }

    #[test]
    fn text_on_the_accent_passes_wcag_aa() {
        // Calmo's own dark accent takes Calmo's dark ink, as the stylesheets do.
        assert_eq!(
            on_accent(Rgb {
                red: 0x88,
                green: 0x98,
                blue: 0xf7
            }),
            Some(DARK_INK)
        );
        assert_eq!(
            on_accent(Rgb {
                red: 0x1f,
                green: 0x3a,
                blue: 0x93
            }),
            Some(WHITE)
        );
        for accent in [
            Rgb {
                red: 0x77,
                green: 0x77,
                blue: 0x77,
            },
            Rgb {
                red: 0xff,
                green: 0x00,
                blue: 0x00,
            },
        ] {
            if let Some(ink) = on_accent(accent) {
                assert!(contrast(accent, ink) >= 4.5);
            }
        }
    }

    #[test]
    fn high_contrast_keeps_the_gated_calmo_accent() {
        let theme = CosmicTheme {
            is_dark: true,
            is_high_contrast: true,
            accent: Some(Rgb {
                red: 0x1f,
                green: 0x3a,
                blue: 0x93,
            }),
        };
        assert_eq!(theme.accent_css(), None);
        let theme = CosmicTheme {
            is_high_contrast: false,
            ..theme
        };
        assert_eq!(
            theme.accent_css().as_deref(),
            Some("@define-color ath_acc #1f3a93;\n@define-color ath_acc_ink #ffffff;\n")
        );
    }
}
