//! The control center's catalog for the life of the process, read by athanor-i18n as the
//! launcher and the bar do. No catalog ships yet (the pages are placeholders), so it speaks
//! English; the language still gives the text direction.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-control-center";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the translations were already loaded; the second load is ignored");
    }
}

pub fn is_rtl() -> bool {
    CATALOG.get_or_init(Catalog::empty).is_rtl()
}
