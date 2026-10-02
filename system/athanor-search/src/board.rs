//! The rows of one query (LA3): groups in a fixed order, capped, and a top hit only when
//! the best row leads: it is learned for this query, or its tier is above the second's.
//! A generation number refuses the answers of an older query (LA4).

use std::collections::BTreeMap;

use crate::command::Refusal;
use crate::item::{Group, Hit};

#[derive(Clone, Debug)]
pub struct Section {
    pub group: Group,
    /// A provider's application name; empty for the other groups.
    pub title: String,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub top: Option<Hit>,
    pub sections: Vec<Section>,
    /// localsearch is indexing: one row says so (LA10).
    pub indexing: bool,
    /// The query is a command that is not run: the only row says why.
    pub refused: Option<Refusal>,
}

#[derive(Debug, Default)]
pub struct Board {
    generation: u64,
    /// Keyed by group, then by the source within it (a provider's bus name).
    sections: BTreeMap<(Group, String), Section>,
    indexing: bool,
    refused: Option<Refusal>,
}

impl Board {
    pub fn start(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.sections.clear();
        self.indexing = false;
        self.refused = None;
        self.generation
    }

    pub fn put(&mut self, generation: u64, group: Group, source: &str, title: &str, hits: Vec<Hit>) -> bool {
        if generation != self.generation {
            return false;
        }
        let key = (group, source.to_owned());
        if hits.is_empty() {
            self.sections.remove(&key);
        } else {
            self.sections.insert(key, Section { group, title: title.to_owned(), hits });
        }
        true
    }

    pub fn set_indexing(&mut self, generation: u64, indexing: bool) -> bool {
        if generation != self.generation {
            return false;
        }
        self.indexing = indexing;
        true
    }

    /// The query of `generation` is a command that is not run.
    pub fn refuse(&mut self, generation: u64, why: Refusal) -> bool {
        if generation != self.generation {
            return false;
        }
        self.refused = Some(why);
        true
    }

    pub fn rows(&self) -> Rows {
        if let Some(why) = self.refused {
            return Rows { refused: Some(why), ..Rows::default() };
        }
        if let Some(command) = self.sections.get(&(Group::Command, String::new())) {
            return Rows { top: None, sections: vec![command.clone()], indexing: false, refused: None };
        }
        let mut sections: Vec<Section> = self
            .sections
            .values()
            .map(|section| {
                let mut section = section.clone();
                section.hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
                section.hits.truncate(match section.group {
                    Group::Providers => Group::PROVIDER_ROWS,
                    _ => Group::ROWS,
                });
                section
            })
            .collect();
        let top = leader(&sections).map(|at| sections[at].hits.remove(0));
        sections.retain(|section| !section.hits.is_empty());
        Rows { top, sections, indexing: self.indexing, refused: None }
    }
}

/// The section whose first row leads every other competing row (LA3), if one does.
fn leader(sections: &[Section]) -> Option<usize> {
    let mut competing: Vec<(usize, &Hit)> = sections
        .iter()
        .enumerate()
        .filter(|(_, section)| section.group.competes_for_top())
        .flat_map(|(at, section)| section.hits.iter().map(move |hit| (at, hit)))
        .collect();
    competing.sort_by_key(|a| std::cmp::Reverse(a.1.score));
    match competing.as_slice() {
        [] => None,
        [(at, _)] => Some(*at),
        [(at, first), (_, second), ..] => (first.learned || first.tier > second.tier).then_some(*at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Action;
    use crate::rank::Tier;

    fn hit(group: Group, title: &str, tier: Tier, score: i64, learned: bool) -> Hit {
        Hit {
            group,
            key: format!("k:{title}"),
            title: title.into(),
            subtitle: String::new(),
            icon: None,
            tier,
            score: tier as i64 * crate::rank::TIER_STEP + score,
            learned,
            action: Action::Copy { text: title.into() },
        }
    }

    fn titles(rows: &Rows) -> Vec<(Group, Vec<String>)> {
        rows.sections.iter().map(|s| (s.group, s.hits.iter().map(|h| h.title.clone()).collect())).collect()
    }

    #[test]
    fn a_stale_generation_is_refused() {
        let mut board = Board::default();
        let old = board.start();
        let new = board.start();
        assert!(!board.put(old, Group::Apps, "", "", vec![hit(Group::Apps, "Old", Tier::Prefix, 0, false)]));
        assert!(!board.set_indexing(old, true));
        assert!(board.rows().sections.is_empty() && !board.rows().indexing);
        assert!(board.put(new, Group::Apps, "", "", vec![hit(Group::Apps, "New", Tier::Prefix, 0, false)]));
        assert_eq!(board.rows().top.map(|h| h.title), Some("New".into()));
    }

    #[test]
    fn groups_keep_their_order_and_their_caps() {
        let mut board = Board::default();
        let g = board.start();
        let apps = (0..7).map(|i| hit(Group::Apps, &format!("A{i}"), Tier::Substring, i, false)).collect();
        board.put(g, Group::Web, "", "", vec![hit(Group::Web, "web", Tier::Scattered, 0, false)]);
        board.put(g, Group::Providers, "org.b", "B", (0..5).map(|i| hit(Group::Providers, &format!("B{i}"), Tier::Scattered, 0, false)).collect());
        board.put(g, Group::Providers, "org.a", "A", vec![hit(Group::Providers, "A0", Tier::Scattered, 0, false)]);
        board.put(g, Group::Calc, "", "", vec![hit(Group::Calc, "8", Tier::Prefix, 0, false)]);
        board.put(g, Group::Apps, "", "", apps);
        let rows = board.rows();
        let order: Vec<_> = rows.sections.iter().map(|s| (s.group, s.title.as_str())).collect();
        assert_eq!(order, [(Group::Apps, ""), (Group::Calc, ""), (Group::Providers, "A"), (Group::Providers, "B"), (Group::Web, "")]);
        assert_eq!(rows.sections[0].hits.len(), Group::ROWS);
        assert_eq!(rows.sections[0].hits[0].title, "A6", "best first");
        assert_eq!(rows.sections[3].hits.len(), Group::PROVIDER_ROWS);
        assert!(rows.top.is_none(), "seven apps of one tier: no row leads");
    }

    #[test]
    fn a_row_one_tier_ahead_becomes_the_top_hit_and_leaves_its_group() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Apps, "", "", vec![hit(Group::Apps, "Firefox", Tier::Prefix, 0, false), hit(Group::Apps, "Thunar", Tier::Substring, 0, false)]);
        board.put(g, Group::Files, "", "", vec![hit(Group::Files, "fire.txt", Tier::Substring, 9, false)]);
        let rows = board.rows();
        assert_eq!(rows.top.as_ref().map(|h| h.title.as_str()), Some("Firefox"));
        assert_eq!(titles(&rows), [(Group::Apps, vec!["Thunar".into()]), (Group::Files, vec!["fire.txt".into()])]);
    }

    #[test]
    fn a_learned_row_is_the_top_hit_even_in_a_tie() {
        let mut board = Board::default();
        let g = board.start();
        let mut learned = hit(Group::Files, "Relazione_Q3.odt", Tier::Prefix, 0, true);
        learned.score += crate::usage::LEARNED_BONUS;
        board.put(g, Group::Files, "", "", vec![learned]);
        board.put(g, Group::Apps, "", "", vec![hit(Group::Apps, "Release Notes", Tier::Prefix, 5, false)]);
        assert_eq!(board.rows().top.map(|h| h.title), Some("Relazione_Q3.odt".into()));
    }

    #[test]
    fn calculator_providers_and_web_never_take_the_top() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Calc, "", "", vec![hit(Group::Calc, "8", Tier::Prefix, 0, false)]);
        board.put(g, Group::Web, "", "", vec![hit(Group::Web, "2+2*3", Tier::Scattered, 0, false)]);
        assert!(board.rows().top.is_none());
    }

    #[test]
    fn a_command_is_shown_alone() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Apps, "", "", vec![hit(Group::Apps, "htop", Tier::Prefix, 0, false)]);
        board.put(g, Group::Command, "", "", vec![hit(Group::Command, "htop", Tier::Prefix, 0, false)]);
        let rows = board.rows();
        assert!(rows.top.is_none());
        assert_eq!(titles(&rows), [(Group::Command, vec!["htop".into()])]);
    }

    #[test]
    fn a_refused_command_is_shown_alone() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Web, "", "", vec![hit(Group::Web, ">\"x", Tier::Scattered, 0, false)]);
        assert!(board.refuse(g, Refusal::Quoting));
        let rows = board.rows();
        assert!(rows.sections.is_empty() && rows.top.is_none());
        assert_eq!(rows.refused, Some(Refusal::Quoting));
        let next = board.start();
        assert!(!board.refuse(g, Refusal::Empty), "a stale generation is refused");
        assert!(board.rows().refused.is_none());
        assert_ne!(g, next);
    }

    #[test]
    fn an_empty_answer_removes_its_section() {
        let mut board = Board::default();
        let g = board.start();
        board.put(g, Group::Files, "", "", vec![hit(Group::Files, "a", Tier::Prefix, 0, false), hit(Group::Files, "b", Tier::Prefix, 0, false)]);
        board.put(g, Group::Files, "", "", Vec::new());
        assert!(board.rows().sections.is_empty());
    }
}
