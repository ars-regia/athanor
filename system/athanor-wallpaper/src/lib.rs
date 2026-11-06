//! The logic of `athanor-wallpaper` (doc_session_daemons.md, SD3, SD6, SD16): which image
//! each output shows, the zoom geometry, the regular-file check and the one-time migration
//! from cosmic-bg. It knows no display, no toolkit and no GSettings: the daemon reads the
//! keys into a [`Settings`] and draws what this crate resolves.

mod file;
mod migrate;
mod zoom;

pub use file::{open, open_regular};
pub use migrate::migrate;
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
    /// What an output shows: `picture` when `same-on-all` is true (`per-output` is then
    /// ignored) or when the output is not listed, else its entry. A value that is neither
    /// `"hearth"` nor an absolute path shows the hearth, with one warning.
    pub fn source_for(&self, connector: &str) -> Source {
        let value = match self.per_output.get(connector) {
            Some(entry) if !self.same_on_all => entry,
            _ => &self.picture,
        };
        Source::parse(value).unwrap_or_else(|| {
            tracing::warn!(
                output = connector,
                value,
                "not \"hearth\" or an absolute path; showing the hearth"
            );
            Source::Hearth
        })
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

    #[test]
    fn the_shipped_schema_has_the_defaults() {
        let xml = include_str!(
            "../../../forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/org.athanor.desktop.background.gschema.xml"
        );
        assert!(xml.contains(r#"<schema id="org.athanor.desktop.background""#));
        for needle in [
            r#"name="picture" type="s""#,
            r#"name="per-output" type="a{ss}""#,
            r#"name="same-on-all" type="b""#,
            r#"name="migrated" type="b""#,
            "<default>'hearth'</default>",
            "<default>{}</default>",
            "<default>true</default>",
            "<default>false</default>",
        ] {
            assert!(xml.contains(needle), "{needle}");
        }
    }
}
