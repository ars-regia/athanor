//! Settings' translations: one catalog for the life of the process, read by athanor-i18n, and
//! handed to athanor-settings-core so that the registry's titles and keywords speak it too.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-settings";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be read is
/// logged and Settings speaks English.
pub fn init() {
    let catalog = CATALOG.get_or_init(|| {
        Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
            tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
            Catalog::empty()
        })
    });
    let _first = athanor_settings_core::i18n::set_catalog(catalog);
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

pub fn is_rtl() -> bool {
    catalog().is_rtl()
}
