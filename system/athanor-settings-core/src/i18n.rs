//! The registry's strings, translated by the catalog of the program that shows them, as
//! `athanor-apps` does: Settings lists this crate's sources in its own `POTFILES.in` and hands
//! its catalog over once, after loading it.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

static CATALOG: OnceLock<&'static Catalog> = OnceLock::new();

/// Hands the program's catalog to this crate. A second call is refused with `false`.
pub fn set_catalog(catalog: &'static Catalog) -> bool {
    CATALOG.set(catalog).is_ok()
}

/// The translation of `msgid`, or `msgid` itself before a catalog is set.
pub fn tr(msgid: &str) -> String {
    match CATALOG.get() {
        Some(catalog) => catalog.tr(msgid).to_string(),
        None => msgid.to_owned(),
    }
}
