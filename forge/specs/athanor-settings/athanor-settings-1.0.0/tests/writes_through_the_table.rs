//! doc_settings.md SE7: Settings writes GSettings only through the handles of
//! athanor-settings-core's table of writable keys. Step 1 writes no key; the first page that
//! does adds one module, `src/gsettings.rs`, which takes nothing but a handle. Any other source
//! that names `gio::Settings` or one of its setters fails here.

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
];

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
        for setter in SETTERS {
            if text.contains(setter) {
                offenders.push(format!("{}: {setter}", file.display()));
            }
        }
    }
    assert!(offenders.is_empty(), "GSettings written outside src/gsettings.rs: {offenders:?}");
}
