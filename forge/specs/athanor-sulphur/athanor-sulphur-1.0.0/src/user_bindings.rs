//! The user's key bindings, `$XDG_CONFIG_HOME/athanor/sulphur/bindings.toml` (doc_own_stack.md
//! decision 38), read at start and after each change through decision 27's watch, as
//! `windows.toml` is. athanor-sulphur-client's `shortcuts` module writes it; its
//! `data/os13-bindings.toml` holds the defaults in the file's form, which a test here compares
//! with `bindings::DEFAULTS` both ways.
//!
//! While the session is unlocked a binding of the user's comes before the defaults, `disable`
//! takes a default off its key, and `[system_actions]` gives a system action the user's command
//! line, which the user manager runs as `/bin/sh -c <line>` (D6 as decision 38 amends it). While
//! it is locked only the defaults act, with their own actions: no binding or command of the
//! user's runs on the lock screen. Skipped, each alone and logged: a binding or a `disable` on a
//! protected key (`bindings::protected`, the terminal switches included), a command for the lock
//! or the screen reader, a binding by key code or with no key, a key name xkbcommon does not
//! know, and an action the compositor does not carry out. A file refused whole changes nothing.

use crate::bindings::{cosmic, on, protected, Action, Trigger, ALT, CTRL, DEFAULTS, SHIFT, SUPER};
use crate::input_settings::{follow_file, table};
use crate::session_calls::{
    CONTROL_CENTER, LAUNCHER, LOG_OUT, MIC_MUTE, MUTE, NEXT, PLAY_PAUSE, POWER_OFF, PREVIOUS,
};
use crate::state::State;
use crate::tiling::Direction;
use crate::workspace::Step;
use crate::xkb_config::{read, root};
use smithay::input::keyboard::{xkb, Keysym};
use smithay::reexports::calloop::LoopHandle;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// The file, under the configuration root.
const RELATIVE: &str = "athanor/sulphur/bindings.toml";

/// The bindings in force while the session is unlocked, and the user's command lines they run.
#[derive(Debug)]
pub(crate) struct UserBindings {
    table: Vec<(Trigger, Action)>,
    commands: Vec<String>,
}

impl Default for UserBindings {
    fn default() -> Self {
        UserBindings {
            table: DEFAULTS.to_vec(),
            commands: Vec::new(),
        }
    }
}

impl UserBindings {
    pub(crate) fn table(&self) -> &[(Trigger, Action)] {
        &self.table
    }

    /// The command line of `Action::Spawn(n)`.
    pub(crate) fn command(&self, n: usize) -> Option<&str> {
        self.commands.get(n).map(String::as_str)
    }
}

/// COSMIC's system actions and the default action each names; one with none acts only with a
/// command of the user's.
const SYSTEMS: &[(&str, Option<Action>)] = &[
    ("app_library", None),
    (
        "brightness_down",
        Some(Action::Call(&cosmic::BRIGHTNESS_DOWN)),
    ),
    ("brightness_up", Some(Action::Call(&cosmic::BRIGHTNESS_UP))),
    ("display_toggle", None),
    ("home_folder", None),
    ("input_source_switch", Some(Action::NextGroup)),
    ("keyboard_brightness_down", None),
    ("keyboard_brightness_up", None),
    ("launcher", Some(Action::Call(&LAUNCHER))),
    ("lock_screen", Some(Action::Lock)),
    ("log_out", Some(Action::Run(&LOG_OUT))),
    ("mute", Some(Action::Run(&MUTE))),
    ("mute_mic", Some(Action::Run(&MIC_MUTE))),
    ("play_pause", Some(Action::Run(&PLAY_PAUSE))),
    ("play_next", Some(Action::Run(&NEXT))),
    ("play_prev", Some(Action::Run(&PREVIOUS))),
    ("power_off", Some(Action::Run(&POWER_OFF))),
    ("screen_reader", Some(Action::Call(&cosmic::SCREEN_READER))),
    ("screenshot", None),
    ("suspend", None),
    ("terminal", None),
    ("touchpad_toggle", Some(Action::TouchpadToggle)),
    ("volume_lower", Some(Action::Call(&cosmic::VOLUME_DOWN))),
    ("volume_raise", Some(Action::Call(&cosmic::VOLUME_UP))),
    ("web_browser", None),
    ("window_switcher", None),
    ("window_switcher_previous", None),
    ("workspace_overview", None),
];

/// What one `[[binding]]` table does.
enum Row {
    Bind(Trigger, Action),
    Disable(Trigger),
}

/// The modifiers and the key's sym of `keys`, COSMIC's form, the key resolved as COSMIC
/// resolves it: one character as its sym in lower case, a name case-sensitive first, then not.
fn resolve(keys: &str) -> Result<(u8, Keysym), String> {
    let mut mods = 0;
    let mut sym = None;
    for token in keys.split('+').map(str::trim) {
        let bit = [
            ("super", SUPER),
            ("ctrl", CTRL),
            ("alt", ALT),
            ("shift", SHIFT),
        ]
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token));
        match (bit, sym) {
            (Some((_, bit)), _) => mods |= bit,
            (None, Some(_)) => return Err(format!("{keys}: two keys")),
            (None, None) => {
                let mut chars = token.chars();
                let found = match (chars.next(), chars.next()) {
                    (Some(c), None) => c.to_lowercase().next().map(Keysym::from_char),
                    _ => [xkb::KEYSYM_NO_FLAGS, xkb::KEYSYM_CASE_INSENSITIVE]
                        .into_iter()
                        .map(|flags| xkb::keysym_from_name(token, flags))
                        .find(|sym| *sym != Keysym::NoSymbol),
                };
                sym = Some(found.ok_or_else(|| format!("{keys}: {token:?} is not a key"))?);
            }
        }
    }
    Ok((mods, sym.ok_or_else(|| format!("{keys}: no key"))?))
}

/// `resolve`'s trigger, unless its key is protected.
fn trigger(keys: &str) -> Result<Trigger, String> {
    let (mods, sym) = resolve(keys)?;
    if protected(mods, &[sym]) {
        return Err(format!("{keys}: a protected key"));
    }
    Ok(on(mods, sym))
}

/// The direction of a table's `direction`, of the four.
fn direction(table: &Table) -> Option<Direction> {
    match table.get("direction").and_then(Value::as_str)? {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

/// A table's `workspace`, from 1 to 255.
fn workspace(table: &Table) -> Option<u8> {
    let n = table.get("workspace").and_then(Value::as_integer)?;
    u8::try_from(n).ok().filter(|n| *n > 0)
}

/// A command line the worker can hand to `/bin/sh -c`.
fn command_line(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|line| !line.trim().is_empty() && !line.contains('\0'))
}

/// One `[[binding]]` table; a `spawn` adds its command to `commands`, and a `system` action
/// whose command the user replaced, the n-th of `replaced`, runs it.
fn row(
    entry: &Value,
    replaced: &[(&str, usize)],
    commands: &mut Vec<String>,
) -> Result<Row, String> {
    let table = entry.as_table().ok_or("not a table")?;
    let keys = table.get("keys").and_then(Value::as_str).ok_or("no keys")?;
    if table.contains_key("keycode") {
        return Err(format!("{keys}: a binding by key code"));
    }
    let trigger = trigger(keys)?;
    let name = table
        .get("action")
        .and_then(Value::as_str)
        .ok_or("no action")?;
    let missing = || format!("{keys}: {name} without its argument");
    let action = match name {
        "disable" => return Ok(Row::Disable(trigger)),
        "close" => Action::CloseFocused,
        "terminate" => Action::Terminate,
        "focus" => Action::FocusToward(direction(table).ok_or_else(missing)?),
        "move" => Action::SwapToward(direction(table).ok_or_else(missing)?),
        "toggle_orientation" => Action::TurnSplit,
        "resizing" => match table.get("direction").and_then(Value::as_str) {
            Some("outwards") => Action::GrowTile,
            Some("inwards") => Action::ShrinkTile,
            _ => return Err(missing()),
        },
        "toggle_window_floating" => Action::ToggleFloating,
        "zoom_in" => Action::ZoomIn,
        "zoom_out" => Action::ZoomOut,
        "workspace" => Action::Workspace(workspace(table).ok_or_else(missing)?),
        "last_workspace" => Action::LastWorkspace,
        "move_to_workspace" => Action::MoveTo(workspace(table).ok_or_else(missing)?),
        "move_to_last_workspace" => Action::MoveToLast,
        "next_workspace" => Action::Step(Step::Next),
        "previous_workspace" => Action::Step(Step::Previous),
        "move_to_next_workspace" => Action::Carry(Step::Next),
        "move_to_previous_workspace" => Action::Carry(Step::Previous),
        "reveal" => Action::Reveal,
        "focus_cycle" => Action::FocusCycle,
        "control_center" => Action::Call(&CONTROL_CENTER),
        "spawn" => {
            let line = table
                .get("command")
                .and_then(command_line)
                .ok_or_else(missing)?;
            commands.push(line.to_owned());
            Action::Spawn(commands.len() - 1)
        }
        "system" => {
            let system = table
                .get("system")
                .and_then(Value::as_str)
                .ok_or_else(missing)?;
            let found = replaced.iter().find(|(name, _)| *name == system);
            match (found, SYSTEMS.iter().find(|(name, _)| *name == system)) {
                (Some((_, n)), _) => Action::Spawn(*n),
                (None, Some((_, Some(action)))) => *action,
                (None, Some((_, None))) => return Err(format!("{keys}: {system} has no command")),
                (None, None) => return Err(format!("{keys}: {system} is not a system action")),
            }
        }
        other => {
            return Err(format!(
                "{keys}: {other} is not carried out by the compositor"
            ))
        }
    };
    Ok(Row::Bind(trigger, action))
}

/// Reads one `bindings.toml`, with what it skipped, each with its reason.
pub(crate) fn parse(text: &str) -> Result<(UserBindings, Vec<String>), String> {
    let mut file = table(text)?;
    let mut skipped = Vec::new();
    let mut commands = Vec::new();
    let mut replaced: Vec<(&str, usize)> = Vec::new();
    match file.remove("system_actions") {
        None => {}
        Some(Value::Table(entries)) => {
            for (name, value) in &entries {
                let known = SYSTEMS.iter().find(|(system, _)| system == name);
                match (known, command_line(value)) {
                    (None, _) => skipped.push(format!("system_actions.{name}: unknown")),
                    (Some(_), _) if ["lock_screen", "screen_reader"].contains(&name.as_str()) => {
                        skipped.push(format!("system_actions.{name}: not the user's to replace"))
                    }
                    (Some((system, _)), Some(line)) => {
                        replaced.push((system, commands.len()));
                        commands.push(line.to_owned());
                    }
                    (Some(_), None) => {
                        skipped.push(format!("system_actions.{name}: not a command line"))
                    }
                }
            }
        }
        Some(other) => skipped.push(format!("system_actions: a {}", other.type_str())),
    }
    let mut user = Vec::new();
    let mut disabled = Vec::new();
    match file.remove("binding") {
        None => {}
        Some(Value::Array(entries)) => {
            for (at, entry) in entries.iter().enumerate() {
                match row(entry, &replaced, &mut commands) {
                    Ok(Row::Bind(trigger, action)) => user.push((trigger, action)),
                    Ok(Row::Disable(trigger)) => disabled.push(trigger),
                    Err(reason) => skipped.push(format!("binding {}: {reason}", at + 1)),
                }
            }
        }
        Some(other) => skipped.push(format!("binding: a {}", other.type_str())),
    }
    skipped.extend(file.keys().map(|key| format!("{key}: unknown key")));
    // A default whose system action has the user's command runs it.
    let replacing = |action: &Action| {
        SYSTEMS
            .iter()
            .filter(|(_, default)| default.as_ref() == Some(action))
            .find_map(|(system, _)| replaced.iter().find(|(name, _)| name == system))
            .map(|(_, n)| Action::Spawn(*n))
    };
    let defaults = DEFAULTS
        .iter()
        .filter(|(trigger, _)| !disabled.contains(trigger))
        .map(|(trigger, action)| (*trigger, replacing(action).unwrap_or(*action)));
    let table = user.into_iter().chain(defaults).collect();
    Ok((UserBindings { table, commands }, skipped))
}

/// Reads the file at start and after each change of it. Without a path or a watch the defaults
/// or the first read stay.
pub(crate) fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    let Some(root) = root() else {
        tracing::warn!("no configuration root: the key bindings keep their defaults");
        return;
    };
    follow(state, handle, root.join(RELATIVE));
}

/// Watches `path`, then reads it, so a write between the two is not missed.
pub(crate) fn follow(state: &mut State, handle: &LoopHandle<'static, State>, path: PathBuf) {
    follow_file(state, handle, path, State::load_user_bindings);
}

impl State {
    /// Reads `path` and applies it. An absent file gives the defaults; a refused one changes
    /// nothing.
    pub(crate) fn load_user_bindings(&mut self, path: &Path) {
        let read = read(path).map_err(|reason| reason.to_string());
        self.user_bindings = match read.and_then(|text| text.map(|text| parse(&text)).transpose()) {
            Ok(Some((bindings, skipped))) => {
                for reason in skipped {
                    tracing::warn!(?path, %reason, "bindings.toml: an entry is skipped");
                }
                bindings
            }
            Ok(None) => {
                tracing::info!(?path, "no bindings.toml: the defaults apply");
                UserBindings::default()
            }
            Err(reason) => {
                tracing::warn!(?path, %reason, "bindings.toml refused: the bindings stay");
                return;
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client's copy of the defaults, in the file's form.
    const OS13: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../system/athanor-sulphur-client/data/os13-bindings.toml"
    ));

    fn read(text: &str) -> UserBindings {
        let (bindings, skipped) = parse(&format!("schema = 1\n{text}")).expect("read");
        assert!(skipped.is_empty(), "{skipped:?}");
        bindings
    }

    fn action(bindings: &UserBindings, mods: u8, sym: Keysym) -> Option<Action> {
        bindings
            .table()
            .iter()
            .find(|(trigger, _)| *trigger == on(mods, sym))
            .map(|(_, action)| *action)
    }

    /// The client's copy reads as `DEFAULTS`, row for row, but the protected defaults, which
    /// a user's binding cannot take; its protected keys are protected here, and every default
    /// no inhibitor takes is among them.
    #[test]
    fn the_clients_copy_is_the_defaults_both_ways() {
        let mut copy: Table = toml::from_str(OS13).expect("toml");
        let listed: Vec<Trigger> = copy["protected"]
            .as_array()
            .expect("protected")
            .iter()
            .map(|key| {
                let (mods, sym) = resolve(key.as_str().expect("a string")).expect("a key");
                assert!(protected(mods, &[sym]), "{key}");
                on(mods, sym)
            })
            .collect();
        let kept = |action: &Action| action.never_inhibited();
        for (trigger, action) in DEFAULTS.iter().filter(|(_, action)| kept(action)) {
            assert!(listed.contains(trigger), "{trigger:?} {action:?} is not listed");
        }
        copy.remove("protected");
        copy.remove("cosmic");
        let (bindings, skipped) = parse(&toml::to_string(&copy).expect("text")).expect("read");
        let ours = DEFAULTS.iter().filter(|(_, action)| !kept(action)).copied();
        let user = bindings.table().len() - DEFAULTS.len();
        assert_eq!(bindings.table()[..user], ours.collect::<Vec<_>>()[..]);
        assert_eq!(skipped.len(), DEFAULTS.len() - user, "{skipped:?}");
        assert!(skipped.iter().all(|reason| reason.ends_with("a protected key")), "{skipped:?}");
    }

    #[test]
    fn a_binding_comes_before_the_defaults_and_disable_takes_one_off() {
        let bindings = read(
            "[[binding]]\nkeys = \"Super+T\"\naction = \"spawn\"\ncommand = \"foot\"\n\
             [[binding]]\nkeys = \"Super+q\"\naction = \"workspace\"\nworkspace = 3\n\
             [[binding]]\nkeys = \"Super+o\"\naction = \"disable\"\n",
        );
        assert_eq!(action(&bindings, SUPER, Keysym::t), Some(Action::Spawn(0)));
        assert_eq!(bindings.command(0), Some("foot"));
        assert_eq!(
            bindings.table()[1],
            (on(SUPER, Keysym::q), Action::Workspace(3))
        );
        assert_eq!(action(&bindings, SUPER, Keysym::o), None, "disabled");
        assert_eq!(
            action(&bindings, SUPER, Keysym::g),
            Some(Action::ToggleFloating)
        );
    }

    /// A system action with the user's command runs it from its default key and from a binding.
    #[test]
    fn a_system_action_takes_the_users_command() {
        let bindings = read(
            "[system_actions]\nmute = \"my-mute\"\nterminal = \"foot\"\n\
             [[binding]]\nkeys = \"Super+Return\"\naction = \"system\"\nsystem = \"terminal\"\n",
        );
        let mute = action(&bindings, 0, Keysym::XF86_AudioMute).expect("mute");
        let Action::Spawn(n) = mute else {
            panic!("{mute:?}")
        };
        assert_eq!(bindings.command(n), Some("my-mute"));
        let Some(Action::Spawn(n)) = action(&bindings, SUPER, Keysym::Return) else {
            panic!("terminal")
        };
        assert_eq!(bindings.command(n), Some("foot"));
    }

    /// A protected key, an imported terminal switch included, and the lock's command are
    /// skipped; an unknown action or key is skipped alone.
    #[test]
    fn what_is_refused_is_skipped_alone() {
        let (bindings, skipped) = parse(
            "schema = 1\n[system_actions]\nlock_screen = \"true\"\nscreen_reader = \"true\"\n\
             [[binding]]\nkeys = \"Super+Escape\"\naction = \"spawn\"\ncommand = \"foot\"\n\
             [[binding]]\nkeys = \"Ctrl+Alt+F2\"\naction = \"spawn\"\ncommand = \"foot\"\n\
             [[binding]]\nkeys = \"Super+Alt+t\"\naction = \"disable\"\n\
             [[binding]]\nkeys = \"Super+m\"\naction = \"maximize\"\n\
             [[binding]]\nkeys = \"Super+NoSuchKey\"\naction = \"close\"\n\
             [[binding]]\nkeys = \"Super\"\nkeycode = 38\naction = \"close\"\n\
             [[binding]]\nkeys = \"Super\"\naction = \"close\"\n\
             [[binding]]\nkeys = \"Super+b\"\naction = \"system\"\nsystem = \"terminal\"\n\
             [[binding]]\nkeys = \"Super+n\"\naction = \"close\"\n",
        )
        .expect("read");
        assert_eq!(skipped.len(), 10, "{skipped:?}");
        assert_eq!(
            bindings.table()[0],
            (on(SUPER, Keysym::n), Action::CloseFocused)
        );
        assert_eq!(bindings.table().len(), DEFAULTS.len() + 1);
        assert_eq!(action(&bindings, SUPER, Keysym::Escape), Some(Action::Lock));
        assert_eq!(
            action(&bindings, SUPER | ALT, Keysym::t),
            Some(Action::Reveal)
        );
    }

    #[test]
    fn a_file_without_its_schema_is_refused() {
        assert!(parse("[[binding]]\nkeys = \"Super+t\"\naction = \"close\"\n").is_err());
        assert!(parse("schema = 1\n[[binding]\n").is_err());
    }
}
