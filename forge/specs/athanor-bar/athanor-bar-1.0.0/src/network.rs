//! NetworkManager as the network module shows it (doc_bar.md, BR3): the wired state, the
//! Wi-Fi networks, the VPNs and airplane mode; the settings of a network to join; and the
//! requests of NetworkManager's secret agent. Names and SSIDs come from the network itself:
//! they are sanitised and bounded here, and the list of networks is bounded too.

use std::collections::BTreeMap;

use athanor_unit::text::{line, NAME_CHARS};
use glib::prelude::*;
use glib::Variant;

use crate::props::{self, Objects, Props};

pub const NM: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
/// NetworkManager's object manager.
pub const NM_ROOT: &str = "/org/freedesktop";
pub const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const SETTINGS_CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
pub const AGENT_MANAGER: &str = "org.freedesktop.NetworkManager.AgentManager";
pub const AGENT_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager/AgentManager";
pub const AGENT_IFACE: &str = "org.freedesktop.NetworkManager.SecretAgent";
/// Fixed by NetworkManager: it calls every agent at this path.
pub const AGENT_PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
pub const AGENT_ID: &str = "os.athanor.Bar";
pub const SECURITY_SETTING: &str = "802-11-wireless-security";
pub const MAX_NETWORKS: usize = 24;

const TYPE_ETHERNET: u32 = 1;
const TYPE_WIFI: u32 = 2;
const DEVICE_ACTIVATED: u32 = 100;
const ACTIVE_ACTIVATING: u32 = 1;
const ACTIVE_ACTIVATED: u32 = 2;
const AP_PRIVACY: u32 = 0x1;
const KEY_MGMT_PSK: u32 = 0x100;
const KEY_MGMT_8021X: u32 = 0x200;
const KEY_MGMT_SAE: u32 = 0x400;
const ALLOW_INTERACTION: u32 = 0x1;
const REQUEST_NEW: u32 = 0x2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    /// WPA or WPA2 with a password; `sae` for WPA3 Personal.
    Personal {
        sae: bool,
    },
    /// Enterprise, WEP, OWE: the bar does not join these; Settings does.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Link {
    Idle,
    Connecting,
    Connected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub ssid: Vec<u8>,
    pub label: String,
    pub strength: u8,
    pub security: Security,
    pub access_point: String,
    pub link: Link,
    /// The active connection, to disconnect.
    pub active: Option<String>,
    /// A saved connection for this SSID, to activate without a password.
    pub saved: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wifi {
    pub device: String,
    pub enabled: bool,
    pub networks: Vec<Network>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vpn {
    pub connection: String,
    pub label: String,
    pub active: Option<String>,
    pub link: Link,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkState {
    /// `None` without an Ethernet device; `Some(true)` when one is activated.
    pub wired: Option<bool>,
    pub wifi: Option<Wifi>,
    pub vpns: Vec<Vpn>,
    pub airplane: bool,
}

/// What the bar needs of a saved connection, from `GetSettings`. Never a secret:
/// `GetSettings` does not return them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub id: String,
    pub kind: String,
    pub ssid: Option<Vec<u8>>,
}

/// The reply of `Settings.Connection.GetSettings`, `(a{sa{sv}})`.
pub fn connection_info(reply: &Variant) -> Option<ConnectionInfo> {
    let (settings,) = reply.get::<(BTreeMap<String, Props>,)>()?;
    let connection = settings.get("connection")?;
    Some(ConnectionInfo {
        id: props::value::<String>(connection, "id").unwrap_or_default(),
        kind: props::value::<String>(connection, "type")?,
        ssid: settings
            .get("802-11-wireless")
            .and_then(|wireless| props::value::<Vec<u8>>(wireless, "ssid")),
    })
}

pub fn security(flags: u32, wpa: u32, rsn: u32) -> Security {
    let keys = wpa | rsn;
    if keys & KEY_MGMT_8021X != 0 {
        Security::Other
    } else if keys & (KEY_MGMT_PSK | KEY_MGMT_SAE) != 0 {
        Security::Personal {
            sae: keys & KEY_MGMT_PSK == 0,
        }
    } else if keys == 0 && flags & AP_PRIVACY == 0 {
        Security::Open
    } else {
        Security::Other
    }
}

fn link(state: u32) -> Link {
    match state {
        ACTIVE_ACTIVATING => Link::Connecting,
        ACTIVE_ACTIVATED => Link::Connected,
        _ => Link::Idle,
    }
}

/// The active connections: connection path -> (active path, link).
fn active_connections(objects: &Objects, manager: &Props) -> BTreeMap<String, (String, Link)> {
    props::paths(manager, "ActiveConnections")
        .into_iter()
        .filter_map(|active| {
            let props = props::lookup(objects, &active, ACTIVE)?;
            let connection = props::path(props, "Connection")?;
            let state = props::value::<u32>(props, "State").unwrap_or(0);
            Some((connection, (active, link(state))))
        })
        .collect()
}

fn device_type(objects: &Objects, device: &str) -> Option<u32> {
    props::value::<u32>(props::lookup(objects, device, DEVICE)?, "DeviceType")
}

fn networks(
    objects: &Objects,
    device: &str,
    connections: &BTreeMap<String, ConnectionInfo>,
    active: &BTreeMap<String, (String, Link)>,
) -> Vec<Network> {
    let saved_for = |ssid: &[u8]| {
        connections
            .iter()
            .find(|(_, info)| info.kind == "802-11-wireless" && info.ssid.as_deref() == Some(ssid))
            .map(|(path, _)| path.clone())
    };
    let Some(wireless) = props::lookup(objects, device, WIRELESS) else {
        return Vec::new();
    };
    let mut by_ssid: BTreeMap<Vec<u8>, Network> = BTreeMap::new();
    for access_point in props::paths(wireless, "AccessPoints") {
        let Some(ap) = props::lookup(objects, &access_point, ACCESS_POINT) else {
            continue;
        };
        let Some(ssid) = props::value::<Vec<u8>>(ap, "Ssid").filter(|ssid| !ssid.is_empty()) else {
            continue;
        };
        let label = line(&String::from_utf8_lossy(&ssid), NAME_CHARS);
        if label.trim().is_empty() {
            continue;
        }
        let saved = saved_for(&ssid);
        let (active_path, link) = saved
            .as_ref()
            .and_then(|path| active.get(path))
            .map_or((None, Link::Idle), |(path, link)| {
                (Some(path.clone()), *link)
            });
        let network = Network {
            label,
            strength: props::value::<u8>(ap, "Strength").unwrap_or(0).min(100),
            security: security(
                props::value::<u32>(ap, "Flags").unwrap_or(0),
                props::value::<u32>(ap, "WpaFlags").unwrap_or(0),
                props::value::<u32>(ap, "RsnFlags").unwrap_or(0),
            ),
            access_point,
            link,
            active: active_path,
            saved,
            ssid: ssid.clone(),
        };
        // Several access points of one network: one row, the strongest.
        match by_ssid.get(&ssid) {
            Some(held) if held.strength >= network.strength => {}
            _ => {
                by_ssid.insert(ssid, network);
            }
        }
    }
    let mut networks: Vec<Network> = by_ssid.into_values().collect();
    networks.sort_by(|a, b| {
        b.link
            .cmp(&a.link)
            .then(b.strength.cmp(&a.strength))
            .then_with(|| a.label.cmp(&b.label))
    });
    networks.truncate(MAX_NETWORKS);
    networks
}

/// The module's state; `None` when NetworkManager is absent, and then the module hides.
pub fn state(
    objects: &Objects,
    connections: &BTreeMap<String, ConnectionInfo>,
) -> Option<NetworkState> {
    let manager = props::lookup(objects, NM_PATH, NM)?;
    let wireless_enabled = props::value::<bool>(manager, "WirelessEnabled").unwrap_or(false);
    let wwan_enabled = props::value::<bool>(manager, "WwanEnabled").unwrap_or(false);
    let devices = props::paths(manager, "Devices");
    let active = active_connections(objects, manager);
    let wired: Vec<bool> = devices
        .iter()
        .filter(|device| device_type(objects, device) == Some(TYPE_ETHERNET))
        .map(|device| {
            props::lookup(objects, device, DEVICE)
                .and_then(|props| props::value::<u32>(props, "State"))
                == Some(DEVICE_ACTIVATED)
        })
        .collect();
    let wifi = devices
        .iter()
        .find(|device| device_type(objects, device) == Some(TYPE_WIFI))
        .map(|device| Wifi {
            device: device.clone(),
            enabled: wireless_enabled,
            networks: if wireless_enabled {
                networks(objects, device, connections, &active)
            } else {
                Vec::new()
            },
        });
    let mut vpns: Vec<Vpn> = connections
        .iter()
        .filter(|(_, info)| info.kind == "vpn" || info.kind == "wireguard")
        .map(|(path, info)| {
            let (active_path, link) = active
                .get(path)
                .map_or((None, Link::Idle), |(active, link)| {
                    (Some(active.clone()), *link)
                });
            Vpn {
                connection: path.clone(),
                label: line(&info.id, NAME_CHARS),
                active: active_path,
                link,
            }
        })
        .filter(|vpn| !vpn.label.trim().is_empty())
        .collect();
    vpns.sort_by(|a, b| a.label.cmp(&b.label));
    vpns.truncate(MAX_NETWORKS);
    Some(NetworkState {
        wired: (!wired.is_empty()).then(|| wired.iter().any(|&on| on)),
        wifi,
        vpns,
        airplane: !wireless_enabled && !wwan_enabled,
    })
}

pub fn signal_icon(strength: u8) -> &'static str {
    match strength {
        81.. => "network-wireless-signal-excellent-symbolic",
        56..=80 => "network-wireless-signal-good-symbolic",
        31..=55 => "network-wireless-signal-ok-symbolic",
        6..=30 => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

/// The module's icon: the best link wins, wired over Wi-Fi.
pub fn icon(state: &NetworkState) -> &'static str {
    if state.airplane {
        return "airplane-mode-symbolic";
    }
    if state.wired == Some(true) {
        return "network-wired-symbolic";
    }
    let best = state.wifi.as_ref().and_then(|wifi| {
        wifi.networks
            .iter()
            .find(|network| network.link != Link::Idle)
    });
    match (best, &state.wifi) {
        (Some(network), _) if network.link == Link::Connected => signal_icon(network.strength),
        (Some(_), _) => "network-wireless-acquiring-symbolic",
        (None, Some(wifi)) if !wifi.enabled => "network-wireless-disabled-symbolic",
        (None, Some(_)) => "network-wireless-offline-symbolic",
        (None, None) => "network-wired-disconnected-symbolic",
    }
}

/// The settings of a network to join. The password, when there is one, goes to
/// NetworkManager inside these settings and nowhere else.
fn wifi_settings(network: &Network, password: Option<&str>) -> Variant {
    let mut settings: BTreeMap<String, Props> = BTreeMap::new();
    settings.insert(
        "connection".into(),
        Props::from([
            ("type".into(), "802-11-wireless".to_variant()),
            ("id".into(), network.label.to_variant()),
        ]),
    );
    settings.insert(
        "802-11-wireless".into(),
        Props::from([
            (
                "ssid".into(),
                Variant::array_from_fixed_array(&network.ssid),
            ),
            ("mode".into(), "infrastructure".to_variant()),
        ]),
    );
    if let (Security::Personal { sae }, Some(password)) = (network.security, password) {
        settings.insert(
            SECURITY_SETTING.into(),
            Props::from([
                (
                    "key-mgmt".into(),
                    if sae { "sae" } else { "wpa-psk" }.to_variant(),
                ),
                ("psk".into(), password.to_variant()),
            ]),
        );
    }
    settings.to_variant()
}

/// The arguments of `AddAndActivateConnection`, `(a{sa{sv}}oo)`.
pub fn add_and_activate(
    network: &Network,
    password: Option<&str>,
    device: &str,
) -> Option<Variant> {
    Some(Variant::tuple_from_iter([
        wifi_settings(network, password),
        props::object_path(device)?,
        props::object_path(&network.access_point)?,
    ]))
}

/// The arguments of `ActivateConnection`, `(ooo)`. A VPN passes `/` for the device and the
/// specific object: NetworkManager picks them.
pub fn activate(connection: &str, device: &str, specific: &str) -> Option<Variant> {
    Some(Variant::tuple_from_iter([
        props::object_path(connection)?,
        props::object_path(device)?,
        props::object_path(specific)?,
    ]))
}

/// The arguments of `DeactivateConnection`, `(o)`.
pub fn deactivate(active: &str) -> Option<Variant> {
    Some(Variant::tuple_from_iter([props::object_path(active)?]))
}

/// WPA2 Personal: 8 to 63 printable ASCII characters, or 64 hexadecimal digits. WPA3
/// Personal: any non-empty password of at most 128 characters without a control character.
pub fn password_acceptable(security: Security, password: &str) -> bool {
    match security {
        Security::Personal { sae: false } => {
            let printable = password.chars().all(|c| c.is_ascii_graphic() || c == ' ');
            (printable && (8..=63).contains(&password.len()))
                || (password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()))
        }
        Security::Personal { sae: true } => {
            !password.is_empty()
                && password.chars().count() <= 128
                && !password.chars().any(char::is_control)
        }
        Security::Open | Security::Other => false,
    }
}

/// A `GetSecrets` call of NetworkManager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretsRequest {
    pub connection: String,
    pub setting: String,
    pub flags: u32,
    pub label: String,
    pub key_mgmt: Option<String>,
}

/// What the password prompt shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordPrompt {
    pub label: String,
    pub security: Security,
    /// NetworkManager asks again because the password it had was refused.
    pub retry: bool,
}

/// The arguments of `GetSecrets`, `(a{sa{sv}}osasu)`.
pub fn secrets_request(params: &Variant) -> Option<SecretsRequest> {
    if !props::has_type(params, "(a{sa{sv}}osasu)") {
        return None;
    }
    let settings = params
        .try_child_value(0)?
        .get::<BTreeMap<String, Props>>()?;
    let connection = params.try_child_value(1)?.str()?.to_owned();
    let setting = params.try_child_value(2)?.str()?.to_owned();
    let flags = params.try_child_value(4)?.get::<u32>()?;
    let ssid = settings
        .get("802-11-wireless")
        .and_then(|wireless| props::value::<Vec<u8>>(wireless, "ssid"));
    let id = settings
        .get("connection")
        .and_then(|connection| props::value::<String>(connection, "id"))
        .unwrap_or_default();
    let label = match ssid {
        Some(ssid) => line(&String::from_utf8_lossy(&ssid), NAME_CHARS),
        None => line(&id, NAME_CHARS),
    };
    let key_mgmt = settings
        .get(SECURITY_SETTING)
        .and_then(|security| props::value::<String>(security, "key-mgmt"));
    Some(SecretsRequest {
        connection,
        setting,
        flags,
        label,
        key_mgmt,
    })
}

impl SecretsRequest {
    /// The prompt, when the bar can answer this request: a WPA or WPA3 Personal password, and
    /// NetworkManager allows interaction. Anything else is answered `NoSecrets`.
    pub fn prompt(&self) -> Option<PasswordPrompt> {
        if self.setting != SECURITY_SETTING
            || self.flags & ALLOW_INTERACTION == 0
            || self.label.trim().is_empty()
        {
            return None;
        }
        let security = match self.key_mgmt.as_deref() {
            Some("wpa-psk") => Security::Personal { sae: false },
            Some("sae") => Security::Personal { sae: true },
            _ => return None,
        };
        Some(PasswordPrompt {
            label: self.label.clone(),
            security,
            retry: self.flags & REQUEST_NEW != 0,
        })
    }
}

/// The reply of `GetSecrets`, `(a{sa{sv}})`: the password and nothing else.
pub fn secrets_reply(password: &str) -> Variant {
    let secrets: BTreeMap<String, Props> = BTreeMap::from([(
        SECURITY_SETTING.to_owned(),
        Props::from([("psk".to_owned(), password.to_variant())]),
    )]);
    (secrets,).to_variant()
}

/// The arguments of `CancelGetSecrets`, `(os)`: the connection and the setting.
pub fn cancel_request(params: &Variant) -> Option<(String, String)> {
    if !props::has_type(params, "(os)") {
        return None;
    }
    Some((
        params.try_child_value(0)?.str()?.to_owned(),
        params.try_child_value(1)?.str()?.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::VariantTy;

    fn op(path: &str) -> Variant {
        props::object_path(path).unwrap()
    }

    fn ops(paths: &[&str]) -> Variant {
        Variant::array_from_iter_with_type(
            VariantTy::OBJECT_PATH,
            paths.iter().map(|path| op(path)),
        )
    }

    fn map(pairs: Vec<(&str, Variant)>) -> Props {
        pairs
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect()
    }

    fn add(objects: &mut Objects, path: &str, interface: &str, pairs: Vec<(&str, Variant)>) {
        objects
            .entry(path.to_owned())
            .or_default()
            .insert(interface.to_owned(), map(pairs));
    }

    fn ap(objects: &mut Objects, path: &str, ssid: &[u8], strength: u8, rsn: u32) {
        add(
            objects,
            path,
            ACCESS_POINT,
            vec![
                ("Ssid", ssid.to_vec().to_variant()),
                ("Strength", strength.to_variant()),
                ("Flags", u32::from(rsn != 0).to_variant()),
                ("WpaFlags", 0u32.to_variant()),
                ("RsnFlags", rsn.to_variant()),
            ],
        );
    }

    /// A manager with a wired device, a Wi-Fi device and the access points `aps`.
    fn world(aps: &[&str], active: &[&str], wireless: bool) -> Objects {
        let mut objects = Objects::new();
        add(
            &mut objects,
            NM_PATH,
            NM,
            vec![
                ("WirelessEnabled", wireless.to_variant()),
                ("WwanEnabled", false.to_variant()),
                ("Devices", ops(&["/d/eth0", "/d/wlan0"])),
                ("ActiveConnections", ops(active)),
            ],
        );
        add(
            &mut objects,
            "/d/eth0",
            DEVICE,
            vec![
                ("DeviceType", 1u32.to_variant()),
                ("State", 100u32.to_variant()),
            ],
        );
        add(
            &mut objects,
            "/d/wlan0",
            DEVICE,
            vec![
                ("DeviceType", 2u32.to_variant()),
                ("State", 30u32.to_variant()),
            ],
        );
        add(
            &mut objects,
            "/d/wlan0",
            WIRELESS,
            vec![("AccessPoints", ops(aps))],
        );
        objects
    }

    fn saved(ssid: &[u8]) -> ConnectionInfo {
        ConnectionInfo {
            id: String::from_utf8_lossy(ssid).into_owned(),
            kind: "802-11-wireless".into(),
            ssid: Some(ssid.to_vec()),
        }
    }

    #[test]
    fn no_manager_no_module() {
        assert_eq!(state(&Objects::new(), &BTreeMap::new()), None);
    }

    #[test]
    fn the_active_network_comes_first_then_the_strongest() {
        let mut objects = world(&["/ap/1", "/ap/2", "/ap/3"], &["/active/1"], true);
        ap(&mut objects, "/ap/1", b"Cafe", 60, 0);
        ap(&mut objects, "/ap/2", b"Home", 40, KEY_MGMT_PSK);
        ap(&mut objects, "/ap/3", b"Lab", 90, KEY_MGMT_PSK);
        add(
            &mut objects,
            "/active/1",
            ACTIVE,
            vec![("Connection", op("/s/1")), ("State", 2u32.to_variant())],
        );
        let connections = BTreeMap::from([("/s/1".to_owned(), saved(b"Home"))]);
        let state = state(&objects, &connections).unwrap();
        assert_eq!(state.wired, Some(true));
        let wifi = state.wifi.unwrap();
        let labels: Vec<_> = wifi.networks.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Home", "Lab", "Cafe"]);
        assert_eq!(wifi.networks[0].link, Link::Connected);
        assert_eq!(wifi.networks[0].active.as_deref(), Some("/active/1"));
        assert_eq!(wifi.networks[0].saved.as_deref(), Some("/s/1"));
        assert_eq!(wifi.networks[2].security, Security::Open);
    }

    #[test]
    fn a_hostile_ssid_is_sanitised_and_an_empty_one_is_left_out() {
        let mut objects = world(&["/ap/1", "/ap/2", "/ap/3"], &[], true);
        ap(&mut objects, "/ap/1", b"\xffEvil\xe2\x80\xaeNet\n", 50, 0);
        ap(&mut objects, "/ap/2", b"", 99, 0);
        ap(&mut objects, "/ap/3", b"\x07\x1b\n", 98, 0);
        let networks = state(&objects, &BTreeMap::new())
            .unwrap()
            .wifi
            .unwrap()
            .networks;
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].label, "\u{fffd}EvilNet");
        assert_eq!(networks[0].ssid, b"\xffEvil\xe2\x80\xaeNet\n");
    }

    #[test]
    fn the_list_is_bounded() {
        let names: Vec<String> = (0..1000).map(|n| format!("/ap/{n}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut objects = world(&refs, &[], true);
        for (n, path) in names.iter().enumerate() {
            ap(
                &mut objects,
                path,
                format!("net{n}").as_bytes(),
                (n % 100) as u8,
                0,
            );
        }
        let long = "x".repeat(500);
        ap(&mut objects, "/ap/0", long.as_bytes(), 100, 0);
        let networks = state(&objects, &BTreeMap::new())
            .unwrap()
            .wifi
            .unwrap()
            .networks;
        assert_eq!(networks.len(), MAX_NETWORKS);
        assert_eq!(networks[0].label.chars().count(), NAME_CHARS);
    }

    #[test]
    fn duplicates_keep_the_strongest_access_point() {
        let mut objects = world(&["/ap/1", "/ap/2"], &[], true);
        ap(&mut objects, "/ap/1", b"Mesh", 30, 0);
        ap(&mut objects, "/ap/2", b"Mesh", 70, 0);
        let networks = state(&objects, &BTreeMap::new())
            .unwrap()
            .wifi
            .unwrap()
            .networks;
        assert_eq!(networks.len(), 1);
        assert_eq!(
            (networks[0].access_point.as_str(), networks[0].strength),
            ("/ap/2", 70)
        );
    }

    #[test]
    fn security_from_the_flags() {
        assert_eq!(security(0, 0, 0), Security::Open);
        assert_eq!(
            security(1, 0, KEY_MGMT_PSK),
            Security::Personal { sae: false }
        );
        assert_eq!(
            security(1, KEY_MGMT_PSK, 0),
            Security::Personal { sae: false }
        );
        assert_eq!(
            security(1, 0, KEY_MGMT_SAE),
            Security::Personal { sae: true }
        );
        assert_eq!(
            security(1, 0, KEY_MGMT_PSK | KEY_MGMT_SAE),
            Security::Personal { sae: false }
        );
        assert_eq!(security(1, 0, KEY_MGMT_8021X), Security::Other);
        assert_eq!(security(1, 0, 0), Security::Other, "WEP");
    }

    #[test]
    fn airplane_is_both_radios_off_and_hides_the_list() {
        let mut objects = world(&["/ap/1"], &[], false);
        ap(&mut objects, "/ap/1", b"Cafe", 60, 0);
        let state = state(&objects, &BTreeMap::new()).unwrap();
        assert!(state.airplane);
        assert!(state.wifi.as_ref().unwrap().networks.is_empty());
        assert_eq!(icon(&state), "airplane-mode-symbolic");
    }

    #[test]
    fn vpns_come_from_the_saved_connections() {
        let mut objects = world(&[], &["/active/9"], true);
        add(
            &mut objects,
            "/active/9",
            ACTIVE,
            vec![("Connection", op("/s/9")), ("State", 1u32.to_variant())],
        );
        let connections = BTreeMap::from([
            (
                "/s/9".to_owned(),
                ConnectionInfo {
                    id: "Office\u{202e}VPN".into(),
                    kind: "vpn".into(),
                    ssid: None,
                },
            ),
            (
                "/s/8".to_owned(),
                ConnectionInfo {
                    id: "Home WG".into(),
                    kind: "wireguard".into(),
                    ssid: None,
                },
            ),
            ("/s/7".to_owned(), saved(b"Cafe")),
        ]);
        let vpns = state(&objects, &connections).unwrap().vpns;
        assert_eq!(vpns.len(), 2);
        assert_eq!(
            (vpns[0].label.as_str(), vpns[0].link),
            ("Home WG", Link::Idle)
        );
        assert_eq!(
            (vpns[1].label.as_str(), vpns[1].link),
            ("OfficeVPN", Link::Connecting)
        );
        assert_eq!(vpns[1].active.as_deref(), Some("/active/9"));
    }

    #[test]
    fn the_settings_carry_the_ssid_as_bytes_and_the_key_only_when_secured() {
        let mut network = Network {
            ssid: b"Lab\xff".to_vec(),
            label: "Lab\u{fffd}".into(),
            strength: 80,
            security: Security::Personal { sae: true },
            access_point: "/ap/1".into(),
            link: Link::Idle,
            active: None,
            saved: None,
        };
        let args = add_and_activate(&network, Some("correct horse"), "/d/wlan0").unwrap();
        assert_eq!(args.type_().as_str(), "(a{sa{sv}}oo)");
        let settings = args
            .try_child_value(0)
            .unwrap()
            .get::<BTreeMap<String, Props>>()
            .unwrap();
        assert_eq!(
            props::value::<Vec<u8>>(&settings["802-11-wireless"], "ssid").unwrap(),
            b"Lab\xff"
        );
        assert_eq!(
            props::value::<String>(&settings[SECURITY_SETTING], "key-mgmt").as_deref(),
            Some("sae")
        );
        network.security = Security::Open;
        let open = add_and_activate(&network, None, "/d/wlan0").unwrap();
        let settings = open
            .try_child_value(0)
            .unwrap()
            .get::<BTreeMap<String, Props>>()
            .unwrap();
        assert!(!settings.contains_key(SECURITY_SETTING));
        assert_eq!(
            activate("/s/1", "/", "/").unwrap().type_().as_str(),
            "(ooo)"
        );
        assert!(deactivate("bad path").is_none());
    }

    #[test]
    fn password_rules() {
        let wpa2 = Security::Personal { sae: false };
        assert!(password_acceptable(wpa2, "12345678"));
        assert!(password_acceptable(wpa2, &"a".repeat(63)));
        assert!(password_acceptable(wpa2, &"0f".repeat(32)));
        assert!(!password_acceptable(wpa2, "1234567"));
        assert!(!password_acceptable(wpa2, &"g".repeat(64)));
        assert!(!password_acceptable(wpa2, "pässwörd1"));
        assert!(!password_acceptable(wpa2, "tab\there1"));
        let wpa3 = Security::Personal { sae: true };
        assert!(password_acceptable(wpa3, "pässwörd"));
        assert!(!password_acceptable(wpa3, ""));
        assert!(!password_acceptable(wpa3, "new\nline"));
        assert!(!password_acceptable(Security::Open, "12345678"));
    }

    fn request(key_mgmt: &str, setting: &str, flags: u32) -> Variant {
        let text = format!(
            "({{'connection': {{'id': <'Home'>, 'type': <'802-11-wireless'>}}, \
              '802-11-wireless': {{'ssid': <@ay [72, 111, 109, 101]>}}, \
              '802-11-wireless-security': {{'key-mgmt': <'{key_mgmt}'>}}}}, \
              objectpath '/s/1', '{setting}', @as [], uint32 {flags})"
        );
        Variant::parse(Some(VariantTy::new("(a{sa{sv}}osasu)").unwrap()), &text).unwrap()
    }

    #[test]
    fn a_secrets_request_prompts_only_for_a_personal_network_with_interaction() {
        let asked = secrets_request(&request("wpa-psk", SECURITY_SETTING, 0x3)).unwrap();
        assert_eq!(asked.connection, "/s/1");
        assert_eq!(
            asked.prompt(),
            Some(PasswordPrompt {
                label: "Home".into(),
                security: Security::Personal { sae: false },
                retry: true
            })
        );
        assert_eq!(
            secrets_request(&request("sae", SECURITY_SETTING, 0x1))
                .unwrap()
                .prompt()
                .unwrap()
                .security,
            Security::Personal { sae: true }
        );
        assert_eq!(
            secrets_request(&request("wpa-psk", SECURITY_SETTING, 0x0))
                .unwrap()
                .prompt(),
            None,
            "no interaction"
        );
        assert_eq!(
            secrets_request(&request("wpa-eap", SECURITY_SETTING, 0x1))
                .unwrap()
                .prompt(),
            None
        );
        assert_eq!(
            secrets_request(&request("wpa-psk", "vpn", 0x1))
                .unwrap()
                .prompt(),
            None
        );
        assert!(secrets_request(&("x",).to_variant()).is_none());
    }

    #[test]
    fn the_reply_carries_the_psk_and_nothing_else() {
        let reply = secrets_reply("hunter2hunter2");
        assert_eq!(reply.type_().as_str(), "(a{sa{sv}})");
        let (secrets,) = reply.get::<(BTreeMap<String, Props>,)>().unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[SECURITY_SETTING].len(), 1);
        assert_eq!(
            props::value::<String>(&secrets[SECURITY_SETTING], "psk").as_deref(),
            Some("hunter2hunter2")
        );
    }

    #[test]
    fn a_cancel_names_the_connection_and_the_setting() {
        let params = Variant::tuple_from_iter([op("/s/1"), SECURITY_SETTING.to_variant()]);
        assert_eq!(
            cancel_request(&params),
            Some(("/s/1".into(), SECURITY_SETTING.into()))
        );
        assert_eq!(
            cancel_request(&("/s/1", "x").to_variant()),
            None,
            "s is not o"
        );
    }
}
