//! The user's input settings, `$XDG_CONFIG_HOME/athanor/sulphur/input.toml` (doc_own_stack.md
//! decision 38, SE-Q5; doc_settings.md SE15 and SE16), read at start and after each change
//! through decision 27's watch, and applied to each libinput device when it appears and to
//! every device when the file changes. A key that is absent takes the compiled default:
//! libinput's for the device and, for a touchpad, the values cosmic-comp shipped. A file
//! refused whole changes nothing, a key refused is skipped alone, and both are logged.

use crate::state::State;
use crate::xkb_config::{read, root, Watch};
use smithay::output::Output;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use smithay::reexports::input::{
    AccelProfile, ClickMethod, Device, DeviceCapability, DragLockState, ScrollMethod,
    SendEventsMode, TapButtonMap,
};
use std::any::Any;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// The file, under the configuration root.
const RELATIVE: &str = "athanor/sulphur/input.toml";

/// The one schema of decision 38's files.
const SCHEMA: i64 = 1;

/// The keys of `[mouse]` (SE16): speed and acceleration profile, primary button, natural
/// scrolling, scroll speed, middle-button emulation.
const MOUSE: &[&str] = &[
    "speed",
    "acceleration_profile",
    "left_handed",
    "natural_scroll",
    "scroll_factor",
    "middle_button_emulation",
];

/// The keys of `[touchpad]` (SE16).
const TOUCHPAD: &[&str] = &[
    "state",
    "tap_to_click",
    "tap_drag",
    "tap_drag_lock",
    "tap_button_map",
    "click_method",
    "natural_scroll",
    "scroll_method",
    "disable_while_typing",
    "speed",
];

/// The keys of a `[[device]]` besides its `name`: those of both sections, and the output a
/// tablet or a touchscreen maps to. `left_handed` is a tablet's orientation.
const DEVICE: &[&str] = &[
    "state",
    "speed",
    "acceleration_profile",
    "left_handed",
    "natural_scroll",
    "scroll_factor",
    "middle_button_emulation",
    "tap_to_click",
    "tap_drag",
    "tap_drag_lock",
    "tap_button_map",
    "click_method",
    "scroll_method",
    "disable_while_typing",
    "map_to_output",
];

/// The bounds of `scroll_factor`, a multiple of the device's scroll.
const SCROLL_FACTOR: std::ops::RangeInclusive<f64> = 0.1..=10.0;

/// The settings of one device or section; `None` takes the default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    pub state: Option<SendEventsMode>,
    pub speed: Option<f64>,
    pub acceleration_profile: Option<AccelProfile>,
    pub left_handed: Option<bool>,
    pub natural_scroll: Option<bool>,
    pub scroll_factor: Option<f64>,
    pub middle_button_emulation: Option<bool>,
    pub tap_to_click: Option<bool>,
    pub tap_drag: Option<bool>,
    pub tap_drag_lock: Option<bool>,
    pub tap_button_map: Option<TapButtonMap>,
    pub click_method: Option<ClickMethod>,
    pub scroll_method: Option<ScrollMethod>,
    pub disable_while_typing: Option<bool>,
    pub map_to_output: Option<String>,
}

/// What a touchpad takes for a key no section sets: cosmic-comp's shipped `input_touchpad`
/// (`/usr/share/cosmic/com.system76.CosmicComp/v1/input_touchpad`, 1.11.0), which leaves the
/// image with cosmic-comp (SE16).
const TOUCHPAD_DEFAULTS: Keys = Keys {
    state: Some(SendEventsMode::ENABLED),
    click_method: Some(ClickMethod::Clickfinger),
    scroll_method: Some(ScrollMethod::TwoFinger),
    tap_to_click: Some(true),
    tap_button_map: Some(TapButtonMap::LeftRightMiddle),
    tap_drag: Some(true),
    tap_drag_lock: Some(false),
    speed: None,
    acceleration_profile: None,
    left_handed: None,
    natural_scroll: None,
    scroll_factor: None,
    middle_button_emulation: None,
    disable_while_typing: None,
    map_to_output: None,
};

impl Keys {
    /// These keys, and `under`'s for those these leave unset.
    fn or(&self, under: &Keys) -> Keys {
        Keys {
            state: self.state.or(under.state),
            speed: self.speed.or(under.speed),
            acceleration_profile: self.acceleration_profile.or(under.acceleration_profile),
            left_handed: self.left_handed.or(under.left_handed),
            natural_scroll: self.natural_scroll.or(under.natural_scroll),
            scroll_factor: self.scroll_factor.or(under.scroll_factor),
            middle_button_emulation: self
                .middle_button_emulation
                .or(under.middle_button_emulation),
            tap_to_click: self.tap_to_click.or(under.tap_to_click),
            tap_drag: self.tap_drag.or(under.tap_drag),
            tap_drag_lock: self.tap_drag_lock.or(under.tap_drag_lock),
            tap_button_map: self.tap_button_map.or(under.tap_button_map),
            click_method: self.click_method.or(under.click_method),
            scroll_method: self.scroll_method.or(under.scroll_method),
            disable_while_typing: self.disable_while_typing.or(under.disable_while_typing),
            map_to_output: self
                .map_to_output
                .clone()
                .or_else(|| under.map_to_output.clone()),
        }
    }

    /// Sets `key` from `value`, or says why it is refused.
    fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        match key {
            "state" => {
                self.state = Some(word(
                    value,
                    &[
                        ("enabled", SendEventsMode::ENABLED),
                        ("disabled", SendEventsMode::DISABLED),
                        (
                            "disabled_on_external_mouse",
                            SendEventsMode::DISABLED_ON_EXTERNAL_MOUSE,
                        ),
                    ],
                )?)
            }
            "speed" => self.speed = Some(number(value, &(-1.0..=1.0))?),
            "acceleration_profile" => {
                self.acceleration_profile = Some(word(
                    value,
                    &[
                        ("flat", AccelProfile::Flat),
                        ("adaptive", AccelProfile::Adaptive),
                    ],
                )?)
            }
            "left_handed" => self.left_handed = Some(boolean(value)?),
            "natural_scroll" => self.natural_scroll = Some(boolean(value)?),
            "scroll_factor" => self.scroll_factor = Some(number(value, &SCROLL_FACTOR)?),
            "middle_button_emulation" => self.middle_button_emulation = Some(boolean(value)?),
            "tap_to_click" => self.tap_to_click = Some(boolean(value)?),
            "tap_drag" => self.tap_drag = Some(boolean(value)?),
            "tap_drag_lock" => self.tap_drag_lock = Some(boolean(value)?),
            "tap_button_map" => {
                self.tap_button_map = Some(word(
                    value,
                    &[
                        ("left_right_middle", TapButtonMap::LeftRightMiddle),
                        ("left_middle_right", TapButtonMap::LeftMiddleRight),
                    ],
                )?)
            }
            "click_method" => {
                self.click_method = Some(word(
                    value,
                    &[
                        ("button_areas", ClickMethod::ButtonAreas),
                        ("clickfinger", ClickMethod::Clickfinger),
                    ],
                )?)
            }
            "scroll_method" => {
                self.scroll_method = Some(word(
                    value,
                    &[
                        ("no_scroll", ScrollMethod::NoScroll),
                        ("two_finger", ScrollMethod::TwoFinger),
                        ("edge", ScrollMethod::Edge),
                        ("on_button_down", ScrollMethod::OnButtonDown),
                    ],
                )?)
            }
            "disable_while_typing" => self.disable_while_typing = Some(boolean(value)?),
            "map_to_output" => match value {
                Value::String(name) if !name.is_empty() => self.map_to_output = Some(name),
                other => return Err(format!("{} is not an output's name", other.type_str())),
            },
            _ => return Err("unknown key".into()),
        }
        Ok(())
    }
}

fn boolean(value: Value) -> Result<bool, String> {
    match value {
        Value::Boolean(value) => Ok(value),
        other => Err(format!("a {}, not a boolean", other.type_str())),
    }
}

fn number(value: Value, range: &std::ops::RangeInclusive<f64>) -> Result<f64, String> {
    let number = match value {
        Value::Float(number) => number,
        Value::Integer(number) => number as f64,
        other => return Err(format!("a {}, not a number", other.type_str())),
    };
    if range.contains(&number) {
        Ok(number)
    } else {
        Err(format!(
            "{number} is outside {} to {}",
            range.start(),
            range.end()
        ))
    }
}

fn word<T: Copy>(value: Value, words: &[(&str, T)]) -> Result<T, String> {
    let Value::String(text) = value else {
        return Err(format!("a {}, not a string", value.type_str()));
    };
    words
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("{text:?} is not one of its values"))
}

/// What kind of device a section applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// A device that can tap: `[touchpad]`, and the key of D16.
    Touchpad,
    /// Any other pointing device: `[mouse]`.
    Mouse,
    /// A keyboard, a tablet, a touchscreen: only its `[[device]]`.
    Other,
}

impl Class {
    pub fn of(device: &Device) -> Class {
        if device.config_tap_finger_count() > 0 {
            Class::Touchpad
        } else if device.has_capability(DeviceCapability::Pointer) {
            Class::Mouse
        } else {
            Class::Other
        }
    }
}

/// The settings of one `input.toml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputSettings {
    mouse: Keys,
    touchpad: Keys,
    /// Each `[[device]]`, by its name.
    devices: Vec<(String, Keys)>,
    /// `[keyboard] numlock_state`: Num Lock on at the start of the session (SE15).
    pub num_lock: Option<bool>,
}

impl InputSettings {
    /// The settings of a device of `class` named `name`: its `[[device]]`, over its section,
    /// over a touchpad's defaults.
    pub fn resolve(&self, class: Class, name: &str) -> Keys {
        let device = self
            .devices
            .iter()
            .find(|(device, _)| device == name)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_default();
        match class {
            Class::Touchpad => device.or(&self.touchpad).or(&TOUCHPAD_DEFAULTS),
            Class::Mouse => device.or(&self.mouse),
            Class::Other => device,
        }
    }
}

/// The table of one of decision 38's files, its `schema` checked and taken out. A file that is
/// not TOML, or of another schema, is refused whole.
pub(crate) fn table(text: &str) -> Result<Table, String> {
    let mut table: Table = text
        .parse()
        .map_err(|error: toml::de::Error| error.message().to_string())?;
    match table.remove("schema") {
        Some(Value::Integer(SCHEMA)) => Ok(table),
        Some(Value::Integer(other)) => Err(format!("schema {other}, not {SCHEMA}")),
        Some(other) => Err(format!("schema is a {}, not an integer", other.type_str())),
        None => Err("no schema".into()),
    }
}

/// Reads one `input.toml`, with what it skipped, each with its reason.
pub fn parse(text: &str) -> Result<(InputSettings, Vec<String>), String> {
    let mut settings = InputSettings::default();
    let mut skipped = Vec::new();
    for (name, value) in table(text)? {
        match (name.as_str(), value) {
            ("mouse", Value::Table(table)) => {
                settings.mouse = section(table, MOUSE, "mouse", &mut skipped)
            }
            ("touchpad", Value::Table(table)) => {
                settings.touchpad = section(table, TOUCHPAD, "touchpad", &mut skipped)
            }
            ("keyboard", Value::Table(table)) => settings.num_lock = keyboard(table, &mut skipped),
            ("device", Value::Array(entries)) => {
                for (at, entry) in entries.into_iter().enumerate() {
                    device(&mut settings.devices, entry, at, &mut skipped);
                }
            }
            (section @ ("mouse" | "touchpad" | "keyboard" | "device"), other) => {
                skipped.push(format!("{section}: a {}, not a table", other.type_str()))
            }
            (other, _) => skipped.push(format!("{other}: unknown key")),
        }
    }
    Ok((settings, skipped))
}

fn section(table: Table, allowed: &[&str], at: &str, skipped: &mut Vec<String>) -> Keys {
    let mut keys = Keys::default();
    for (key, value) in table {
        let set = if allowed.contains(&key.as_str()) {
            keys.set(&key, value)
        } else {
            Err("unknown key".into())
        };
        if let Err(reason) = set {
            skipped.push(format!("{at}.{key}: {reason}"));
        }
    }
    keys
}

fn device(devices: &mut Vec<(String, Keys)>, entry: Value, at: usize, skipped: &mut Vec<String>) {
    let Value::Table(mut table) = entry else {
        skipped.push(format!("device {at}: a {}, not a table", entry.type_str()));
        return;
    };
    let name = match table.remove("name") {
        Some(Value::String(name)) if !name.is_empty() => name,
        _ => {
            skipped.push(format!("device {at}: no name"));
            return;
        }
    };
    if devices.iter().any(|(device, _)| *device == name) {
        skipped.push(format!("device {name:?}: given twice"));
        return;
    }
    let keys = section(table, DEVICE, &format!("device {name:?}"), skipped);
    devices.push((name, keys));
}

fn keyboard(table: Table, skipped: &mut Vec<String>) -> Option<bool> {
    let mut num_lock = None;
    for (key, value) in table {
        let set = match key.as_str() {
            "numlock_state" => word(value, &[("boot_on", true), ("boot_off", false)])
                .map(|value| num_lock = Some(value)),
            _ => Err("unknown key".into()),
        };
        if let Err(reason) = set {
            skipped.push(format!("keyboard.{key}: {reason}"));
        }
    }
    num_lock
}

/// Logs a setting libinput refused.
macro_rules! set {
    ($device:ident, $key:literal, $call:expr) => {
        if let Err(error) = $call {
            tracing::warn!(
                device = %$device.name(),
                key = $key,
                ?error,
                "input.toml: a setting is not applied"
            );
        }
    };
}

/// Turns a touchpad's events on as its `state` has it, or off while the touchpad key holds them
/// off (D16), which wins over the file. A device of another class is left alone.
pub(crate) fn switch(device: &Device, settings: &InputSettings, touchpads_off: bool) {
    if Class::of(device) != Class::Touchpad {
        return;
    }
    let keys = settings.resolve(Class::Touchpad, &device.name());
    let mode = match keys.state {
        _ if touchpads_off => SendEventsMode::DISABLED,
        Some(mode) => mode,
        None => SendEventsMode::ENABLED,
    };
    set!(device, "state", device.config_send_events_set_mode(mode));
}

/// Applies `settings` to `device`: each key it has, the file's value or, when the file has
/// none, the default. A key the device does not have is passed over.
pub(crate) fn apply(device: &mut Device, settings: &InputSettings, touchpads_off: bool) {
    let keys = settings.resolve(Class::of(device), &device.name());
    switch(device, settings, touchpads_off);
    if device.config_accel_is_available() {
        let speed = keys
            .speed
            .unwrap_or_else(|| device.config_accel_default_speed());
        set!(device, "speed", device.config_accel_set_speed(speed));
        let profile = keys
            .acceleration_profile
            .or_else(|| device.config_accel_default_profile());
        if let Some(profile) =
            profile.filter(|profile| device.config_accel_profiles().contains(profile))
        {
            set!(
                device,
                "acceleration_profile",
                device.config_accel_set_profile(profile)
            );
        }
    }
    if device.config_left_handed_is_available() {
        let left = keys
            .left_handed
            .unwrap_or_else(|| device.config_left_handed_default());
        set!(device, "left_handed", device.config_left_handed_set(left));
    }
    if device.config_scroll_has_natural_scroll() {
        let natural = keys
            .natural_scroll
            .unwrap_or_else(|| device.config_scroll_default_natural_scroll_enabled());
        set!(
            device,
            "natural_scroll",
            device.config_scroll_set_natural_scroll_enabled(natural)
        );
    }
    if device.config_middle_emulation_is_available() {
        let middle = keys
            .middle_button_emulation
            .unwrap_or_else(|| device.config_middle_emulation_default_enabled());
        set!(
            device,
            "middle_button_emulation",
            device.config_middle_emulation_set_enabled(middle)
        );
    }
    if device.config_tap_finger_count() > 0 {
        let tap = keys
            .tap_to_click
            .unwrap_or_else(|| device.config_tap_default_enabled());
        set!(device, "tap_to_click", device.config_tap_set_enabled(tap));
        let drag = keys
            .tap_drag
            .unwrap_or_else(|| device.config_tap_default_drag_enabled());
        set!(device, "tap_drag", device.config_tap_set_drag_enabled(drag));
        // cosmic-comp's mapping: drag lock on is libinput's timeout mode.
        let lock = match keys.tap_drag_lock {
            Some(true) => DragLockState::EnabledTimeout,
            Some(false) => DragLockState::Disabled,
            None => device.config_tap_default_drag_lock_enabled(),
        };
        set!(
            device,
            "tap_drag_lock",
            device.config_tap_set_drag_lock_enabled(lock)
        );
        if let Some(map) = keys
            .tap_button_map
            .or_else(|| device.config_tap_default_button_map())
        {
            set!(
                device,
                "tap_button_map",
                device.config_tap_set_button_map(map)
            );
        }
    }
    let click = keys
        .click_method
        .or_else(|| device.config_click_default_method());
    if let Some(click) = click.filter(|click| device.config_click_methods().contains(click)) {
        set!(
            device,
            "click_method",
            device.config_click_set_method(click)
        );
    }
    let scroll = keys
        .scroll_method
        .or_else(|| device.config_scroll_default_method());
    if let Some(scroll) = scroll.filter(|scroll| device.config_scroll_methods().contains(scroll)) {
        set!(
            device,
            "scroll_method",
            device.config_scroll_set_method(scroll)
        );
    }
    if device.config_dwt_is_available() {
        let dwt = keys
            .disable_while_typing
            .unwrap_or_else(|| device.config_dwt_default_enabled());
        set!(
            device,
            "disable_while_typing",
            device.config_dwt_set_enabled(dwt)
        );
    }
}

/// The libinput device of a backend's `device`; none for another backend's.
fn libinput<D: 'static>(device: &D) -> Option<&Device> {
    let device: &dyn Any = device;
    device.downcast_ref::<Device>()
}

/// Reads the file at start and after each change of it, then turns Num Lock on or off once,
/// for the session (SE15). Without a path or a watch the defaults or the first read stay.
pub fn start(state: &mut State, handle: &LoopHandle<'static, State>) {
    let Some(root) = root() else {
        tracing::warn!("no configuration root: the input devices keep their defaults");
        return;
    };
    follow(state, handle, root.join(RELATIVE));
    state.set_num_lock_at_start();
}

/// Watches `path`, then reads it, so a write between the two is not missed.
pub fn follow(state: &mut State, handle: &LoopHandle<'static, State>, path: PathBuf) {
    follow_file(state, handle, path, State::load_input_settings);
}

/// Watches `path` with decision 27's watch, then loads it with `load`, and again after each
/// change of it; `windows.toml` is followed the same way.
pub(crate) fn follow_file(
    state: &mut State,
    handle: &LoopHandle<'static, State>,
    path: PathBuf,
    load: fn(&mut State, &Path),
) {
    if let Err(error) = watch(handle, path.clone(), load) {
        tracing::warn!(%error, ?path, "the file is read once: no live reload");
    }
    load(state, &path);
}

fn watch(
    handle: &LoopHandle<'static, State>,
    path: PathBuf,
    load: fn(&mut State, &Path),
) -> Result<(), Box<dyn std::error::Error>> {
    let mut watch = Watch::new(&path)?;
    let fd = watch.inotify.as_fd().try_clone_to_owned()?;
    handle.insert_source(
        Generic::new(fd, Interest::READ, Mode::Level),
        move |_, _, state| {
            if watch.changed() {
                load(state, &path);
            }
            Ok(PostAction::Continue)
        },
    )?;
    Ok(())
}

impl State {
    /// Reads `path` and applies it to every device. An absent file gives the defaults; a
    /// refused one changes nothing.
    pub fn load_input_settings(&mut self, path: &Path) {
        let read = read(path).map_err(|reason| reason.to_string());
        let settings = match read.and_then(|text| text.map(|text| parse(&text)).transpose()) {
            Ok(Some((settings, skipped))) => {
                for reason in skipped {
                    tracing::warn!(?path, %reason, "input.toml: a key is skipped");
                }
                settings
            }
            Ok(None) => {
                tracing::info!(?path, "no input.toml: the defaults apply");
                InputSettings::default()
            }
            Err(reason) => {
                tracing::warn!(?path, %reason, "input.toml refused: the settings stay");
                return;
            }
        };
        if settings != self.input_settings {
            self.input_settings = settings;
            self.apply_input_settings();
        }
    }

    /// `[keyboard] numlock_state`, set once at the start of the session; the default is off.
    pub(crate) fn set_num_lock_at_start(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let on = self.input_settings.num_lock.unwrap_or(false);
        let modifiers = keyboard.modifier_state();
        if modifiers.num_lock != on {
            keyboard.set_modifier_state(smithay::input::keyboard::ModifiersState {
                num_lock: on,
                ..modifiers
            });
        }
    }

    /// The scroll speed of the device that scrolled: its `scroll_factor`, else 1.
    pub(crate) fn scroll_factor<D: 'static>(&self, device: &D) -> f64 {
        libinput(device)
            .and_then(|device| {
                self.input_settings
                    .resolve(Class::of(device), &device.name())
                    .scroll_factor
            })
            .unwrap_or(1.0)
    }

    /// The output a tablet or a touchscreen is mapped to by its `map_to_output`, when that
    /// output is shown; none leaves the device's own mapping.
    pub(crate) fn mapped_output<D: 'static>(&self, device: &D) -> Option<Output> {
        let device = libinput(device)?;
        let name = self
            .input_settings
            .resolve(Class::of(device), &device.name())
            .map_to_output?;
        self.space
            .outputs()
            .find(|output| output.name() == name)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (InputSettings, Vec<String>) {
        parse(text).expect("read")
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let (settings, skipped) = parsed("schema = 1\n");
        assert!(skipped.is_empty());
        assert_eq!(settings, InputSettings::default());
        assert_eq!(settings.resolve(Class::Mouse, "a mouse"), Keys::default());
        assert_eq!(settings.resolve(Class::Other, "a tablet"), Keys::default());
        assert_eq!(
            settings.resolve(Class::Touchpad, "a touchpad"),
            TOUCHPAD_DEFAULTS
        );
    }

    #[test]
    fn a_file_without_its_schema_is_refused() {
        assert!(parse("").is_err());
        assert!(parse("schema = 2\n").is_err());
        assert!(parse("schema = \"1\"\n").is_err());
        assert!(parse("schema = 1\n[mouse\n").is_err());
    }

    #[test]
    fn every_key_is_read() {
        let (settings, skipped) = parsed(
            "schema = 1\n\
             [mouse]\nspeed = -0.5\nacceleration_profile = \"flat\"\nleft_handed = true\n\
             natural_scroll = true\nscroll_factor = 2\nmiddle_button_emulation = true\n\
             [touchpad]\nstate = \"disabled_on_external_mouse\"\ntap_to_click = false\n\
             tap_drag = false\ntap_drag_lock = true\ntap_button_map = \"left_middle_right\"\n\
             click_method = \"button_areas\"\nnatural_scroll = true\nscroll_method = \"edge\"\n\
             disable_while_typing = false\nspeed = 0.25\n\
             [keyboard]\nnumlock_state = \"boot_on\"\n\
             [[device]]\nname = \"Wacom Intuos\"\nmap_to_output = \"HDMI-A-1\"\nleft_handed = true\n",
        );
        assert!(skipped.is_empty(), "{skipped:?}");
        let mouse = Keys {
            speed: Some(-0.5),
            acceleration_profile: Some(AccelProfile::Flat),
            left_handed: Some(true),
            natural_scroll: Some(true),
            scroll_factor: Some(2.0),
            middle_button_emulation: Some(true),
            ..Keys::default()
        };
        assert_eq!(settings.resolve(Class::Mouse, "a mouse"), mouse);
        let touchpad = Keys {
            state: Some(SendEventsMode::DISABLED_ON_EXTERNAL_MOUSE),
            tap_to_click: Some(false),
            tap_drag: Some(false),
            tap_drag_lock: Some(true),
            tap_button_map: Some(TapButtonMap::LeftMiddleRight),
            click_method: Some(ClickMethod::ButtonAreas),
            natural_scroll: Some(true),
            scroll_method: Some(ScrollMethod::Edge),
            disable_while_typing: Some(false),
            speed: Some(0.25),
            ..Keys::default()
        };
        assert_eq!(settings.resolve(Class::Touchpad, "a touchpad"), touchpad);
        assert_eq!(settings.num_lock, Some(true));
        let tablet = Keys {
            map_to_output: Some("HDMI-A-1".into()),
            left_handed: Some(true),
            ..Keys::default()
        };
        assert_eq!(settings.resolve(Class::Other, "Wacom Intuos"), tablet);
    }

    /// A device's own keys win over its section, the others come from the section.
    #[test]
    fn a_device_is_laid_over_its_section() {
        let (settings, skipped) = parsed(
            "schema = 1\n[mouse]\nspeed = 0.5\nnatural_scroll = true\n\
             [[device]]\nname = \"Trackball\"\nspeed = -1\n",
        );
        assert!(skipped.is_empty(), "{skipped:?}");
        let trackball = settings.resolve(Class::Mouse, "Trackball");
        assert_eq!(trackball.speed, Some(-1.0));
        assert_eq!(trackball.natural_scroll, Some(true));
        assert_eq!(settings.resolve(Class::Mouse, "Other").speed, Some(0.5));
        // The section of a mouse is not a touchpad's.
        assert_eq!(settings.resolve(Class::Touchpad, "Pad").speed, None);
    }

    /// A key at its compiled default reads as the same settings as no key.
    #[test]
    fn a_key_at_its_default_changes_nothing() {
        let (settings, _) = parsed(
            "schema = 1\n[touchpad]\nstate = \"enabled\"\nclick_method = \"clickfinger\"\n\
             scroll_method = \"two_finger\"\ntap_to_click = true\ntap_drag = true\n\
             tap_drag_lock = false\ntap_button_map = \"left_right_middle\"\n",
        );
        let (none, _) = parsed("schema = 1\n");
        assert_eq!(
            settings.resolve(Class::Touchpad, "Pad"),
            none.resolve(Class::Touchpad, "Pad")
        );
    }

    /// Each refused key is skipped alone, with its reason; the rest apply.
    #[test]
    fn a_refused_key_is_skipped_alone() {
        let (settings, skipped) = parsed(
            "schema = 1\nwallpaper = 1\nkeyboard = 3\n\
             [mouse]\nspeed = 2.0\nnatural_scroll = \"yes\"\ntap_to_click = true\n\
             scroll_factor = 0\nleft_handed = true\n\
             [touchpad]\nclick_method = \"tap\"\nscroll_factor = 2\nnatural_scroll = true\n\
             [[device]]\nspeed = 1\n\
             [[device]]\nname = \"Pen\"\nmap_to_output = 1\nleft_handed = true\n\
             [[device]]\nname = \"Pen\"\nleft_handed = false\n",
        );
        assert_eq!(skipped.len(), 11, "{skipped:?}");
        let mouse = settings.resolve(Class::Mouse, "a mouse");
        assert_eq!(
            mouse,
            Keys {
                left_handed: Some(true),
                ..Keys::default()
            }
        );
        let touchpad = settings.resolve(Class::Touchpad, "Pad");
        assert_eq!(touchpad.natural_scroll, Some(true));
        assert_eq!(touchpad.click_method, Some(ClickMethod::Clickfinger));
        assert_eq!(touchpad.scroll_factor, None);
        let pen = settings.resolve(Class::Other, "Pen");
        assert_eq!(pen.left_handed, Some(true));
        assert_eq!(pen.map_to_output, None);
        assert_eq!(settings.num_lock, None);
    }

    #[test]
    fn num_lock_takes_two_values() {
        let (settings, _) = parsed("schema = 1\n[keyboard]\nnumlock_state = \"boot_off\"\n");
        assert_eq!(settings.num_lock, Some(false));
        let (settings, skipped) = parsed("schema = 1\n[keyboard]\nnumlock_state = \"last_boot\"\n");
        assert_eq!(settings.num_lock, None);
        assert_eq!(skipped.len(), 1);
    }
}
