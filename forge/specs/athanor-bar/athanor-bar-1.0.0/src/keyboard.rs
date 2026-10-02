//! The input source (doc_bar.md, BR3): shown only when there is something to switch between.

pub fn shown(layouts: &[String]) -> bool {
    layouts.len() >= 2
}

/// The active layout's name. The group can point past the list for a moment while the
/// compositor reconfigures; that is no name, not a crash.
pub fn active(layouts: &[String], group: u32) -> Option<&str> {
    layouts
        .get(usize::try_from(group).ok()?)
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn shown_only_with_something_to_switch_between() {
        assert!(!shown(&names(&[])));
        assert!(!shown(&names(&["English (US)"])));
        assert!(shown(&names(&["English (US)", "Italian"])));
    }

    #[test]
    fn the_active_name_is_the_group_index_and_nothing_past_the_end() {
        let layouts = names(&["English (US)", "Italian"]);
        assert_eq!(active(&layouts, 1), Some("Italian"));
        assert_eq!(active(&layouts, 2), None);
        assert_eq!(active(&layouts, u32::MAX), None);
    }
}
