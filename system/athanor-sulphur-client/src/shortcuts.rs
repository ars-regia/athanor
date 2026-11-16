//! The user's copy of COSMIC's `system_actions` (doc_launcher.md, LA8). cosmic-comp lays
//! the user's entries over the system's one by one, so the copy holds only what was
//! changed. The file is a flat RON map of action names to command lines; comments in it
//! are not kept when it is written again.
//!
//! A value the user chose is never overwritten: only an absent entry, the system file's value
//! (the stock cosmic-launcher) or the command itself counts as ours to set. The caller binds
//! once per user (`set_system_action_once`): after the first try the entry is the user's,
//! so removing it or choosing another value in COSMIC Settings gives the action back.
//!
//! The read-modify-write takes no lock: a write by cosmic-settings between the read and the
//! rename is lost, as cosmic-config's own writers would lose it. A copy that is a symbolic
//! link is replaced by a regular file; the permissions of an existing copy are kept.

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::cosmic_config;

/// The prefix of the per-page desktop entries cosmic-settings ships.
pub const SETTINGS_PAGE_PREFIX: &str = "com.system76.CosmicSettings.";
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
    #[error("the command for {action} holds a control character other than a line feed")]
    Command { action: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The directory of the user's copy, which a sandboxed caller must be allowed to write.
pub fn dir() -> Option<PathBuf> {
    cosmic_config::user_dir().map(|dir| cosmic_config::component(&dir, COMPONENT))
}

/// What `set_system_action` found in the user's entry for the action, and did.
#[derive(Debug, PartialEq, Eq)]
pub enum Binding {
    /// The user's copy had no entry for the action: the command was added.
    Added,
    /// The entry held the system file's value: the command replaced it.
    ReplacedDefault,
    /// The entry already held the command: nothing was written.
    Unchanged,
    /// The user chose another command: it was left alone.
    UserChoice,
}

/// Binds `action` to `command` in the user's copy, unless the user chose another command
/// for it (see `Binding`).
pub fn set_system_action(action: &str, command: &str) -> Result<Binding, ShortcutError> {
    let dir = dir().ok_or(ShortcutError::NoConfig)?;
    set_in(&dir.join(KEY), action, command, system_value(action).as_deref())
}

/// Binds as `set_system_action` does, once per user: when `marker` exists nothing is read or
/// written and the result is `None`. Otherwise the marker is created after the binding,
/// whatever it found; a binding that fails leaves no marker, so the next start tries again.
pub fn set_system_action_once(action: &str, command: &str, marker: &Path) -> Result<Option<Binding>, ShortcutError> {
    once(marker, || set_system_action(action, command))
}

fn once(marker: &Path, bind: impl FnOnce() -> Result<Binding, ShortcutError>) -> Result<Option<Binding>, ShortcutError> {
    if marker.try_exists()? {
        return Ok(None);
    }
    let binding = bind()?;
    let created = marker.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| File::create(marker));
    if let Err(err) = created {
        // The binding stands; without the marker the next start checks the entry again.
        tracing::warn!(marker = %marker.display(), "the marker of the one-time binding was not written: {err}");
    }
    Ok(Some(binding))
}

/// The user's copy of COSMIC's `custom` key: key bindings to actions, in the shape of the
/// shipped `defaults` (RON map of `(modifiers: [..], key: ".."): Action`).
const CUSTOM_KEY: &str = "custom";

/// Binds `modifiers` + `key` to `Spawn(command)` in the user's `custom` copy, once per user:
/// as `set_system_action_once`, `marker` ends the attempts. A combination that already has
/// an action is the user's and is left alone (`Binding::UserChoice`, or `Unchanged` when it
/// already spawns `command`).
pub fn set_custom_binding_once(
    modifiers: &[&str],
    key: &str,
    command: &str,
    marker: &Path,
) -> Result<Option<Binding>, ShortcutError> {
    once(marker, || {
        let dir = dir().ok_or(ShortcutError::NoConfig)?;
        bind_custom(&dir.join(CUSTOM_KEY), modifiers, key, command)
    })
}

fn bind_custom(path: &Path, modifiers: &[&str], key: &str, command: &str) -> Result<Binding, ShortcutError> {
    let format = |reason: String| ShortcutError::Format { path: path.to_owned(), reason };
    let name = |word: &str| !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !name(key) || !modifiers.iter().all(|modifier| name(modifier)) {
        return Err(format(format!("{modifiers:?} + {key:?} is not a key combination")));
    }
    if command.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ShortcutError::Command { action: format!("{key} (custom)") });
    }
    let text = read_copy(path)?;
    let action = format!("Spawn(\"{}\")", escape(command));
    let wanted: std::collections::BTreeSet<&str> = modifiers.iter().copied().collect();
    let head = if text.trim().is_empty() {
        "{".to_owned()
    } else {
        let bare = strip_comments(&text);
        for (found, found_key, found_action) in bindings(&bare).map_err(format)? {
            if found_key.as_deref() == Some(key) && found.iter().map(String::as_str).collect::<std::collections::BTreeSet<_>>() == wanted {
                return Ok(if found_action == action { Binding::Unchanged } else { Binding::UserChoice });
            }
        }
        let end = text.trim_end().strip_suffix('}').ok_or_else(|| format("it does not end with '}'".to_owned()))?.trim_end();
        // The comma goes right after `end`, so a line comment still open there would swallow it.
        let in_line_comment = !strip_comments(&format!("{end}X")).ends_with('X');
        if in_line_comment && !strip_comments(end).trim_end().ends_with(['{', ',']) {
            return Err(format("a line comment ends the map; the binding cannot be added after it".to_owned()));
        }
        end.to_owned()
    };
    let comma = if strip_comments(&head).trim_end().ends_with(['{', ',']) { "" } else { "," };
    let list = modifiers.join(", ");
    let entry = format!("(modifiers: [{list}], key: \"{key}\"): {action}");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    athanor_layout::atomic::write_atomically(path, &format!("{head}{comma}\n    {entry},\n}}\n"))?;
    Ok(Binding::Added)
}

/// `text` with every `//` and `/* */` comment blanked out, strings left as they are.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_string = !in_string;
                out.push(c);
            }
            '\\' if in_string => {
                out.push(c);
                out.extend(chars.next());
            }
            '/' if !in_string && chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|c| *c != '\n') {
                    chars.next();
                }
            }
            '/' if !in_string && chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
                out.push(' ');
            }
            c => out.push(c),
        }
    }
    out
}

/// The index of the first `stop` in `s` outside strings and brackets (`s.len()` when none).
fn top_level(s: &str, stop: char) -> usize {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (at, c) in s.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if c == stop && depth == 0 {
            return at;
        } else {
            match c {
                '"' => in_string = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    s.len()
}

/// The modifiers, the key and the action's text of one entry.
type Entry = (Vec<String>, Option<String>, String);

/// The entries of a `custom` map, comments already stripped. Only the shape is read; the
/// action is never interpreted.
fn bindings(bare: &str) -> Result<Vec<Entry>, String> {
    let mut body = bare
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or("it is not a map")?
        .trim();
    let mut found = Vec::new();
    while !body.is_empty() {
        let tuple = body.strip_prefix('(').ok_or("an entry does not start with '('")?;
        let close = top_level(tuple, ')');
        let fields = tuple.get(..close).ok_or("an entry is cut short")?;
        let rest = tuple.get(close + 1..).ok_or("an entry is cut short")?.trim_start();
        let rest = rest.strip_prefix(':').ok_or("an entry has no ':' after its key combination")?;
        let end = top_level(rest, ',');
        let action = rest.get(..end).ok_or("an entry is cut short")?.trim().to_owned();
        let (mut modifiers, mut key) = (Vec::new(), None);
        let mut fields = fields;
        while !fields.trim().is_empty() {
            let end = top_level(fields, ',');
            let (label, value) = fields.get(..end).unwrap_or_default().split_once(':').ok_or("a field has no ':'")?;
            match label.trim() {
                "modifiers" => {
                    let list = value.trim().strip_prefix('[').and_then(|v| v.strip_suffix(']')).ok_or("modifiers is not a list")?;
                    modifiers = list.split(',').map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned).collect();
                }
                "key" => key = Some(value.trim().trim_matches('"').to_owned()),
                // Real fields of COSMIC's Binding; neither names the key this writer compares.
                "keycode" | "description" => {}
                other => return Err(format!("a field named {other:?}")),
            }
            fields = fields.get(end + 1..).unwrap_or_default();
        }
        found.push((modifiers, key, action));
        body = rest.get(end + 1..).unwrap_or_default().trim_start();
    }
    Ok(found)
}

/// The action's value in the system file, which cosmic-comp lays beneath the user's copy.
/// A system file that is absent or does not parse gives none: every user value then
/// counts as the user's choice.
fn system_value(action: &str) -> Option<String> {
    let text = cosmic_config::key(&cosmic_config::system_dirs(), COMPONENT, KEY)?;
    match parse(&text) {
        Ok(entries) => entries.into_iter().find(|(name, _)| name == action).map(|(_, value)| value),
        Err(reason) => {
            tracing::warn!("the system's {KEY} of {COMPONENT} is not a map of actions to commands: {reason}");
            None
        }
    }
}

/// The user's copy as text; an absent copy is empty.
fn read_copy(path: &Path) -> Result<String, ShortcutError> {
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
    Ok(text)
}

/// `default` is the system file's value for the action.
fn set_in(path: &Path, action: &str, command: &str, default: Option<&str>) -> Result<Binding, ShortcutError> {
    // The action is written as a bare name; anything else would corrupt the map.
    let mut scanner = Scanner { rest: action };
    if !matches!(scanner.ident(), Ok(name) if name == action) {
        return Err(ShortcutError::Format {
            path: path.to_owned(),
            reason: format!("{action:?} is not an action name"),
        });
    }
    // Only `\n` has an escape every reader of the file is known to accept.
    if command.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ShortcutError::Command { action: action.to_owned() });
    }
    let text = read_copy(path)?;
    let mut entries = if text.trim().is_empty() {
        Vec::new()
    } else {
        parse(&text).map_err(|reason| ShortcutError::Format { path: path.to_owned(), reason })?
    };
    let binding = match entries.iter_mut().find(|(name, _)| name == action) {
        Some((_, current)) if current == command => return Ok(Binding::Unchanged),
        Some((_, current)) if Some(current.as_str()) == default => {
            command.clone_into(current);
            Binding::ReplacedDefault
        }
        Some(_) => return Ok(Binding::UserChoice),
        None => {
            entries.push((action.to_owned(), command.to_owned()));
            Binding::Added
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let permissions = fs::metadata(path).map(|meta| meta.permissions()).ok();
    athanor_layout::atomic::write_atomically(path, &render(&entries))?;
    if let Some(permissions) = permissions {
        // The write succeeded: a mode that cannot be restored is not a failed binding.
        let mode = permissions.mode() & 0o666;
        if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
            tracing::warn!(path = %path.display(), "the file's mode was not restored: {err}");
        }
    }
    Ok(binding)
}

fn render(entries: &[(String, String)]) -> String {
    let mut out = String::from("{\n");
    for (name, command) in entries {
        out.push_str(&format!("    {name}: \"{}\",\n", escape(command)));
    }
    out.push_str("}\n");
    out
}

/// A command as a RON string body: every character the parser unescapes is escaped, so a
/// value read from the file, control characters included, is written back unchanged.
fn escape(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    for c in command.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
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

    fn require(&mut self, c: char) -> Result<(), String> {
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
        self.require('"')?;
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

#[doc(hidden)] // public for the fuzz target and its corpus replay only
pub fn parse(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut s = Scanner { rest: text };
    s.skip()?;
    s.require('{')?;
    let mut entries = Vec::new();
    loop {
        s.skip()?;
        if s.eat('}') {
            break;
        }
        let name = s.ident()?;
        s.skip()?;
        s.require(':')?;
        s.skip()?;
        entries.push((name, s.string()?));
        s.skip()?;
        if !s.eat(',') {
            s.skip()?;
            s.require('}')?;
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

    const STOCK: Option<&str> = Some("cosmic-launcher");

    #[test]
    fn an_absent_copy_gets_the_one_entry() {
        let path = scratch("absent");
        assert_eq!(set_in(&path, "Launcher", "gdbus call x", STOCK).unwrap(), Binding::Added);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n    Launcher: \"gdbus call x\",\n}\n");
    }

    #[test]
    fn an_absent_entry_is_added_beside_the_others() {
        let path = scratch("added");
        std::fs::write(&path, "{\n    Terminal: \"foot\",\n}\n").unwrap();
        assert_eq!(set_in(&path, "Launcher", "ours", STOCK).unwrap(), Binding::Added);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n    Terminal: \"foot\",\n    Launcher: \"ours\",\n}\n");
    }

    #[test]
    fn the_stock_value_is_replaced_and_a_second_call_writes_nothing() {
        let path = scratch("merge");
        std::fs::write(
            &path,
            "{\n    // mine\n    Terminal: \"foot\",\n    /* old */ Launcher: \"cosmic-launcher\"\n}",
        )
        .unwrap();
        assert_eq!(set_in(&path, "Launcher", "ours", STOCK).unwrap(), Binding::ReplacedDefault);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\n    Terminal: \"foot\",\n    Launcher: \"ours\",\n}\n");
        assert_eq!(set_in(&path, "Launcher", "ours", STOCK).unwrap(), Binding::Unchanged);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_value_the_user_chose_is_left_alone() {
        let path = scratch("chosen");
        let mine = "{\n    // mine\n    Launcher: \"my-launcher --fast\",\n}";
        std::fs::write(&path, mine).unwrap();
        assert_eq!(set_in(&path, "Launcher", "ours", STOCK).unwrap(), Binding::UserChoice);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine, "not even rewritten");
        // With no readable system file, nothing the user wrote counts as the default.
        let stock = "{ Launcher: \"cosmic-launcher\" }";
        std::fs::write(&path, stock).unwrap();
        assert_eq!(set_in(&path, "Launcher", "ours", None).unwrap(), Binding::UserChoice);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), stock);
    }

    #[test]
    fn every_escape_survives_a_rewrite() {
        let entries = vec![
            ("Launcher".to_owned(), "ours".to_owned()),
            ("Other".to_owned(), "a\\b \"q\" 'x'\n\t\r\0\u{1b}[0m\u{7f}\u{85}\u{9f} \u{e9}\u{1F600}".to_owned()),
        ];
        let text = render(&entries);
        assert!(!text.contains(['\t', '\r', '\0', '\u{1b}', '\u{7f}', '\u{85}']), "no raw control character: {text:?}");
        assert_eq!(parse(&text).unwrap(), entries);
    }

    #[test]
    fn the_binding_is_tried_once_per_user() {
        let path = scratch("once");
        let marker = path.with_file_name("state/super-bound");
        let bind = || set_in(&path, "Launcher", "ours", STOCK);
        assert_eq!(once(&marker, bind).unwrap(), Some(Binding::Added));
        assert!(marker.exists());
        // The user removes the entry: a second start leaves it removed.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(once(&marker, bind).unwrap(), None);
        assert!(!path.exists(), "nothing was written");
        // A value the user chose also sets the marker, and a failed binding does not.
        let other = path.with_file_name("other-marker");
        std::fs::write(&path, "{ Launcher: \"mine\" }").unwrap();
        assert_eq!(once(&other, bind).unwrap(), Some(Binding::UserChoice));
        assert!(other.exists());
        let failed = path.with_file_name("failed-marker");
        std::fs::write(&path, "not a map").unwrap();
        assert!(once(&failed, bind).is_err());
        assert!(!failed.exists(), "the next start tries again");
    }

    #[test]
    fn a_command_with_a_control_character_is_refused() {
        let path = scratch("control");
        for command in ["a\rb", "a\tb", "a\0b", "a\u{1b}[0m", "a\u{85}b"] {
            assert!(matches!(set_in(&path, "Launcher", command, STOCK), Err(ShortcutError::Command { .. })), "{command:?}");
        }
        assert!(!path.exists(), "nothing was written");
        assert_eq!(set_in(&path, "Launcher", "a\nb", STOCK).unwrap(), Binding::Added, "a line feed has its escape");
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()).unwrap(), [("Launcher".to_owned(), "a\nb".to_owned())]);
    }

    #[test]
    fn a_file_it_cannot_read_is_left_alone() {
        let path = scratch("foreign");
        let foreign = "{ Launcher: Some(\"x\") }";
        std::fs::write(&path, foreign).unwrap();
        assert!(matches!(set_in(&path, "Launcher", "ours", STOCK), Err(ShortcutError::Format { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
    }

    #[test]
    fn quotes_and_backslashes_round_trip() {
        let path = scratch("quotes");
        let command = r#"sh -c "echo \"a\\b\"""#;
        set_in(&path, "Custom", command, None).unwrap();
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()).unwrap(), [("Custom".to_owned(), command.to_owned())]);
    }

    #[test]
    fn an_action_that_is_not_a_name_is_refused() {
        let path = scratch("name");
        assert!(matches!(set_in(&path, "A: \"x\", B", "c", None), Err(ShortcutError::Format { .. })));
        assert!(!path.exists());
    }

    /// Verbatim lines of `/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults`
    /// as shipped in the rig image (Fedora cosmic-settings-daemon), the file COSMIC's `custom`
    /// key has the same type as. `Spawn` is the variant cosmic-comp carries for a command; the
    /// rig ships no file that uses it, so its line is the one written here.
    const REAL_SAMPLE: &str = "{\n    (modifiers: [Super, Alt], key: \"Escape\"): Terminate,\n    (modifiers: [Super], key: \"t\"): System(Terminal),\n    (modifiers: [Super]): System(Launcher),\n    (modifiers: [Alt], key: \"Tab\"): System(WindowSwitcher),\n}\n";
    const CC: &str = "busctl --user call os.athanor.ControlCenter1 /os/athanor/ControlCenter1 os.athanor.ControlCenter1 Toggle";

    #[test]
    fn a_custom_binding_on_an_empty_copy_writes_the_one_entry() {
        let path = scratch("custom-empty");
        assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Added);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, format!("{{\n    (modifiers: [Super], key: \"c\"): Spawn(\"{CC}\"),\n}}\n"));
        assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Unchanged);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_custom_binding_is_added_beside_real_ones_and_keeps_them_byte_for_byte() {
        let path = scratch("custom-other");
        std::fs::write(&path, REAL_SAMPLE).unwrap();
        assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Added);
        let text = std::fs::read_to_string(&path).unwrap();
        let kept = REAL_SAMPLE.trim_end().trim_end_matches('}');
        assert_eq!(text, format!("{kept}    (modifiers: [Super], key: \"c\"): Spawn(\"{CC}\"),\n}}\n"));
        // The same file without a trailing comma on its last entry.
        std::fs::write(&path, "{ (modifiers: [Super], key: \"t\"): System(Terminal) }").unwrap();
        assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Added);
        assert!(text_parses_with_both(&path));
    }

    fn text_parses_with_both(path: &Path) -> bool {
        let text = std::fs::read_to_string(path).unwrap();
        text.contains("System(Terminal),") && text.contains("key: \"c\"")
    }

    #[test]
    fn a_combination_that_already_has_an_action_is_the_users() {
        let path = scratch("custom-taken");
        // Modifier order and spacing do not matter; a comment mentioning it does not count.
        let mine = "{\n    // (modifiers: [Super], key: \"c\"): Nothing,\n    (modifiers:[ Alt ,Super ], key:\"c\"): Disable,\n    (modifiers: [Super], key: \"c\"): Spawn(\"mine\")\n}";
        std::fs::write(&path, mine).unwrap();
        assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::UserChoice);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine, "not rewritten");
        assert_eq!(bind_custom(&path, &["Super"], "c", "mine").unwrap(), Binding::Unchanged);
        // Alt+Super+c is another combination than Super+c.
        std::fs::write(&path, "{ (modifiers: [Alt, Super], key: \"c\"): Disable }").unwrap();
        assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Added);
    }

    #[test]
    fn a_binding_with_a_description_or_a_key_code_is_kept_and_super_c_is_added() {
        for (name, other) in [
            ("desc", "{\n    (modifiers: [Super], key: \"t\", description: Some(\"Term, inal: (x)\")): Spawn(\"foot\"),\n}\n"),
            ("code", "{\n    (modifiers: [Super], keycode: Some(54), description: None): Spawn(\"foot\"),\n}\n"),
        ] {
            let path = scratch(&format!("custom-{name}"));
            std::fs::write(&path, other).unwrap();
            assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Added);
            let text = std::fs::read_to_string(&path).unwrap();
            let kept = other.trim_end().trim_end_matches('}');
            assert_eq!(text, format!("{kept}    (modifiers: [Super], key: \"c\"): Spawn(\"{CC}\"),\n}}\n"));
            assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Unchanged);
            // A described binding of Super+C itself is still the user's.
            std::fs::write(&path, "{ (modifiers: [Super], key: \"c\", description: Some(\"Mine\")): Spawn(\"x\") }").unwrap();
            assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::UserChoice);
        }
    }

    #[test]
    fn once_marks_a_file_with_a_described_binding() {
        let dir = scratch("custom-once-desc");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("custom");
        std::fs::write(&file, "{ (modifiers: [Super], key: \"t\", description: Some(\"Term\")): Spawn(\"foot\") }").unwrap();
        let marker = dir.join("marker");
        let done = once(&marker, || bind_custom(&file, &["Super"], "c", CC)).unwrap();
        assert_eq!(done, Some(Binding::Added));
        assert!(marker.exists());
    }

    #[test]
    fn a_custom_binding_refuses_what_it_cannot_write_safely() {
        let path = scratch("custom-refuse");
        assert!(matches!(bind_custom(&path, &["Super"], "c", "a\tb"), Err(ShortcutError::Command { .. })));
        assert!(matches!(bind_custom(&path, &["Su\"per"], "c", CC), Err(ShortcutError::Format { .. })));
        assert!(matches!(bind_custom(&path, &["Super"], "c\"", CC), Err(ShortcutError::Format { .. })));
        std::fs::write(&path, "not a map").unwrap();
        assert!(matches!(bind_custom(&path, &["Super"], "c", CC), Err(ShortcutError::Format { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a map");
    }

    #[test]
    fn the_custom_binding_is_tried_once_per_user() {
        let path = scratch("custom-once");
        let marker = path.with_file_name("state/super-c-bound");
        assert_eq!(set_in_custom(&path, &marker), Some(Binding::Added));
        assert!(marker.exists());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(set_in_custom(&path, &marker), None);
        assert!(!path.exists());
    }

    #[test]
    fn a_comment_inside_the_map_does_not_stop_the_binding() {
        let path = scratch("custom-comment");
        for (name, mine) in [
            ("inside", "{\n    // my terminal\n    (modifiers: [Super], key: \"t\"): Spawn(\"foot\")\n}\n"),
            ("block", "{\n    /* mine */ (modifiers: [Super], key: \"t\"): Spawn(\"foot\")\n}\n"),
            ("after a comma", "{\n    (modifiers: [Super], key: \"t\"): Spawn(\"foot\"), // mine\n}\n"),
        ] {
            std::fs::write(&path, mine).unwrap();
            assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Added, "{name}");
            assert_eq!(bind_custom(&path, &["Super"], "c", CC).unwrap(), Binding::Unchanged, "{name}");
        }
        // A line comment right where the comma must go cannot take it: refused, file untouched.
        let last = "{\n    (modifiers: [Super], key: \"t\"): Spawn(\"foot\") // mine\n}\n";
        std::fs::write(&path, last).unwrap();
        assert!(matches!(bind_custom(&path, &["Super"], "c", CC), Err(ShortcutError::Format { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), last);
    }

    fn set_in_custom(path: &Path, marker: &Path) -> Option<Binding> {
        once(marker, || bind_custom(path, &["Super"], "c", CC)).unwrap()
    }
}
