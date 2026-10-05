//! The unread badge of the notifications button (F-bar-08): the text it shows and the name
//! assistive technologies read. No GTK type, so it is tested without a display.

/// The most the badge counts before it says "99+".
const BADGE_MAX: usize = 99;

/// What the badge shows: nothing for none, the count up to 99, then "99+".
pub fn badge_text(unread: usize) -> Option<String> {
    match unread {
        0 => None,
        1..=BADGE_MAX => Some(unread.to_string()),
        _ => Some(format!("{BADGE_MAX}+")),
    }
}

/// The accessible name of the button. `tr` and `tr_n` are the catalog of the session;
/// `{count}` in the plural message is replaced by the number, and the catalog's own plural
/// rule picks the form (Italian "non letta" for one, "non lette" for the others).
pub fn accessible_name(
    unread: usize,
    tr: &dyn Fn(&str) -> String,
    tr_n: &dyn Fn(&str, &str, u64) -> String,
) -> String {
    match unread {
        0 => tr("Notifications"),
        n => tr_n(
            "Notifications, {count} unread",
            "Notifications, {count} unread",
            u64::try_from(n).unwrap_or(u64::MAX),
        )
        .replace("{count}", &n.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use athanor_i18n::Catalog;

    const IT: &[u8] = include_bytes!("../tests/fixtures/badge-it.mo");

    fn name(catalog: &Catalog, unread: usize) -> String {
        accessible_name(
            unread,
            &|msgid| catalog.tr(msgid).to_owned(),
            &|msgid, plural, n| catalog.tr_n(msgid, plural, n).to_owned(),
        )
    }

    #[test]
    fn the_badge_hides_for_none_and_caps_at_ninety_nine() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(1).as_deref(), Some("1"));
        assert_eq!(badge_text(99).as_deref(), Some("99"));
        assert_eq!(badge_text(100).as_deref(), Some("99+"));
        assert_eq!(badge_text(usize::MAX).as_deref(), Some("99+"));
    }

    #[test]
    fn untranslated_the_name_counts_the_unread() {
        let english = Catalog::empty();
        assert_eq!(name(&english, 0), "Notifications");
        assert_eq!(name(&english, 1), "Notifications, 1 unread");
        assert_eq!(name(&english, 3), "Notifications, 3 unread");
    }

    #[test]
    fn the_name_follows_the_catalogs_plural_rule() {
        let it = Catalog::parse(IT).expect("the fixture is msgfmt output");
        assert_eq!(name(&it, 0), "Notifiche");
        assert_eq!(name(&it, 1), "Notifiche, 1 non letta");
        assert_eq!(name(&it, 3), "Notifiche, 3 non lette");
        assert_eq!(name(&it, 150), "Notifiche, 150 non lette");
    }
}
