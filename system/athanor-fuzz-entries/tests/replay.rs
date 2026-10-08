//! Replays the committed corpus (`fuzz/corpus/<target>`) through the entry point of each fuzz
//! target. A crash a fuzzer finds is committed there as a file and stays a regression test.

use std::path::Path;

fn replay(target: &str, entry: fn(&[u8]) -> athanor_fuzz_entries::Setup) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus").join(target);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("corpus {}: {err}", dir.display()))
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "corpus {target} is empty");
    for file in files {
        eprintln!("replay {}", file.display());
        entry(&std::fs::read(&file).expect("corpus file")).expect("harness setup");
    }
}

#[test]
#[cfg(feature = "update")]
fn update_claims() {
    replay("update_claims", athanor_fuzz_entries::update::claims);
}

#[test]
#[cfg(feature = "update")]
fn update_status() {
    replay("update_status", athanor_fuzz_entries::update::status);
}

#[test]
#[cfg(feature = "update")]
fn update_local() {
    replay("update_local", athanor_fuzz_entries::update::local);
}

#[test]
#[cfg(feature = "gtk")]
fn bar_tray() {
    replay("bar_tray", athanor_fuzz_entries::bar::tray);
}

#[test]
#[cfg(feature = "gtk")]
fn bar_dbusmenu() {
    replay("bar_dbusmenu", athanor_fuzz_entries::bar::dbusmenu);
}

#[test]
#[cfg(feature = "gtk")]
fn bar_notice() {
    replay("bar_notice", athanor_fuzz_entries::bar::notice);
}

#[test]
#[cfg(feature = "gtk")]
fn bar_png() {
    replay("bar_png", athanor_fuzz_entries::bar::png);
}

#[test]
#[cfg(feature = "shelld")]
fn shelld_hints() {
    replay("shelld_hints", athanor_fuzz_entries::shelld::hints);
}

#[test]
#[cfg(feature = "shelld")]
fn shelld_image() {
    replay("shelld_image", athanor_fuzz_entries::shelld::image);
}

#[test]
#[cfg(feature = "shelld")]
fn shelld_icon() {
    replay("shelld_icon", athanor_fuzz_entries::shelld::icon);
}

#[test]
#[cfg(feature = "gtk")]
fn compositor_shortcuts() {
    replay("compositor_shortcuts", athanor_fuzz_entries::compositor::shortcuts);
}

#[test]
#[cfg(feature = "gtk")]
fn compositor_theme() {
    replay("compositor_theme", athanor_fuzz_entries::compositor::theme);
}

#[test]
#[cfg(feature = "layout")]
fn layout_document() {
    replay("layout_document", athanor_fuzz_entries::layout::document);
}

#[test]
#[cfg(feature = "layout")]
fn layout_favorites() {
    replay("layout_favorites", athanor_fuzz_entries::layout::favorites);
}

#[test]
#[cfg(feature = "layout")]
fn trust_state() {
    replay("trust_state", athanor_fuzz_entries::layout::trust_state);
}
