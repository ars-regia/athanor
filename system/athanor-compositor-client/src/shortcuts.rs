//! The user's copy of COSMIC's `system_actions` (doc_launcher.md, LA8). cosmic-comp lays
//! the user's entries over the system's one by one, so the copy holds only what was
//! changed. The file is a flat RON map of action names to command lines; comments in it
//! are not kept when it is written again.
//!
//! The read-modify-write takes no lock: a write by cosmic-settings between the read and the
//! rename is lost, as cosmic-config's own writers would lose it. A copy that is a symbolic
//! link is replaced by a regular file; the permissions of an existing copy are kept.

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};

use crate::cosmic_config;

const COMPONENT: &str = "com.system76.CosmicSettings.Shortcuts";
const KEY: &str = "system_actions";
/// The largest copy that is read; a real one holds a handful of lines.
const MAX_BYTES: u64 = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum ShortcutError {
    #[error("{path} is not a map of actions to commands: {reason}")]
    Format { path: PathBuf, reason: String },
    #[error("neither XDG_CONFIG_HOME nor HOME is set")]
    NoConfig,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The directory of the user's copy, which a sandboxed caller must be allowed to write.
pub fn dir() -> Option<PathBuf> {
    cosmic_config::user_dir().map(|dir| cosmic_config::component(&dir, COMPONENT))
}

/// Binds `action` to `command` in the user's copy; `Ok(false)` when it already was.
pub fn set_system_action(action: &str, command: &str) -> Result<bool, ShortcutError> {
    let dir = dir().ok_or(ShortcutError::NoConfig)?;
    set_in(&dir.join(KEY), action, command)
}

fn set_in(path: &Path, action: &str, command: &str) -> Result<bool, ShortcutError> {
    // The action is written as a bare name; anything else would corrupt the map.
    let mut scanner = Scanner { rest: action };
    if !matches!(scanner.ident(), Ok(name) if name == action) {
        return Err(ShortcutError::Format {
            path: path.to_owned(),
            reason: format!("{action:?} is not an action name"),
        });
    }
    let mut text = String::new();
    match File::open(path) {
        Ok(file) => {
            file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
            if text.len() as u64 > MAX_BYTES {
                return Err(ShortcutError::Format {
                    path: path.to_owned(),
                    reason: "it is too large".to_owned(),
                });
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let mut entries = if text.trim().is_empty() {
        Vec::new()
    } else {
        parse(&text).map_err(|reason| ShortcutError::Format { path: path.to_owned(), reason })?
    };
    match entries.iter_mut().find(|(name, _)| name == action) {
        Some((_, current)) if current == command => return Ok(false),
        Some((_, current)) => command.clone_into(current),
        None => entries.push((action.to_owned(), command.to_owned())),
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let permissions = fs::metadata(path).map(|meta| meta.permissions()).ok();
    athanor_layout::atomic::write_atomically(path, &render(&entries))?;
    if let Some(permissions) = permissions {
        fs::set_permissions(path, permissions)?;
    }
    Ok(true)
}

fn render(entries: &[(String, String)]) -> String {
    let mut out = String::from("{\n");
    for (name, command) in entries {
        let escaped = command.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        out.push_str(&format!("    {name}: \"{escaped}\",\n"));
    }
    out.push_str("}\n");
    out
}

struct Scanner<'a> {
    rest: &'a str,
}

impl Scanner<'_> {
    /// Skips white space and `//` and `/* */` comments.
    fn skip(&mut self) -> Result<(), String> {
        loop {
            self.rest = self.rest.trim_start();
            if let Some(after) = self.rest.strip_prefix("//") {
                self.rest = after.split_once('\n').map_or("", |(_, rest)| rest);
            } else if let Some(after) = self.rest.strip_prefix("/*") {
                let (_, rest) = after.split_once("*/").ok_or("an unclosed comment")?;
                self.rest = rest;
            } else {
                return Ok(());
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        match self.rest.strip_prefix(c) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.eat(c) { Ok(()) } else { Err(format!("expected '{c}'")) }
    }

    fn ident(&mut self) -> Result<String, String> {
        let end = self
            .rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(self.rest.len());
        let ident = &self.rest[..end];
        if ident.is_empty() || ident.starts_with(|c: char| c.is_ascii_digit()) {
            return Err("expected an action name".to_owned());
        }
        self.rest = &self.rest[end..];
        Ok(ident.to_owned())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        let mut chars = self.rest.char_indices();
        while let Some((at, c)) = chars.next() {
            match c {
                '"' => {
                    self.rest = &self.rest[at + 1..];
                    return Ok(out);
                }
                '\\' => match chars.next().map(|(_, c)| c) {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('0') => out.push('\0'),
                    Some('\'') => out.push('\''),
                    Some('u') => {
                        // `\u{hex}`, as `char::escape_debug` writes it.
                        let mut hex = String::new();
                        if chars.next().map(|(_, c)| c) != Some('{') {
                            return Err("a malformed \\u escape".to_owned());
                        }
                        loop {
                            match chars.next().map(|(_, c)| c) {
                                Some('}') => break,
                                Some(c) if c.is_ascii_hexdigit() && hex.len() < 6 => hex.push(c),
                                _ => return Err("a malformed \\u escape".to_owned()),
                            }
                        }
                        let c = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or("a \\u escape that is not a character")?;
                        out.push(c);
                    }
                    _ => return Err("an escape it does not know".to_owned()),
                },
                c => out.push(c),
            }
        }
        Err("an unclosed string".to_owned())
    }
}

fn parse(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut s = Scanner { rest: text };
    s.skip()?;
    s.expect('{')?;
    let mut entries = Vec::new();
    loop {
        s.skip()?;
        if s.eat('}') {
            break;
        }
        let name = s.ident()?;
        s.skip()?;
        s.expect(':')?;
        s.skip()?;
        entries.push((name, s.string()?));
        s.skip()?;
        if !s.eat(',') {
            s.skip()?;
            s.expect('}')?;
            break;
        }
    }
    s.skip()?;
    if s.rest.is_empty() { Ok(entries) } else { Err("text after the map".to_owned()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path of a `system_actions` in a directory of its own, removed on drop.
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for Scratch {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    fn scratch(name: &str) -> Scratch {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("athanor-shortcuts-{name}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir.join("system_actions"))
    }

    #[test]
    fn every_escape_cosmic_writes_is_read() {
        let path = scratch("escapes");
        std::fs::write(
            &path,
            "{\n    Terminal: \"sh -c \\'x\\'\",\n    Other: \"a\\r\\0\\u{e9}\\u{1F600}\",\n}\n",
        )
        .unwrap();
        let entries = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(entries[0].1, "sh -c 'x'");
        assert_eq!(entries[1].1, "a\r\0\u{e9}\u{1F600}");
        assert!(parse("{ A: \"\\u{d800}\" }").is_err());
        assert!(parse("{ A: \"\\u{110000}\" }").is_err());
        assert!(parse("{ A: \"\\u41\" }").is_err());
    }

    #[test]
    fn an_absent_copy_gets_the_one_entry() {
        let path = scratch("absent");
        assert!(set_in(&path, "Launcher", "gdbus call x").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n    Launcher: \"gdbus call x\",\n}\n");
    }

    #[test]
    fn the_user_entries_survive_and_a_second_call_writes_nothing() {
        let path = scratch("merge");
        std::fs::write(
            &path,
            "{\n    // mine\n    Terminal: \"foot\",\n    /* old */ Launcher: \"cosmic-launcher\"\n}",
        )
        .unwrap();
        assert!(set_in(&path, "Launcher", "ours").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\n    Terminal: \"foot\",\n    Launcher: \"ours\",\n}\n");
        assert!(!set_in(&path, "Launcher", "ours").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_file_it_cannot_read_is_left_alone() {
        let path = scratch("foreign");
        let foreign = "{ Launcher: Some(\"x\") }";
        std::fs::write(&path, foreign).unwrap();
        assert!(matches!(set_in(&path, "Launcher", "ours"), Err(ShortcutError::Format { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
    }

    #[test]
    fn quotes_and_backslashes_round_trip() {
        let path = scratch("quotes");
        let command = r#"sh -c "echo \"a\\b\"""#;
        set_in(&path, "Custom", command).unwrap();
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()).unwrap(), [("Custom".to_owned(), command.to_owned())]);
    }

    #[test]
    fn an_action_that_is_not_a_name_is_refused() {
        let path = scratch("name");
        assert!(matches!(set_in(&path, "A: \"x\", B", "c"), Err(ShortcutError::Format { .. })));
        assert!(!path.exists());
    }
}
