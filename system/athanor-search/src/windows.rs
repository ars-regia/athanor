//! Open windows (LA2), from the snapshot of the compositor client the query runs against.

use athanor_unit::text;

use crate::item::{Action, Group, Hit};
use crate::rank::Ranker;
use crate::usage::Usage;

/// A window as the launcher hands it over: `index` is its place in the snapshot, which
/// `Action::Window` points back to. `title` is the raw title; it is cleaned here.
#[derive(Clone, Debug)]
pub struct WindowEntry {
    pub index: usize,
    pub title: String,
    pub app_id: String,
    pub app_name: Option<String>,
    /// The icon of the application's desktop entry, when one resolves.
    pub icon: Option<gio::Icon>,
}

pub fn search(windows: &[WindowEntry], query: &str, ranker: &mut Ranker, usage: &Usage, now: u64) -> Vec<Hit> {
    windows
        .iter()
        .filter_map(|window| {
            let title = text::line(&window.title, text::TITLE_CHARS);
            let app = text::line(window.app_name.as_deref().unwrap_or(&window.app_id), text::NAME_CHARS);
            let app_id = text::line(&window.app_id, text::NAME_CHARS);
            let (tier, score) = ranker.score(&title, &[&app, &app_id])?;
            let key = format!("window:{app_id}");
            let (bonus, learned) = usage.bonus(query, &key, now);
            Some(Hit {
                group: Group::Windows,
                title,
                subtitle: app,
                icon: window.icon.clone(),
                tier,
                score: score.saturating_add(bonus),
                learned,
                action: Action::Window { index: window.index },
                key,
            })
        })
        .collect()
}
