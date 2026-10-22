//! The words of the two notifications, in the two shipped locales.
// ponytail: a two-language table. When plan 1a's `system/athanor-i18n` lands, replace the
// body of `texts` with catalog lookups; the callers do not change.
use crate::notices::{Notice, ACTION_APPLY, ACTION_GO_BACK, ACTION_LATER, ACTION_POSTPONE};
use chrono::Datelike as _;

pub struct Texts {
    pub ready_summary: &'static str,
    pub ready_body: &'static str,
    pub restart: &'static str,
    pub later: &'static str,
    pub running_summary: &'static str,
    /// `{version}` and `{date}` are replaced.
    pub running_body: &'static str,
    pub go_back: &'static str,
    pub security_title: &'static str,
    pub security_body: &'static str,
    pub postpone: &'static str,
    /// `{time}` is replaced.
    pub postponed_until: &'static str,
    months: [&'static str; 12],
}

const EN: Texts = Texts {
    ready_summary: "A system update is ready",
    ready_body: "It is installed when you restart. Nothing changes until then.",
    restart: "Restart to update",
    later: "Later",
    running_summary: "The system was updated",
    running_body: "Version {version} of {date} is now running.",
    go_back: "Go back to the previous version",
    security_title: "A security update is ready",
    security_body: "It installs at the next shutdown or restart. Nothing restarts by itself.",
    postpone: "Postpone",
    postponed_until: "The security update is postponed, at most until {time}; then it installs at the next shutdown or restart.",
    months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
};

const IT: Texts = Texts {
    ready_summary: "Un aggiornamento di sistema è pronto",
    ready_body: "Viene installato al riavvio. Fino ad allora non cambia nulla.",
    restart: "Riavvia per aggiornare",
    later: "Più tardi",
    running_summary: "Il sistema è stato aggiornato",
    running_body: "Ora è in esecuzione la versione {version} del {date}.",
    go_back: "Torna alla versione precedente",
    security_title: "Un aggiornamento di sicurezza è pronto",
    security_body: "Si installa al prossimo spegnimento o riavvio. Il computer non si riavvia da solo.",
    postpone: "Rimanda",
    postponed_until: "L'aggiornamento di sicurezza è rimandato, al massimo fino al {time}; poi si installa al prossimo spegnimento o riavvio.",
    months: ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
};

impl Texts {
    /// "28 October, 14:30" (Italian "28 ottobre, 14:30"), local time. `/etc/localtime` links
    /// into `/usr/share/zoneinfo`, which the Landlock ruleset grants.
    #[must_use]
    pub fn time(&self, seconds: i64) -> String {
        let Some(time) = chrono::DateTime::from_timestamp(seconds, 0).map(|time| time.with_timezone(&chrono::Local)) else { return String::new() };
        format!("{} {}, {}", time.day(), self.months[time.month0() as usize], time.format("%H:%M"))
    }
}

/// The flat (identifier, label, ...) action list a notice is sent with.
#[must_use]
pub fn actions(notice: &Notice, t: &Texts) -> Vec<&'static str> {
    match notice {
        Notice::Ready { .. } => vec![ACTION_APPLY, t.restart, ACTION_LATER, t.later],
        Notice::Running { .. } => vec![ACTION_GO_BACK, t.go_back],
        Notice::Security { until: None, .. } => vec![ACTION_POSTPONE, t.postpone],
        Notice::Security { until: Some(_), .. } => vec![],
    }
}

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

    #[test]
    fn italian_texts_exist_for_every_security_string() {
        let (en, it) = (texts(None), texts(Some("it_IT")));
        for (english, italian) in [(en.security_title, it.security_title), (en.security_body, it.security_body), (en.postpone, it.postpone), (en.postponed_until, it.postponed_until)] {
            assert!(!italian.is_empty() && italian != english, "{italian}");
        }
        assert_eq!(it.postpone, "Rimanda");
        assert!(it.postponed_until.contains("{time}") && en.postponed_until.contains("{time}"));
        let (english, italian) = (en.time(1_790_604_800), it.time(1_790_604_800));
        assert!(english.starts_with("28 September, ") && italian.starts_with("28 settembre, "), "{english} / {italian}");
    }
}
