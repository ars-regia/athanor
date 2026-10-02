//! The words of the two notifications, in the two shipped locales.
// ponytail: a two-language table. When plan 1a's `system/athanor-i18n` lands, replace the
// body of `texts` with catalog lookups; the callers do not change.

pub struct Texts {
    pub ready_summary: &'static str,
    pub ready_body: &'static str,
    pub restart: &'static str,
    pub later: &'static str,
    pub running_summary: &'static str,
    /// `{version}` and `{date}` are replaced.
    pub running_body: &'static str,
    pub go_back: &'static str,
}

const EN: Texts = Texts {
    ready_summary: "A system update is ready",
    ready_body: "It is installed when you restart. Nothing changes until then.",
    restart: "Restart to update",
    later: "Later",
    running_summary: "The system was updated",
    running_body: "Version {version} of {date} is now running.",
    go_back: "Go back to the previous version",
};

const IT: Texts = Texts {
    ready_summary: "Un aggiornamento di sistema è pronto",
    ready_body: "Viene installato al riavvio. Fino ad allora non cambia nulla.",
    restart: "Riavvia per aggiornare",
    later: "Più tardi",
    running_summary: "Il sistema è stato aggiornato",
    running_body: "Ora è in esecuzione la versione {version} del {date}.",
    go_back: "Torna alla versione precedente",
};

/// The locale of the session: `LC_ALL`, then `LC_MESSAGES`, then `LANG`.
#[must_use]
pub fn texts(locale: Option<&str>) -> &'static Texts {
    if locale.is_some_and(|locale| locale.starts_with("it")) { &IT } else { &EN }
}

#[must_use]
pub fn session_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn italian_for_an_italian_session_english_otherwise() {
        assert_eq!(texts(Some("it_IT.UTF-8")).later, "Più tardi");
        assert_eq!(texts(Some("de_DE.UTF-8")).later, "Later");
        assert_eq!(texts(None).restart, "Restart to update");
    }
}
