//! The GNOME keys of the keyboard aids (doc_accessibility.md AX11): the bar sends them to the
//! compositor at its start and at each change, until `athanor-a11y watch` takes that over, and
//! writes an aid's key when the compositor turned the aid off or the user accepts an offer.

use athanor_compositor_client::{KeyboardAid, KeyboardAids, MAX_AID_DELAY};
use gtk4::gio;
use gtk4::gio::prelude::*;
use gtk4::glib;

pub const SCHEMA: &str = "org.gnome.desktop.a11y.keyboard";
const STICKY: &str = "stickykeys-enable";
const TWO_KEY_OFF: &str = "stickykeys-two-key-off";
const SLOW: &str = "slowkeys-enable";
const SLOW_DELAY: &str = "slowkeys-delay";
const BOUNCE: &str = "bouncekeys-enable";
const BOUNCE_DELAY: &str = "bouncekeys-delay";
const GESTURES: &str = "enable";

/// Every key read, which the schema must hold: GSettings aborts on a key it does not.
pub const KEYS: [&str; 7] = [
    STICKY,
    TWO_KEY_OFF,
    SLOW,
    SLOW_DELAY,
    BOUNCE,
    BOUNCE_DELAY,
    GESTURES,
];

/// The schema bounds no delay and the compositor ends a connection that sends one out of
/// bounds, so a key out of them is clamped, and logged.
fn delay(settings: &gio::Settings, key: &str) -> u32 {
    let value = settings.int(key);
    let bounded = value.clamp(0, MAX_AID_DELAY as i32);
    if bounded != value {
        tracing::warn!(
            key,
            value,
            bounded,
            "a keyboard aid's delay is out of bounds"
        );
    }
    bounded.unsigned_abs()
}

pub fn read(settings: &gio::Settings) -> KeyboardAids {
    KeyboardAids {
        sticky: settings.boolean(STICKY),
        two_key_off: settings.boolean(TWO_KEY_OFF),
        slow: settings.boolean(SLOW),
        slow_delay: delay(settings, SLOW_DELAY),
        bounce: settings.boolean(BOUNCE),
        bounce_delay: delay(settings, BOUNCE_DELAY),
        gestures: settings.boolean(GESTURES),
    }
}

/// Calls `send` now and at each change of a key of `KEYS`, at no other key's.
pub fn follow(settings: &gio::Settings, send: impl Fn(KeyboardAids) + 'static) {
    send(read(settings));
    settings.connect_changed(None, move |settings, key| {
        if KEYS.contains(&key) {
            send(read(settings));
        }
    });
}

/// Writes whether `aid` is on.
pub fn write(settings: &gio::Settings, aid: KeyboardAid, on: bool) -> Result<(), glib::BoolError> {
    let key = match aid {
        KeyboardAid::Sticky => STICKY,
        KeyboardAid::Slow => SLOW,
        KeyboardAid::Bounce => BOUNCE,
    };
    settings.set_boolean(key, on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::OnceLock;

    /// GNOME's schema, with a neighbour no aid reads, compiled into a directory of its own.
    const SCHEMA_XML: &str = r#"<schemalist>
  <schema id="org.gnome.desktop.a11y.keyboard" path="/org/gnome/desktop/a11y/keyboard/">
    <key name="enable" type="b"><default>false</default></key>
    <key name="stickykeys-enable" type="b"><default>false</default></key>
    <key name="stickykeys-two-key-off" type="b"><default>true</default></key>
    <key name="slowkeys-enable" type="b"><default>false</default></key>
    <key name="slowkeys-delay" type="i"><default>300</default></key>
    <key name="bouncekeys-enable" type="b"><default>false</default></key>
    <key name="bouncekeys-delay" type="i"><default>300</default></key>
    <key name="timeout" type="i"><default>120</default></key>
  </schema>
</schemalist>
"#;

    /// The compiled schema, once: the tests run side by side.
    fn schemas() -> &'static PathBuf {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = std::env::temp_dir()
                .join(format!("athanor-bar-keyboard-aids-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("the schema directory");
            std::fs::write(
                dir.join("org.gnome.desktop.a11y.keyboard.gschema.xml"),
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
        for key in KEYS {
            assert!(schema.has_key(key), "{key}");
        }
        gio::Settings::new_full(
            &schema,
            Some(&gio::memory_settings_backend_new()),
            None::<&str>,
        )
    }

    #[test]
    fn a_delay_out_of_bounds_is_clamped() {
        glib::MainContext::new()
            .with_thread_default(|| {
                let settings = settings();
                for (key, value, read) in [
                    (SLOW_DELAY, 2001, MAX_AID_DELAY),
                    (BOUNCE_DELAY, -1, 0),
                    (BOUNCE_DELAY, 2000, MAX_AID_DELAY),
                ] {
                    settings.set_int(key, value).expect("writable");
                    assert_eq!(delay(&settings, key), read, "{key} at {value}");
                }
            })
            .expect("the context");
    }

    #[test]
    fn the_keys_are_sent_at_start_and_at_each_change_of_theirs() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let settings = settings();
                let sent = Rc::new(RefCell::new(Vec::new()));
                let log = sent.clone();
                follow(&settings, move |aids| log.borrow_mut().push(aids));
                let start = KeyboardAids {
                    two_key_off: true,
                    slow_delay: 300,
                    bounce_delay: 300,
                    ..KeyboardAids::default()
                };
                assert_eq!(*sent.borrow(), [start], "at start");

                settings.set_int("timeout", 60).expect("writable");
                while context.iteration(false) {}
                assert_eq!(sent.borrow().len(), 1, "a neighbour's change is not sent");

                write(&settings, KeyboardAid::Sticky, true).expect("writable");
                write(&settings, KeyboardAid::Slow, true).expect("writable");
                write(&settings, KeyboardAid::Bounce, true).expect("writable");
                settings.set_boolean(GESTURES, true).expect("writable");
                settings.set_boolean(TWO_KEY_OFF, false).expect("writable");
                settings.set_int(SLOW_DELAY, 400).expect("writable");
                settings.set_int(BOUNCE_DELAY, 5000).expect("writable");
                while context.iteration(false) {}
                assert_eq!(
                    sent.borrow().last(),
                    Some(&KeyboardAids {
                        sticky: true,
                        two_key_off: false,
                        slow: true,
                        slow_delay: 400,
                        bounce: true,
                        bounce_delay: MAX_AID_DELAY,
                        gestures: true,
                    })
                );
            })
            .expect("the context");
    }
}
