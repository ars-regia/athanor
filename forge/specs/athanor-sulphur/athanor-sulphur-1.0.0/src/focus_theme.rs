//! The focus outline's width and colour, and the windows' corners and divider (decision 31, #918),
//! from the user's `appearance.toml` of the preference store (decision 38; `doc_visual_language.md`
//! VL4), which `athanor_sulphur_client::theme` writes from the appearance of VL4. The outline's
//! on or off is `windows.toml`'s `active_hint` (`window_settings.rs`). Nothing here writes. A key
//! that is absent takes the value of Calmo's light, purple theme, `Appearance::default()`, one that cannot be read leaves the last one
//! read, logged: the outline is never lost to a bad file.

use crate::input_settings::{follow_file, table};
use crate::state::State;
use crate::xkb_config::{read, root};
use smithay::reexports::calloop::LoopHandle;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// The file, in its own subdirectory of the store, under the configuration root.
const RELATIVE: &str = "athanor/sulphur/appearance/appearance.toml";
/// The factory accent of `doc_visual_language.md`, `#9141ac`.
const FACTORY_ACCENT: [f32; 4] = [
    0x91 as f32 / 255.0,
    0x41 as f32 / 255.0,
    0xac as f32 / 255.0,
    1.0,
];
const FACTORY_WIDTH: i32 = 3;
/// A wider `active_hint` is refused, so that a bad value does not cover the desktop.
const MAX_WIDTH: i64 = 32;
/// Calmo's `radius_s` of 8, as cosmic-comp widens it for a window.
const FACTORY_RADIUS: [i32; 4] = [12; 4];
/// A larger `radius_s` is refused, as a bad value.
const MAX_RADIUS: f64 = 64.0;
/// The divider of Calmo's light, normal-contrast theme, `#c3c5d1`: the factory appearance is
/// light and purple (VL4), and a test compares it with what `athanor_sulphur_client::theme`
/// writes for it.
const FACTORY_DIVIDER: [f32; 4] = [
    0xc3 as f32 / 255.0,
    0xc5 as f32 / 255.0,
    0xd1 as f32 / 255.0,
    1.0,
];

/// What a frame draws: a width in logical pixels and a straight colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub width: i32,
    pub colour: [f32; 4],
}

/// What every window neither maximised nor full-screen draws under cosmic-comp 1.11.0: its
/// corners, top left, top right, bottom right and bottom left, in logical pixels, and the
/// straight colour of its divider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub radius: [i32; 4],
    pub divider: [f32; 4],
}

/// The values last read.
pub struct FocusTheme {
    /// `windows.toml`'s `active_hint` (`window_settings.rs`, decision 38): the outline is drawn
    /// only while it is on.
    pub(crate) hint: bool,
    outline: Outline,
    edge: Edge,
    /// The last `window_hint` read, which a refused one leaves in place.
    window_hint: Option<[f32; 4]>,
}

impl Default for FocusTheme {
    fn default() -> Self {
        Self {
            hint: true,
            outline: Outline {
                width: FACTORY_WIDTH,
                colour: FACTORY_ACCENT,
            },
            edge: Edge {
                radius: FACTORY_RADIUS,
                divider: FACTORY_DIVIDER,
            },
            window_hint: None,
        }
    }
}

/// `#` and `digits` hexadecimal digits, as straight channels.
fn parse_hex(value: &Value, digits: usize) -> Result<[f32; 4], String> {
    let text = value.as_str().ok_or("not a string")?;
    let hex = text
        .strip_prefix('#')
        .filter(|hex| hex.len() == digits && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("not `#` and the hexadecimal digits the key takes")?;
    let value = u32::from_str_radix(hex, 16).map_err(|error| error.to_string())?;
    let value = if digits == 6 {
        value << 8 | 0xff
    } else {
        value
    };
    Ok(value.to_be_bytes().map(|byte| f32::from(byte) / 255.0))
}

fn parse_accent(value: &Value) -> Result<[f32; 4], String> {
    parse_hex(value, 6)
}

fn parse_hint(value: &Value) -> Result<Option<[f32; 4]>, String> {
    parse_hex(value, 6).map(Some)
}

fn parse_divider(value: &Value) -> Result<[f32; 4], String> {
    parse_hex(value, 8)
}

fn parse_bool(value: &Value) -> Result<bool, String> {
    value.as_bool().ok_or_else(|| "not a boolean".to_owned())
}

fn parse_width(value: &Value) -> Result<i32, String> {
    value
        .as_integer()
        .filter(|width| (0..=MAX_WIDTH).contains(width))
        .and_then(|width| i32::try_from(width).ok())
        .ok_or_else(|| format!("not an integer from 0 to {MAX_WIDTH}"))
}

/// `radius_s`, the four small corner radii, widened by 4 from 4 up and rounded, as cosmic-comp
/// 1.11.0 rounds a window's corners.
fn parse_radius(value: &Value) -> Result<[i32; 4], String> {
    let corners: Vec<f64> = value
        .as_array()
        .ok_or("not an array")?
        .iter()
        .map(|corner| {
            corner
                .as_float()
                .or_else(|| corner.as_integer().map(|corner| corner as f64))
                .filter(|corner| (0.0..=MAX_RADIUS).contains(corner))
                .ok_or_else(|| format!("not a number from 0 to {MAX_RADIUS}"))
        })
        .collect::<Result<_, _>>()?;
    let corners: [f64; 4] = corners.try_into().map_err(|_| "not four corners")?;
    Ok(corners.map(|corner| {
        let widened = if corner < 4.0 { corner } else { corner + 4.0 };
        widened.round() as i32
    }))
}

/// The value of `name` read by `parse`: `factory` when the key is absent, `last`, logged, when
/// it cannot be read.
fn key<T>(
    table: &mut Table,
    name: &str,
    parse: fn(&Value) -> Result<T, String>,
    factory: T,
    last: T,
) -> T {
    match table.remove(name) {
        None => factory,
        Some(value) => parse(&value).unwrap_or_else(|reason| {
            tracing::warn!(key = name, %reason, "appearance.toml: a key is refused, the last value stays");
            last
        }),
    }
}

impl FocusTheme {
    /// With `windows.toml`'s hint on and a width, what a frame draws.
    pub fn outline(&self) -> Option<Outline> {
        (self.hint && self.outline.width > 0).then_some(self.outline)
    }

    pub fn edge(&self) -> Edge {
        self.edge
    }

    /// Reads `path`. An absent file gives the factory values; a file that cannot be read, or is
    /// refused whole, changes nothing, logged.
    pub fn load(&mut self, path: &Path) {
        match read(path).map_err(|reason| reason.to_string()) {
            Ok(Some(text)) => {
                if let Err(reason) = self.load_text(&text) {
                    tracing::warn!(?path, %reason, "appearance.toml refused: the theme stays");
                }
            }
            Ok(None) => {
                tracing::info!(?path, "no appearance.toml: the factory theme applies");
                *self = Self {
                    hint: self.hint,
                    ..Self::default()
                };
            }
            Err(reason) => {
                tracing::warn!(?path, %reason, "appearance.toml cannot be read: the theme stays");
            }
        }
    }

    /// Applies one `appearance.toml`; a file of another schema, or not TOML, is refused whole.
    fn load_text(&mut self, text: &str) -> Result<(), String> {
        let mut table = table(text)?;
        let factory = Self::default();
        // `is_dark` is only parsed, so that a value of another type is logged: nothing reads the
        // mode, the other values being those of that mode already.
        key(&mut table, "is_dark", parse_bool, true, true);
        let accent = key(
            &mut table,
            "accent",
            parse_accent,
            factory.outline.colour,
            self.outline.colour,
        );
        let hint = key(&mut table, "window_hint", parse_hint, None, self.window_hint);
        self.window_hint = hint;
        self.outline.colour = hint.unwrap_or(accent);
        self.outline.width = key(
            &mut table,
            "active_hint",
            parse_width,
            factory.outline.width,
            self.outline.width,
        );
        self.edge.radius = key(
            &mut table,
            "radius_s",
            parse_radius,
            factory.edge.radius,
            self.edge.radius,
        );
        self.edge.divider = key(
            &mut table,
            "divider",
            parse_divider,
            factory.edge.divider,
            self.edge.divider,
        );
        for name in table.keys() {
            tracing::warn!(key = %name, "appearance.toml: unknown key");
        }
        Ok(())
    }
}

/// Reads `appearance.toml` of the configuration root and follows it. Without that root the
/// factory theme stays.
pub fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    let Some(root) = root() else {
        tracing::warn!("no configuration root: the focus outline takes the factory theme");
        return;
    };
    follow(state, handle, root.join(RELATIVE));
}

/// Watches `path`, then reads it, so a write between the two is not missed.
pub fn follow(state: &mut State, handle: &LoopHandle<'static, State>, path: PathBuf) {
    follow_file(state, handle, path, State::load_focus_theme);
}

impl State {
    /// Reads `path` and draws the outline with what it holds.
    pub fn load_focus_theme(&mut self, path: &Path) {
        self.focus_theme.load(path);
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `athanor_sulphur_client::theme` writes for the factory appearance, light, normal
    /// contrast and purple, which a test of that module compares: the factory theme.
    const WRITTEN: &str = "schema = 1\nis_dark = false\naccent = \"#9141ac\"\nactive_hint = 3\n\
        radius_s = [8.0, 8.0, 8.0, 8.0]\ndivider = \"#c3c5d1ff\"\n";

    fn loaded(text: &str) -> FocusTheme {
        let mut theme = FocusTheme::default();
        theme.load_text(text).expect("a file");
        theme
    }

    fn hex(colour: &str) -> [f32; 4] {
        let value = Value::String(colour.to_owned());
        parse_hex(&value, colour.len() - 1).expect("a colour")
    }

    #[test]
    fn what_the_writer_writes_for_the_factory_accent_is_the_factory_theme() {
        let theme = loaded(WRITTEN);
        let factory = FocusTheme::default();
        assert_eq!(theme.outline(), factory.outline());
        assert_eq!(theme.edge(), factory.edge());
    }

    #[test]
    fn a_colour_is_hex_of_the_length_its_key_takes() {
        let orange = [1.0, 128.0 / 255.0, 0.0, 1.0];
        assert_eq!(hex("#ff8000"), orange);
        assert_eq!(hex("#ff8000ff"), orange);
        let text = |text: &str| Value::String(text.to_owned());
        assert_eq!(parse_accent(&text("#FF8000")).expect("an accent"), orange);
        assert!(parse_accent(&text("#ff8000ff")).is_err());
        assert!(parse_divider(&text("#ff8000")).is_err());
        assert!(parse_accent(&text("#gg8000")).is_err());
        assert!(parse_accent(&text("ff8000")).is_err());
        assert!(parse_accent(&Value::Integer(0)).is_err());
        assert_eq!(parse_hint(&text("#ff8000")).expect("a hint"), Some(orange));
    }

    /// Each of libadwaita's nine accents, in each mode, reaches the outline as the key says.
    #[test]
    fn each_mode_and_accent_reaches_the_outline() {
        let dividers = [(true, "#393b4aff"), (false, "#c3c5d1ff")];
        for accent in [
            "#3584e4", "#2190a4", "#3a944a", "#c88800", "#ed5b00", "#e62d42", "#d56199", "#9141ac",
            "#6f8396",
        ] {
            for (dark, divider) in dividers {
                let theme = loaded(&format!(
                    "schema = 1\nis_dark = {dark}\naccent = \"{accent}\"\ndivider = \"{divider}\"\n"
                ));
                let outline = theme.outline().expect("an outline");
                assert_eq!(outline.colour, hex(accent), "{accent}");
                assert_eq!(theme.edge().divider, hex(divider), "{dark}");
            }
        }
    }

    #[test]
    fn a_window_hint_wins_over_the_accent_until_it_is_removed() {
        let mut theme = loaded("schema = 1\naccent = \"#112233\"\nwindow_hint = \"#445566\"\n");
        assert_eq!(theme.outline.colour, hex("#445566"));
        theme
            .load_text("schema = 1\naccent = \"#112233\"\n")
            .expect("a file");
        assert_eq!(theme.outline.colour, hex("#112233"));
    }

    #[test]
    fn a_refused_window_hint_keeps_the_last_good_one() {
        let mut theme = loaded("schema = 1\naccent = \"#112233\"\nwindow_hint = \"#445566\"\n");
        theme
            .load_text("schema = 1\naccent = \"#112233\"\nwindow_hint = \"red\"\n")
            .expect("a file");
        assert_eq!(theme.outline.colour, hex("#445566"), "the hint of before");
        theme
            .load_text("schema = 1\naccent = \"#112233\"\nwindow_hint = 3\n")
            .expect("a file");
        assert_eq!(theme.outline.colour, hex("#445566"));
    }

    #[test]
    fn the_file_is_in_the_appearance_subdirectory_of_the_store() {
        assert_eq!(RELATIVE, "athanor/sulphur/appearance/appearance.toml");
    }

    #[test]
    fn a_width_is_bounded() {
        assert_eq!(parse_width(&Value::Integer(3)).expect("a width"), 3);
        assert!(parse_width(&Value::Integer(-1)).is_err());
        assert!(parse_width(&Value::Integer(33)).is_err());
        assert!(parse_width(&Value::Float(3.0)).is_err());
    }

    /// cosmic-comp widens `radius_s` by 4 from 4 up: Calmo's 8 is a window's 12.
    #[test]
    fn a_window_takes_radius_s_widened() {
        let radii = |text: &str| parse_radius(&text.parse::<Value>().expect("a value"));
        assert_eq!(radii("[8.0, 8.0, 8.0, 8.0]").expect("radii"), [12; 4]);
        assert_eq!(
            radii("[2.0, 3.6, 4.0, 10.4]").expect("radii"),
            [2, 4, 8, 14]
        );
        assert_eq!(radii("[8, 8, 8, 8]").expect("integers"), [12; 4]);
        assert!(radii("[8.0, 8.0, 8.0]").is_err());
        assert!(radii("[8.0, 8.0, 8.0, 99.0]").is_err());
        assert!(radii("[8.0, 8.0, -1.0, 8.0]").is_err());
        assert!(radii("[8.0, 8.0, 8.0, \"x\"]").is_err());
        assert!(radii("8.0").is_err());
    }

    /// A key that is absent takes the factory value; one that is refused leaves the last.
    #[test]
    fn an_absent_key_is_the_factory_and_a_bad_one_leaves_the_last() {
        let mut theme = loaded(
            "schema = 1\naccent = \"#112233\"\nactive_hint = 7\nradius_s = [2.0, 2.0, 2.0, 2.0]\n\
             divider = \"#ff0000ff\"\n",
        );
        let read = Outline {
            width: 7,
            colour: hex("#112233"),
        };
        assert_eq!(theme.outline(), Some(read));
        assert_eq!(theme.edge().radius, [2; 4]);
        theme
            .load_text(
                "schema = 1\naccent = \"pink\"\nactive_hint = \"many\"\nradius_s = 1\n\
                 divider = \"#fff\"\nunknown = 1\n",
            )
            .expect("a file");
        assert_eq!(theme.outline(), Some(read), "each bad key leaves its last");
        assert_eq!(theme.edge().radius, [2; 4]);
        assert_eq!(theme.edge().divider, hex("#ff0000ff"));
        theme.load_text("schema = 1\n").expect("a file");
        let factory = FocusTheme::default();
        assert_eq!(theme.outline(), factory.outline());
        assert_eq!(theme.edge(), factory.edge());
    }

    /// A file of another schema, or not TOML, is refused whole.
    #[test]
    fn a_file_of_another_schema_is_refused() {
        let mut theme = loaded("schema = 1\nactive_hint = 5\n");
        assert!(theme.load_text("schema = 2\nactive_hint = 6\n").is_err());
        assert!(theme.load_text("active_hint = 6\n").is_err());
        assert!(theme.load_text("schema = 1\nactive_hint = \n").is_err());
        assert_eq!(theme.outline.width, 5);
    }

    #[test]
    fn the_outline_is_off_with_the_hint_or_no_width() {
        let mut theme = FocusTheme::default();
        assert!(theme.outline().is_some());
        theme.hint = false;
        assert_eq!(theme.outline(), None);
        theme.hint = true;
        theme.outline.width = 0;
        assert_eq!(theme.outline(), None);
    }

    /// An absent file gives the factory values back and keeps `windows.toml`'s hint.
    #[test]
    fn a_removed_file_gives_the_factory_theme_back() {
        let dir = std::env::temp_dir().join(format!("athanor-focus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        let file = dir.join("appearance.toml");
        let mut theme = FocusTheme {
            hint: false,
            ..FocusTheme::default()
        };
        std::fs::write(&file, "schema = 1\nactive_hint = 9\n").expect("a file");
        theme.load(&file);
        assert_eq!(theme.outline.width, 9);
        std::fs::remove_file(&file).expect("removed");
        theme.load(&file);
        assert_eq!(theme.outline.width, FACTORY_WIDTH);
        assert!(!theme.hint, "windows.toml's hint is not this file's");
        std::fs::remove_dir_all(dir).expect("the directory");
    }
}
