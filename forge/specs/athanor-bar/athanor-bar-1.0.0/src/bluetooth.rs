//! BlueZ as the Bluetooth module shows it (doc_bar.md, BR3): the adapter's power, the paired
//! devices, the devices nearby while discovering, and the passkey a pairing asks to confirm.
//! Device names come from the radio: they are sanitised and bounded here.

use athanor_unit::text::{line, NAME_CHARS};
use glib::Variant;

use crate::props::{self, Objects, Props};

pub const BLUEZ: &str = "org.bluez";
/// BlueZ's object manager.
pub const ROOT: &str = "/";
pub const ADAPTER: &str = "org.bluez.Adapter1";
pub const DEVICE: &str = "org.bluez.Device1";
pub const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
pub const AGENT_MANAGER_PATH: &str = "/org/bluez";
pub const AGENT_IFACE: &str = "org.bluez.Agent1";
pub const AGENT_PATH: &str = "/os/athanor/Bar/BluezAgent";
/// The bar shows a passkey and asks yes or no; it never types one.
pub const CAPABILITY: &str = "DisplayYesNo";
pub const REJECTED: &str = "org.bluez.Error.Rejected";
pub const CANCELED: &str = "org.bluez.Error.Canceled";
pub const MAX_DEVICES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub label: String,
    pub icon: &'static str,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothState {
    pub adapter: String,
    pub powered: bool,
    pub discovering: bool,
    pub paired: Vec<Device>,
    /// Unpaired devices with a name, only while discovering.
    pub nearby: Vec<Device>,
}

/// BlueZ's `Icon` property, mapped to an icon the theme has; anything else is generic.
pub fn icon(bluez: &str) -> &'static str {
    match bluez {
        "audio-headphones" | "audio-headset" => "audio-headphones-symbolic",
        "audio-card" | "audio-speakers" => "audio-speakers-symbolic",
        "input-keyboard" => "input-keyboard-symbolic",
        "input-mouse" | "input-tablet" => "input-mouse-symbolic",
        "input-gaming" => "input-gaming-symbolic",
        "phone" => "phone-symbolic",
        "computer" => "computer-symbolic",
        _ => "bluetooth-symbolic",
    }
}

fn name(props: &Props, key: &str) -> Option<String> {
    props::value::<String>(props, key)
        .map(|text| line(&text, NAME_CHARS))
        .filter(|text| !text.trim().is_empty())
}

fn device(path: &str, props: &Props) -> Option<Device> {
    let paired = props::value::<bool>(props, "Paired").unwrap_or(false);
    // BlueZ fills `Alias` with the address when a device has no name: an unpaired device
    // needs a real `Name`, or the list fills with addresses.
    let label = if paired {
        name(props, "Alias").or_else(|| name(props, "Name"))
    } else {
        name(props, "Name")
    }?;
    Some(Device {
        path: path.to_owned(),
        label,
        icon: icon(&props::value::<String>(props, "Icon").unwrap_or_default()),
        paired,
        connected: props::value::<bool>(props, "Connected").unwrap_or(false),
    })
}

/// The module's state; `None` without an adapter, and then the module hides. With several
/// adapters the first by path is shown.
pub fn state(objects: &Objects) -> Option<BluetoothState> {
    let (adapter, adapter_props) = objects
        .iter()
        .find_map(|(path, interfaces)| Some((path, interfaces.get(ADAPTER)?)))?;
    let discovering = props::value::<bool>(adapter_props, "Discovering").unwrap_or(false);
    let (mut paired, mut nearby): (Vec<Device>, Vec<Device>) = objects
        .iter()
        .filter_map(|(path, interfaces)| {
            let props = interfaces.get(DEVICE)?;
            (props::path(props, "Adapter").as_deref() == Some(adapter.as_str())).then_some(())?;
            device(path, props)
        })
        .partition(|device| device.paired);
    paired.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.label.cmp(&b.label)));
    paired.truncate(MAX_DEVICES);
    if discovering {
        nearby.sort_by(|a, b| a.label.cmp(&b.label));
        nearby.truncate(MAX_DEVICES);
    } else {
        nearby.clear();
    }
    Some(BluetoothState {
        adapter: adapter.clone(),
        powered: props::value::<bool>(adapter_props, "Powered").unwrap_or(false),
        discovering,
        paired,
        nearby,
    })
}

pub fn module_icon(state: &BluetoothState) -> &'static str {
    if !state.powered {
        "bluetooth-disabled-symbolic"
    } else if state.paired.iter().any(|device| device.connected) {
        "bluetooth-active-symbolic"
    } else {
        "bluetooth-symbolic"
    }
}

/// A passkey as six digits; `None` beyond six digits, which Bluetooth never sends.
pub fn passkey_label(passkey: u32) -> Option<String> {
    (passkey <= 999_999).then(|| format!("{passkey:06}"))
}

/// The device and the passkey of `RequestConfirmation` `(ou)` or `DisplayPasskey` `(ouq)`.
pub fn device_and_passkey(params: &Variant) -> Option<(String, u32)> {
    if !(props::has_type(params, "(ou)") || props::has_type(params, "(ouq)")) {
        return None;
    }
    Some((
        params.try_child_value(0)?.str()?.to_owned(),
        params.try_child_value(1)?.get::<u32>()?,
    ))
}

/// The device of `RequestAuthorization` `(o)` or `AuthorizeService` `(os)`.
pub fn device_of(params: &Variant) -> Option<String> {
    if !(props::has_type(params, "(o)") || props::has_type(params, "(os)")) {
        return None;
    }
    Some(params.try_child_value(0)?.str()?.to_owned())
}

/// A device's label for the confirmation page.
pub fn device_label(objects: &Objects, path: &str) -> Option<String> {
    let props = props::lookup(objects, path, DEVICE)?;
    name(props, "Alias").or_else(|| name(props, "Name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::prelude::*;

    fn add_device(objects: &mut Objects, path: &str, pairs: Vec<(&str, Variant)>) {
        let mut props: Props = pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        props.insert("Adapter".into(), props::object_path("/org/bluez/hci0").unwrap());
        objects.entry(path.to_owned()).or_default().insert(DEVICE.into(), props);
    }

    fn world(discovering: bool) -> Objects {
        let mut objects = Objects::new();
        objects.entry("/org/bluez/hci0".into()).or_default().insert(
            ADAPTER.into(),
            Props::from([
                ("Powered".into(), true.to_variant()),
                ("Discovering".into(), discovering.to_variant()),
            ]),
        );
        add_device(&mut objects, "/org/bluez/hci0/dev_1", vec![
            ("Alias", "Keyboard".to_variant()), ("Paired", true.to_variant()), ("Icon", "input-keyboard".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_2", vec![
            ("Alias", "Headphones".to_variant()), ("Paired", true.to_variant()), ("Connected", true.to_variant()),
            ("Icon", "audio-headset".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_3", vec![
            ("Alias", "AA-BB".to_variant()), ("Name", "Phone".to_variant()), ("Icon", "phone".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_4", vec![("Alias", "CC-DD".to_variant())]);
        objects
    }

    #[test]
    fn no_adapter_no_module() {
        assert_eq!(state(&Objects::new()), None);
    }

    #[test]
    fn connected_first_and_nearby_only_while_discovering() {
        let idle = state(&world(false)).unwrap();
        let labels: Vec<_> = idle.paired.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(labels, ["Headphones", "Keyboard"]);
        assert_eq!(idle.paired[0].icon, "audio-headphones-symbolic");
        assert!(idle.nearby.is_empty());
        assert_eq!(module_icon(&idle), "bluetooth-active-symbolic");
        let scanning = state(&world(true)).unwrap();
        let nearby: Vec<_> = scanning.nearby.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(nearby, ["Phone"], "a device with no name is not listed");
    }

    #[test]
    fn a_device_name_is_sanitised() {
        let mut objects = world(false);
        add_device(&mut objects, "/org/bluez/hci0/dev_5", vec![
            ("Alias", format!("Evil\u{202e}{}\n", "x".repeat(200)).to_variant()), ("Paired", true.to_variant()),
            ("Icon", "../../etc/passwd".to_variant()),
        ]);
        add_device(&mut objects, "/org/bluez/hci0/dev_6", vec![("Alias", "\u{1b}\u{7}".to_variant()), ("Paired", true.to_variant())]);
        let paired = state(&objects).unwrap().paired;
        let evil = paired.iter().find(|d| d.path.ends_with("dev_5")).unwrap();
        assert!(evil.label.starts_with("Evilxxx"));
        assert_eq!(evil.label.chars().count(), NAME_CHARS);
        assert_eq!(evil.icon, "bluetooth-symbolic");
        assert!(paired.iter().all(|d| !d.path.ends_with("dev_6")), "an empty name is left out");
    }

    #[test]
    fn a_device_of_another_adapter_is_not_listed() {
        let mut objects = world(false);
        let mut props = Props::from([("Alias".into(), "Other".to_variant()), ("Paired".into(), true.to_variant())]);
        props.insert("Adapter".into(), props::object_path("/org/bluez/hci1").unwrap());
        objects.entry("/org/bluez/hci1/dev_9".into()).or_default().insert(DEVICE.into(), props);
        assert!(state(&objects).unwrap().paired.iter().all(|d| d.label != "Other"));
    }

    #[test]
    fn passkeys_are_six_digits() {
        assert_eq!(passkey_label(42).as_deref(), Some("000042"));
        assert_eq!(passkey_label(999_999).as_deref(), Some("999999"));
        assert_eq!(passkey_label(1_000_000), None);
        let dev = props::object_path("/org/bluez/hci0/dev_3").unwrap();
        let confirm = Variant::tuple_from_iter([dev.clone(), 123_456u32.to_variant()]);
        assert_eq!(device_and_passkey(&confirm), Some(("/org/bluez/hci0/dev_3".into(), 123_456)));
        let display = Variant::tuple_from_iter([dev.clone(), 7u32.to_variant(), 2u16.to_variant()]);
        assert_eq!(device_and_passkey(&display).map(|(_, key)| key), Some(7));
        assert_eq!(device_and_passkey(&("x", 1u32).to_variant()), None);
        assert_eq!(device_of(&Variant::tuple_from_iter([dev])).as_deref(), Some("/org/bluez/hci0/dev_3"));
        assert_eq!(device_label(&world(false), "/org/bluez/hci0/dev_3").as_deref(), Some("AA-BB"));
    }
}
