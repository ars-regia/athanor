//! The magnifier's GNOME key (doc_accessibility.md AX10): it decides whether the magnifier is
//! on for a session, so the bar turns the magnifier on at its start when the key is true, sends
//! each change of the key to the compositor, and the magnifier row writes it.

use gtk4::gio;
use gtk4::gio::prelude::*;

pub const SCHEMA: &str = "org.gnome.desktop.a11y.applications";
pub const KEY: &str = "screen-magnifier-enabled";

/// Calls `send` with true now if the key is true, and with the key's value at each change of
/// it, at no other key's. A false key at the start says nothing: Super+= turns the magnifier on
/// and persists nothing, so a bar restarting must leave it as it is.
pub fn follow(settings: &gio::Settings, send: impl Fn(bool) + 'static) {
    if settings.boolean(KEY) {
        send(true);
    }
    settings.connect_changed(Some(KEY), move |settings, _| send(settings.boolean(KEY)));
}

/// The magnifier row, switched by the user: the state goes to the compositor whatever the key
/// holds, and to the key. Super+= turns the magnifier on with the key false, and writing the
/// value a key already holds is no change, so `send` is what turns it off then. A double send
/// is harmless: the compositor ignores the state it is in.
pub fn switch(
    key: Option<&gio::Settings>,
    enabled: bool,
    send: impl FnOnce(bool) -> Result<(), String>,
) -> Result<(), String> {
    let written = key.map_or(Ok(()), |key| {
        key.set_boolean(KEY, enabled).map_err(|err| err.to_string())
    });
    send(enabled).and(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::OnceLock;

    /// GNOME's schema reduced to the key and a neighbour, compiled into a directory of its own.
    const SCHEMA_XML: &str = r#"<schemalist>
  <schema id="org.gnome.desktop.a11y.applications" path="/org/gnome/desktop/a11y/applications/">
    <key name="screen-magnifier-enabled" type="b"><default>false</default></key>
    <key name="screen-reader-enabled" type="b"><default>false</default></key>
  </schema>
</schemalist>
"#;

    /// The compiled schema, once: the tests run side by side.
    fn schemas() -> &'static PathBuf {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
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
            dir
        })
    }

    fn settings() -> gio::Settings {
        let source =
            gio::SettingsSchemaSource::from_directory(schemas(), None, false).expect("the source");
        let schema = source.lookup(SCHEMA, false).expect("the schema");
        gio::Settings::new_full(
            &schema,
            Some(&gio::memory_settings_backend_new()),
            None::<&str>,
        )
    }

    #[test]
    fn a_false_key_sends_nothing_at_start() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let settings = settings();
                let sent = Rc::new(RefCell::new(Vec::new()));
                let log = sent.clone();
                follow(&settings, move |enabled| log.borrow_mut().push(enabled));
                assert!(sent.borrow().is_empty(), "the state is left as it is");
                settings.set_boolean(KEY, true).expect("writable");
                while context.iteration(false) {}
                assert_eq!(*sent.borrow(), [true], "a change still counts");
            })
            .expect("the context");
    }

    #[test]
    fn the_row_reaches_the_compositor_with_the_key_unchanged() {
        glib::MainContext::new()
            .with_thread_default(|| {
                let settings = settings();
                let sent = RefCell::new(Vec::new());
                let send = |enabled| {
                    sent.borrow_mut().push(enabled);
                    Ok(())
                };
                switch(Some(&settings), false, send).expect("switched");
                assert_eq!(*sent.borrow(), [false], "off, with the key already false");
                switch(Some(&settings), true, send).expect("switched");
                assert!(settings.boolean(KEY), "the key is written");
                switch(None, false, send).expect("switched");
                assert_eq!(*sent.borrow(), [false, true, false], "without the schema");
                let refused = switch(Some(&settings), false, |_| Err("no compositor".into()));
                assert_eq!(refused, Err("no compositor".to_string()));
                assert!(!settings.boolean(KEY), "the key is written all the same");
            })
            .expect("the context");
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
