//! The bar's translations: one catalog for the life of the process, read by athanor-i18n,
//! as the greeter and the chooser do.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-bar";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be
/// read is logged and the chooser speaks English.
pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the translations were already loaded; the second load is ignored");
    }
    // The row and the openers of athanor-apps speak through the same catalog.
    athanor_apps::i18n::set_catalog(self::catalog());
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

/// The form of a message that agrees with `n`, by the plural rule of the catalog.
pub fn tr_n(msgid: &str, msgid_plural: &str, n: u64) -> String {
    catalog().tr_n(msgid, msgid_plural, n).to_string()
}

/// The translation of `msgid` with `{key}` replaced by `value`. Translators move the
/// placeholder freely; a value is never part of a message id.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    catalog().tr(msgid).replace(&format!("{{{key}}}"), value)
}

pub fn is_rtl() -> bool {
    catalog().is_rtl()
}
