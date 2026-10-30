//! The magnifier's GNOME key (doc_accessibility.md AX10): it decides whether the magnifier is
//! on, so the bar sends it to the compositor at its start and at each change, and the
//! magnifier row writes it.

use gtk4::gio;
use gtk4::gio::prelude::*;

pub const SCHEMA: &str = "org.gnome.desktop.a11y.applications";
pub const KEY: &str = "screen-magnifier-enabled";

/// Calls `send` with the key's value now and at each change of it, and at no other key's.
pub fn follow(settings: &gio::Settings, send: impl Fn(bool) + 'static) {
    send(settings.boolean(KEY));
    settings.connect_changed(Some(KEY), move |settings, _| send(settings.boolean(KEY)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// GNOME's schema reduced to the key and a neighbour, compiled into a directory of its own.
    const SCHEMA_XML: &str = r#"<schemalist>
  <schema id="org.gnome.desktop.a11y.applications" path="/org/gnome/desktop/a11y/applications/">
    <key name="screen-magnifier-enabled" type="b"><default>false</default></key>
    <key name="screen-reader-enabled" type="b"><default>false</default></key>
  </schema>
</schemalist>
"#;

    fn settings() -> gio::Settings {
        let dir =
            std::env::temp_dir().join(format!("athanor-bar-magnifier-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the schema directory");
        std::fs::write(
            dir.join("org.gnome.desktop.a11y.applications.gschema.xml"),
            SCHEMA_XML,
        )
        .expect("the schema");
        let status = std::process::Command::new("glib-compile-schemas")
            .arg(&dir)
            .status()
            .expect("glib-compile-schemas");
        assert!(status.success(), "glib-compile-schemas: {status}");
        let source =
            gio::SettingsSchemaSource::from_directory(&dir, None, false).expect("the source");
        let schema = source.lookup(SCHEMA, false).expect("the schema");
        gio::Settings::new_full(
            &schema,
            Some(&gio::memory_settings_backend_new()),
            None::<&str>,
        )
    }

    #[test]
    fn the_key_is_sent_at_start_and_at_each_change() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let settings = settings();
                settings.set_boolean(KEY, true).expect("writable");
                let sent = Rc::new(RefCell::new(Vec::new()));
                let log = sent.clone();
                follow(&settings, move |enabled| log.borrow_mut().push(enabled));
                assert_eq!(*sent.borrow(), [true], "at start");
                settings.set_boolean(KEY, false).expect("writable");
                settings
                    .set_boolean("screen-reader-enabled", true)
                    .expect("writable");
                settings.set_boolean(KEY, true).expect("writable");
                while context.iteration(false) {}
                assert_eq!(
                    *sent.borrow(),
                    [true, false, true],
                    "at each change of the key alone"
                );
            })
            .expect("the context");
    }
}
