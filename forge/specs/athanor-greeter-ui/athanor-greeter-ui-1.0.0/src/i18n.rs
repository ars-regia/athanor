//! The greeter's translations: one catalog for the life of the process, read by
//! athanor-i18n. No setlocale(), no textdomain(): nothing process-global changes in the
//! process that reads the password (doc_shell.md, SH13; decision of 2026-09-19).

use std::sync::OnceLock;

use athanor_i18n::Catalog;

pub const DOMAIN: &str = "athanor-greeter-ui";

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Loads the catalog for the language of the environment, or of /etc/locale.conf. A
/// catalog that cannot be read is logged and the greeter speaks English: a greeter in
/// the wrong language still lets the user in.
pub fn init() {
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
        Catalog::empty()
    });
    tracing::info!(
        language = catalog.language().unwrap_or("en (message ids)"),
        "translations loaded"
    );
    // A second call keeps the first catalog; there is none in this program.
    let _already_set = CATALOG.set(catalog);
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(Catalog::empty)
}

/// The translation of `msgid`.
pub fn tr(msgid: &str) -> String {
    catalog().tr(msgid).to_string()
}

/// The translation of `msgid` with `{key}` replaced by `value`. Translators move the
/// placeholder freely; a value is never part of a message id.
pub fn tr_with(msgid: &str, key: &str, value: &str) -> String {
    catalog().tr(msgid).replace(&format!("{{{key}}}"), value)
}

/// Whether the language in use is written right to left.
pub fn is_rtl() -> bool {
    static RTL: OnceLock<bool> = OnceLock::new();
    *RTL.get_or_init(|| {
        athanor_i18n::is_rtl(DOMAIN).unwrap_or_else(|(path, err)| {
            tracing::error!(path = %path.display(), error = %err, "translations are unavailable");
            false
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_catalog_the_message_id_is_the_text() {
        assert_eq!(tr("Shut down"), "Shut down");
    }

    #[test]
    fn the_placeholder_is_replaced_and_nothing_else() {
        assert_eq!(
            tr_with("Password for {name}", "name", "Ada {x}"),
            "Password for Ada {x}"
        );
    }
}
