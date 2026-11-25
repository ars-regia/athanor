//! The control center's catalog for the life of the process, read by athanor-i18n as the
//! launcher and the bar do. The pages of athanor-controls speak through it as well: its
//! `POTFILES.in` lists their sources.

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
    // The pages of athanor-controls speak through the same catalog.
    athanor_controls::i18n::set_catalog(catalog_ref());
}

fn catalog_ref() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn is_rtl() -> bool {
    catalog_ref().is_rtl()
}

pub fn tr(msgid: &str) -> String {
    catalog_ref().tr(msgid).to_string()
}

/// The translation of `msgid` with `{key}` replaced by `value`. Translators move the
/// placeholder freely; a value is never part of a message id.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    tr(msgid).replace(&format!("{{{key}}}"), value)
}
