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

/// The layouts as a person reads them: a symbols file may name none, and a blank entry is no
/// choice, so `unnamed` words the place of such a layout, counted from 1.
pub fn labels(layouts: &[String], unnamed: impl Fn(usize) -> String) -> Vec<String> {
    let label = |(index, name): (usize, &String)| match name.as_str() {
        "" => unnamed(index + 1),
        _ => name.clone(),
    };
    layouts.iter().enumerate().map(label).collect()
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
    fn an_unnamed_layout_is_labelled_by_its_place() {
        let labels = labels(&names(&["English (US)", ""]), |n| format!("Layout {n}"));
        assert_eq!(labels, ["English (US)", "Layout 2"]);
        assert!(shown(&labels));
    }

    #[test]
    fn the_active_name_is_the_group_index_and_nothing_past_the_end() {
        let layouts = names(&["English (US)", "Italian"]);
        assert_eq!(active(&layouts, 1), Some("Italian"));
        assert_eq!(active(&layouts, 2), None);
        assert_eq!(active(&layouts, u32::MAX), None);
    }
}
