//! The reset of COSMIC's appearance values (`doc_session_daemons.md` SD16, amended
//! 2026-10-15, #473), at each start of the daemon. cosmic-settings-daemon wrote these values
//! into every account before it was put on `GSETTINGS_BACKEND=memory` (#474). A key is reset
//! only when the user's value is exactly the daemon's: any other value is the person's choice
//! and is kept, and so is `button-layout`, where the daemon copied the person's own COSMIC
//! choice. The user value is what dconf's database holds, never the schema's default or the
//! vendor override. The reset goes through the dconf service on the bus.

use std::fmt;

use gio::prelude::*;

pub const INTERFACE: &str = "org.gnome.desktop.interface";

/// Each key, and the value cosmic-settings-daemon wrote into it.
pub const COSMIC_VALUES: [(&str, &str); 3] = [
    ("icon-theme", "Cosmic"),
    ("font-name", "Open Sans"),
    ("monospace-font-name", "Noto Sans Mono"),
];

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The schema is not installed.
    NoSchema,
    /// The schema has no such key: GIO would abort on it.
    NoKey(&'static str),
    /// The key still holds the daemon's value after the reset was written.
    NotReset(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoSchema => write!(f, "the schema {INTERFACE} is not installed"),
            Error::NoKey(key) => write!(f, "the schema {INTERFACE} has no key {key}"),
            Error::NotReset(key) => write!(f, "{key} still holds cosmic-settings-daemon's value"),
        }
    }
}

impl std::error::Error for Error {}

/// The GNOME interface settings on the default backend, dconf in a session.
pub fn open() -> Result<gio::Settings, Error> {
    let schema = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(INTERFACE, true))
        .ok_or(Error::NoSchema)?;
    Ok(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

/// Resets each key of [`COSMIC_VALUES`] whose user value is the daemon's, waits for the
/// writes, and reads the keys again. Returns the keys it reset.
pub fn reset(settings: &gio::Settings) -> Result<Vec<&'static str>, Error> {
    let schema = settings.settings_schema().ok_or(Error::NoSchema)?;
    if let Some((key, _)) = COSMIC_VALUES.iter().find(|(key, _)| !schema.has_key(key)) {
        return Err(Error::NoKey(key));
    }
    let holds = |key: &str, cosmic: &str| {
        settings
            .user_value(key)
            .is_some_and(|value| value.str() == Some(cosmic))
    };
    let reset: Vec<&'static str> = COSMIC_VALUES
        .iter()
        .filter(|(key, cosmic)| holds(key, cosmic))
        .map(|(key, _)| *key)
        .collect();
    if reset.is_empty() {
        return Ok(reset);
    }
    for key in &reset {
        settings.reset(key);
    }
    // GSettings reports no failed write: one dconf refused leaves the old value in place,
    // which the second read finds.
    gio::Settings::sync();
    match COSMIC_VALUES
        .iter()
        .find(|(key, cosmic)| reset.contains(key) && holds(key, cosmic))
    {
        Some((key, _)) => Err(Error::NotReset(key)),
        None => Ok(reset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gio::glib;

    /// The three keys under the GNOME schema's id, compiled into a directory of the test's
    /// own, so the test does not depend on the build machine's schemas.
    const SCHEMA: &str = r#"<schemalist>
  <schema id="org.gnome.desktop.interface" path="/org/gnome/desktop/interface/">
    <key name="icon-theme" type="s"><default>'Adwaita'</default></key>
    <key name="font-name" type="s"><default>'Adwaita Sans 11'</default></key>
    <key name="monospace-font-name" type="s"><default>'Adwaita Mono 11'</default></key>
  </schema>
</schemalist>"#;

    fn settings(name: &str, keys: &str) -> gio::Settings {
        let dir =
            std::env::temp_dir().join(format!("athanor-sessiond-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("test.gschema.xml"), keys).expect("schema");
        let status = std::process::Command::new("glib-compile-schemas")
            .arg("--strict")
            .arg(&dir)
            .status()
            .expect("glib-compile-schemas");
        assert!(status.success(), "the schema compiles: {status}");
        let source = gio::SettingsSchemaSource::from_directory(&dir, None, true).expect("source");
        let schema = source.lookup(INTERFACE, false).expect("schema");
        let backend = gio::memory_settings_backend_new();
        gio::Settings::new_full(&schema, Some(&backend), None)
    }

    fn set(settings: &gio::Settings, key: &str, value: &str) {
        settings
            .set_value(key, &glib::Variant::from(value))
            .expect("writable");
    }

    #[test]
    fn only_the_daemons_values_are_reset() {
        let settings = settings("values", SCHEMA);
        set(&settings, "icon-theme", "Cosmic");
        set(&settings, "font-name", "Inter 11");
        set(&settings, "monospace-font-name", "Noto Sans Mono");
        assert_eq!(
            reset(&settings),
            Ok(vec!["icon-theme", "monospace-font-name"])
        );
        assert!(settings.user_value("icon-theme").is_none());
        assert!(settings.user_value("monospace-font-name").is_none());
        assert_eq!(
            settings.string("font-name"),
            "Inter 11",
            "the person's choice"
        );
        assert_eq!(
            settings.string("icon-theme"),
            "Adwaita",
            "the default applies"
        );
        // Idempotent: a second start finds nothing to reset.
        assert_eq!(reset(&settings), Ok(vec![]));
    }

    #[test]
    fn a_value_close_to_the_daemons_is_the_persons() {
        let settings = settings("close", SCHEMA);
        set(&settings, "icon-theme", "cosmic");
        set(&settings, "font-name", "Open Sans 11");
        assert_eq!(reset(&settings), Ok(vec![]));
        assert_eq!(settings.string("icon-theme"), "cosmic");
    }

    #[test]
    fn a_schema_without_a_key_is_refused_before_anything_is_written() {
        let partial = SCHEMA.replace(
            r#"<key name="monospace-font-name" type="s"><default>'Adwaita Mono 11'</default></key>"#,
            "",
        );
        let settings = settings("partial", &partial);
        set(&settings, "icon-theme", "Cosmic");
        assert_eq!(reset(&settings), Err(Error::NoKey("monospace-font-name")));
        assert_eq!(settings.string("icon-theme"), "Cosmic");
    }
}
