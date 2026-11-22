//! doc_settings.md SE7: Settings writes GSettings only through the handles of
//! athanor-settings-core's table of writable keys. Step 1 writes no key; the first page that
//! does adds one module, `src/gsettings.rs`, which takes nothing but a handle. Any other source
//! that brings in gio's `Settings` or calls one of its setters fails here.

use std::fs;
use std::path::Path;

const SETTERS: &[&str] = &[
    "gio::Settings",
    ".set_boolean(",
    ".set_int(",
    ".set_uint(",
    ".set_double(",
    ".set_string(",
    ".set_strv(",
    ".set_enum(",
    ".set_flags(",
    "SettingsExt",
    ".set_int64(",
    ".set_uint64(",
    ".set_value(",
];

/// Whether `word` stands in `text` as a whole identifier, as a regex's `\bword\b`.
fn has_token(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text.get(..at).and_then(|t| t.chars().next_back());
        let after = text.get(at + word.len()..).and_then(|t| t.chars().next());
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

/// What in `text` writes GSettings: a setter, or a `use` that names `gio` and brings in
/// `Settings` as a token (`use gio::{Settings}`, `use gio::Settings as Store`) or all of gio.
/// Settings' own window type is also named `Settings`, so the token counts only in a `use` of
/// gio; a path through gio is `gio::Settings`, which SETTERS names.
fn offences(text: &str) -> Vec<String> {
    let mut found: Vec<String> = SETTERS
        .iter()
        .filter(|setter| text.contains(*setter))
        .map(|setter| (*setter).to_owned())
        .collect();
    let uses = text.match_indices("use ").filter(|(at, _)| {
        let before = text.get(..*at).and_then(|t| t.chars().next_back());
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
    });
    for (at, _) in uses {
        let statement = text.get(at..).and_then(|t| t.split(';').next()).unwrap_or_default();
        let renamed = statement.split_whitespace().collect::<Vec<_>>().join(" ").contains("self as");
        if has_token(statement, "gio")
            && (has_token(statement, "Settings") || statement.contains("gio::*") || renamed)
        {
            found.push(statement.trim().to_owned());
        }
    }
    // gio under another name (`use gio as g`, `extern crate gio as g`) hides `g::Settings`
    // from every other test here, so the rename is an offence in itself.
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect();
    for pair in words.windows(2) {
        if let [first, second] = pair {
            if *first == "gio" && *second == "as" {
                found.push("gio as".to_owned());
            }
        }
    }
    found
}

#[test]
fn the_scan_finds_settings_however_it_is_imported() {
    for text in [
        "use gio::{Settings};",
        "use gio::{prelude::*, Settings as Store};",
        "use gio::Settings as G;",
        "use gio::*;",
        "let s = gio::Settings::new(\"org.gnome.desktop.interface\");",
        "s.set_string(\"font-name\", \"Inter\");",
        "use gio as g;\nlet s = g::Settings::new(\"org.gnome.desktop.interface\");",
        "use gio as g;",
        "extern crate gio as g;",
        "use {gio as g};",
        "use gio::{self as g};",
        "use gio::{prelude::*, self  as\n g};",
        "s.set_value(\"font-name\", &\"Inter\".to_variant());",
    ] {
        assert!(!offences(text).is_empty(), "missed: {text}");
    }
    for text in [
        "pub struct Settings {",
        "use athanor_settings::window::{self, Settings};",
        "let settings = Settings::new(registry);",
        "use gio::glib;",
        "/// Settings' unit\nfn trust(bus: &gio::DBusConnection) {",
    ] {
        assert_eq!(offences(text), Vec::<String>::new(), "flagged: {text}");
    }
}

fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("the source directory").flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_source_but_the_gsettings_module_writes_a_key() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    assert!(files.len() > 3, "the scan found the sources");
    let mut offenders = Vec::new();
    for file in files {
        if file.file_name().is_some_and(|name| name == "gsettings.rs") {
            continue;
        }
        let text = fs::read_to_string(&file).expect("a source file");
        for offence in offences(&text) {
            offenders.push(format!("{}: {offence}", file.display()));
        }
    }
    assert!(offenders.is_empty(), "GSettings written outside src/gsettings.rs: {offenders:?}");
}
