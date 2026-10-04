//! The daemon's translations: one catalog for the life of the process, read by athanor-i18n,
//! as the bar and the launcher do. The only text the daemon shows is the summary it posts
//! when do not disturb ends.

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-shelld";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment. A catalog that cannot be read is
/// logged and the daemon speaks English.
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

/// The plural form for `n`; the caller replaces `{n}`.
pub fn tr_n(msgid: &str, msgid_plural: &str, n: u64) -> String {
    catalog().tr_n(msgid, msgid_plural, n).to_string()
}
