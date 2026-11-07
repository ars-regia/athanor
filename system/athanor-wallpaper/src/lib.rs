//! The logic of `athanor-wallpaper` (doc_session_daemons.md, SD3, SD6, SD16, SD17): which
//! image each output shows, the zoom geometry and the scaling it draws, the regular-file
//! check, the one-time migration from cosmic-bg, and what the daemon and its decoder say to
//! each other. It knows no display, no toolkit and no GSettings: the daemon reads the keys
//! into a [`Settings`] and draws what this crate resolves, and the decoder scales with it.

mod file;
mod migrate;
pub mod plan;
pub mod protocol;
mod scale;
mod zoom;

pub use file::{open, open_regular};
pub use migrate::migrate;
pub use scale::write_zoomed;
pub use zoom::{zoom, Crop};

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The directory of the prebuilt hearth images (SD6).
pub const HEARTH_DIR: &str = "/usr/share/backgrounds/athanor";
/// The value of `picture` and of a `per-output` entry that names the hearth.
pub const HEARTH: &str = "hearth";

/// The variant of `org.gnome.desktop.interface color-scheme` the hearth follows (SD6, VL4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

impl ColorScheme {
    /// `prefer-dark` is dark; `default`, which VL4 stores for light, and any other value is
    /// light.
    pub fn from_key(value: &str) -> Self {
        if value == "prefer-dark" {
            Self::Dark
        } else {
            Self::Light
        }
    }
}

/// The hearth image for a scheme. ponytail: the accent is not a parameter until VL's
/// eighteen files ship (SD6); then this takes it and the names change.
pub fn hearth_path(scheme: ColorScheme) -> PathBuf {
    let name = match scheme {
        ColorScheme::Light => "hearth-light.png",
        ColorScheme::Dark => "hearth-dark.png",
    };
    PathBuf::from(HEARTH_DIR).join(name)
}

/// What a key value names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Hearth,
    Path(PathBuf),
}

impl Source {
    /// `"hearth"` or an absolute path; anything else is not a source.
    pub fn parse(value: &str) -> Option<Self> {
        if value == HEARTH {
            return Some(Self::Hearth);
        }
        let path = PathBuf::from(value);
        path.is_absolute().then_some(Self::Path(path))
    }

    /// The file to open: the hearth resolves to the image of the scheme.
    pub fn path(&self, scheme: ColorScheme) -> PathBuf {
        match self {
            Self::Hearth => hearth_path(scheme),
            Self::Path(path) => path.clone(),
        }
    }
}

/// The keys of `org.athanor.desktop.background` (SD3), as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub picture: String,
    pub per_output: BTreeMap<String, String>,
    pub same_on_all: bool,
    pub migrated: bool,
}

impl Default for Settings {
    /// The schema's defaults.
    fn default() -> Self {
        Self {
            picture: HEARTH.to_owned(),
            per_output: BTreeMap::new(),
            same_on_all: true,
            migrated: false,
        }
    }
}

impl Settings {
    /// The value that decides what an output shows: `picture` when `same-on-all` is true
    /// (`per-output` is then ignored) or when the output is not listed, else its entry.
    fn value_for(&self, connector: &str) -> &str {
        match self.per_output.get(connector) {
            Some(entry) if !self.same_on_all => entry,
            _ => &self.picture,
        }
    }

    /// What an output shows. A value that is neither `"hearth"` nor an absolute path shows the
    /// hearth; [`Settings::invalid_value`] names it.
    pub fn source_for(&self, connector: &str) -> Source {
        Source::parse(self.value_for(connector)).unwrap_or(Source::Hearth)
    }

    /// The value that makes `source_for` fall back to the hearth, if there is one. The daemon
    /// logs it once when it changes an output's source, not at every call.
    /// ponytail: W3 owns the "once per (output, value)" memory; this crate keeps no state.
    pub fn invalid_value(&self, connector: &str) -> Option<&str> {
        let value = self.value_for(connector);
        Source::parse(value).is_none().then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(same_on_all: bool) -> Settings {
        Settings {
            picture: "/a.png".into(),
            per_output: [
                ("DP-1".to_owned(), "hearth".to_owned()),
                ("HDMI-A-1".to_owned(), "/b.png".to_owned()),
            ]
            .into(),
            same_on_all,
            migrated: true,
        }
    }

    #[test]
    fn same_on_all_ignores_per_output() {
        let s = settings(true);
        assert_eq!(s.source_for("DP-1"), Source::Path("/a.png".into()));
        assert_eq!(s.source_for("HDMI-A-1"), Source::Path("/a.png".into()));
    }

    #[test]
    fn per_output_entry_wins_and_unlisted_shows_picture() {
        let s = settings(false);
        assert_eq!(s.source_for("DP-1"), Source::Hearth);
        assert_eq!(s.source_for("HDMI-A-1"), Source::Path("/b.png".into()));
        assert_eq!(s.source_for("eDP-1"), Source::Path("/a.png".into()));
    }

    #[test]
    fn defaults_show_the_hearth_everywhere() {
        assert_eq!(Settings::default().source_for("eDP-1"), Source::Hearth);
    }

    #[test]
    fn invalid_values_show_the_hearth() {
        let mut s = Settings {
            same_on_all: false,
            picture: "relative.png".into(),
            ..Settings::default()
        };
        s.per_output.insert("DP-1".into(), String::new());
        assert_eq!(s.source_for("DP-1"), Source::Hearth);
        assert_eq!(s.source_for("eDP-1"), Source::Hearth);
        assert_eq!(s.invalid_value("DP-1"), Some(""));
        assert_eq!(s.invalid_value("eDP-1"), Some("relative.png"));
        assert_eq!(Settings::default().invalid_value("eDP-1"), None);
    }

    #[test]
    fn hearth_follows_the_scheme() {
        assert_eq!(ColorScheme::from_key("prefer-dark"), ColorScheme::Dark);
        assert_eq!(ColorScheme::from_key("default"), ColorScheme::Light);
        assert_eq!(
            Source::Hearth.path(ColorScheme::Dark),
            PathBuf::from("/usr/share/backgrounds/athanor/hearth-dark.png")
        );
        assert_eq!(
            Source::Hearth.path(ColorScheme::Light),
            PathBuf::from("/usr/share/backgrounds/athanor/hearth-light.png")
        );
    }

    /// The (type, default) of each `<key name="...">` block of the schema.
    fn schema_keys(xml: &str) -> BTreeMap<String, (String, String)> {
        let attr = |tag: &str, name: &str| {
            let rest = &tag[tag.find(&format!("{name}=\""))? + name.len() + 2..];
            Some(rest[..rest.find('"')?].to_owned())
        };
        xml.split("<key ")
            .skip(1)
            .filter_map(|block| {
                let tag = &block[..block.find('>')?];
                let default = block
                    .split("<default>")
                    .nth(1)?
                    .split("</default>")
                    .next()?;
                Some((attr(tag, "name")?, (attr(tag, "type")?, default.to_owned())))
            })
            .collect()
    }

    const SCHEMA: &str = include_str!(
        "../../../forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/org.athanor.desktop.background.gschema.xml"
    );

    #[test]
    fn the_shipped_schema_has_the_defaults() {
        assert!(SCHEMA.contains(r#"<schema id="org.athanor.desktop.background""#));
        let want = |t: &str, d: &str| (t.to_owned(), d.to_owned());
        let keys = schema_keys(SCHEMA);
        assert_eq!(
            keys,
            BTreeMap::from([
                ("picture".to_owned(), want("s", "'hearth'")),
                ("per-output".to_owned(), want("a{ss}", "{}")),
                ("same-on-all".to_owned(), want("b", "true")),
                ("migrated".to_owned(), want("b", "false")),
            ])
        );
        // The defaults are the ones of `Settings::default()`.
        let d = Settings::default();
        assert_eq!(keys["picture"].1, format!("'{}'", d.picture));
        assert_eq!(keys["same-on-all"].1, d.same_on_all.to_string());
        assert_eq!(keys["migrated"].1, d.migrated.to_string());
        assert!(d.per_output.is_empty());
    }

    #[test]
    fn a_changed_default_is_caught() {
        let flipped = SCHEMA.replace("<default>true</default>", "<default>false</default>");
        assert_ne!(
            schema_keys(&flipped)["same-on-all"].1,
            Settings::default().same_on_all.to_string()
        );
    }
}
