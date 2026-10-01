use std::path::PathBuf;

use athanor_search::item::{Action, Group};
use athanor_search::rank::Ranker;
use athanor_search::usage::Usage;
use athanor_search::{apps, command, web, windows};
use gio::prelude::*;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/applications")
}

fn catalog() -> apps::Catalog {
    let infos = std::fs::read_dir(fixtures())
        .expect("fixtures")
        .filter_map(|entry| gio_unix::DesktopAppInfo::from_filename(entry.ok()?.path()))
        .map(|info| info.upcast::<gio::AppInfo>());
    apps::Catalog::from_infos(infos)
}

fn titles(hits: &[athanor_search::item::Hit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.title.as_str()).collect()
}

#[test]
fn applications_and_settings_pages_are_split_and_hidden_entries_dropped() {
    let catalog = catalog();
    let apps: Vec<_> = catalog.apps.iter().map(|e| e.id.as_str()).collect();
    let settings: Vec<_> = catalog.settings.iter().map(|e| e.id.as_str()).collect();
    assert!(apps.contains(&"org.mozilla.firefox.desktop"));
    assert!(!apps.contains(&"hidden.desktop") && !settings.contains(&"hidden.desktop"));
    assert_eq!(settings, ["com.system76.CosmicSettings.Wireless.desktop"]);
}

#[test]
fn an_application_is_found_by_its_generic_name_and_keywords() {
    let catalog = catalog();
    let usage = Usage::default();
    for query in ["browser", "www", "firefox"] {
        let hits = apps::search(&catalog.apps, Group::Apps, query, &mut Ranker::new(query), &usage, 0);
        assert_eq!(titles(&hits), ["Firefox"], "{query}");
        assert_eq!(hits[0].action, Action::Launch { desktop_id: "org.mozilla.firefox.desktop".into() });
        assert_eq!(hits[0].key, "app:org.mozilla.firefox.desktop");
    }
}

#[test]
fn a_settings_page_is_found_by_a_keyword() {
    let catalog = catalog();
    let hits = apps::search(&catalog.settings, Group::Settings, "network", &mut Ranker::new("network"), &Usage::default(), 0);
    assert_eq!(titles(&hits), ["Wi-Fi"]);
    assert_eq!(hits[0].group, Group::Settings);
}

#[test]
fn a_name_with_a_bidirectional_override_is_shown_without_it() {
    let catalog = catalog();
    let evil = catalog.apps.iter().find(|e| e.id == "bidi.desktop").expect("fixture");
    assert!(!evil.name.contains('\u{202e}'), "{:?}", evil.name);
}

#[test]
fn usage_reorders_within_a_tier_and_a_learned_query_wins() {
    let catalog = catalog();
    let mut usage = Usage::default();
    usage.record("w", "app:org.mozilla.firefox.desktop", 0);
    let hits = apps::search(&catalog.apps, Group::Apps, "w", &mut Ranker::new("w"), &usage, 0);
    assert!(hits.iter().any(|h| h.learned && h.title == "Firefox"));
}

#[test]
fn windows_are_found_by_title_and_application() {
    let open = [
        windows::WindowEntry { index: 0, title: "Relazione_Q3.odt — LibreOffice".into(), app_id: "libreoffice-writer".into(), app_name: Some("LibreOffice Writer".into()) },
        windows::WindowEntry { index: 1, title: "~ : bash".into(), app_id: "com.system76.CosmicTerm".into(), app_name: Some("COSMIC Terminal".into()) },
    ];
    let hits = windows::search(&open, "relaz", &mut Ranker::new("relaz"), &Usage::default(), 0);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].action, Action::Window { index: 0 });
    let hits = windows::search(&open, "terminal", &mut Ranker::new("terminal"), &Usage::default(), 0);
    assert_eq!(hits[0].action, Action::Window { index: 1 });
}

#[test]
fn the_command_prefix_parses_a_command_line() {
    assert_eq!(command::parse("> htop -d 5"), Some(vec!["htop".into(), "-d".into(), "5".into()]));
    assert_eq!(command::parse(">'my tool' \"a b\""), Some(vec!["my tool".into(), "a b".into()]));
    assert_eq!(command::parse(">"), None);
    assert_eq!(command::parse(">   "), None);
    assert_eq!(command::parse("> 'unterminated"), None);
    assert_eq!(command::parse("htop"), None);
    assert_eq!(command::hit("> htop").map(|h| h.group), Some(Group::Command));
}

#[test]
fn the_web_entry_percent_encodes_the_whole_query() {
    let hit = web::hit("a b&c=d/é?#").expect("hit");
    assert_eq!(hit.group, Group::Web);
    assert_eq!(
        hit.action,
        Action::Web { url: format!("{}a%20b%26c%3Dd%2F%C3%A9%3F%23", web::ENGINE) }
    );
    assert!(web::hit("   ").is_none());
}

#[test]
fn an_entry_read_by_path_keeps_its_file_name_as_id() {
    let path = fixtures().join("org.mozilla.firefox.desktop");
    let info = gio_unix::DesktopAppInfo::from_filename(path).expect("fixture");
    assert_eq!(info.id().as_deref(), Some("org.mozilla.firefox.desktop"));
}

#[test]
fn an_entry_whose_name_is_only_hidden_characters_is_skipped() {
    let catalog = catalog();
    assert!(catalog.apps.iter().all(|e| e.id != "blank.desktop"));
}

#[test]
fn a_command_that_would_show_differently_from_what_runs_is_refused() {
    assert!(command::hit("> echo a\u{200b}b").is_none());
    assert!(command::hit("> echo \u{202e}gpj").is_none());
    assert!(command::hit(&format!("> {}", "a".repeat(athanor_unit::text::TITLE_CHARS + 1))).is_none());
    let shown = command::hit("> htop -d 5").expect("hit");
    assert_eq!(shown.title, "htop -d 5");
}

#[test]
fn long_names_and_titles_are_bounded() {
    let open = [windows::WindowEntry { index: 0, title: "t".repeat(10_000), app_id: "a".repeat(10_000), app_name: None }];
    let hits = windows::search(&open, "t", &mut Ranker::new("t"), &Usage::default(), 0);
    assert!(hits[0].title.chars().count() <= athanor_unit::text::TITLE_CHARS);
    assert!(hits[0].subtitle.chars().count() <= athanor_unit::text::NAME_CHARS);
    assert!(hits[0].key.chars().count() <= "window:".len() + athanor_unit::text::NAME_CHARS);
}

#[test]
fn the_title_keeps_every_character_the_command_runs() {
    let hit = command::hit(">>ls").expect("hit");
    assert_eq!(hit.title, ">ls");
    assert_eq!(hit.action, Action::Command { argv: vec![">ls".into()] });
}
