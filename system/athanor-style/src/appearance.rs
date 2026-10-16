//! The appearance preferences (doc_visual_language.md, VL4): where they are read, how a
//! surface follows them, and the one function every writer calls.
//!
//! The store is the GNOME keys `color-scheme`, `accent-color` and `high-contrast`, plus
//! the two keys of `org.athanor.desktop.appearance` GNOME has no key for. A change written
//! by any tool is the user's choice; nothing here keeps a copy that could disagree.
//!
//! GIO aborts the process on a schema or a key it cannot find, and a panic is a loss of
//! service here, so every schema and key is looked up before it is touched. One that is
//! missing reads as its default and refuses writes.

use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;

use gtk4::{gdk, gio, prelude::*};

use crate::calmo::{self, Variant};

const INTERFACE: &str = "org.gnome.desktop.interface";
const A11Y: &str = "org.gnome.desktop.a11y.interface";
const ATHANOR: &str = "org.athanor.desktop.appearance";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }

    /// `#rrggbb`, and nothing else.
    pub fn from_hex(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        Some(Self {
            red: channel(0)?,
            green: channel(2)?,
            blue: channel(4)?,
        })
    }
}

/// libadwaita's nine accent presets, the values of `accent-color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
    Blue,
    Teal,
    Green,
    Yellow,
    Orange,
    Red,
    Pink,
    Purple,
    Slate,
}

impl Accent {
    pub const ALL: [Accent; 9] = [
        Accent::Blue,
        Accent::Teal,
        Accent::Green,
        Accent::Yellow,
        Accent::Orange,
        Accent::Red,
        Accent::Pink,
        Accent::Purple,
        Accent::Slate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Green => "green",
            Accent::Yellow => "yellow",
            Accent::Orange => "orange",
            Accent::Red => "red",
            Accent::Pink => "pink",
            Accent::Purple => "purple",
            Accent::Slate => "slate",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|accent| accent.name() == name)
    }

    /// libadwaita's value (`adw-accent-color.c`), the same in both variants.
    pub fn rgb(self) -> Rgb {
        let (red, green, blue) = match self {
            Accent::Blue => (0x35, 0x84, 0xe4),
            Accent::Teal => (0x21, 0x90, 0xa4),
            Accent::Green => (0x3a, 0x94, 0x4a),
            Accent::Yellow => (0xc8, 0x88, 0x00),
            Accent::Orange => (0xed, 0x5b, 0x00),
            Accent::Red => (0xe6, 0x2d, 0x42),
            Accent::Pink => (0xd5, 0x61, 0x99),
            Accent::Purple => (0x91, 0x41, 0xac),
            Accent::Slate => (0x6f, 0x83, 0x96),
        };
        Rgb { red, green, blue }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccentMode {
    Fixed,
    Wallpaper,
}

impl AccentMode {
    pub fn name(self) -> &'static str {
        match self {
            AccentMode::Fixed => "fixed",
            AccentMode::Wallpaper => "wallpaper",
        }
    }
}

/// The user's appearance, as the store holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub dark: bool,
    pub high_contrast: bool,
    /// The fixed accent; in the wallpaper mode, the preset nearest the computed colour.
    pub accent: Accent,
    pub accent_mode: AccentMode,
    /// The corrected colour of the wallpaper mode; `None` when the key is empty or not
    /// `#rrggbb`, which leaves the fixed accent in force (VL3).
    pub accent_computed: Option<Rgb>,
}

impl Default for Appearance {
    /// The factory settings: light, purple, fixed (VL4).
    fn default() -> Self {
        Appearance {
            dark: false,
            high_contrast: false,
            accent: Accent::Purple,
            accent_mode: AccentMode::Fixed,
            accent_computed: None,
        }
    }
}

/// One schema of the store, opened only when the system has it.
struct Store {
    settings: gio::Settings,
    schema: gio::SettingsSchema,
}

impl Store {
    fn open(id: &str) -> Option<Store> {
        let schema = gio::SettingsSchemaSource::default()?.lookup(id, true)?;
        Some(Store {
            settings: gio::Settings::new(id),
            schema,
        })
    }

    fn string(&self, key: &str) -> Option<String> {
        self.schema
            .has_key(key)
            .then(|| self.settings.string(key).to_string())
    }

    fn boolean(&self, key: &str) -> Option<bool> {
        self.schema.has_key(key).then(|| self.settings.boolean(key))
    }

    fn set_string(&self, key: &str, value: &str) -> io::Result<()> {
        self.writable(key)?;
        self.settings
            .set_string(key, value)
            .map_err(|err| io::Error::other(format!("{key}: {err}")))
    }

    fn set_boolean(&self, key: &str, value: bool) -> io::Result<()> {
        self.writable(key)?;
        self.settings
            .set_boolean(key, value)
            .map_err(|err| io::Error::other(format!("{key}: {err}")))
    }

    fn writable(&self, key: &str) -> io::Result<()> {
        if !self.schema.has_key(key) {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the schema {} has no key {key}", self.schema.id()),
            ))
        } else if !self.settings.is_writable(key) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("the key {key} of {} is locked", self.schema.id()),
            ))
        } else {
            Ok(())
        }
    }
}

/// The three schemas of the store; a missing one is `None` and reads as its defaults.
struct Stores {
    interface: Option<Store>,
    a11y: Option<Store>,
    athanor: Option<Store>,
}

impl Stores {
    fn open() -> Stores {
        let open = |id| {
            let store = Store::open(id);
            if store.is_none() {
                tracing::warn!(
                    schema = id,
                    "schema not installed: its appearance keys keep their defaults"
                );
            }
            store
        };
        Stores {
            interface: open(INTERFACE),
            a11y: open(A11Y),
            athanor: open(ATHANOR),
        }
    }

    fn read(&self) -> Appearance {
        let default = Appearance::default();
        let interface = |key| self.interface.as_ref().and_then(|store| store.string(key));
        let athanor = |key| self.athanor.as_ref().and_then(|store| store.string(key));
        Appearance {
            dark: interface("color-scheme").is_some_and(|scheme| scheme == "prefer-dark"),
            high_contrast: self
                .a11y
                .as_ref()
                .and_then(|store| store.boolean("high-contrast"))
                .unwrap_or(default.high_contrast),
            accent: interface("accent-color")
                .and_then(|name| Accent::from_name(&name))
                .unwrap_or(default.accent),
            accent_mode: match athanor("accent-mode").as_deref() {
                Some("wallpaper") => AccentMode::Wallpaper,
                _ => AccentMode::Fixed,
            },
            accent_computed: athanor("accent-computed").and_then(|text| Rgb::from_hex(&text)),
        }
    }

    fn all(&self) -> impl Iterator<Item = &Store> {
        [&self.interface, &self.a11y, &self.athanor]
            .into_iter()
            .flatten()
    }
}

/// A value `apply` writes to one key.
enum Value<'a> {
    Text(&'a str),
    Flag(bool),
}

fn required<'a>(store: &'a Option<Store>, id: &str) -> io::Result<&'a Store> {
    store.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("the schema {id} is not installed"),
        )
    })
}

impl Appearance {
    /// The user's appearance from the store.
    pub fn read() -> Appearance {
        Stores::open().read()
    }

    pub fn variant(&self) -> Variant {
        let base = if self.dark {
            Variant::Dark
        } else {
            Variant::Light
        };
        base.with_high_contrast(self.high_contrast)
    }

    /// The accent the shell shows: the computed colour in the wallpaper mode when there is
    /// one, the preset otherwise.
    pub fn accent_rgb(&self) -> Rgb {
        match (self.accent_mode, self.accent_computed) {
            (AccentMode::Wallpaper, Some(computed)) => computed,
            _ => self.accent.rgb(),
        }
    }

    /// The named colours that replace Calmo's accent, or `None` to keep it: in high
    /// contrast, whose accent the contrast gate has checked.
    pub fn accent_css(&self) -> Option<String> {
        if self.high_contrast {
            return None;
        }
        let accent = self.accent_rgb();
        Some(format!(
            "@define-color ath_acc {};\n@define-color ath_acc_ink {};\n",
            accent.hex(),
            on_accent(accent).hex()
        ))
    }
}

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

/// The text colour on `accent`: white when it reaches WCAG AA, otherwise Calmo's dark ink
/// (VL2). The contrast check of VL11, not this function, holds the presets to AA.
pub fn on_accent(accent: Rgb) -> Rgb {
    if contrast(accent, WHITE) >= MIN_CONTRAST {
        WHITE
    } else {
        DARK_INK
    }
}

thread_local! {
    static ACCENT_PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

/// Installs the Calmo variant of `appearance` and its accent on `display`. Called again
/// after a change, it replaces what it installed.
pub fn load(display: &gdk::Display, appearance: &Appearance) {
    calmo::load(display, appearance.variant());
    let provider = appearance.accent_css().map(|css| {
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

/// Calls `on_change` with the new appearance each time a key of the store changes it, for
/// the rest of the process: every surface follows the appearance for its whole life, so
/// the handlers own the settings they listen to and nothing has to hold the watch.
pub fn watch(on_change: impl Fn(Appearance) + 'static) {
    let stores = Rc::new(Stores::open());
    let on_change = Rc::new(on_change);
    let last = Rc::new(Cell::new(Appearance::default()));
    for store in stores.all() {
        let (stores, last, on_change) = (stores.clone(), last.clone(), on_change.clone());
        store.settings.connect_changed(None, move |_, _| {
            let appearance = stores.read();
            if last.replace(appearance) != appearance {
                on_change(appearance);
            }
        });
    }
    // GSettings signals a change only of a key read since its handler was connected.
    last.set(stores.read());
}

/// Applies `change` to the appearance the store holds and writes the result, in VL4's
/// order: the GNOME keys, then `org.athanor.desktop.appearance`, then the compositor's keys
/// through `cosmic` (athanor-compositor-client's `theme::write_cosmic`, the only code that
/// knows COSMIC). The change is applied to the store's values read here, so a key another
/// process changed since the caller last read it is not written back.
/// A failure of the last step is logged and leaves the result `Ok`: the shell reads the
/// store, and only cosmic-comp's borders and indicators lag.
pub fn apply(
    change: impl FnOnce(&mut Appearance),
    cosmic: impl FnOnce(&Appearance) -> io::Result<()>,
) -> io::Result<()> {
    let stores = Stores::open();
    let current = stores.read();
    let mut choice = current;
    change(&mut choice);
    let computed = choice.accent_computed.map(Rgb::hex).unwrap_or_default();
    let scheme = if choice.dark {
        "prefer-dark"
    } else {
        "default"
    };
    // Only the keys the choice changes: a value read as its default, such as an accent
    // outside the table, stays as the other tool wrote it.
    let writes = [
        (
            choice.dark != current.dark,
            &stores.interface,
            INTERFACE,
            "color-scheme",
            Value::Text(scheme),
        ),
        (
            choice.accent != current.accent,
            &stores.interface,
            INTERFACE,
            "accent-color",
            Value::Text(choice.accent.name()),
        ),
        (
            choice.high_contrast != current.high_contrast,
            &stores.a11y,
            A11Y,
            "high-contrast",
            Value::Flag(choice.high_contrast),
        ),
        (
            choice.accent_mode != current.accent_mode,
            &stores.athanor,
            ATHANOR,
            "accent-mode",
            Value::Text(choice.accent_mode.name()),
        ),
        (
            choice.accent_computed != current.accent_computed,
            &stores.athanor,
            ATHANOR,
            "accent-computed",
            Value::Text(&computed),
        ),
    ];
    // Every key checked before the first write, so a missing or locked one changes nothing.
    let mut checked = Vec::new();
    for (changed, store, id, key, value) in writes {
        if changed {
            let store = required(store, id)?;
            store.writable(key)?;
            checked.push((store, key, value));
        }
    }
    for (store, key, value) in checked {
        match value {
            Value::Text(text) => store.set_string(key, text)?,
            Value::Flag(flag) => store.set_boolean(key, flag)?,
        }
    }
    gio::Settings::sync();
    if let Err(err) = cosmic(&choice) {
        tracing::warn!(error = %err, "cannot write the compositor's theme keys; its borders and indicators keep the previous appearance");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_anything_else() {
        let teal = Accent::Teal.rgb();
        assert_eq!(Rgb::from_hex(&teal.hex()), Some(teal));
        assert_eq!(Rgb::from_hex("#9141AC"), Some(Accent::Purple.rgb()));
        for bad in ["", "9141ac", "#9141a", "#9141acff", "#9141ag", "#+141ac"] {
            assert_eq!(Rgb::from_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn accent_names_round_trip() {
        for accent in Accent::ALL {
            assert_eq!(Accent::from_name(accent.name()), Some(accent));
        }
        assert_eq!(Accent::from_name("indigo"), None);
    }

    #[test]
    fn the_factory_appearance_is_light_purple_and_fixed() {
        let factory = Appearance::default();
        assert_eq!(factory.variant(), Variant::Light);
        assert_eq!(factory.accent_rgb().hex(), "#9141ac");
    }

    #[test]
    fn variant_follows_scheme_and_contrast() {
        let dark_hc = Appearance {
            dark: true,
            high_contrast: true,
            ..Appearance::default()
        };
        assert_eq!(dark_hc.variant(), Variant::DarkHc);
    }

    #[test]
    fn the_computed_accent_counts_only_in_the_wallpaper_mode() {
        let computed = Rgb::from_hex("#336699");
        let wallpaper = Appearance {
            accent_mode: AccentMode::Wallpaper,
            accent_computed: computed,
            ..Appearance::default()
        };
        assert_eq!(Some(wallpaper.accent_rgb()), computed);
        let fixed = Appearance {
            accent_computed: computed,
            ..Appearance::default()
        };
        assert_eq!(fixed.accent_rgb(), Accent::Purple.rgb());
        let empty = Appearance {
            accent_mode: AccentMode::Wallpaper,
            ..Appearance::default()
        };
        assert_eq!(empty.accent_rgb(), Accent::Purple.rgb());
    }

    #[test]
    fn accent_css_keeps_calmo_in_high_contrast() {
        let factory = Appearance::default();
        let css = factory.accent_css().unwrap_or_default();
        assert!(css.contains("@define-color ath_acc #9141ac;"), "{css}");
        let high_contrast = Appearance {
            high_contrast: true,
            ..factory
        };
        assert_eq!(high_contrast.accent_css(), None);
    }

    #[test]
    fn the_ink_is_white_only_where_white_reaches_aa() {
        assert_eq!(on_accent(Accent::Purple.rgb()), WHITE);
        // 4.36:1 in white, short of AA.
        assert_eq!(on_accent(Accent::Red.rgb()), DARK_INK);
        assert_eq!(on_accent(Accent::Yellow.rgb()), DARK_INK);
    }
}
