//! The history grouped by application (NC4, NC11), and which rows the panel shows and marks
//! read. Pure: no widget.

use std::collections::HashSet;

use athanor_services::notifications::wire::WireNotification;

/// A group of more rows than this collapses to its newest one.
const COLLAPSE_ABOVE: usize = 3;

pub struct Group {
    /// `""` is Other.
    pub app_id: String,
    /// Newest first.
    pub rows: Vec<u32>,
}

/// The groups of `entries`, the one with the newest notification first.
pub fn groups(entries: &[WireNotification]) -> Vec<Group> {
    // The daemon sends newest last; a stable sort of the reversed list puts the newest
    // first and keeps the daemon's order between equal times. A transient notification
    // never reaches the list (BR4).
    let mut listed: Vec<&WireNotification> = entries.iter().rev().filter(|n| !n.transient).collect();
    listed.sort_by_key(|n| std::cmp::Reverse(n.time));
    let mut groups: Vec<Group> = Vec::new();
    for entry in listed {
        match groups.iter_mut().find(|group| group.app_id == entry.app_id) {
            Some(group) => group.rows.push(entry.id),
            None => groups.push(Group {
                app_id: entry.app_id.clone(),
                rows: vec![entry.id],
            }),
        }
    }
    groups
}

/// Every row when the group is expanded or has at most three, else the newest one.
pub fn shown_rows(group: &Group, expanded: bool) -> &[u32] {
    if expanded || group.rows.len() <= COLLAPSE_ABOVE {
        &group.rows
    } else {
        &group.rows[..1]
    }
}

/// The ids to tell the daemon are read: the unread ones among the rows the panel shows,
/// minus those already asked for, so a busy chat does not become a stream of calls.
pub fn unread_to_mark(
    entries: &[WireNotification],
    groups: &[Group],
    expanded: &HashSet<String>,
    asked: &HashSet<u32>,
) -> Vec<u32> {
    let unread: HashSet<u32> = entries.iter().filter(|n| !n.read).map(|n| n.id).collect();
    groups
        .iter()
        .flat_map(|group| shown_rows(group, expanded.contains(&group.app_id)))
        .copied()
        .filter(|id| unread.contains(id) && !asked.contains(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, app_id: &str, time: i64, read: bool) -> WireNotification {
        WireNotification {
            id,
            app_id: app_id.into(),
            app_name: app_id.into(),
            summary: String::new(),
            body: String::new(),
            body_spans: Vec::new(),
            actions: Vec::new(),
            actions_available: true,
            urgency: 1,
            transient: false,
            resident: false,
            read,
            time,
            desktop_entry: String::new(),
            icon_name: String::new(),
            icon_file: String::new(),
            image_width: 0,
            image_height: 0,
            image_rgba: Vec::new(),
            timeout_ms: 0,
            popup_ms_left: 0,
            popup: false,
            value: -1,
            reply: false,
            reply_placeholder: String::new(),
        }
    }

    fn four_of_a() -> Vec<WireNotification> {
        (1..=4).map(|id| entry(id, "a", i64::from(id), false)).collect()
    }

    #[test]
    fn three_interleaved_apps_are_three_groups_newest_first() {
        let entries = [
            entry(1, "a", 10, false),
            entry(2, "b", 20, false),
            entry(3, "c", 30, false),
            entry(4, "a", 40, false),
            entry(5, "b", 50, false),
        ];
        let got = groups(&entries);
        let ids: Vec<_> = got.iter().map(|g| g.app_id.as_str()).collect();
        assert_eq!(ids, ["b", "a", "c"]);
        assert_eq!(got[0].rows, [5, 2]);
        assert_eq!(got[1].rows, [4, 1]);
    }

    #[test]
    fn a_group_of_four_collapses_to_one_row_and_expands_to_four() {
        let got = groups(&four_of_a());
        assert_eq!(shown_rows(&got[0], false), [4]);
        assert_eq!(shown_rows(&got[0], true), [4, 3, 2, 1]);
    }

    #[test]
    fn a_group_of_three_is_never_collapsed() {
        let entries = &four_of_a()[..3];
        assert_eq!(shown_rows(&groups(entries)[0], false), [3, 2, 1]);
    }

    #[test]
    fn the_empty_app_id_is_one_group() {
        let entries = [entry(1, "", 1, false), entry(2, "", 2, false)];
        let got = groups(&entries);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].app_id, "");
    }

    #[test]
    fn a_transient_notification_is_not_listed() {
        let mut gone = entry(1, "a", 1, false);
        gone.transient = true;
        assert!(groups(&[gone]).is_empty());
    }

    #[test]
    fn only_shown_unread_rows_are_marked() {
        let mut entries = four_of_a();
        entries[3].read = true; // id 4, the newest, shown while collapsed
        let got = groups(&entries);
        let (no_groups, none): (HashSet<String>, HashSet<u32>) = (HashSet::new(), HashSet::new());
        assert!(unread_to_mark(&entries, &got, &no_groups, &none).is_empty());
        entries[3].read = false;
        assert_eq!(unread_to_mark(&entries, &got, &no_groups, &none), [4]);
        let expanded = HashSet::from(["a".to_owned()]);
        assert_eq!(unread_to_mark(&entries, &got, &expanded, &none), [4, 3, 2, 1]);
    }

    #[test]
    fn an_id_already_asked_for_is_not_asked_again() {
        let entries = four_of_a();
        let got = groups(&entries);
        let expanded = HashSet::from(["a".to_owned()]);
        let asked: HashSet<u32> = HashSet::from([4, 2]);
        assert_eq!(unread_to_mark(&entries, &got, &expanded, &asked), [3, 1]);
    }
}
