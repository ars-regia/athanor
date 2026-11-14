//! The apply function against GSettings' memory backend. A test binary of its own: GIO
//! reads the backend and the schema directories once per process, from the environment.

use std::cell::Cell;
use std::io;
use std::path::Path;
use std::process::Command;

use athanor_style::appearance::{self, Accent, AccentMode, Appearance, Rgb};
use gtk4::gio;
use gtk4::prelude::*;

const SCHEMA: &str = "org.athanor.desktop.appearance.gschema.xml";

/// Our schema compiled from the package's source; GNOME's come from the system.
fn memory_store() {
    let dir = std::env::temp_dir().join(format!("athanor-apply-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas")
        .join(SCHEMA);
    std::fs::copy(source, dir.join(SCHEMA)).expect("schema");
    let status = Command::new("glib-compile-schemas")
        .arg(&dir)
        .status()
        .expect("glib-compile-schemas");
    assert!(status.success());
    std::env::set_var("GSETTINGS_BACKEND", "memory");
    std::env::set_var("GSETTINGS_SCHEMA_DIR", &dir);
}

#[test]
fn apply_writes_the_store_then_the_compositor() {
    memory_store();
    let choice = Appearance {
        dark: true,
        high_contrast: true,
        accent: Accent::Teal,
        accent_mode: AccentMode::Wallpaper,
        accent_computed: Rgb::from_hex("#336699"),
        animations: true,
        font_size: Appearance::default().font_size,
    };
    let seen = Cell::new(None);
    appearance::apply(
        |current| *current = choice,
        |written| {
            seen.set(Some((*written, Appearance::read())));
            Ok(())
        },
    )
    .expect("apply");
    assert_eq!(
        seen.get(),
        Some((choice, choice)),
        "the compositor is written last, after the store holds the choice"
    );
    let interface = gio::Settings::new("org.gnome.desktop.interface");
    assert_eq!(interface.string("color-scheme"), "prefer-dark");
    assert_eq!(interface.string("accent-color"), "teal");
    assert!(gio::Settings::new("org.gnome.desktop.a11y.interface").boolean("high-contrast"));
    let ours = gio::Settings::new("org.athanor.desktop.appearance");
    assert_eq!(ours.string("accent-mode"), "wallpaper");
    assert_eq!(ours.string("accent-computed"), "#336699");

    // The compositor's failure leaves the store written and the result Ok.
    let back = Appearance::default();
    appearance::apply(
        |current| *current = back,
        |_| Err(io::Error::other("no cosmic")),
    )
    .expect("apply");
    assert_eq!(Appearance::read(), back);
    assert_eq!(interface.string("color-scheme"), "default");
    assert_eq!(ours.string("accent-computed"), "");

    // Another tool's value that reads as the default is left alone when another key changes.
    interface
        .set_string("color-scheme", "prefer-light")
        .expect("color-scheme");
    let contrast = Appearance {
        high_contrast: true,
        ..back
    };
    appearance::apply(|current| current.high_contrast = true, |_| Ok(())).expect("apply");
    assert_eq!(interface.string("color-scheme"), "prefer-light");
    assert_eq!(Appearance::read(), contrast);

    // The change lands on the store's values, not on a copy the caller read earlier: a
    // scheme another process wrote in between is kept.
    let stale = Appearance::read();
    interface
        .set_string("color-scheme", "prefer-dark")
        .expect("color-scheme");
    appearance::apply(
        |current| current.high_contrast = !stale.high_contrast,
        |_| Ok(()),
    )
    .expect("apply");
    assert_eq!(interface.string("color-scheme"), "prefer-dark");
    assert!(!Appearance::read().high_contrast);

    // Reduced motion is GNOME's `enable-animations` turned off; `apply` never writes it.
    assert!(Appearance::read().animations);
    interface
        .set_boolean("enable-animations", false)
        .expect("enable-animations");
    assert!(!Appearance::read().animations);
    appearance::apply(|current| current.high_contrast = true, |_| Ok(())).expect("apply");
    assert!(!interface.boolean("enable-animations") && !Appearance::read().animations);

    // The font's size is GNOME's `font-name` at `text-scaling-factor`, as GTK reads them;
    // `apply` never writes them.
    interface
        .set_string("font-name", "Inter Bold 12")
        .expect("font-name");
    interface
        .set_double("text-scaling-factor", 1.5)
        .expect("text-scaling-factor");
    assert_eq!(
        Appearance::read().font_size,
        24.0,
        "12 pt at 96 dpi, times 1.5"
    );
    appearance::apply(|current| current.font_size = 9.0, |_| Ok(())).expect("apply");
    assert_eq!(interface.string("font-name"), "Inter Bold 12");
    assert_eq!(Appearance::read().font_size, 24.0);
}
