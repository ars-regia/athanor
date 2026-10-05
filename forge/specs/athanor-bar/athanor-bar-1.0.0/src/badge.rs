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

/// The accessible name of the button. `translate` turns a message id into the language of
/// the session; `{count}` in the plural message is replaced by the number. The singular is
/// a message of its own, because languages differ in how they agree it (it, "1 non letta").
pub fn accessible_name(unread: usize, translate: &dyn Fn(&str) -> String) -> String {
    match unread {
        0 => translate("Notifications"),
        1 => translate("Notifications, 1 unread"),
        n => translate("Notifications, {count} unread").replace("{count}", &n.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(msgid: &str) -> String {
        msgid.to_owned()
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
    fn the_name_counts_the_unread() {
        assert_eq!(accessible_name(0, &identity), "Notifications");
        assert_eq!(accessible_name(1, &identity), "Notifications, 1 unread");
        assert_eq!(accessible_name(3, &identity), "Notifications, 3 unread");
    }

    #[test]
    fn the_name_goes_through_the_translation() {
        let it = |msgid: &str| match msgid {
            "Notifications" => "Notifiche".to_owned(),
            "Notifications, 1 unread" => "Notifiche, una non letta".to_owned(),
            "Notifications, {count} unread" => "Notifiche, {count} non lette".to_owned(),
            other => other.to_owned(),
        };
        assert_eq!(accessible_name(1, &it), "Notifiche, una non letta");
        assert_eq!(accessible_name(3, &it), "Notifiche, 3 non lette");
    }
}
