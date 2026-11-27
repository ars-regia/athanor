//! The writer of `input.toml` (`doc_own_stack.md` decision 38, SE15, SE16): the one module that
//! writes the file, through the store's lock and atomic write (`store`). A key and its value
//! are checked against `KEYS`, the list of Sulphur's README that its reader matches; a key or a
//! table the list does not name, already in the file, is kept (ADR-0150 point 3).

use crate::store::{self, Error, Store};
use std::path::Path;
use toml::{Table, Value};

/// The file, in the store's directory.
pub const FILE: &str = "input.toml";

/// Where a key is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section<'a> {
    /// `[mouse]`.
    Mouse,
    /// `[touchpad]`.
    Touchpad,
    /// `[keyboard]`.
    Keyboard,
    /// The `[[device]]` whose `name` is the libinput device's name.
    Device(&'a str),
}

/// What a key accepts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Accepts {
    Boolean,
    /// A number from the first to the second, both included.
    Number(f64, f64),
    /// One of these strings.
    OneOf(&'static [&'static str]),
    /// A name that is not empty: an output's.
    Name,
}

const MOUSE: u8 = 1;
const TOUCHPAD: u8 = 2;
const DEVICE: u8 = 4;
const KEYBOARD: u8 = 8;

/// Each key, the sections it is written in, and what it accepts: the table of
/// `forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/README.md`, which a test compares.
pub(crate) const KEYS: &[(&str, u8, Accepts)] = &[
    (
        "speed",
        MOUSE | TOUCHPAD | DEVICE,
        Accepts::Number(-1.0, 1.0),
    ),
    (
        "acceleration_profile",
        MOUSE | DEVICE,
        Accepts::OneOf(&["flat", "adaptive"]),
    ),
    ("left_handed", MOUSE | DEVICE, Accepts::Boolean),
    (
        "natural_scroll",
        MOUSE | TOUCHPAD | DEVICE,
        Accepts::Boolean,
    ),
    ("scroll_factor", MOUSE | DEVICE, Accepts::Number(0.1, 10.0)),
    ("middle_button_emulation", MOUSE | DEVICE, Accepts::Boolean),
    (
        "state",
        TOUCHPAD | DEVICE,
        Accepts::OneOf(&["enabled", "disabled", "disabled_on_external_mouse"]),
    ),
    ("tap_to_click", TOUCHPAD | DEVICE, Accepts::Boolean),
    ("tap_drag", TOUCHPAD | DEVICE, Accepts::Boolean),
    ("tap_drag_lock", TOUCHPAD | DEVICE, Accepts::Boolean),
    (
        "tap_button_map",
        TOUCHPAD | DEVICE,
        Accepts::OneOf(&["left_right_middle", "left_middle_right"]),
    ),
    (
        "click_method",
        TOUCHPAD | DEVICE,
        Accepts::OneOf(&["button_areas", "clickfinger"]),
    ),
    (
        "scroll_method",
        TOUCHPAD | DEVICE,
        Accepts::OneOf(&["no_scroll", "two_finger", "edge", "on_button_down"]),
    ),
    ("disable_while_typing", TOUCHPAD | DEVICE, Accepts::Boolean),
    ("map_to_output", DEVICE, Accepts::Name),
    (
        "numlock_state",
        KEYBOARD,
        Accepts::OneOf(&["boot_on", "boot_off"]),
    ),
];

/// Sets `key` of `section` to `value`, or removes it with `None`, so that the default applies.
/// A key the section does not take, or a value the key does not accept, is refused and nothing
/// is written.
pub fn set(
    store: &Store,
    section: Section<'_>,
    key: &str,
    value: Option<Value>,
) -> Result<(), Error> {
    store.update(FILE, |path, table| apply(path, table, section, key, value))
}

fn apply(
    path: &Path,
    table: &mut Table,
    section: Section<'_>,
    key: &str,
    value: Option<Value>,
) -> Result<(), Error> {
    let bit = match section {
        Section::Mouse => MOUSE,
        Section::Touchpad => TOUCHPAD,
        Section::Keyboard => KEYBOARD,
        Section::Device(_) => DEVICE,
    };
    let refuse = |reason: String| Error::Refused {
        path: path.to_owned(),
        reason,
    };
    let Some(&(_, _, accepts)) = KEYS
        .iter()
        .find(|(name, sections, _)| *name == key && sections & bit != 0)
    else {
        return Err(refuse(format!("{key} is not a key of {section:?}")));
    };
    if let Some(value) = &value {
        check(accepts, value).map_err(|why| refuse(format!("{key}: {why}")))?;
    }
    match section {
        Section::Mouse => write_key(store::table_mut(path, table, "mouse")?, key, value),
        Section::Touchpad => write_key(store::table_mut(path, table, "touchpad")?, key, value),
        Section::Keyboard => write_key(store::table_mut(path, table, "keyboard")?, key, value),
        Section::Device(name) => device(path, table, name, key, value)?,
    }
    Ok(())
}

fn write_key(table: &mut Table, key: &str, value: Option<Value>) {
    match value {
        Some(value) => {
            table.insert(key.to_owned(), value);
        }
        None => {
            table.remove(key);
        }
    }
}

/// The key of the `[[device]]` named `name`, which is made for a value and left out once only
/// its name is left.
fn device(
    path: &Path,
    table: &mut Table,
    name: &str,
    key: &str,
    value: Option<Value>,
) -> Result<(), Error> {
    let refuse = |reason: &str| Error::Refused {
        path: path.to_owned(),
        reason: reason.to_owned(),
    };
    if name.is_empty() {
        return Err(refuse("a device's name is empty"));
    }
    let Value::Array(devices) = table
        .entry("device")
        .or_insert_with(|| Value::Array(Vec::new()))
    else {
        return Err(refuse("device is not an array of tables"));
    };
    let mut found = None;
    for (at, entry) in devices.iter().enumerate() {
        let Value::Table(entry) = entry else {
            return Err(refuse("device is not an array of tables"));
        };
        if entry.get("name").and_then(Value::as_str) == Some(name) {
            found = Some(at);
        }
    }
    let at = match (found, &value) {
        (Some(at), _) => at,
        (None, None) => return Ok(()),
        (None, Some(_)) => {
            let mut entry = Table::new();
            entry.insert("name".to_owned(), Value::String(name.to_owned()));
            devices.push(Value::Table(entry));
            devices.len().saturating_sub(1)
        }
    };
    let Some(Value::Table(entry)) = devices.get_mut(at) else {
        return Err(refuse("device is not an array of tables"));
    };
    write_key(entry, key, value);
    if entry.len() == 1 {
        devices.remove(at);
    }
    Ok(())
}

/// Whether `value` is one `accepts` takes.
pub(crate) fn check(accepts: Accepts, value: &Value) -> Result<(), String> {
    match (accepts, value) {
        (Accepts::Boolean, Value::Boolean(_)) => Ok(()),
        (Accepts::Number(low, high), Value::Float(number)) if (low..=high).contains(number) => {
            Ok(())
        }
        (Accepts::Number(low, high), Value::Integer(number))
            if (low..=high).contains(&(*number as f64)) =>
        {
            Ok(())
        }
        (Accepts::Number(low, high), _) => Err(format!("not a number from {low} to {high}")),
        (Accepts::OneOf(values), Value::String(text)) if values.contains(&text.as_str()) => Ok(()),
        (Accepts::OneOf(values), _) => Err(format!("not one of {values:?}")),
        (Accepts::Name, Value::String(text)) if !text.is_empty() => Ok(()),
        (Accepts::Name, _) => Err("not a name".to_owned()),
        (Accepts::Boolean, _) => Err("not a boolean".to_owned()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::tests::scratch;
    use std::collections::BTreeSet;
    use std::fs;

    fn read(dir: &Path) -> Table {
        toml::from_str(&fs::read_to_string(dir.join(FILE)).expect("read")).expect("toml")
    }

    #[test]
    fn a_key_is_written_and_removed() {
        let dir = scratch("input-set");
        let store = Store::at(dir.clone());
        set(&store, Section::Mouse, "speed", Some(Value::Float(0.5))).expect("speed");
        set(&store, Section::Touchpad, "state", Some("disabled".into())).expect("state");
        set(
            &store,
            Section::Keyboard,
            "numlock_state",
            Some("boot_on".into()),
        )
        .expect("numlock");
        let table = read(&dir);
        assert_eq!(table["schema"].as_integer(), Some(1));
        assert_eq!(table["mouse"]["speed"].as_float(), Some(0.5));
        assert_eq!(table["touchpad"]["state"].as_str(), Some("disabled"));
        assert_eq!(table["keyboard"]["numlock_state"].as_str(), Some("boot_on"));
        set(&store, Section::Mouse, "speed", None).expect("remove");
        assert!(read(&dir)["mouse"].get("speed").is_none());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_device_is_made_and_left_out_with_its_last_key() {
        let dir = scratch("input-device");
        let store = Store::at(dir.clone());
        let pad = Section::Device("SYNA Touchpad");
        set(&store, pad, "tap_to_click", Some(false.into())).expect("tap");
        set(&store, pad, "speed", Some(Value::Integer(1))).expect("speed");
        let devices = read(&dir)["device"].as_array().expect("devices").clone();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0]["name"].as_str(), Some("SYNA Touchpad"));
        assert_eq!(devices[0]["tap_to_click"].as_bool(), Some(false));
        set(&store, pad, "tap_to_click", None).expect("remove");
        set(&store, pad, "speed", None).expect("remove");
        assert_eq!(read(&dir)["device"].as_array().map(Vec::len), Some(0));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_key_or_a_value_out_of_the_list_is_refused() {
        let dir = scratch("input-refused");
        let store = Store::at(dir.clone());
        let refused = [
            (Section::Touchpad, "scroll_factor", Value::Float(1.0)),
            (Section::Mouse, "speed", Value::Float(1.5)),
            (Section::Mouse, "scroll_factor", Value::Float(0.05)),
            (Section::Mouse, "left_handed", Value::Integer(1)),
            (Section::Mouse, "acceleration_profile", "custom".into()),
            (Section::Keyboard, "numlock_state", "last_boot".into()),
            (Section::Mouse, "edge_snap_threshold", Value::Integer(8)),
            (Section::Device(""), "speed", Value::Float(0.0)),
            (Section::Device("pen"), "map_to_output", "".into()),
            (Section::Device("pen"), "name", "other".into()),
        ];
        for (section, key, value) in refused {
            let result = set(&store, section, key, Some(value));
            assert!(
                matches!(result, Err(Error::Refused { .. })),
                "{key}: {result:?}"
            );
        }
        assert!(!dir.join(FILE).exists(), "nothing written");
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// Decision 38's file keeps what a newer image wrote (ADR-0150 point 3).
    #[test]
    fn a_rewrite_keeps_unknown_keys_and_tables() {
        let dir = scratch("input-unknown");
        fs::write(
            dir.join(FILE),
            "schema = 1\nfuture = true\n[mouse]\nspeed = 0.2\nwheel_tilt = 3\n\
             [gestures]\nthree_finger = \"workspace\"\n\
             [[device]]\nname = \"pen\"\npressure_curve = [0, 1]\n",
        )
        .expect("seed");
        set(
            &Store::at(dir.clone()),
            Section::Mouse,
            "speed",
            Some(Value::Float(-0.3)),
        )
        .expect("set");
        let table = read(&dir);
        assert_eq!(table["future"].as_bool(), Some(true));
        assert_eq!(table["mouse"]["speed"].as_float(), Some(-0.3));
        assert_eq!(table["mouse"]["wheel_tilt"].as_integer(), Some(3));
        assert_eq!(
            table["gestures"]["three_finger"].as_str(),
            Some("workspace")
        );
        assert_eq!(
            table["device"][0]["pressure_curve"],
            Value::Array(vec![0.into(), 1.into()])
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The README's table of the two files' keys, as (key, where) to the cell of the values.
    pub(crate) fn readme() -> Vec<(String, String, String)> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/README.md"
        );
        let text = fs::read_to_string(path).expect("Sulphur's README");
        let mut rows = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            if line.starts_with("| Key | Where | Accepted values |") {
                inside = true;
                continue;
            }
            if !inside || line.starts_with("| ---") {
                continue;
            }
            if !line.starts_with('|') {
                break;
            }
            let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
            let [keys, places, values] = cells.as_slice() else {
                panic!("a row of three cells: {line}");
            };
            for key in ticked(keys) {
                rows.push((key, (*places).to_owned(), (*values).to_owned()));
            }
        }
        assert!(!rows.is_empty(), "the README's table");
        rows
    }

    /// The text between each pair of backticks.
    pub(crate) fn ticked(cell: &str) -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    /// What a README cell of accepted values says, in `Accepts`.
    pub(crate) fn accepts(cell: &str) -> Accepts {
        if cell == "`true`, `false`" {
            return Accepts::Boolean;
        }
        if let Some(range) = cell.strip_prefix("a number from ") {
            let (low, high) = range.split_once(" to ").expect("a range");
            return Accepts::Number(low.parse().expect("low"), high.parse().expect("high"));
        }
        if cell.starts_with("the name of an output") {
            return Accepts::Name;
        }
        let values: Vec<String> = ticked(cell)
            .into_iter()
            .map(|value| value.trim_matches('"').to_owned())
            .collect();
        let leaked: &'static [&'static str] = Vec::leak(
            values
                .into_iter()
                .map(|value| &*String::leak(value))
                .collect(),
        );
        Accepts::OneOf(leaked)
    }

    /// `KEYS` is the README's list of `input.toml`'s keys, both ways.
    #[test]
    fn the_keys_are_the_readmes() {
        let mut listed = BTreeSet::new();
        for (key, places, values) in readme() {
            if key == "schema" || key == "name" || key == "active_hint" {
                continue;
            }
            let mut sections = 0;
            for (place, bit) in [
                ("`[mouse]`", MOUSE),
                ("`[touchpad]`", TOUCHPAD),
                ("`[[device]]`", DEVICE),
                ("`[keyboard]`", KEYBOARD),
            ] {
                if places.contains(place) {
                    sections |= bit;
                }
            }
            listed.insert(format!("{key} {sections} {:?}", accepts(&values)));
        }
        let ours: BTreeSet<String> = KEYS
            .iter()
            .map(|(key, sections, accepts)| format!("{key} {sections} {accepts:?}"))
            .collect();
        assert_eq!(ours, listed);
    }
}
