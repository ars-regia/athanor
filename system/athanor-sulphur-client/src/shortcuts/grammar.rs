//! The grammar of `bindings.toml` (`doc_own_stack.md` decision 38): the form of a binding's
//! keys, the actions it names with their argument, and COSMIC 1.11's `custom` and
//! `system_actions` read into it for the import. The actions are COSMIC's, in snake case
//! (cosmic-settings-daemon `config/src/shortcuts/action.rs` at `epoch-1.11.0`), and the three
//! of the compositor's own defaults that COSMIC has no action for. Only the form is checked
//! here: the compositor resolves a key's name with xkbcommon, which the client does not link.

use super::Scanner;
use toml::{Table, Value};

/// The argument an action takes, under the key of the same name in its table.
#[derive(Clone, Copy, Debug)]
pub(super) enum Argument {
    None,
    /// `workspace`, a number from 1 to 255.
    Workspace,
    /// `command`, a command line the compositor runs with `/bin/sh -c`.
    Command,
    /// A name from the list, under the key given.
    Choice(&'static str, &'static [&'static str]),
}

const DIRECTIONS: &[&str] = &["left", "right", "up", "down"];
const FOCUS: &[&str] = &["left", "right", "up", "down", "in", "out"];
const RESIZE: &[&str] = &["inwards", "outwards"];
const ORIENTATIONS: &[&str] = &["horizontal", "vertical"];

/// COSMIC's system actions, which `[system_actions]` maps to a command line.
pub(super) const SYSTEMS: &[&str] = &[
    "app_library",
    "brightness_down",
    "brightness_up",
    "display_toggle",
    "home_folder",
    "input_source_switch",
    "keyboard_brightness_down",
    "keyboard_brightness_up",
    "launcher",
    "lock_screen",
    "log_out",
    "mute",
    "mute_mic",
    "play_pause",
    "play_next",
    "play_prev",
    "power_off",
    "screen_reader",
    "screenshot",
    "suspend",
    "terminal",
    "touchpad_toggle",
    "volume_lower",
    "volume_raise",
    "web_browser",
    "window_switcher",
    "window_switcher_previous",
    "workspace_overview",
];

/// The system actions whose command no user replaces: the lock and the screen reader, which
/// no inhibitor takes either (decision 30).
pub(super) const KEPT_SYSTEMS: &[&str] = &["lock_screen", "screen_reader"];

/// Every action, with its argument.
pub(super) const ACTIONS: &[(&str, Argument)] = &[
    ("close", Argument::None),
    ("debug", Argument::None),
    ("disable", Argument::None),
    ("focus", Argument::Choice("direction", FOCUS)),
    ("last_workspace", Argument::None),
    ("maximize", Argument::None),
    ("fullscreen", Argument::None),
    ("migrate_workspace_to_next_output", Argument::None),
    (
        "migrate_workspace_to_output",
        Argument::Choice("direction", DIRECTIONS),
    ),
    ("migrate_workspace_to_previous_output", Argument::None),
    ("minimize", Argument::None),
    ("move", Argument::Choice("direction", DIRECTIONS)),
    ("move_to_last_workspace", Argument::None),
    ("move_to_next_output", Argument::None),
    ("move_to_next_workspace", Argument::None),
    ("move_to_output", Argument::Choice("direction", DIRECTIONS)),
    ("move_to_previous_output", Argument::None),
    ("move_to_previous_workspace", Argument::None),
    ("move_to_workspace", Argument::Workspace),
    ("next_output", Argument::None),
    ("next_workspace", Argument::None),
    ("orientation", Argument::Choice("orientation", ORIENTATIONS)),
    ("previous_output", Argument::None),
    ("previous_workspace", Argument::None),
    ("resizing", Argument::Choice("direction", RESIZE)),
    ("send_to_last_workspace", Argument::None),
    ("send_to_next_output", Argument::None),
    ("send_to_next_workspace", Argument::None),
    ("send_to_output", Argument::Choice("direction", DIRECTIONS)),
    ("send_to_previous_output", Argument::None),
    ("send_to_previous_workspace", Argument::None),
    ("send_to_workspace", Argument::Workspace),
    ("swap_window", Argument::None),
    ("switch_output", Argument::Choice("direction", DIRECTIONS)),
    ("system", Argument::Choice("system", SYSTEMS)),
    ("spawn", Argument::Command),
    ("terminate", Argument::None),
    ("toggle_orientation", Argument::None),
    ("toggle_stacking", Argument::None),
    ("toggle_sticky", Argument::None),
    ("toggle_tiling", Argument::None),
    ("toggle_window_floating", Argument::None),
    ("workspace", Argument::Workspace),
    ("zoom_in", Argument::None),
    ("zoom_out", Argument::None),
    // The compositor's own (OS13): the reveal key, the focus cycle, the control center.
    ("reveal", Argument::None),
    ("focus_cycle", Argument::None),
    ("control_center", Argument::None),
];

/// The modifiers, in the order the canonical form writes them.
const MODIFIERS: [&str; 4] = ["Super", "Ctrl", "Alt", "Shift"];

/// `keys` in its canonical form: each modifier named once, in any case, written in the order
/// Super, Ctrl, Alt, Shift, then the key, if any. A key is one character, written in lower
/// case as COSMIC reads it, or a key symbol's name, of letters, digits and underscores.
pub(super) fn keys(keys: &str) -> Result<String, String> {
    let mut held = [false; 4];
    let mut key = None;
    for token in keys.split('+').map(str::trim) {
        if let Some((on, name)) = held
            .iter_mut()
            .zip(MODIFIERS)
            .find(|(_, m)| m.eq_ignore_ascii_case(token))
        {
            if std::mem::replace(on, true) {
                return Err(format!("{keys:?} names {name} twice"));
            }
            continue;
        }
        if key.is_some() {
            return Err(format!("{keys:?} names two keys"));
        }
        let mut chars = token.chars();
        key = Some(match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_control() && !c.is_whitespace() => c.to_lowercase().collect(),
            (Some(_), Some(_)) if token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                token.to_owned()
            }
            _ => return Err(format!("{keys:?}: {token:?} is not a key")),
        });
    }
    if key.is_none() && !held.contains(&true) {
        return Err(format!("{keys:?} names no key"));
    }
    let mut parts: Vec<String> = MODIFIERS
        .iter()
        .zip(held)
        .filter(|(_, on)| *on)
        .map(|(m, _)| (*m).to_owned())
        .collect();
    parts.extend(key);
    Ok(parts.join("+"))
}

/// The key names xkbcommon gives the characters of the protected keys.
const NAMED: &[(&str, &str)] = &[("=", "equal"), ("-", "minus")];

/// The modifiers and the key of canonical `keys`, the key as the compositor resolves it: a
/// name in any case, a character by its name.
fn split(keys: &str) -> (&str, String) {
    let (mods, key) = keys.rsplit_once('+').unwrap_or(("", keys));
    let key = NAMED.iter().find(|(c, _)| *c == key).map_or(key, |(_, name)| name);
    (mods, key.to_ascii_lowercase())
}

/// Whether the canonical `keys` are a protected key of `protected`'s list or a terminal
/// switch, Ctrl+Alt with F1 to F12 and any other modifier. The compositor checks again as it
/// reads, by keysym, and skips a protected key spelled otherwise.
pub(super) fn protected(keys: &str, protected: &[String]) -> bool {
    let (mods, key) = split(keys);
    let held: Vec<&str> = mods.split('+').collect();
    let console = held.contains(&"Ctrl")
        && held.contains(&"Alt")
        && key
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=12).contains(&n));
    console || protected.iter().any(|p| split(p) == (mods, key.clone()))
}

/// A command line: not empty, with no control character but a tab or a line feed.
pub(super) fn command(line: &str) -> Result<(), String> {
    if line.trim().is_empty() {
        return Err("an empty command".to_owned());
    }
    if line
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\n')
    {
        return Err(
            "a command with a control character other than a tab or a line feed".to_owned(),
        );
    }
    Ok(())
}

/// One `[[binding]]` table checked, its keys in canonical form: `keys`, an optional `keycode`,
/// `action` with its argument, an optional `description`, and no other field.
pub(super) fn binding(table: &Table) -> Result<Table, String> {
    let mut out = Table::new();
    let Some(Value::String(written)) = table.get("keys") else {
        return Err("a binding without keys".to_owned());
    };
    out.insert("keys".to_owned(), Value::String(self::keys(written)?));
    let Some(Value::String(name)) = table.get("action") else {
        return Err(format!("{written}: no action"));
    };
    let Some(&(_, argument)) = ACTIONS.iter().find(|(known, _)| known == name) else {
        return Err(format!("{written}: {name:?} is not an action"));
    };
    out.insert("action".to_owned(), Value::String(name.clone()));
    let mut fields = vec!["keys", "action", "keycode", "description"];
    match argument {
        Argument::None => {}
        Argument::Workspace => {
            match table.get("workspace") {
                Some(Value::Integer(n)) if (1..=255).contains(n) => {
                    out.insert("workspace".to_owned(), Value::Integer(*n))
                }
                _ => {
                    return Err(format!(
                        "{written}: workspace is not a number from 1 to 255"
                    ))
                }
            };
            fields.push("workspace");
        }
        Argument::Command => {
            let Some(Value::String(line)) = table.get("command") else {
                return Err(format!("{written}: no command"));
            };
            command(line).map_err(|why| format!("{written}: {why}"))?;
            out.insert("command".to_owned(), Value::String(line.clone()));
            fields.push("command");
        }
        Argument::Choice(key, choices) => {
            match table.get(key) {
                Some(Value::String(choice)) if choices.contains(&choice.as_str()) => {
                    out.insert(key.to_owned(), Value::String(choice.clone()))
                }
                _ => {
                    return Err(format!(
                        "{written}: {key} is not one of {}",
                        choices.join(", ")
                    ))
                }
            };
            fields.push(key);
        }
    }
    match table.get("keycode") {
        None => {}
        Some(Value::Integer(code)) if u32::try_from(*code).is_ok() => {
            out.insert("keycode".to_owned(), Value::Integer(*code));
        }
        Some(_) => return Err(format!("{written}: keycode is not a key code")),
    }
    match table.get("description") {
        None => {}
        Some(Value::String(text)) => {
            out.insert("description".to_owned(), Value::String(text.clone()));
        }
        Some(_) => return Err(format!("{written}: description is not a string")),
    }
    if let Some(other) = table.keys().find(|key| !fields.contains(&key.as_str())) {
        return Err(format!("{written}: {other} is not a field of a binding"));
    }
    Ok(out)
}

/// `CamelCase` as `snake_case`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (at, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && at > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// A system action's COSMIC name in snake case, if it is one.
pub(super) fn system(name: &str) -> Option<&'static str> {
    let name = snake(name);
    SYSTEMS.iter().copied().find(|known| *known == name)
}

/// A value of a COSMIC entry: `Some(v)` as `v`; `None` as nothing.
fn optional<T>(
    s: &mut Scanner<'_>,
    read: impl Fn(&mut Scanner<'_>) -> Result<T, String>,
) -> Result<Option<T>, String> {
    if let Some(rest) = s.rest.strip_prefix("None") {
        s.rest = rest;
        return Ok(None);
    }
    let wrapped = s.rest.strip_prefix("Some");
    if let Some(rest) = wrapped {
        s.rest = rest;
        s.skip()?;
        s.require('(')?;
        s.skip()?;
    }
    let value = read(s)?;
    if wrapped.is_some() {
        s.skip()?;
        s.require(')')?;
    }
    Ok(Some(value))
}

fn number(s: &mut Scanner<'_>) -> Result<i64, String> {
    let end = s
        .rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.rest.len());
    let (digits, rest) = s.rest.split_at_checked(end).unwrap_or((s.rest, ""));
    let n = digits
        .parse()
        .map_err(|_| "expected a number".to_owned())?;
    s.rest = rest;
    Ok(n)
}

/// One COSMIC action, `Close`, `Focus(Left)`, `Workspace(3)`, `System(Terminal)` or
/// `Spawn("foot")`, as the fields of a binding's table.
fn action(s: &mut Scanner<'_>) -> Result<Table, String> {
    let variant = s.ident()?;
    let name = snake(&variant);
    let &(_, argument) = ACTIONS
        .iter()
        .find(|(known, _)| *known == name)
        .ok_or_else(|| format!("{variant} is not an action of COSMIC 1.11"))?;
    let mut table = Table::new();
    table.insert("action".to_owned(), Value::String(name));
    let key = match argument {
        Argument::None => return Ok(table),
        Argument::Workspace => "workspace",
        Argument::Command => "command",
        Argument::Choice(key, _) => key,
    };
    s.skip()?;
    s.require('(')?;
    s.skip()?;
    let value = match argument {
        Argument::Workspace => Value::Integer(number(s)?),
        Argument::Command => Value::String(s.string()?),
        Argument::None | Argument::Choice(..) => Value::String(snake(&s.ident()?)),
    };
    table.insert(key.to_owned(), value);
    s.skip()?;
    s.require(')')?;
    Ok(table)
}

/// The entries of COSMIC's `custom` map, `{ (modifiers: [Super], key: "t"): Spawn("foot"), }`,
/// each as a binding's fields before `binding` checks them; an entry whose action the scan
/// cannot read is its error alone. A map whose shape the scan cannot follow is an error.
pub(super) fn custom(text: &str) -> Result<Vec<Result<Table, String>>, String> {
    let mut s = Scanner { rest: text };
    s.skip()?;
    s.require('{')?;
    let mut entries = Vec::new();
    loop {
        s.skip()?;
        if s.eat('}') {
            return Ok(entries);
        }
        s.require('(')?;
        let mut table = Table::new();
        let mut keys: Vec<&str> = Vec::new();
        loop {
            s.skip()?;
            if s.eat(')') {
                break;
            }
            let field = s.ident()?;
            s.skip()?;
            s.require(':')?;
            s.skip()?;
            match field.as_str() {
                "modifiers" => {
                    s.require('[')?;
                    loop {
                        s.skip()?;
                        if s.eat(']') {
                            break;
                        }
                        let modifier = s.ident()?;
                        let known = MODIFIERS
                            .iter()
                            .find(|m| m.eq_ignore_ascii_case(&modifier))
                            .ok_or_else(|| format!("{modifier} is not a modifier"))?;
                        keys.push(known);
                        s.skip()?;
                        s.eat(',');
                    }
                }
                "key" => {
                    if let Some(key) = optional(&mut s, |s| s.string())? {
                        table.insert("key".to_owned(), Value::String(key));
                    }
                }
                "keycode" => {
                    if let Some(code) = optional(&mut s, number)? {
                        table.insert("keycode".to_owned(), Value::Integer(code));
                    }
                }
                "description" => {
                    if let Some(text) = optional(&mut s, |s| s.string())? {
                        table.insert("description".to_owned(), Value::String(text));
                    }
                }
                other => return Err(format!("a binding's field named {other:?}")),
            }
            s.skip()?;
            s.eat(',');
        }
        let key = table.remove("key");
        let mut written: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
        if let Some(Value::String(key)) = key {
            written.push(key);
        }
        table.insert("keys".to_owned(), Value::String(written.join("+")));
        s.skip()?;
        s.require(':')?;
        s.skip()?;
        // An action the scan does not know ends the entry at the next top-level comma, so the
        // entries after it are still read.
        let start = s.rest;
        let entry = match action(&mut s) {
            Ok(fields) => {
                table.extend(fields);
                Ok(table)
            }
            Err(why) => {
                let end = super::top_level(start, ',').min(super::top_level(start, '}'));
                s.rest = start.get(end..).unwrap_or("");
                Err(why)
            }
        };
        entries.push(entry);
        s.skip()?;
        if !s.eat(',') {
            s.skip()?;
            s.require('}')?;
            return Ok(entries);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_take_the_canonical_form() {
        assert_eq!(keys("shift+super+T").as_deref(), Ok("Super+Shift+t"));
        assert_eq!(keys("Super + Escape").as_deref(), Ok("Super+Escape"));
        assert_eq!(keys("XF86AudioMute").as_deref(), Ok("XF86AudioMute"));
        assert_eq!(keys("Super").as_deref(), Ok("Super"));
        for bad in [
            "",
            "Super+Super+t",
            "Super+t+u",
            "Super+a b",
            "Ctrl+\u{7}",
            "Super+",
        ] {
            assert!(keys(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_terminal_switch_and_a_listed_key_are_protected() {
        let list = vec!["Super+Escape".to_owned(), "Super+equal".to_owned()];
        for key in [
            "Ctrl+Alt+F2",
            "Super+Ctrl+Alt+F12",
            "Ctrl+Alt+f2",
            "Super+Escape",
            "Super+escape",
            "Super+=",
            "Super+EQUAL",
        ] {
            assert!(protected(key, &list), "{key}");
        }
        for key in [
            "Ctrl+F2",
            "Ctrl+Alt+F13",
            "Super+Shift+Escape",
            "Ctrl+Alt+t",
        ] {
            assert!(!protected(key, &list), "{key}");
        }
    }

    #[test]
    fn a_binding_is_checked_field_by_field() {
        let table = |text: &str| -> Table { toml::from_str(text).expect("toml") };
        let good = binding(&table(
            "keys = \"super+T\"\naction = \"spawn\"\ncommand = \"foot\"\ndescription = \"Term\"",
        ))
        .expect("good");
        assert_eq!(good["keys"].as_str(), Some("Super+t"));
        assert!(binding(&table(
            "keys = \"Super+1\"\naction = \"workspace\"\nworkspace = 1"
        ))
        .is_ok());
        assert!(binding(&table("keys = \"Super\"\nkeycode = 38\naction = \"close\"")).is_ok());
        for bad in [
            "action = \"close\"",
            "keys = \"Super+q\"",
            "keys = \"Super+q\"\naction = \"explode\"",
            "keys = \"Super+q\"\naction = \"workspace\"\nworkspace = 0",
            "keys = \"Super+q\"\naction = \"workspace\"\nworkspace = 256",
            "keys = \"Super+q\"\naction = \"focus\"\ndirection = \"sideways\"",
            "keys = \"Super+q\"\naction = \"spawn\"\ncommand = \"\"",
            "keys = \"Super+q\"\naction = \"spawn\"\ncommand = \"a\\u001bb\"",
            "keys = \"Super+q\"\naction = \"close\"\ncommand = \"foot\"",
            "keys = \"Super+q\"\naction = \"close\"\nkeycode = -1",
        ] {
            assert!(binding(&table(bad)).is_err(), "{bad}");
        }
    }

    /// Every action of COSMIC 1.11 and every form of its argument reads into a binding the
    /// check takes; an action the scan does not know is that entry's error alone.
    #[test]
    fn every_cosmic_form_is_read() {
        let text = r#"{
            (modifiers: [Super], key: "t", description: Some("Term \"x\"")): Spawn("foot -e \"top\""),
            (modifiers: [Super, Shift], key: "Left"): Move(Left),
            (modifiers: [Ctrl, Alt], key: Some("Down")): Focus(Out),
            (modifiers: [Super], keycode: Some(38)): Close,
            (modifiers: [Super], key: "r"): Resizing(Inwards),
            (modifiers: [Super], key: "o"): Orientation(Horizontal),
            (modifiers: [Super], key: "4"): SendToWorkspace(4),
            (modifiers: [Super], key: "b"): System(WebBrowser),
            (modifiers: [Super], key: "y"): Explode,
            (modifiers: [Super], key: "x"): MigrateWorkspaceToPreviousOutput,
            // A comment.
            (modifiers: [Super]): System(Launcher),
        }"#;
        let entries = custom(text).expect("a map");
        assert_eq!(entries.len(), 11);
        assert!(entries[8].is_err(), "Explode");
        let checked: Vec<Table> = entries
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != 8)
            .map(|(_, entry)| binding(entry.as_ref().expect("read")).expect("checked"))
            .collect();
        assert_eq!(checked[0]["command"].as_str(), Some("foot -e \"top\""));
        assert_eq!(checked[0]["description"].as_str(), Some("Term \"x\""));
        assert_eq!(checked[1]["keys"].as_str(), Some("Super+Shift+Left"));
        assert_eq!(checked[1]["direction"].as_str(), Some("left"));
        assert_eq!(checked[2]["keys"].as_str(), Some("Ctrl+Alt+Down"));
        assert_eq!(checked[2]["direction"].as_str(), Some("out"));
        assert_eq!(checked[3]["keycode"].as_integer(), Some(38));
        assert_eq!(checked[4]["direction"].as_str(), Some("inwards"));
        assert_eq!(checked[5]["orientation"].as_str(), Some("horizontal"));
        assert_eq!(checked[6]["workspace"].as_integer(), Some(4));
        assert_eq!(checked[7]["system"].as_str(), Some("web_browser"));
        assert_eq!(
            checked[8]["action"].as_str(),
            Some("migrate_workspace_to_previous_output")
        );
        assert_eq!(checked[9]["keys"].as_str(), Some("Super"));
        for bad in [
            "",
            "{ (modifiers: [Hyper], key: \"t\"): Close }",
            "{ (key: \"t\") Close }",
        ] {
            assert!(custom(bad).is_err(), "{bad}");
        }
    }

    /// Each action has one row, and every name of `SYSTEMS` reads back from COSMIC's.
    #[test]
    fn the_names_are_unique_and_round_trip() {
        for (at, (name, _)) in ACTIONS.iter().enumerate() {
            assert!(
                !ACTIONS[..at].iter().any(|(other, _)| other == name),
                "{name}"
            );
        }
        assert_eq!(
            system("WindowSwitcherPrevious"),
            Some("window_switcher_previous")
        );
        assert_eq!(system("Rocket"), None);
    }
}
