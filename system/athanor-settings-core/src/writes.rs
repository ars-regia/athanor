//! The GSettings keys Settings may write (doc_settings.md SE7): one entry per key, with the page
//! that writes it and the values its schema accepts. A page writes through a typed handle taken
//! from this table, and the program's test refuses any other way to `gio::Settings::set_*`, so
//! "the Accessibility page writes the keys of AX2 and no others" is checked, not promised.
//!
//! Types and ranges are those of the installed schemas: GNOME's under
//! `/usr/share/glib-2.0/schemas` and Athanor's in `forge/specs/athanor-system-config`. Keys whose
//! schema is not shipped yet (`org.athanor.desktop.a11y`, `org.athanor.desktop.audio`) join the
//! table with it.

use std::fmt;

/// The values a key accepts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Type {
    Bool,
    /// `i`, within the schema's range when it has one.
    Int(Option<(i32, i32)>),
    /// `u`, within the schema's range.
    UInt(u32, u32),
    /// `d`, within the schema's range.
    Double(f64, f64),
    /// `s`, free text.
    Str,
}

/// One writable key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub page: &'static str,
    pub schema: &'static str,
    pub key: &'static str,
    pub kind: Type,
}

const A11Y: &str = "org.gnome.desktop.a11y";
const A11Y_APPS: &str = "org.gnome.desktop.a11y.applications";
const A11Y_IFACE: &str = "org.gnome.desktop.a11y.interface";
const A11Y_KEYBOARD: &str = "org.gnome.desktop.a11y.keyboard";
const INTERFACE: &str = "org.gnome.desktop.interface";
const BACKGROUND: &str = "org.athanor.desktop.background";
const IDLE: &str = "org.athanor.desktop.idle";

const fn entry(page: &'static str, schema: &'static str, key: &'static str, kind: Type) -> Entry {
    Entry {
        page,
        schema,
        key,
        kind,
    }
}

/// Every key Settings may write.
pub const TABLE: &[Entry] = &[
    // Accessibility: the GSettings keys of doc_accessibility.md AX2 whose schema is installed.
    entry(
        "accessibility",
        A11Y_APPS,
        "screen-reader-enabled",
        Type::Bool,
    ),
    entry(
        "accessibility",
        INTERFACE,
        "toolkit-accessibility",
        Type::Bool,
    ),
    entry(
        "accessibility",
        A11Y_APPS,
        "screen-magnifier-enabled",
        Type::Bool,
    ),
    entry(
        "accessibility",
        A11Y_APPS,
        "screen-keyboard-enabled",
        Type::Bool,
    ),
    entry("accessibility", A11Y_IFACE, "high-contrast", Type::Bool),
    entry(
        "accessibility",
        INTERFACE,
        "text-scaling-factor",
        Type::Double(0.5, 3.0),
    ),
    entry("accessibility", INTERFACE, "enable-animations", Type::Bool),
    entry("accessibility", INTERFACE, "cursor-size", Type::Int(None)),
    entry(
        "accessibility",
        A11Y_KEYBOARD,
        "stickykeys-enable",
        Type::Bool,
    ),
    entry(
        "accessibility",
        A11Y_KEYBOARD,
        "slowkeys-enable",
        Type::Bool,
    ),
    entry(
        "accessibility",
        A11Y_KEYBOARD,
        "slowkeys-delay",
        Type::Int(None),
    ),
    entry(
        "accessibility",
        A11Y_KEYBOARD,
        "bouncekeys-enable",
        Type::Bool,
    ),
    entry(
        "accessibility",
        A11Y_KEYBOARD,
        "bouncekeys-delay",
        Type::Int(None),
    ),
    entry(
        "accessibility",
        A11Y_IFACE,
        "show-status-shapes",
        Type::Bool,
    ),
    entry(
        "accessibility",
        A11Y,
        "always-show-universal-access-status",
        Type::Bool,
    ),
    // Desktop: the hot corner (SE14).
    entry("desktop", INTERFACE, "enable-hot-corners", Type::Bool),
    // Wallpaper (SE13): picture and same-on-all; per-output joins the table with its type.
    entry("wallpaper", BACKGROUND, "picture", Type::Str),
    entry("wallpaper", BACKGROUND, "same-on-all", Type::Bool),
    // Power: blank and suspend (`power/idle`).
    entry("power", IDLE, "blank-after", Type::UInt(0, 7200)),
    entry("power", IDLE, "suspend-after-battery", Type::UInt(0, 14400)),
    entry("power", IDLE, "suspend-after-ac", Type::UInt(0, 14400)),
];

/// A value refused before it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct Invalid {
    pub schema: &'static str,
    pub key: &'static str,
    pub value: String,
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {} is out of range",
            self.schema, self.key, self.value
        )
    }
}

/// A handle to a key of the table whose values are `T`. Only the table makes one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle<T> {
    entry: &'static Entry,
    _value: std::marker::PhantomData<fn(T)>,
}

impl<T> Handle<T> {
    pub fn schema(&self) -> &'static str {
        self.entry.schema
    }

    pub fn key(&self) -> &'static str {
        self.entry.key
    }

    fn invalid(&self, value: impl fmt::Display) -> Invalid {
        Invalid {
            schema: self.entry.schema,
            key: self.entry.key,
            value: value.to_string(),
        }
    }
}

impl Handle<bool> {
    pub fn check(&self, value: bool) -> Result<bool, Invalid> {
        Ok(value)
    }
}

impl Handle<i32> {
    pub fn check(&self, value: i32) -> Result<i32, Invalid> {
        match self.entry.kind {
            Type::Int(Some((min, max))) if !(min..=max).contains(&value) => {
                Err(self.invalid(value))
            }
            _ => Ok(value),
        }
    }
}

impl Handle<u32> {
    pub fn check(&self, value: u32) -> Result<u32, Invalid> {
        match self.entry.kind {
            Type::UInt(min, max) if !(min..=max).contains(&value) => Err(self.invalid(value)),
            _ => Ok(value),
        }
    }
}

impl Handle<f64> {
    pub fn check(&self, value: f64) -> Result<f64, Invalid> {
        match self.entry.kind {
            Type::Double(min, max) if (min..=max).contains(&value) => Ok(value),
            _ => Err(self.invalid(value)),
        }
    }
}

impl Handle<String> {
    pub fn check(&self, value: String) -> Result<String, Invalid> {
        Ok(value)
    }
}

/// The value type of a handle, tied to the schema's type.
pub trait Value {
    fn fits(kind: Type) -> bool;
}

impl Value for bool {
    fn fits(kind: Type) -> bool {
        kind == Type::Bool
    }
}

impl Value for i32 {
    fn fits(kind: Type) -> bool {
        matches!(kind, Type::Int(_))
    }
}

impl Value for u32 {
    fn fits(kind: Type) -> bool {
        matches!(kind, Type::UInt(..))
    }
}

impl Value for f64 {
    fn fits(kind: Type) -> bool {
        matches!(kind, Type::Double(..))
    }
}

impl Value for String {
    fn fits(kind: Type) -> bool {
        kind == Type::Str
    }
}

/// The handle page `page` writes `schema key` through, when the table gives it that key with
/// values of type `T`.
pub fn handle<T: Value>(page: &str, schema: &str, key: &str) -> Option<Handle<T>> {
    TABLE
        .iter()
        .find(|entry| entry.page == page && entry.schema == schema && entry.key == key)
        .filter(|entry| T::fits(entry.kind))
        .map(|entry| Handle {
            entry,
            _value: std::marker::PhantomData,
        })
}

/// The keys page `page` writes, as `schema key`.
pub fn keys_of(page: &str) -> Vec<String> {
    TABLE
        .iter()
        .filter(|entry| entry.page == page)
        .map(|entry| format!("{} {}", entry.schema, entry.key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accessibility_page_writes_the_keys_of_ax2_and_no_others() {
        let mut keys = keys_of("accessibility");
        keys.sort();
        let mut ax2 = vec![
            "org.gnome.desktop.a11y always-show-universal-access-status",
            "org.gnome.desktop.a11y.applications screen-keyboard-enabled",
            "org.gnome.desktop.a11y.applications screen-magnifier-enabled",
            "org.gnome.desktop.a11y.applications screen-reader-enabled",
            "org.gnome.desktop.a11y.interface high-contrast",
            "org.gnome.desktop.a11y.interface show-status-shapes",
            "org.gnome.desktop.a11y.keyboard bouncekeys-delay",
            "org.gnome.desktop.a11y.keyboard bouncekeys-enable",
            "org.gnome.desktop.a11y.keyboard slowkeys-delay",
            "org.gnome.desktop.a11y.keyboard slowkeys-enable",
            "org.gnome.desktop.a11y.keyboard stickykeys-enable",
            "org.gnome.desktop.interface cursor-size",
            "org.gnome.desktop.interface enable-animations",
            "org.gnome.desktop.interface text-scaling-factor",
            "org.gnome.desktop.interface toolkit-accessibility",
        ];
        ax2.sort_unstable();
        assert_eq!(keys, ax2);
    }

    #[test]
    fn each_key_is_in_the_table_once() {
        for (index, entry) in TABLE.iter().enumerate() {
            let again = TABLE
                .iter()
                .skip(index + 1)
                .any(|other| other.schema == entry.schema && other.key == entry.key);
            assert!(
                !again,
                "{} {} is twice in the table",
                entry.schema, entry.key
            );
        }
    }

    #[test]
    fn a_handle_needs_the_page_the_key_and_its_type() {
        assert!(handle::<bool>("desktop", INTERFACE, "enable-hot-corners").is_some());
        assert!(handle::<bool>("accessibility", INTERFACE, "enable-hot-corners").is_none());
        assert!(handle::<i32>("desktop", INTERFACE, "enable-hot-corners").is_none());
        assert!(handle::<bool>("desktop", INTERFACE, "gtk-theme").is_none());
        let picture = handle::<String>("wallpaper", BACKGROUND, "picture").expect("in the table");
        assert_eq!((picture.schema(), picture.key()), (BACKGROUND, "picture"));
    }

    #[test]
    fn a_value_outside_the_schema_range_is_refused() {
        let scale =
            handle::<f64>("accessibility", INTERFACE, "text-scaling-factor").expect("in the table");
        assert_eq!(scale.check(1.25), Ok(1.25));
        assert!(scale.check(3.5).is_err());
        assert!(scale.check(f64::NAN).is_err());
        let blank = handle::<u32>("power", IDLE, "blank-after").expect("in the table");
        assert_eq!(blank.check(7200), Ok(7200));
        let refused = blank.check(7201).expect_err("above the range");
        assert_eq!(
            refused.to_string(),
            "org.athanor.desktop.idle blank-after: 7201 is out of range"
        );
        let cursor =
            handle::<i32>("accessibility", INTERFACE, "cursor-size").expect("in the table");
        assert_eq!(cursor.check(48), Ok(48));
    }
}
