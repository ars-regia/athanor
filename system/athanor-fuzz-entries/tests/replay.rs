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

// One line per fuzz target: the feature that builds its entry point, the target name (also
// the corpus directory and the `[[bin]]` of fuzz/Cargo.toml) and the entry point. The macro
// generates the replay test and the list `every_target_is_replayed` compares to the disk.
macro_rules! replay_tests {
    ($($feature:literal $name:ident => $entry:path;)*) => {
        const TARGETS: &[&str] = &[$(stringify!($name)),*];
        $(
            #[test]
            #[cfg(feature = $feature)]
            fn $name() {
                replay(stringify!($name), $entry);
            }
        )*
    };
}

replay_tests! {
    "update" update_claims => athanor_fuzz_entries::update::claims;
    "update" update_status => athanor_fuzz_entries::update::status;
    "update" update_local => athanor_fuzz_entries::update::local;
    "gtk" bar_tray => athanor_fuzz_entries::bar::tray;
    "gtk" bar_dbusmenu => athanor_fuzz_entries::bar::dbusmenu;
    "gtk" bar_notice => athanor_fuzz_entries::bar::notice;
    "gtk" bar_png => athanor_fuzz_entries::bar::png;
    "shelld" shelld_hints => athanor_fuzz_entries::shelld::hints;
    "shelld" shelld_image => athanor_fuzz_entries::shelld::image;
    "shelld" shelld_icon => athanor_fuzz_entries::shelld::icon;
    "gtk" compositor_shortcuts => athanor_fuzz_entries::compositor::shortcuts;
    "gtk" compositor_theme => athanor_fuzz_entries::compositor::theme;
    "layout" layout_document => athanor_fuzz_entries::layout::document;
    "layout" layout_favorites => athanor_fuzz_entries::layout::favorites;
    "layout" trust_state => athanor_fuzz_entries::layout::trust_state;
}

/// A corpus directory or a `[[bin]]` without a line above is a target nobody replays.
#[test]
fn every_target_is_replayed() {
    let fuzz = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz");
    let mut on_disk: Vec<String> = std::fs::read_dir(fuzz.join("corpus"))
        .expect("fuzz/corpus")
        .map(|entry| entry.expect("corpus entry"))
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let manifest = std::fs::read_to_string(fuzz.join("Cargo.toml")).expect("fuzz/Cargo.toml");
    let mut bins = Vec::new();
    let mut in_bin = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_bin = line == "[[bin]]";
        } else if let (true, Some(name)) = (in_bin, line.strip_prefix("name = \"")) {
            bins.push(name.trim_end_matches('"').to_owned());
        }
    }
    on_disk.extend(bins.iter().cloned());
    on_disk.sort();
    on_disk.dedup();
    let mut listed: Vec<String> = TARGETS.iter().map(|name| (*name).to_owned()).collect();
    listed.sort();
    assert_eq!(on_disk, listed, "corpus directories and [[bin]] names against replay_tests!");
    assert_eq!(bins.len(), listed.len(), "a corpus directory has no [[bin]] or the reverse");
}
