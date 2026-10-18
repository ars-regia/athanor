//! Calmo, the Athanor identity, as GTK4 stylesheets.
//!
//! The four sheets are generated from `calmo/tokens.toml` by `calmo/generate.py` and
//! embedded here, so a surface cannot start without its stylesheet and no CSS file has
//! to be installed, found or readable inside a sandbox. Colours are named colours
//! (`@ath_acc`, ...): a surface that follows the user's COSMIC accent re-defines
//! `ath_acc` from a provider of higher priority; the greeter never does.

use std::cell::RefCell;

use gtk4::gdk;

pub use crate::variant::Variant;

thread_local! {
    static PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

/// Installs `variant` on `display`, replacing the sheet a previous call installed. GTK
/// inherits nothing from COSMIC (spike P1), so its own dark preference is set here too:
/// whatever a widget draws that our rules do not reach then matches the variant.
pub fn load(display: &gdk::Display, variant: Variant) {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(variant.css());
    PROVIDER.with(|slot| {
        if let Some(previous) = slot.borrow_mut().replace(provider.clone()) {
            gtk4::style_context_remove_provider_for_display(display, &previous);
        }
    });
    gtk4::style_context_add_provider_for_display(
        display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(variant.is_dark());
    }
}
