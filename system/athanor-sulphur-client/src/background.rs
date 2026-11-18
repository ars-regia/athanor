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
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const COMPONENT: &str = "com.system76.CosmicBackground";
/// The largest key that is read; a real one holds a few lines.
const MAX_BYTES: u64 = 1 << 20;
/// The longest path that is kept (`PATH_MAX`).
const MAX_PATH: usize = 4096;
/// The most `output.*` entries that are read; a real machine has a handful of outputs.
const MAX_OUTPUTS: usize = 64;

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
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    // A sorted order, so the cap below keeps the same entries at every start.
    names.sort();
    let mut found = Background::default();
    let mut capped = false;
    for name in names {
        let path = dir.join(&name);
        if name == "all" {
            found.all = source_of(&name, &path);
        } else if let Some(output) = name.strip_prefix("output.") {
            if found.outputs.len() == MAX_OUTPUTS {
                if !capped {
                    tracing::warn!("{COMPONENT} has more than {MAX_OUTPUTS} output entries; the rest are skipped");
                    capped = true;
                }
                continue;
            }
            if let Some(source) = source_of(&name, &path) {
                found.outputs.insert(output.to_owned(), source);
            }
        } else if name == "same-on-all" {
            found.same_on_all = read(&name, &path).and_then(|t| match t.trim() {
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

/// A key's text; an unreadable, oversized or irregular key is skipped with one warning. The
/// key is opened `O_NOFOLLOW | O_NONBLOCK` and must be a regular file, so a FIFO or a link
/// another process put there cannot stop the migration.
fn read(name: &str, path: &Path) -> Option<String> {
    let result = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY)
        .open(path)
        .and_then(|file| {
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            let mut text = String::new();
            file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
            Ok(text)
        });
    match result {
        Ok(text) if text.len() as u64 <= MAX_BYTES => Some(text),
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
            '"' => break,
            '\\' => path.push(match chars.next()? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '0' => '\0',
                c @ ('"' | '\\' | '\'') => c,
                'x' => {
                    let hex: String = chars.by_ref().take(2).collect();
                    // Exactly two hex digits: from_str_radix would also take a leading `+`.
                    if hex.len() != 2 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return None;
                    }
                    let byte = u8::from_str_radix(&hex, 16).ok().filter(|b| *b < 0x80)?;
                    char::from(byte)
                }
                'u' => {
                    if chars.next()? != '{' {
                        return None;
                    }
                    let mut hex = String::new();
                    loop {
                        match chars.next()? {
                            '}' => break,
                            c if c.is_ascii_hexdigit() && hex.len() < 6 => hex.push(c),
                            _ => return None,
                        }
                    }
                    char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?
                }
                _ => return None,
            }),
            c => path.push(c),
        }
        if path.len() > MAX_PATH {
            return None;
        }
    }
    Some(Source::Path(PathBuf::from(path)))
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

    #[test]
    fn rust_escapes_are_decoded() {
        assert_eq!(
            parse_source(r#"(source: Path("/a/\u{e9}\r\x41\u{1F600}"))"#),
            Some(Source::Path("/a/\u{e9}\rA\u{1F600}".into()))
        );
        for bad in [
            r#"(source: Path("/a\x+5"))"#,
            r#"(source: Path("/a\x4"))"#,
            r#"(source: Path("/a\u{110000}"))"#,
            r#"(source: Path("/a\u{zz}"))"#,
            r#"(source: Path("/a\q"))"#,
        ] {
            assert_eq!(parse_source(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_path_over_path_max_is_skipped() {
        let at = format!("(source: Path(\"/{}\"))", "a".repeat(MAX_PATH - 1));
        let over = format!("(source: Path(\"/{}\"))", "a".repeat(MAX_PATH));
        assert!(parse_source(&at).is_some());
        assert_eq!(parse_source(&over), None);
    }

    #[test]
    fn fifo_symlink_and_directory_keys_are_skipped_without_blocking() {
        let dir = std::env::temp_dir().join(format!("athanor-bg-irregular-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("real"), SHIPPED).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(dir.join("all"))
            .status()
            .unwrap();
        assert!(made.success());
        std::os::unix::fs::symlink(dir.join("real"), dir.join("output.DP-1")).unwrap();
        std::fs::create_dir_all(dir.join("output.HDMI-A-1")).unwrap();
        let found = read_in(&dir).unwrap();
        assert_eq!(found, Background::default());
    }

    #[test]
    fn output_entries_are_capped_in_name_order() {
        let dir = std::env::temp_dir().join(format!("athanor-bg-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("same-on-all"), "false").unwrap();
        for n in 0..MAX_OUTPUTS + 5 {
            std::fs::write(dir.join(format!("output.O{n:03}")), SHIPPED).unwrap();
        }
        let found = read_in(&dir).unwrap();
        assert_eq!(found.outputs.len(), MAX_OUTPUTS);
        assert!(found.outputs.contains_key("O000"));
        assert!(!found.outputs.contains_key(&format!("O{:03}", MAX_OUTPUTS)));
        // `same-on-all` sorts after the outputs and is still read.
        assert_eq!(found.same_on_all, Some(false));
    }
}
