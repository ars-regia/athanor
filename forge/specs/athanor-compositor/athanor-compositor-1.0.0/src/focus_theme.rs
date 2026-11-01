//! The focus outline's width and colour, from the keys of COSMIC's theme that cosmic-comp reads
//! (doc_own_stack.md decision 31; spike VL-S1, section 2.2). Nothing here writes. A value that
//! cannot be read leaves the last one, logged, and before any was read the outline takes the
//! factory accent: the outline is never lost to a bad file.

use crate::state::State;
use crate::xkb_config::{read, Refused, Watch};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

const MODE: &str = "com.system76.CosmicTheme.Mode/v1";
pub(crate) const DARK: &str = "com.system76.CosmicTheme.Dark";
const LIGHT: &str = "com.system76.CosmicTheme.Light";
pub(crate) const COMP: &str = "com.system76.CosmicComp/v1";
/// `$XDG_DATA_DIRS` when it is unset, as the base directory specification has it.
const DATA_DIRS: &str = "/usr/local/share:/usr/share";
/// The factory accent of `doc_visual_language.md`, `#9141ac`.
const FACTORY_ACCENT: [f32; 4] = [
    0x91 as f32 / 255.0,
    0x41 as f32 / 255.0,
    0xac as f32 / 255.0,
    1.0,
];
const FACTORY_WIDTH: i32 = 3;
/// A wider `active_hint` is refused, so that a bad value does not cover the desktop.
const MAX_WIDTH: i32 = 32;

/// What a frame draws: a width in logical pixels and a straight colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub width: i32,
    pub colour: [f32; 4],
}

/// The values last read, and the `cosmic` directories they come from.
pub struct FocusTheme {
    user: Option<PathBuf>,
    system: Vec<PathBuf>,
    dark: bool,
    pub(crate) enabled: bool,
    outline: Outline,
}

impl Default for FocusTheme {
    fn default() -> Self {
        Self {
            user: None,
            system: system_layers(),
            dark: true,
            enabled: true,
            outline: Outline {
                width: FACTORY_WIDTH,
                colour: FACTORY_ACCENT,
            },
        }
    }
}

/// `#rrggbb` or `#rrggbbaa`, up to the closing quote.
fn parse_hex(text: &str) -> Result<[f32; 4], Refused> {
    let digits = text
        .split('"')
        .next()
        .and_then(|text| text.strip_prefix('#'))
        .filter(|digits| matches!(digits.len(), 6 | 8))
        .filter(|digits| digits.bytes().all(|digit| digit.is_ascii_hexdigit()))
        .ok_or(Refused::Syntax(0))?;
    let value = u32::from_str_radix(digits, 16).map_err(|_| Refused::Syntax(0))?;
    let value = if digits.len() == 6 {
        value << 8 | 0xff
    } else {
        value
    };
    Ok(value.to_be_bytes().map(|byte| f32::from(byte) / 255.0))
}

/// The first colour of `text`: a quoted hex value, as `v2` of a theme holds it, or
/// `(red: r, green: g, blue: b)`, with an optional `alpha`, in any order.
fn parse_colour(text: &str) -> Result<[f32; 4], Refused> {
    let close = text.find(')');
    if let Some(quote) = text
        .find('"')
        .filter(|&at| close.is_none_or(|close| at < close))
    {
        return parse_hex(&text[quote + 1..]);
    }
    let close = close.ok_or(Refused::Syntax(0))?;
    let open = text[..close].rfind('(').ok_or(Refused::Syntax(close))?;
    let mut colour = [None, None, None, Some(1.0)];
    for field in text[open + 1..close].split(',').map(str::trim) {
        if field.is_empty() {
            continue;
        }
        let (name, value) = field.split_once(':').ok_or(Refused::Syntax(open))?;
        let slot = match name.trim() {
            "red" => 0,
            "green" => 1,
            "blue" => 2,
            "alpha" => 3,
            other => return Err(Refused::Unknown(other.into())),
        };
        let value: f32 = value.trim().parse().map_err(|_| Refused::Syntax(open))?;
        if !(0.0..=1.0).contains(&value) {
            return Err(Refused::Syntax(open));
        }
        colour[slot] = Some(value);
    }
    let channel = |slot: usize, name| colour[slot].ok_or(Refused::Missing(name));
    Ok([
        channel(0, "red")?,
        channel(1, "green")?,
        channel(2, "blue")?,
        channel(3, "alpha")?,
    ])
}

/// `window_hint`: `None`, or `Some` of a colour.
fn parse_hint(text: &str) -> Result<Option<[f32; 4]>, Refused> {
    match text.trim() {
        "None" => Ok(None),
        hint if hint.starts_with("Some") => parse_colour(hint).map(Some),
        _ => Err(Refused::Syntax(0)),
    }
}

/// `accent`, a whole `Component`, of which the outline takes `base`, written first.
fn parse_accent(text: &str) -> Result<[f32; 4], Refused> {
    let (_, base) = text.split_once("base:").ok_or(Refused::Missing("base"))?;
    parse_colour(base)
}

fn parse_bool(text: &str) -> Result<bool, Refused> {
    text.trim().parse().map_err(|_| Refused::Syntax(0))
}

fn parse_width(text: &str) -> Result<i32, Refused> {
    match text.trim().parse() {
        Ok(width) if (0..=MAX_WIDTH).contains(&width) => Ok(width),
        _ => Err(Refused::Syntax(0)),
    }
}

impl FocusTheme {
    /// With the setting on and a width, what a frame draws.
    pub fn outline(&self) -> Option<Outline> {
        (self.enabled && self.outline.width > 0).then_some(self.outline)
    }

    /// The directories COSMIC reads `key` from, first found first (spike VL-S1, section 2.1).
    fn layers(&self, component: &str, key: &str) -> Vec<PathBuf> {
        let mut layers = Vec::new();
        if let Some(user) = &self.user {
            layers.push(user.join(component).join("v2").join(key));
            layers.push(user.join(component).join("v1").join(key));
        }
        layers.extend(
            self.system
                .iter()
                .map(|layer| layer.join(component).join("v2").join(key)),
        );
        layers
    }

    /// The first layer's text, parsed. `None` when no layer has the key.
    fn theme_key<T>(
        &self,
        key: &str,
        parse: impl Fn(&str) -> Result<T, Refused>,
    ) -> Option<Result<T, Refused>> {
        let component = if self.dark { DARK } else { LIGHT };
        self.layers(component, key).iter().find_map(|path| {
            read(path)
                .and_then(|text| text.as_deref().map(&parse).transpose())
                .transpose()
        })
    }

    fn file_key<T>(path: &Path, parse: impl Fn(&str) -> Result<T, Refused>) -> Option<T> {
        match read(path).and_then(|text| text.as_deref().map(parse).transpose()) {
            Ok(value) => value,
            Err(reason) => {
                tracing::warn!(?path, %reason, "a value of the focus outline refused: the last stays");
                None
            }
        }
    }

    /// Reads every key again, keeping the last value of one that cannot be read.
    pub fn load(&mut self) {
        if let Some(user) = self.user.clone() {
            if let Some(dark) = Self::file_key(&user.join(MODE).join("is_dark"), parse_bool) {
                self.dark = dark;
            }
            let setting = user.join(COMP).join("active_hint");
            if let Some(enabled) = Self::file_key(&setting, parse_bool) {
                self.enabled = enabled;
            }
        }
        match self.theme_key("active_hint", parse_width) {
            Some(Ok(width)) => self.outline.width = width,
            Some(Err(reason)) => refused("active_hint", &reason),
            None => {}
        }
        match self.theme_key("window_hint", parse_hint) {
            Some(Ok(Some(hint))) => self.outline.colour = hint,
            Some(Err(reason)) => refused("window_hint", &reason),
            Some(Ok(None)) | None => match self.theme_key("accent", parse_accent) {
                Some(Ok(accent)) => self.outline.colour = accent,
                Some(Err(reason)) => refused("accent", &reason),
                None => {}
            },
        }
    }
}

fn refused(key: &str, reason: &Refused) {
    tracing::warn!(key, %reason, "a value of the focus outline refused: the last stays");
}

/// COSMIC's system layers, in the order of `$XDG_DATA_DIRS`.
fn system_layers() -> Vec<PathBuf> {
    let dirs = std::env::var_os("XDG_DATA_DIRS").filter(|dirs| !dirs.is_empty());
    std::env::split_paths(&dirs.unwrap_or_else(|| DATA_DIRS.into()))
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("cosmic"))
        .collect()
}

/// The files whose change is followed. The system layer is in the read-only image.
fn watched(user: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        user.join(MODE).join("is_dark"),
        user.join(COMP).join("active_hint"),
    ];
    for component in [DARK, LIGHT] {
        for key in ["active_hint", "window_hint", "accent"] {
            for version in ["v2", "v1"] {
                paths.push(user.join(component).join(version).join(key));
            }
        }
    }
    paths
}

/// Follows `user`'s theme and reads it. A file that cannot be watched is read once, logged.
pub fn follow(state: &mut State, handle: &LoopHandle<'static, State>, user: PathBuf) {
    // Watched first, then read, so that a write between the two is not missed.
    for path in watched(&user) {
        if let Err(error) = watch(handle, &path) {
            tracing::warn!(%error, ?path, "a value of the focus outline has no live reload");
        }
    }
    state.focus_theme.user = Some(user);
    state.reload_focus_theme();
}

fn watch(
    handle: &LoopHandle<'static, State>,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut watch = Watch::new(path)?;
    let fd = watch.inotify.as_fd().try_clone_to_owned()?;
    handle.insert_source(
        Generic::new(fd, Interest::READ, Mode::Level),
        move |_, _, state| {
            if watch.changed() {
                state.reload_focus_theme();
            }
            Ok(PostAction::Continue)
        },
    )?;
    Ok(())
}

/// Reads the theme of `$XDG_CONFIG_HOME/cosmic` and follows it. Without that root the system
/// layer's values are read once, logged.
pub fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    match root {
        Some(root) => follow(state, handle, root.join("cosmic")),
        None => {
            tracing::warn!("no configuration root: the focus outline takes the system's theme");
            state.reload_focus_theme();
        }
    }
}

impl State {
    /// The theme's values read again, and the outline drawn with them.
    pub fn reload_focus_theme(&mut self) {
        self.focus_theme.load();
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCENT: &str = "(\n    base: (\n        red: 0.25,\n        green: 0.5,\n        \
        blue: 0.75,\n        alpha: 1.0,\n    ),\n    hover: (\n        red: 0.0,\n        \
        green: 0.0,\n        blue: 0.0,\n        alpha: 1.0,\n    ),\n)";

    #[test]
    fn the_accent_is_its_base() {
        assert_eq!(
            parse_accent(ACCENT).expect("an accent"),
            [0.25, 0.5, 0.75, 1.0]
        );
        assert!(parse_accent("(hover: ())").is_err());
    }

    /// A theme's `v2` holds its colours as text.
    #[test]
    fn a_colour_can_be_hex() {
        let accent = "(\n    base: \"#FF8000FF\",\n    hover: \"#000000FF\",\n)";
        let orange = [1.0, 128.0 / 255.0, 0.0, 1.0];
        assert_eq!(parse_accent(accent).expect("an accent"), orange);
        let hint = parse_hint("Some(\"#FF8000\")").expect("a hint");
        assert_eq!(hint, Some(orange));
        assert!(parse_accent("base: \"#FF80\"").is_err());
        assert!(parse_accent("base: \"#GG8000FF\"").is_err());
        assert!(parse_accent("base: \"FF8000FF\"").is_err());
    }

    #[test]
    fn the_hint_is_an_option_of_a_colour() {
        assert_eq!(parse_hint("None").expect("no hint"), None);
        let some = parse_hint("Some((red: 1.0, green: 0.5, blue: 0.0))").expect("a hint");
        assert_eq!(some, Some([1.0, 0.5, 0.0, 1.0]));
        assert!(parse_hint("Some((red: 2.0, green: 0.5, blue: 0.0))").is_err());
        assert!(parse_hint("Some((red: 1.0))").is_err());
        assert!(parse_hint("pink").is_err());
    }

    #[test]
    fn a_width_is_bounded() {
        assert_eq!(parse_width("3\n").expect("a width"), 3);
        assert!(parse_width("-1").is_err());
        assert!(parse_width("33").is_err());
    }

    #[test]
    fn the_outline_is_off_with_the_setting_or_no_width() {
        let mut theme = FocusTheme::default();
        assert!(theme.outline().is_some());
        theme.enabled = false;
        assert_eq!(theme.outline(), None);
        theme.enabled = true;
        theme.outline.width = 0;
        assert_eq!(theme.outline(), None);
    }

    /// `$XDG_DATA_DIRS` orders the system layers, as COSMIC reads them.
    #[test]
    fn the_first_system_layer_wins() {
        let dir = std::env::temp_dir().join(format!("athanor-layers-{}", std::process::id()));
        for (layer, colour) in [("first", "#FF0000FF"), ("second", "#00FF00FF")] {
            let keys = dir.join(layer).join(DARK).join("v2");
            std::fs::create_dir_all(&keys).expect("a directory");
            std::fs::write(keys.join("accent"), format!("(base: \"{colour}\")")).expect("a colour");
        }
        let mut theme = FocusTheme {
            system: vec![dir.join("none"), dir.join("first"), dir.join("second")],
            ..FocusTheme::default()
        };
        theme.load();
        assert_eq!(theme.outline.colour, [1.0, 0.0, 0.0, 1.0]);
        std::fs::remove_dir_all(&dir).expect("removed");
    }

    #[test]
    fn a_bad_file_leaves_the_last_value() {
        let dir = std::env::temp_dir().join(format!("athanor-focus-{}", std::process::id()));
        let keys = dir.join(DARK).join("v2");
        std::fs::create_dir_all(&keys).expect("a directory");
        let mut theme = FocusTheme {
            user: Some(dir.clone()),
            system: vec![dir.join("none")],
            ..FocusTheme::default()
        };
        std::fs::write(
            keys.join("window_hint"),
            "Some((red: 0.5, green: 0.5, blue: 0.5))",
        )
        .expect("a hint");
        std::fs::write(keys.join("active_hint"), "7").expect("a width");
        theme.load();
        let read = Outline {
            width: 7,
            colour: [0.5, 0.5, 0.5, 1.0],
        };
        assert_eq!(theme.outline(), Some(read));
        std::fs::write(keys.join("window_hint"), "pink").expect("a bad hint");
        std::fs::write(keys.join("active_hint"), "many").expect("a bad width");
        theme.load();
        assert_eq!(theme.outline(), Some(read));
        std::fs::remove_dir_all(dir).expect("the directory");
    }
}
