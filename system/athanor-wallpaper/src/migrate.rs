//! SD16's one-time migration from `com.system76.CosmicBackground` v1. Reading and parsing
//! the COSMIC files is not here (SH2: only `athanor-sulphur-client` knows COSMIC); this is
//! the mapping from what they hold to the schema's keys.

use crate::{Settings, HEARTH, HEARTH_DIR};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A cosmic-bg source (`Path(..)` or `Color(..)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CosmicSource {
    Path(PathBuf),
    /// A colour or gradient: it has no image, and the hearth stands for it.
    Color,
}

/// The keys of the store that were present and valid; a missing or unparsable one is `None`
/// or absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CosmicBackground {
    /// The entry `all`.
    pub all: Option<CosmicSource>,
    /// The entries `output.<name>`, by connector name.
    pub outputs: BTreeMap<String, CosmicSource>,
    /// The key `same-on-all`.
    pub same_on_all: Option<bool>,
}

fn is_hearth(path: &Path) -> bool {
    path.parent() == Some(Path::new(HEARTH_DIR))
        && matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("hearth-light.png" | "hearth-dark.png")
        )
}

fn value(source: &CosmicSource) -> String {
    match source {
        CosmicSource::Path(path) if !is_hearth(path) => path.to_string_lossy().into_owned(),
        _ => HEARTH.to_owned(),
    }
}

/// Migrates once: with `migrated` true the settings are returned untouched, so a value the
/// person set since is never overwritten at a later start. Otherwise each value COSMIC holds
/// replaces the key it maps to, a key it holds nothing for keeps the value it has (the
/// default when nothing set it), and `migrated` becomes true. `None` is a store that is
/// absent or unreadable.
pub fn migrate(settings: Settings, cosmic: Option<&CosmicBackground>) -> Settings {
    if settings.migrated {
        return settings;
    }
    let mut out = settings;
    out.migrated = true;
    let Some(cosmic) = cosmic else { return out };
    if let Some(all) = &cosmic.all {
        out.picture = value(all);
    }
    for (name, source) in &cosmic.outputs {
        out.per_output.insert(name.clone(), value(source));
    }
    if let Some(same) = cosmic.same_on_all {
        out.same_on_all = same;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> CosmicSource {
        CosmicSource::Path(p.into())
    }

    #[test]
    fn maps_all_outputs_and_same_on_all() {
        let cosmic = CosmicBackground {
            all: Some(path("/home/u/a.png")),
            outputs: [
                (
                    "DP-1".to_owned(),
                    path("/usr/share/backgrounds/athanor/hearth-light.png"),
                ),
                ("HDMI-A-1".to_owned(), CosmicSource::Color),
                ("eDP-1".to_owned(), path("/home/u/b.png")),
            ]
            .into(),
            same_on_all: Some(false),
        };
        let out = migrate(Settings::default(), Some(&cosmic));
        assert_eq!(out.picture, "/home/u/a.png");
        assert_eq!(out.per_output["DP-1"], "hearth");
        assert_eq!(out.per_output["HDMI-A-1"], "hearth");
        assert_eq!(out.per_output["eDP-1"], "/home/u/b.png");
        assert!(!out.same_on_all);
        assert!(out.migrated);
    }

    #[test]
    fn the_shipped_hearth_and_a_colour_become_the_hearth() {
        for all in [
            path("/usr/share/backgrounds/athanor/hearth-dark.png"),
            CosmicSource::Color,
        ] {
            let cosmic = CosmicBackground {
                all: Some(all),
                ..Default::default()
            };
            assert_eq!(
                migrate(Settings::default(), Some(&cosmic)).picture,
                "hearth"
            );
        }
        // Another file called hearth-light.png elsewhere is the person's own image.
        let cosmic = CosmicBackground {
            all: Some(path("/home/u/hearth-light.png")),
            ..Default::default()
        };
        assert_eq!(
            migrate(Settings::default(), Some(&cosmic)).picture,
            "/home/u/hearth-light.png"
        );
    }

    #[test]
    fn absent_store_or_keys_keep_the_defaults_and_mark_migrated() {
        let out = migrate(Settings::default(), None);
        assert_eq!(
            out,
            Settings {
                migrated: true,
                ..Settings::default()
            }
        );
        let out = migrate(Settings::default(), Some(&CosmicBackground::default()));
        assert_eq!(
            out,
            Settings {
                migrated: true,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn migrated_settings_are_never_touched() {
        let mine = Settings {
            picture: "/mine.png".into(),
            migrated: true,
            ..Settings::default()
        };
        let cosmic = CosmicBackground {
            all: Some(path("/home/u/a.png")),
            ..Default::default()
        };
        assert_eq!(migrate(mine.clone(), Some(&cosmic)), mine);
    }

    #[test]
    fn a_value_set_before_the_first_start_survives_a_key_cosmic_lacks() {
        let set = Settings {
            picture: "/mine.png".into(),
            ..Settings::default()
        };
        let cosmic = CosmicBackground {
            same_on_all: Some(false),
            ..Default::default()
        };
        let out = migrate(set, Some(&cosmic));
        assert_eq!(out.picture, "/mine.png");
        assert!(!out.same_on_all);
    }
}
