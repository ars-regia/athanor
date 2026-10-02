//! The row's strings, translated by the catalog of the program that shows it: each
//! program lists this crate's sources in its own `POTFILES.in` and hands its catalog over
//! once, after loading it.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

static CATALOG: OnceLock<&'static Catalog> = OnceLock::new();

pub fn set_catalog(catalog: &'static Catalog) {
    if CATALOG.set(catalog).is_err() {
        tracing::warn!("the row's catalog was already set; the second one is ignored");
    }
}

/// The translation of `msgid`, or `msgid` itself before a catalog is set.
pub fn tr(msgid: &str) -> String {
    match CATALOG.get() {
        Some(catalog) => catalog.tr(msgid).to_string(),
        None => msgid.to_owned(),
    }
}

/// The translation of `msgid` with `{key}` replaced by `value`. Translators move the
/// placeholder freely; a value is never part of a message id.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    tr(msgid).replace(&format!("{{{key}}}"), value)
}
