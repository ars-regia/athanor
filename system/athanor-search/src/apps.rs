//! Applications and settings pages (LA2), from the desktop entries GIO knows: system
//! entries and both Flatpak export trees. The caller reads them again when
//! `gio::AppInfoMonitor` says they changed.

use athanor_unit::text;
use gio::prelude::*;

use crate::item::{Action, Group, Hit};
use crate::rank::Ranker;
use crate::usage::Usage;

/// The per-page entries cosmic-settings ships, until our Settings ships its own.
const SETTINGS_PREFIX: &str = "com.system76.CosmicSettings.";

/// Keywords read per entry: a desktop file is untrusted input.
const MAX_KEYWORDS: usize = 64;

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// Generic name, keywords, executable and the last part of the desktop id: matched,
    /// never above a substring.
    pub secondary: Vec<String>,
    pub icon: Option<gio::Icon>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub apps: Vec<Entry>,
    pub settings: Vec<Entry>,
}

impl Catalog {
    pub fn read() -> Catalog {
        Catalog::from_infos(gio::AppInfo::all())
    }

    pub fn from_infos(infos: impl IntoIterator<Item = gio::AppInfo>) -> Catalog {
        let mut catalog = Catalog::default();
        for info in infos {
            let Some(id) = info.id().map(|id| id.to_string()) else { continue };
            if info.should_show() {
                catalog.apps.push(entry(&info, id));
            } else if id.starts_with(SETTINGS_PREFIX) {
                catalog.settings.push(entry(&info, id));
            }
        }
        catalog
    }
}

fn entry(info: &gio::AppInfo, id: String) -> Entry {
    let mut secondary = Vec::new();
    if let Some(desktop) = info.downcast_ref::<gio_unix::DesktopAppInfo>() {
        secondary.extend(desktop.generic_name().map(|name| name.to_string()));
        secondary.extend(desktop.keywords().iter().take(MAX_KEYWORDS).map(|keyword| keyword.to_string()));
    }
    secondary.extend(
        info.executable().file_name().map(|name| name.to_string_lossy().into_owned()),
    );
    secondary.extend(
        id.strip_suffix(".desktop")
            .and_then(|stem| stem.rsplit('.').next())
            .map(str::to_owned),
    );
    Entry {
        name: text::line(&info.display_name(), text::NAME_CHARS),
        secondary: secondary.iter().map(|field| text::line(field, text::NAME_CHARS)).collect(),
        icon: info.icon(),
        id,
    }
}

pub fn search(
    entries: &[Entry],
    group: Group,
    query: &str,
    ranker: &mut Ranker,
    usage: &Usage,
    now: u64,
) -> Vec<Hit> {
    entries
        .iter()
        .filter_map(|entry| {
            let secondary: Vec<&str> = entry.secondary.iter().map(String::as_str).collect();
            let (tier, score) = ranker.score(&entry.name, &secondary)?;
            let key = format!("app:{}", entry.id);
            let (bonus, learned) = usage.bonus(query, &key, now);
            Some(Hit {
                group,
                title: entry.name.clone(),
                subtitle: String::new(),
                icon: entry.icon.clone(),
                tier,
                score: score.saturating_add(bonus),
                learned,
                action: Action::Launch { desktop_id: entry.id.clone() },
                key,
            })
        })
        .collect()
}
