//! The screen reader's GNOME key (doc_accessibility.md AX2): the accessibility menu's row reads
//! and writes it, and what starts Orca follows it (AX3). at-spi2-core up to 2.60 mirrors it as
//! `org.a11y.Status.ScreenReaderEnabled`; 2.61 dropped that property and serves `IsEnabled`
//! alone, so the key is the one source that Fedora 43 and Fedora 45 both serve.

use gtk4::gio;

pub use crate::magnifier::SCHEMA;
pub const KEY: &str = "screen-reader-enabled";

/// The key in `source`, on `backend` (the default one when `None`); `None` when `source` is
/// absent or holds no schema with the key.
pub fn settings(
    source: Option<&gio::SettingsSchemaSource>,
    backend: Option<&gio::SettingsBackend>,
) -> Option<gio::Settings> {
    let schema = source?
        .lookup(SCHEMA, true)
        .filter(|schema| schema.has_key(KEY))?;
    Some(gio::Settings::new_full(&schema, backend, None::<&str>))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::gio::prelude::*;
    use std::path::PathBuf;

    /// GNOME's schema reduced to the magnifier's key, with or without the reader's, compiled
    /// into a directory of its own.
    fn source(with_reader: bool) -> gio::SettingsSchemaSource {
        let reader = if with_reader {
            r#"<key name="screen-reader-enabled" type="b"><default>false</default></key>"#
        } else {
            ""
        };
        let xml = format!(
            r#"<schemalist>
  <schema id="org.gnome.desktop.a11y.applications" path="/org/gnome/desktop/a11y/applications/">
    <key name="screen-magnifier-enabled" type="b"><default>false</default></key>
    {reader}
  </schema>
</schemalist>
"#
        );
        let dir: PathBuf = std::env::temp_dir().join(format!(
            "athanor-bar-screen-reader-{}-{with_reader}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("the schema directory");
        std::fs::write(
            dir.join("org.gnome.desktop.a11y.applications.gschema.xml"),
            xml,
        )
        .expect("the schema");
        let status = std::process::Command::new("glib-compile-schemas")
            .arg(&dir)
            .status()
            .expect("glib-compile-schemas");
        assert!(status.success(), "glib-compile-schemas: {status}");
        gio::SettingsSchemaSource::from_directory(&dir, None, false).expect("the source")
    }

    #[test]
    fn the_row_reads_and_writes_the_key() {
        glib::MainContext::new()
            .with_thread_default(|| {
                let backend = gio::memory_settings_backend_new();
                let key = settings(Some(&source(true)), Some(&backend)).expect("the key");
                assert!(!key.boolean(KEY), "off by default");
                key.set_boolean(KEY, true).expect("writable");
                let again = settings(Some(&source(true)), Some(&backend)).expect("the key");
                assert!(again.boolean(KEY), "the state is the key's");
            })
            .expect("the context");
    }

    #[test]
    fn a_schema_without_the_key_hides_the_row() {
        assert!(settings(Some(&source(false)), None).is_none());
        assert!(settings(None, None).is_none());
    }
}
