//! The overlay's translations: one catalog for the life of the process, read by athanor-i18n.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-osd";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be read is
/// logged and the overlay speaks English.
pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the translations were already loaded; the second load is ignored");
    }
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

/// `msgid` translated, with `{key}` replaced by `value`.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    tr(msgid).replace(&format!("{{{key}}}"), value)
}

pub fn is_rtl() -> bool {
    catalog().is_rtl()
}
