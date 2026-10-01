//! The rows of the list in the order they are shown: the top hit, then each group under its
//! header, with the indexing row where the file group is or would be (LA3, LA10).

use athanor_search::board::Rows;
use athanor_search::item::{Group, Hit};

#[derive(Clone, Debug)]
pub enum Line {
    Top(Hit),
    /// A provider's group carries its application's name; the others are named by the UI.
    Header { group: Group, title: String },
    Hit(Hit),
    Indexing,
}

impl Line {
    pub fn hit(&self) -> Option<&Hit> {
        match self {
            Line::Top(hit) | Line::Hit(hit) => Some(hit),
            Line::Header { .. } | Line::Indexing => None,
        }
    }

    /// What identifies the row across answers: the usage key, or the title for rows that
    /// have none.
    fn id(&self) -> Option<&str> {
        self.hit().map(|hit| if hit.key.is_empty() { hit.title.as_str() } else { hit.key.as_str() })
    }
}

pub fn lines(rows: &Rows) -> Vec<Line> {
    let mut lines: Vec<Line> = rows.top.iter().cloned().map(Line::Top).collect();
    let mut indexing = rows.indexing;
    for section in &rows.sections {
        if indexing && section.group > Group::Files {
            lines.push(Line::Indexing);
            indexing = false;
        }
        lines.push(Line::Header { group: section.group, title: section.title.clone() });
        lines.extend(section.hits.iter().cloned().map(Line::Hit));
        if indexing && section.group == Group::Files {
            lines.push(Line::Indexing);
            indexing = false;
        }
    }
    if indexing {
        lines.push(Line::Indexing);
    }
    lines
}

/// The selectable row `delta` rows away from `from`, stopping at the ends; the first one
/// when nothing is selected.
pub fn step(lines: &[Line], from: Option<usize>, delta: i32) -> Option<usize> {
    let selectable: Vec<usize> = (0..lines.len()).filter(|&index| lines[index].hit().is_some()).collect();
    let Some(from) = from.and_then(|from| selectable.iter().position(|&index| index == from)) else {
        return selectable.first().copied();
    };
    let last = selectable.len().saturating_sub(1);
    let to = if delta < 0 {
        from.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        from.saturating_add(delta as usize).min(last)
    };
    selectable.get(to).copied()
}

/// The row to select in a new answer: the same row when it is still there, else the first.
pub fn reselect(lines: &[Line], selected: Option<&str>) -> Option<usize> {
    selected
        .and_then(|id| lines.iter().position(|line| line.id() == Some(id)))
        .or_else(|| step(lines, None, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_search::board::{Rows, Section};
    use athanor_search::item::{Action, Group, Hit};
    use athanor_search::rank::Tier;

    fn hit(group: Group, key: &str) -> Hit {
        Hit {
            group,
            key: key.to_owned(),
            title: key.to_owned(),
            subtitle: String::new(),
            icon: None,
            tier: Tier::Prefix,
            score: 0,
            learned: false,
            action: Action::Copy { text: key.to_owned() },
        }
    }

    fn rows(indexing: bool) -> Rows {
        Rows {
            top: Some(hit(Group::Apps, "app:firefox.desktop")),
            sections: vec![
                Section { group: Group::Apps, title: String::new(), hits: vec![hit(Group::Apps, "app:files.desktop")] },
                Section { group: Group::Files, title: String::new(), hits: vec![hit(Group::Files, "file:///a")] },
                Section { group: Group::Providers, title: "Calculator".into(), hits: vec![hit(Group::Providers, "")] },
            ],
            indexing,
        }
    }

    #[test]
    fn the_top_hit_comes_first_and_every_section_has_a_header() {
        let lines = lines(&rows(false));
        let shape: Vec<&str> = lines.iter().map(|line| match line {
            Line::Top(_) => "top",
            Line::Header { .. } => "header",
            Line::Hit(_) => "hit",
            Line::Indexing => "indexing",
        }).collect();
        assert_eq!(shape, ["top", "header", "hit", "header", "hit", "header", "hit"]);
        assert!(matches!(&lines[5], Line::Header { group: Group::Providers, title } if title == "Calculator"));
    }

    #[test]
    fn indexing_is_said_where_the_files_are_or_would_be() {
        let with_files = lines(&rows(true));
        assert!(matches!(with_files[5], Line::Indexing), "after the file rows, before the providers");
        let mut no_files = rows(true);
        no_files.sections.remove(1);
        let without = lines(&no_files);
        assert!(matches!(without[3], Line::Indexing), "where the file group would be");
        let only = lines(&Rows { indexing: true, ..Rows::default() });
        assert!(matches!(only.as_slice(), [Line::Indexing]));
    }

    #[test]
    fn the_selection_skips_headers_and_stops_at_the_ends() {
        let lines = lines(&rows(false));
        assert_eq!(step(&lines, None, 1), Some(0));
        assert_eq!(step(&lines, Some(0), 1), Some(2), "past the header");
        assert_eq!(step(&lines, Some(6), 1), Some(6), "the last row stays");
        assert_eq!(step(&lines, Some(2), -1), Some(0));
        assert_eq!(step(&lines, Some(0), -5), Some(0));
        assert_eq!(step(&lines, Some(0), 5), Some(6), "a page moves over five rows at most");
        assert_eq!(step(&[], None, 1), None);
    }

    #[test]
    fn a_new_answer_keeps_the_selected_row_when_it_is_still_there() {
        let lines = lines(&rows(false));
        assert_eq!(reselect(&lines, Some("file:///a")), Some(4));
        assert_eq!(reselect(&lines, Some("gone")), Some(0), "else the first row");
        assert_eq!(reselect(&lines, None), Some(0));
        assert_eq!(reselect(&[Line::Indexing], None), None, "nothing to select");
    }
}
