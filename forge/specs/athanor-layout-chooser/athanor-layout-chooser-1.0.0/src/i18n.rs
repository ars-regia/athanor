//! The chooser's translations: one catalog for the life of the process, read by
//! athanor-i18n, as the greeter does.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-layout-chooser";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be
/// read is logged and the chooser speaks English.
pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    let _already_set = CATALOG.set(catalog);
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

/// `msgid` with `{key}` replaced by `value`.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    catalog().tr(msgid).replace(&format!("{{{key}}}"), value)
}

pub fn is_rtl() -> bool {
    catalog().is_rtl()
}
