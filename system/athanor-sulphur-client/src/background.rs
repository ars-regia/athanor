//! The user's cosmic-bg choice, read from `com.system76.CosmicBackground` v1 for
//! `athanor-wallpaper`'s one-time migration (doc_session_daemons.md, SD16). Only the user's
//! copy is read: the system's `all` is the image's default, not a choice. The key
//! `wallpapers` is cosmic-bg's own record of what it shows, rewritten at run time, and is
//! not read.
//!
//! The entries `all` and `output.<name>` are RON structs of which only `source` matters here;
//! `same-on-all` is a RON bool.

use crate::cosmic_config;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

const COMPONENT: &str = "com.system76.CosmicBackground";
/// The largest key that is read; a real one holds a few lines.
const MAX_BYTES: u64 = 1 << 20;

/// A cosmic-bg source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Path(PathBuf),
    /// A colour or gradient: no image.
    Color,
}

/// The keys that were present and valid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Background {
    /// The entry `all`.
    pub all: Option<Source>,
    /// The entries `output.<name>`, by connector name.
    pub outputs: BTreeMap<String, Source>,
    /// The key `same-on-all`.
    pub same_on_all: Option<bool>,
}

/// The user's choice, or `None` when they have no copy of the store.
pub fn read_user() -> Option<Background> {
    read_in(&cosmic_config::component(
        &cosmic_config::user_dir()?,
        COMPONENT,
    ))
}

fn read_in(dir: &Path) -> Option<Background> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut found = Background::default();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name == "all" {
            found.all = source_of(&name, &entry.path());
        } else if let Some(output) = name.strip_prefix("output.") {
            if let Some(source) = source_of(&name, &entry.path()) {
                found.outputs.insert(output.to_owned(), source);
            }
        } else if name == "same-on-all" {
            found.same_on_all = read(&name, &entry.path()).and_then(|t| match t.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => {
                    tracing::warn!("{COMPONENT} {name} is not a bool; skipped");
                    None
                }
            });
        }
    }
    Some(found)
}

/// A key's text; an unreadable or oversized key is skipped with one warning.
fn read(name: &str, path: &Path) -> Option<String> {
    let mut text = String::new();
    let result = File::open(path).and_then(|f| f.take(MAX_BYTES + 1).read_to_string(&mut text));
    match result {
        Ok(_) if text.len() as u64 <= MAX_BYTES => Some(text),
        Ok(_) => {
            tracing::warn!("{COMPONENT} {name} is too large; skipped");
            None
        }
        Err(error) => {
            tracing::warn!("{COMPONENT} {name} cannot be read ({error}); skipped");
            None
        }
    }
}

fn source_of(name: &str, path: &Path) -> Option<Source> {
    let text = read(name, path)?;
    let source = parse_source(&text);
    if source.is_none() {
        tracing::warn!("{COMPONENT} {name} has no source this client understands; skipped");
    }
    source
}

/// The value of the field `source` of an entry: `Path("...")` or `Color(..)`.
fn parse_source(text: &str) -> Option<Source> {
    let rest = text[text.find("source:")? + "source:".len()..].trim_start();
    if rest.starts_with("Color(") {
        return Some(Source::Color);
    }
    let mut chars = rest
        .strip_prefix("Path(")?
        .trim_start()
        .strip_prefix('"')?
        .chars();
    let mut path = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(Source::Path(PathBuf::from(path))),
            '\\' => path.push(match chars.next()? {
                'n' => '\n',
                't' => '\t',
                c @ ('"' | '\\' | '\'') => c,
                _ => return None,
            }),
            c => path.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// /usr/share/cosmic/com.system76.CosmicBackground/v1/all, byte for byte.
    const SHIPPED: &str = "(\n    output: \"all\",\n    source: Path(\"/usr/share/backgrounds/default-dark.jxl\"),\n    filter_by_theme: #true,\n    rotation_frequency: 3600,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)\n";
    const COLOR: &str = "(\n    output: \"DP-1\",\n    source: Color(Single((0.1, 0.2, 0.3))),\n    filter_by_theme: false,\n    rotation_frequency: 3600,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)\n";

    #[test]
    fn path_and_color_sources() {
        assert_eq!(
            parse_source(SHIPPED),
            Some(Source::Path(
                "/usr/share/backgrounds/default-dark.jxl".into()
            ))
        );
        assert_eq!(parse_source(COLOR), Some(Source::Color));
        assert_eq!(
            parse_source("(source: Path(\"/a \\\"b\\\".png\"))"),
            Some(Source::Path("/a \"b\".png".into()))
        );
    }

    #[test]
    fn broken_sources_are_none() {
        for text in [
            "",
            "(output: \"all\")",
            "(source: Path(\"/unterminated",
            "(source: None)",
            "(source: Path(1))",
        ] {
            assert_eq!(parse_source(text), None, "{text}");
        }
    }

    #[test]
    fn reads_the_user_store_and_ignores_wallpapers() {
        let dir = std::env::temp_dir().join(format!("athanor-bg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("all"), SHIPPED).unwrap();
        std::fs::write(dir.join("output.DP-1"), COLOR).unwrap();
        std::fs::write(dir.join("output.HDMI-A-1"), "garbage").unwrap();
        std::fs::write(dir.join("same-on-all"), "false\n").unwrap();
        std::fs::write(dir.join("wallpapers"), "[(\"DP-1\", Path(\"/x.png\"))]").unwrap();
        let found = read_in(&dir).unwrap();
        assert_eq!(
            found.all,
            Some(Source::Path(
                "/usr/share/backgrounds/default-dark.jxl".into()
            ))
        );
        assert_eq!(found.outputs, [("DP-1".to_owned(), Source::Color)].into());
        assert_eq!(found.same_on_all, Some(false));
        assert_eq!(read_in(&dir.join("missing")), None);
    }
}
