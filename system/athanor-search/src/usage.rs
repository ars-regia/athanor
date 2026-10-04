//! Usage (LA3): each launch records the item and the query that led to it. The weight of
//! an item halves every seven days; a query completed before puts the same item first.
//! The file never leaves the machine and is bounded, so it cannot grow with use.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const HALF_LIFE_SECONDS: f64 = 7.0 * 86_400.0;
pub const MAX_ITEMS: usize = 500;
pub const MAX_QUERIES: usize = 500;
const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 1 << 20;
const BOOST_PER_USE: f64 = 20_000.0;
/// Above any match: a query completed before puts the same item first (LA3).
pub const LEARNED_BONUS: i64 = 10_000_000;
pub(crate) const MAX_BOOST: i64 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Use {
    score: f64,
    at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Learned {
    key: String,
    at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    version: u32,
    items: BTreeMap<String, Use>,
    queries: BTreeMap<String, Learned>,
}

impl Default for Usage {
    fn default() -> Usage {
        Usage { version: VERSION, items: BTreeMap::new(), queries: BTreeMap::new() }
    }
}

/// The ranking bonus of a weight: 20 000 per recent use, at most 200 000, which is below
/// one tier (`rank::TIER_STEP`).
pub fn boost(weight: f64) -> i64 {
    (weight * BOOST_PER_USE).clamp(0.0, MAX_BOOST as f64) as i64
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The file is user-writable: anything larger than `MAX_FILE_BYTES` is not ours.
fn read_bounded(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)?.take(MAX_FILE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "larger than 1 MiB"));
    }
    Ok(text)
}

fn normalize(query: &str) -> String {
    query.trim().to_lowercase()
}

impl Usage {
    /// The stored usage; an absent file is a first run, an unreadable or foreign one is
    /// logged and replaced at the next save.
    pub fn load(path: &Path) -> Usage {
        let text = match read_bounded(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Usage::default(),
            Err(err) => {
                tracing::warn!("cannot read {}: {err}; usage starts empty", path.display());
                return Usage::default();
            }
        };
        match serde_json::from_str::<Usage>(&text) {
            Ok(usage) if usage.version == VERSION => usage.sanitized(now_seconds()),
            Ok(usage) => {
                tracing::warn!("{} has version {}; usage starts empty", path.display(), usage.version);
                Usage::default()
            }
            Err(err) => {
                tracing::warn!("{} is not valid usage: {err}; usage starts empty", path.display());
                Usage::default()
            }
        }
    }

    /// A loaded file is untrusted: drop unusable scores, clamp timestamps to `now` (a
    /// future one would never decay) and enforce the caps by sorting, not by `prune`.
    fn sanitized(mut self, now: u64) -> Usage {
        self.items.retain(|_, used| used.score.is_finite() && used.score > 0.0);
        for used in self.items.values_mut() {
            used.at = used.at.min(now);
        }
        for learned in self.queries.values_mut() {
            learned.at = learned.at.min(now);
        }
        if self.items.len() > MAX_ITEMS {
            let mut ranked: Vec<(String, f64)> =
                self.items.keys().map(|key| (key.clone(), self.weight(key, now))).collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for (key, _) in ranked.split_off(MAX_ITEMS) {
                self.items.remove(&key);
            }
        }
        if self.queries.len() > MAX_QUERIES {
            let mut ranked: Vec<(String, u64)> =
                self.queries.iter().map(|(query, learned)| (query.clone(), learned.at)).collect();
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for (query, _) in ranked.split_off(MAX_QUERIES) {
                self.queries.remove(&query);
            }
        }
        self
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(self).map_err(io::Error::other)?;
        athanor_layout::atomic::write_atomically(path, &text)
    }

    pub fn weight(&self, key: &str, now: u64) -> f64 {
        self.items.get(key).map_or(0.0, |used| {
            let age = now.saturating_sub(used.at) as f64;
            used.score * 0.5_f64.powf(age / HALF_LIFE_SECONDS)
        })
    }

    pub fn learned(&self, query: &str) -> Option<&str> {
        let query = normalize(query);
        if query.is_empty() {
            return None;
        }
        self.queries.get(&query).map(|learned| learned.key.as_str())
    }

    /// What usage adds to an item's match for `query`, and whether the user completed this
    /// query with this item before.
    pub fn bonus(&self, query: &str, key: &str, now: u64) -> (i64, bool) {
        let learned = self.learned(query) == Some(key);
        (boost(self.weight(key, now)) + if learned { LEARNED_BONUS } else { 0 }, learned)
    }

    pub fn record(&mut self, query: &str, key: &str, now: u64) {
        let score = self.weight(key, now) + 1.0;
        self.items.insert(key.to_owned(), Use { score, at: now });
        let query = normalize(query);
        if !query.is_empty() {
            self.queries.insert(query, Learned { key: key.to_owned(), at: now });
        }
        self.prune(now);
    }

    fn prune(&mut self, now: u64) {
        while self.items.len() > MAX_ITEMS {
            let lightest = self
                .items
                .keys()
                .min_by(|a, b| self.weight(a, now).total_cmp(&self.weight(b, now)))
                .cloned();
            match lightest {
                Some(key) => self.items.remove(&key),
                None => break,
            };
        }
        while self.queries.len() > MAX_QUERIES {
            let oldest = self.queries.iter().min_by_key(|(_, learned)| learned.at).map(|(q, _)| q.clone());
            match oldest {
                Some(query) => self.queries.remove(&query),
                None => break,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn the_weight_halves_every_seven_days() {
        let mut usage = Usage::default();
        usage.record("fire", "app:firefox.desktop", 0);
        assert!((usage.weight("app:firefox.desktop", 0) - 1.0).abs() < 1e-9);
        assert!((usage.weight("app:firefox.desktop", 7 * DAY) - 0.5).abs() < 1e-9);
        assert!((usage.weight("app:firefox.desktop", 14 * DAY) - 0.25).abs() < 1e-9);
        assert_eq!(usage.weight("app:unknown.desktop", 0), 0.0);
    }

    #[test]
    fn a_second_use_adds_to_the_decayed_weight() {
        let mut usage = Usage::default();
        usage.record("", "app:firefox.desktop", 0);
        usage.record("", "app:firefox.desktop", 7 * DAY);
        assert!((usage.weight("app:firefox.desktop", 7 * DAY) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn a_completed_query_is_learned_case_and_space_insensitively() {
        let mut usage = Usage::default();
        usage.record(" Rel ", "file:///home/u/Relazione_Q3.odt", 0);
        assert_eq!(usage.learned("rel"), Some("file:///home/u/Relazione_Q3.odt"));
        assert_eq!(usage.learned("re"), None);
        usage.record("", "app:firefox.desktop", 1);
        assert!(usage.learned("").is_none());
    }

    #[test]
    fn the_boost_stays_below_one_tier() {
        assert_eq!(boost(0.0), 0);
        assert_eq!(boost(1.0), 20_000);
        assert_eq!(boost(1_000.0), 200_000);
        assert!(boost(f64::MAX) < crate::rank::TIER_STEP);
    }

    #[test]
    fn a_learned_query_lifts_its_item_over_any_match() {
        let mut usage = Usage::default();
        usage.record("rel", "file:///home/u/Relazione_Q3.odt", 0);
        let (bonus, learned) = usage.bonus("rel", "file:///home/u/Relazione_Q3.odt", 0);
        assert!(learned);
        assert!(bonus > crate::rank::Tier::Prefix as i64 * crate::rank::TIER_STEP + 1_000_000);
        assert_eq!(usage.bonus("rel", "app:firefox.desktop", 0), (0, false));
    }

    #[test]
    fn the_store_is_bounded() {
        let mut usage = Usage::default();
        for i in 0..(MAX_ITEMS as u64 + 50) {
            usage.record(&format!("q{i}"), &format!("app:{i}.desktop"), i);
        }
        assert_eq!(usage.items.len(), MAX_ITEMS);
        assert_eq!(usage.queries.len(), MAX_QUERIES);
        // The oldest, lightest entries went first.
        assert_eq!(usage.weight("app:0.desktop", MAX_ITEMS as u64 + 50), 0.0);
        assert!(usage.learned("q0").is_none());
        assert!(usage.learned(&format!("q{}", MAX_ITEMS + 49)).is_some());
    }

    #[test]
    fn save_and_load_round_trip_and_a_bad_file_starts_empty() {
        let dir = std::env::temp_dir().join(format!("athanor-search-usage-{}", std::process::id()));
        let path = dir.join("search/usage.json");
        let mut usage = Usage::default();
        usage.record("fire", "app:firefox.desktop", 10);
        usage.save(&path).expect("save");
        assert_eq!(Usage::load(&path), usage);
        std::fs::write(&path, "{not json").expect("write");
        assert_eq!(Usage::load(&path), Usage::default());
        std::fs::write(&path, r#"{"version":2,"items":{},"queries":{}}"#).expect("write");
        assert_eq!(Usage::load(&path), Usage::default());
        assert_eq!(Usage::load(&dir.join("absent.json")), Usage::default());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    fn stored(dir: &str, text: &str) -> (std::path::PathBuf, Usage) {
        let dir = std::env::temp_dir().join(format!("athanor-search-{dir}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("usage.json");
        std::fs::write(&path, text).expect("write");
        let usage = Usage::load(&path);
        std::fs::remove_dir_all(&dir).expect("cleanup");
        (path, usage)
    }

    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }

    #[test]
    fn an_oversize_file_loads_empty() {
        let padding = " ".repeat(MAX_FILE_BYTES as usize + 1);
        let (_, usage) = stored("big", &format!(r#"{{"version":1,"items":{{"k":{{"score":1.0,"at":1}}}},"queries":{{}}}}{padding}"#));
        assert_eq!(usage, Usage::default());
    }

    #[test]
    fn a_file_over_the_caps_loads_capped_keeping_the_heaviest() {
        let at = now();
        let items: Vec<String> =
            (0..MAX_ITEMS * 3).map(|i| format!(r#""k{i}":{{"score":{},"at":{at}}}"#, i + 1)).collect();
        let queries: Vec<String> =
            (0..MAX_QUERIES * 3).map(|i| format!(r#""q{i}":{{"key":"k","at":{}}}"#, at - 1000 + i as u64 % 1000)).collect();
        let text = format!(r#"{{"version":1,"items":{{{}}},"queries":{{{}}}}}"#, items.join(","), queries.join(","));
        let (_, usage) = stored("caps", &text);
        assert_eq!(usage.items.len(), MAX_ITEMS);
        assert_eq!(usage.queries.len(), MAX_QUERIES);
        assert!(usage.items.contains_key(&format!("k{}", MAX_ITEMS * 3 - 1)));
        assert!(!usage.items.contains_key("k0"));
    }

    #[test]
    fn a_hostile_score_cannot_leave_the_boost_range() {
        let at = now();
        let text = format!(
            r#"{{"version":1,"items":{{"neg":{{"score":-1e300,"at":{at}}},"huge":{{"score":1e308,"at":{at}}}}},"queries":{{}}}}"#
        );
        let (_, usage) = stored("score", &text);
        for key in ["neg", "huge"] {
            let bonus = usage.bonus("", key, at).0;
            assert!((0..=MAX_BOOST).contains(&bonus), "{key}: {bonus}");
        }
        assert!((0..=MAX_BOOST).contains(&boost(-5.0)));
        assert!((0..=MAX_BOOST).contains(&boost(f64::INFINITY)));
    }

    #[test]
    fn a_future_timestamp_does_not_make_an_entry_immune_to_pruning() {
        let at = now();
        let mut items: Vec<String> =
            (0..MAX_ITEMS).map(|i| format!(r#""k{i}":{{"score":1.0,"at":{at}}}"#)).collect();
        items.push(format!(r#""future":{{"score":0.5,"at":{}}}"#, u64::MAX));
        let text = format!(r#"{{"version":1,"items":{{{}}},"queries":{{}}}}"#, items.join(","));
        let (_, usage) = stored("future", &text);
        assert_eq!(usage.items.len(), MAX_ITEMS);
        assert!(!usage.items.contains_key("future"));
    }
}
