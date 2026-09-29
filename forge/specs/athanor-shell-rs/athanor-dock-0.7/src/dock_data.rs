use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct WindowLayout {
    pub tile_pos_in_workspace_view: Option<(f64, f64)>,
    pub window_size: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NiriWindowInfo {
    pub id: u64,
    pub title: Option<String>,
    pub app_id: Option<String>,
    #[serde(default)]
    pub is_focused: bool,
    pub workspace_id: Option<u64>,
    pub layout: Option<WindowLayout>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct NiriWorkspaceInfo {
    pub id: u64,
    #[serde(default)]
    pub idx: u64,
    pub name: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockItem {
    pub key_id: String,
    pub display_name: String,
    pub icon_name: String,
    pub is_pinned: bool,
    pub window_ids: Vec<u64>,
    pub window_titles: Vec<String>,
    pub is_focused: bool,
}

#[allow(dead_code)]
fn matches_desktop_or_app_id(desktop_id: &str, app_id: &str) -> bool {

    let clean_desktop = desktop_id.trim_end_matches(".desktop").to_lowercase();
    let clean_app = app_id.to_lowercase();
    clean_desktop == clean_app
        || clean_desktop.ends_with(&clean_app)
        || clean_app.ends_with(&clean_desktop)
}

fn derive_display_name_and_icon(key_id: &str) -> (String, String) {
    let clean = key_id.trim_end_matches(".desktop");
    if let Some(pos) = clean.rfind('.') {
        let name = capitalize(&clean[pos + 1..]);
        (name, clean.to_string())
    } else {
        (capitalize(clean), clean.to_string())
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub fn reconcile_dock_items(pinned: &[String], windows: &[NiriWindowInfo]) -> Vec<DockItem> {
    let mut items = Vec::new();
    let mut matched_window_ids = HashSet::new();
    
    // O(P+W): Pre-index windows by lowercased app_id
    use std::collections::HashMap;
    let mut window_map: HashMap<String, Vec<&NiriWindowInfo>> = HashMap::new();
    for win in windows {
        let app_id = win.app_id.clone().unwrap_or_else(|| "unknown".to_string()).to_lowercase();
        window_map.entry(app_id).or_default().push(win);
    }

    // 1. Process pinned items first
    for pin in pinned {
        let (display_name, icon_name) = derive_display_name_and_icon(pin);
        let mut window_ids = Vec::new();
        let mut window_titles = Vec::new();
        let mut is_focused = false;

        let clean_pin = pin.trim_end_matches(".desktop").to_lowercase();

        for (app_id, wins) in &window_map {
            if clean_pin == *app_id || clean_pin.ends_with(app_id) || app_id.ends_with(&clean_pin) {
                for win in wins {
                    window_ids.push(win.id);
                    window_titles.push(win.title.clone().unwrap_or_else(|| display_name.clone()));
                    if win.is_focused {
                        is_focused = true;
                    }
                    matched_window_ids.insert(win.id);
                }
            }
        }

        items.push(DockItem {
            key_id: pin.clone(),
            display_name,
            icon_name,
            is_pinned: true,
            window_ids,
            window_titles,
            is_focused,
        });
    }

    // 2. Process unpinned running windows
    for win in windows {
        if matched_window_ids.contains(&win.id) {
            continue;
        }
        let app_id = win.app_id.clone().unwrap_or_else(|| "unknown".to_string());
        let clean_app = app_id.to_lowercase();
        
        if let Some(existing) = items.iter_mut().find(|it| {
            if it.is_pinned { return false; }
            let clean_it = it.key_id.trim_end_matches(".desktop").to_lowercase();
            clean_it == clean_app || clean_it.ends_with(&clean_app) || clean_app.ends_with(&clean_it)
        }) {
            existing.window_ids.push(win.id);
            existing.window_titles.push(win.title.clone().unwrap_or_else(|| existing.display_name.clone()));
            if win.is_focused {
                existing.is_focused = true;
            }
            matched_window_ids.insert(win.id);
        } else {
            let (display_name, icon_name) = derive_display_name_and_icon(&app_id);
            items.push(DockItem {
                key_id: app_id.clone(),
                display_name: win.title.clone().unwrap_or(display_name),
                icon_name,
                is_pinned: false,
                window_ids: vec![win.id],
                window_titles: vec![win.title.clone().unwrap_or_else(|| app_id.clone())],
                is_focused: win.is_focused,
            });
            matched_window_ids.insert(win.id);
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconcile_dock_items_merging() {
        let pinned = vec!["firefox.desktop".to_string(), "org.gnome.Terminal.desktop".to_string()];
        let windows = vec![
            NiriWindowInfo {
                id: 101,
                title: Some("Mozilla Firefox".to_string()),
                app_id: Some("firefox".to_string()),
                is_focused: true,
                workspace_id: Some(1),
                layout: None,
            },
            NiriWindowInfo {
                id: 102,
                title: Some("Terminal".to_string()),
                app_id: Some("org.gnome.Terminal".to_string()),
                is_focused: false,
                workspace_id: Some(1),
                layout: None,
            },
            NiriWindowInfo {
                id: 103,
                title: Some("Files".to_string()),
                app_id: Some("nautilus".to_string()),
                is_focused: false,
                workspace_id: Some(1),
                layout: None,
            },
        ];

        let items = reconcile_dock_items(&pinned, &windows);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].key_id, "firefox.desktop");
        assert!(items[0].is_pinned);
        assert_eq!(items[0].window_ids, vec![101]);
        assert!(items[0].is_focused);

        assert_eq!(items[1].key_id, "org.gnome.Terminal.desktop");
        assert!(items[1].is_pinned);
        assert_eq!(items[1].window_ids, vec![102]);
        assert!(!items[1].is_focused);

        assert_eq!(items[2].key_id, "nautilus");
        assert!(!items[2].is_pinned);
        assert_eq!(items[2].window_ids, vec![103]);
    }
}
