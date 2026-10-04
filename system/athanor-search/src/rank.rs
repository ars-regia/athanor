//! Match quality (LA3): the tier says how the query matched, nucleo orders within a tier.
//! score = tier × 1 000 000 + nucleo score; usage adds at most 200 000 (`usage::boost`),
//! so it reorders within a tier and never lifts a row over a better tier.

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub const TIER_STEP: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Scattered = 0,
    Substring = 1,
    Initials = 2,
    Prefix = 3,
}

/// The tier of `query` in `candidate`, ignoring case and surrounding space; `None` when
/// it is at best a scattered match, which nucleo decides.
pub fn tier(candidate: &str, query: &str) -> Option<Tier> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let candidate = candidate.to_lowercase();
    if candidate.starts_with(&query) {
        return Some(Tier::Prefix);
    }
    let initials: String = candidate
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'))
        .filter_map(|word| word.chars().next())
        .collect();
    if initials.starts_with(&query) {
        return Some(Tier::Initials);
    }
    candidate.contains(&query).then_some(Tier::Substring)
}

pub struct Ranker {
    query: String,
    pattern: Pattern,
    matcher: Matcher,
    buffer: Vec<char>,
}

impl Ranker {
    pub fn new(query: &str) -> Ranker {
        let query = query.trim().to_owned();
        let pattern = Pattern::new(&query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        Ranker { query, pattern, matcher: Matcher::new(Config::DEFAULT), buffer: Vec::new() }
    }

    /// The best match of the query in `primary` (a name) or in one of `secondary`
    /// (generic name, keywords, executable), whose tier is capped at `Substring`.
    pub fn score(&mut self, primary: &str, secondary: &[&str]) -> Option<(Tier, i64)> {
        let mut best = self.one(primary, Tier::Prefix);
        for field in secondary {
            let candidate = self.one(field, Tier::Substring);
            if candidate.map(|c| c.1) > best.map(|b| b.1) {
                best = candidate;
            }
        }
        best
    }

    fn one(&mut self, field: &str, cap: Tier) -> Option<(Tier, i64)> {
        let fuzzy = self.pattern.score(Utf32Str::new(field, &mut self.buffer), &mut self.matcher)?;
        let tier = tier(field, &self.query).unwrap_or(Tier::Scattered).min(cap);
        Some((tier, tier as i64 * TIER_STEP + i64::from(fuzzy)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visible applications of the maintainer's desktop on 2026-10-01.
    const HOST: &[&str] = &[
        "btop++", "Bulk Rename", "COSMIC Files", "COSMIC Settings", "COSMIC Store",
        "COSMIC Terminal", "COSMIC Text Editor", "Files", "Firefox", "Foot", "Foot Client",
        "Foot Server", "Layout", "mpv Media Player", "Panel", "Removable Drives and Media",
        "Thunar File Manager", "Thunar Preferences", "Virtual Machine Manager",
        "Fine Tuning Extras", "Frame Buffer Index",
    ];

    fn best(query: &str) -> Vec<&'static str> {
        let mut ranker = Ranker::new(query);
        let mut scored: Vec<_> = HOST
            .iter()
            .filter_map(|name| ranker.score(name, &[]).map(|(_, score)| (score, *name)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        scored.into_iter().map(|(_, name)| name).collect()
    }

    #[test]
    fn tiers_follow_the_spec_order() {
        assert_eq!(tier("Firefox", "fire"), Some(Tier::Prefix));
        assert_eq!(tier("COSMIC Text Editor", "cte"), Some(Tier::Initials));
        assert_eq!(tier("Virtual Machine Manager", "machine"), Some(Tier::Substring));
        assert_eq!(tier("Firefox", "fx"), None);
        assert_eq!(tier("Bulk-Rename_tool", "brt"), Some(Tier::Initials));
    }

    #[test]
    fn fx_puts_firefox_first() {
        let order = best("fx");
        assert!(order.len() > 1, "{order:?}");
        assert_eq!(order.first(), Some(&"Firefox"), "{order:?}");
    }

    #[test]
    fn a_prefix_beats_initials_and_initials_beat_a_substring() {
        let order = best("fi");
        let files = order.iter().position(|n| *n == "Files");
        let cosmic_files = order.iter().position(|n| *n == "COSMIC Files");
        assert!(files < cosmic_files, "{order:?}");
        let cs = best("cs");
        assert!(cs[..2].contains(&"COSMIC Settings") && cs[..2].contains(&"COSMIC Store"), "{cs:?}");
    }

    #[test]
    fn each_tier_outscores_the_next_for_the_same_query() {
        let mut ranker = Ranker::new("fo");
        let (t_prefix, prefix) = ranker.score("Foot", &[]).expect("prefix");
        let (t_initials, initials) = ranker.score("Fine Output", &[]).expect("initials");
        let (t_substring, substring) = ranker.score("Big Foot", &[]).expect("substring");
        assert_eq!((t_prefix, t_initials, t_substring), (Tier::Prefix, Tier::Initials, Tier::Substring));
        assert!(prefix > initials && initials > substring, "{prefix} {initials} {substring}");
    }

    #[test]
    fn a_secondary_field_never_scores_above_a_substring() {
        let mut ranker = Ranker::new("net");
        let (tier, _) = ranker.score("Wi-Fi", &["Network", "Connection"]).expect("keyword match");
        assert_eq!(tier, Tier::Substring);
        let (name_tier, name) = ranker.score("Network Monitor", &[]).expect("name match");
        let (_, keyword) = ranker.score("Wi-Fi", &["Network"]).expect("keyword match");
        assert_eq!(name_tier, Tier::Prefix);
        assert!(name > keyword);
    }

    #[test]
    fn no_match_is_none() {
        assert_eq!(Ranker::new("zzz").score("Firefox", &["Browser"]), None);
    }

    #[test]
    fn case_and_surrounding_space_do_not_matter() {
        assert_eq!(tier("Firefox", "FIRE"), Some(Tier::Prefix));
        assert_eq!(Ranker::new("  Fire ").score("Firefox", &[]).map(|(t, _)| t), Some(Tier::Prefix));
    }
}
