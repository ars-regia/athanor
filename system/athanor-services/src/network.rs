//! NetworkManager as the network module shows it (doc_bar.md, BR3; doc_control_center.md,
//! CC3): the wired state, the Wi-Fi networks, the VPNs and airplane mode; joining a network;
//! and NetworkManager's secret agent. One model per process mirrors NetworkManager, is its
//! secret agent and publishes a [`NetworkState`]; the shell's surfaces only draw it. Names and
//! SSIDs come from the network itself: they are sanitised and bounded here, and the list of
//! networks is bounded too.
//!
//! The agent (`/org/freedesktop/NetworkManager/SecretAgent`, `os.athanor.Bar`) answers only
//! the current owner of `org.freedesktop.NetworkManager` (the mirror follows the owner across
//! a restart), every method of it, one request at a time, and only for a Wi-Fi personal
//! password the person types. The password goes from the shell's reply into NetworkManager's
//! and nowhere else: it is never logged, and neither is the settings dict a request carries.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use athanor_unit::text::{line, NAME_CHARS};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;
use zbus::message::Header;
use zbus::proxy::MethodFlags;
use zbus::zvariant::{Array, ObjectPath, Value};
use zbus::{Connection, DBusError, Message, Proxy};

use crate::mirror::{self, Snapshot, Source, TIMEOUT};
use crate::props::{self, Objects, Props};
use crate::runtime::{Bus, Buses};

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
/// How long a password request waits for the person. The bar's agent had no limit of its own
/// (a closed popover answered it); a request nobody can answer must not hold the agent for
/// ever, so the model gives it as long as a call that waits for polkit.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// A call that waits for the person: a connection polkit asks about. `AddAndActivate` and
/// `ActivateConnection` return when the activation has started, not when it has finished, so
/// this bounds a stalled NetworkManager and nothing else.
/// Every call to NetworkManager (but the agent's own) allows interactive authorisation, so
/// polkit can ask: a refused prompt is a refusal, not a silent failure.
const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(120);
/// What the shell has not taken yet: a request is refused rather than queued behind one.
const PROMPTS: usize = 4;

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
    /// Counts the commands of the person that NetworkManager refused or did not answer: the
    /// state is published again after each, so a reader that sees this change knows its last
    /// action did not complete. Beyond the plan's fields, so the bar keeps its "action did
    /// not complete" note. Always 0 in the value of [`state`]; the model fills it in.
    pub refused: u32,
}

/// What the bar needs of a saved connection, from `GetSettings`. Never a secret:
/// `GetSettings` does not return them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub id: String,
    pub kind: String,
    pub ssid: Option<Vec<u8>>,
}

/// The reply of `Settings.Connection.GetSettings`, `a{sa{sv}}`.
pub fn connection_info(settings: &BTreeMap<String, Props>) -> Option<ConnectionInfo> {
    let connection = settings.get("connection")?;
    Some(ConnectionInfo {
        id: props::get_str(connection, "id")
            .unwrap_or_default()
            .to_owned(),
        kind: props::get_str(connection, "type")?.to_owned(),
        ssid: settings
            .get("802-11-wireless")
            .and_then(|wireless| props::get_bytes(wireless, "ssid")),
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
    props::get_paths(manager, "ActiveConnections")
        .unwrap_or_default()
        .into_iter()
        .filter_map(|active| {
            let props = props::lookup(objects, &active, ACTIVE)?;
            let connection = props::get_path(props, "Connection")?;
            let state = props::get_u32(props, "State").unwrap_or(0);
            Some((connection, (active, link(state))))
        })
        .collect()
}

fn device_type(objects: &Objects, device: &str) -> Option<u32> {
    props::get_u32(props::lookup(objects, device, DEVICE)?, "DeviceType")
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
    for access_point in props::get_paths(wireless, "AccessPoints").unwrap_or_default() {
        let Some(ap) = props::lookup(objects, &access_point, ACCESS_POINT) else {
            continue;
        };
        let Some(ssid) = props::get_bytes(ap, "Ssid").filter(|ssid| !ssid.is_empty()) else {
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
            strength: props::get_u8(ap, "Strength").unwrap_or(0).min(100),
            security: security(
                props::get_u32(ap, "Flags").unwrap_or(0),
                props::get_u32(ap, "WpaFlags").unwrap_or(0),
                props::get_u32(ap, "RsnFlags").unwrap_or(0),
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
    let wireless_enabled = props::get_bool(manager, "WirelessEnabled").unwrap_or(false);
    let wwan_enabled = props::get_bool(manager, "WwanEnabled").unwrap_or(false);
    let devices = props::get_paths(manager, "Devices").unwrap_or_default();
    let active = active_connections(objects, manager);
    let wired: Vec<bool> = devices
        .iter()
        .filter(|device| device_type(objects, device) == Some(TYPE_ETHERNET))
        .map(|device| {
            props::lookup(objects, device, DEVICE).and_then(|props| props::get_u32(props, "State"))
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
        refused: 0,
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

/// `a{sa{sv}}`: the settings of a connection.
pub type Settings<'a> = HashMap<&'a str, HashMap<&'a str, Value<'a>>>;

/// The settings of a network to join. The password, when there is one, goes to
/// NetworkManager inside these settings and nowhere else.
pub fn wifi_settings<'a>(network: &'a Network, password: Option<&'a str>) -> Settings<'a> {
    let mut settings = Settings::new();
    settings.insert(
        "connection",
        HashMap::from([
            ("type", Value::from("802-11-wireless")),
            ("id", Value::from(network.label.as_str())),
        ]),
    );
    settings.insert(
        "802-11-wireless",
        HashMap::from([
            ("ssid", Value::from(Array::from(network.ssid.clone()))),
            ("mode", Value::from("infrastructure")),
        ]),
    );
    if let (Security::Personal { sae }, Some(password)) = (network.security, password) {
        settings.insert(
            SECURITY_SETTING,
            HashMap::from([
                ("key-mgmt", Value::from(if sae { "sae" } else { "wpa-psk" })),
                ("psk", Value::from(password)),
            ]),
        );
    }
    settings
}

/// The arguments of `AddAndActivateConnection`, `(a{sa{sv}}oo)`.
pub fn add_and_activate<'a>(
    network: &'a Network,
    password: Option<&'a str>,
    device: &'a str,
) -> Option<(Settings<'a>, ObjectPath<'a>, ObjectPath<'a>)> {
    Some((
        wifi_settings(network, password),
        ObjectPath::try_from(device).ok()?,
        ObjectPath::try_from(network.access_point.as_str()).ok()?,
    ))
}

/// The arguments of `ActivateConnection`, `(ooo)`. A VPN passes `/` for the device and the
/// specific object: NetworkManager picks them.
pub fn activate<'a>(
    connection: &'a str,
    device: &'a str,
    specific: &'a str,
) -> Option<(ObjectPath<'a>, ObjectPath<'a>, ObjectPath<'a>)> {
    Some((
        ObjectPath::try_from(connection).ok()?,
        ObjectPath::try_from(device).ok()?,
        ObjectPath::try_from(specific).ok()?,
    ))
}

/// The arguments of `DeactivateConnection`, `(o)`.
pub fn deactivate(active: &str) -> Option<(ObjectPath<'_>,)> {
    Some((ObjectPath::try_from(active).ok()?,))
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

/// A `GetSecrets` call of NetworkManager. Carries no secret: the connection's settings it
/// came with are reduced to the label and the key management.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretsRequest {
    pub connection: String,
    pub setting: String,
    pub flags: u32,
    pub label: String,
    pub key_mgmt: Option<String>,
}

/// What the password page shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordPage {
    pub label: String,
    pub security: Security,
    /// NetworkManager asks again because the password it had was refused.
    pub retry: bool,
}

/// The arguments of `GetSecrets`: the connection's settings, its path, the setting and the
/// flags.
pub fn secrets_request(
    settings: &BTreeMap<String, Props>,
    connection: &str,
    setting: &str,
    flags: u32,
) -> SecretsRequest {
    let ssid = settings
        .get("802-11-wireless")
        .and_then(|wireless| props::get_bytes(wireless, "ssid"));
    let id = settings
        .get("connection")
        .and_then(|connection| props::get_str(connection, "id"))
        .unwrap_or_default();
    let label = match ssid {
        Some(ssid) => line(&String::from_utf8_lossy(&ssid), NAME_CHARS),
        None => line(id, NAME_CHARS),
    };
    let key_mgmt = settings
        .get(SECURITY_SETTING)
        .and_then(|security| props::get_str(security, "key-mgmt"))
        .map(str::to_owned);
    SecretsRequest {
        connection: connection.to_owned(),
        setting: setting.to_owned(),
        flags,
        label,
        key_mgmt,
    }
}

impl SecretsRequest {
    /// The page, when the shell can answer this request: a WPA or WPA3 Personal password, and
    /// NetworkManager allows interaction. Anything else is answered `NoSecrets`.
    pub fn prompt(&self) -> Option<PasswordPage> {
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
        Some(PasswordPage {
            label: self.label.clone(),
            security,
            retry: self.flags & REQUEST_NEW != 0,
        })
    }
}

/// The reply of `GetSecrets`, `a{sa{sv}}`: the password and nothing else.
pub fn secrets_reply(password: &str) -> Option<BTreeMap<String, Props>> {
    let psk = Value::from(password).try_to_owned().ok()?;
    Some(BTreeMap::from([(
        SECURITY_SETTING.to_owned(),
        Props::from([("psk".to_owned(), psk)]),
    )]))
}

/// What the person can ask of NetworkManager. `Debug` never prints a password.
pub enum NetworkCommand {
    /// Registers the model's secret agent with NetworkManager, now and again with every new
    /// owner of it. NetworkManager takes one agent per user session, so the process that
    /// owns the prompts sends this and no other: a model that never gets it registers nothing
    /// and is never asked for a password.
    RegisterAgent,
    /// The Wi-Fi radio.
    Wireless(bool),
    /// Airplane mode: the Wi-Fi and the mobile radios together. Beyond the plan's commands,
    /// since the bar's airplane switch sets both and its Wi-Fi switch only one.
    Airplane(bool),
    /// Adds the network with its password, if it needs one, and activates it, on the Wi-Fi
    /// device of the last state.
    Join {
        network: Network,
        password: Option<String>,
    },
    Activate {
        connection: String,
        device: String,
        specific: String,
    },
    Deactivate(String),
    /// The settings of a saved connection, read now; `None` when they cannot be read.
    Details(String, oneshot::Sender<Option<ConnectionInfo>>),
}

impl std::fmt::Debug for NetworkCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RegisterAgent => f.write_str("RegisterAgent"),
            Self::Wireless(on) => f.debug_tuple("Wireless").field(on).finish(),
            Self::Airplane(on) => f.debug_tuple("Airplane").field(on).finish(),
            Self::Join { network, password } => f
                .debug_struct("Join")
                .field("network", network)
                .field("password", &password.as_ref().map(|_| "<redacted>"))
                .finish(),
            Self::Activate {
                connection,
                device,
                specific,
            } => f
                .debug_struct("Activate")
                .field("connection", connection)
                .field("device", device)
                .field("specific", specific)
                .finish(),
            Self::Deactivate(active) => f.debug_tuple("Deactivate").field(active).finish(),
            Self::Details(path, _) => f.debug_tuple("Details").field(path).finish(),
        }
    }
}

/// What NetworkManager asks the shell, through the agent: a password for the page
/// `request.prompt()` describes. A request nobody takes up is answered `NoSecrets`, so the
/// shell takes them from the receiver `spawn` returns for as long as it runs.
#[derive(Debug)]
pub struct PasswordPrompt {
    pub request: SecretsRequest,
    /// The password the person typed, or `None` when they cancelled. Dropping the sender
    /// answers `NoSecrets`.
    pub reply: oneshot::Sender<Option<String>>,
    /// Completes with `()` when the agent no longer waits (NetworkManager cancelled the
    /// request or left, or no one answered in time): close the page. Beyond the plan's
    /// fields; without it the page would outlive a cancelled request. Completes with an
    /// error, which is no news, once the request was answered.
    pub withdrawn: oneshot::Receiver<()>,
}

/// The request the agent waits on.
struct Open {
    id: u64,
    connection: String,
    setting: String,
    cancel: oneshot::Sender<()>,
}

/// What the agent and the model share.
struct Shared {
    open: Mutex<Option<Open>>,
    next: AtomicU64,
    prompts: mpsc::Sender<PasswordPrompt>,
}

impl Shared {
    /// Ends the request in progress, whatever it is.
    fn withdraw(&self) {
        if let Ok(mut open) = self.open.lock() {
            if let Some(open) = open.take() {
                // The request has already ended when the send fails.
                open.cancel.send(()).ok();
            }
        }
    }

    /// Ends the request in progress when it is for this connection and setting.
    fn withdraw_matching(&self, connection: &str, setting: &str) {
        if let Ok(mut open) = self.open.lock() {
            if open
                .as_ref()
                .is_some_and(|open| open.connection == connection && open.setting == setting)
            {
                if let Some(open) = open.take() {
                    open.cancel.send(()).ok();
                }
            }
        }
    }
}

/// Frees the request's place however it ends, unless another request took it since.
struct Slot<'a> {
    shared: &'a Shared,
    id: u64,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if let Ok(mut open) = self.shared.open.lock() {
            if open.as_ref().is_some_and(|open| open.id == self.id) {
                *open = None;
            }
        }
    }
}

#[derive(Debug, DBusError)]
#[zbus(
    prefix = "org.freedesktop.NetworkManager.SecretAgent",
    impl_display = true
)]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    PermissionDenied(String),
    NoSecrets(String),
    UserCanceled(String),
    AgentCanceled(String),
    Failed(String),
}

/// `org.freedesktop.NetworkManager.SecretAgent`, served on the system connection.
struct Agent {
    nm: watch::Receiver<Snapshot>,
    shared: Arc<Shared>,
    timeout: Duration,
}

impl Agent {
    /// Only the unique name that owns `org.freedesktop.NetworkManager` now may call the agent.
    fn require_nm(&self, header: &Header<'_>, method: &str) -> Result<(), AgentError> {
        let sender = header.sender().map(|sender| sender.as_str());
        if sender.is_some() && sender == self.nm.borrow().owner.as_deref() {
            return Ok(());
        }
        tracing::warn!(
            method,
            sender = sender.unwrap_or(""),
            "a secret agent call from a process that is not NetworkManager was refused"
        );
        Err(AgentError::PermissionDenied(
            "only NetworkManager may call this agent".to_owned(),
        ))
    }
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.SecretAgent")]
impl Agent {
    async fn get_secrets(
        &self,
        connection: BTreeMap<String, Props>,
        connection_path: ObjectPath<'_>,
        setting_name: String,
        _hints: Vec<String>,
        flags: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<BTreeMap<String, Props>, AgentError> {
        self.require_nm(&header, "GetSecrets")?;
        let request = secrets_request(&connection, connection_path.as_str(), &setting_name, flags);
        // The settings may carry secrets: nothing of them is kept past the request.
        drop(connection);
        if request.prompt().is_none() {
            return Err(AgentError::NoSecrets(
                "this agent answers only Wi-Fi personal passwords".to_owned(),
            ));
        }
        let (reply, answer) = oneshot::channel();
        let (withdrawn_tx, withdrawn) = oneshot::channel();
        let (cancel, cancelled) = oneshot::channel();
        let id = self.shared.next.fetch_add(1, Ordering::Relaxed);
        {
            let mut open = self
                .shared
                .open
                .lock()
                .map_err(|_| AgentError::Failed("the agent is broken".to_owned()))?;
            if open.is_some() {
                return Err(AgentError::NoSecrets("another request is open".to_owned()));
            }
            *open = Some(Open {
                id,
                connection: request.connection.clone(),
                setting: request.setting.clone(),
                cancel,
            });
        }
        let _slot = Slot {
            shared: &self.shared,
            id,
        };
        let prompt = PasswordPrompt {
            request,
            reply,
            withdrawn,
        };
        if self.shared.prompts.try_send(prompt).is_err() {
            return Err(AgentError::NoSecrets(
                "the shell shows no network module".to_owned(),
            ));
        }
        let outcome = tokio::select! {
            answer = timeout(self.timeout, answer) => Some(answer),
            _ = cancelled => None,
        };
        match outcome {
            Some(Ok(Ok(Some(password)))) => secrets_reply(&password)
                .ok_or_else(|| AgentError::Failed("the reply cannot be built".to_owned())),
            Some(Ok(Ok(None))) => Err(AgentError::UserCanceled("the person canceled".to_owned())),
            // The shell went away with the prompt open: no one will answer it.
            Some(Ok(Err(_))) => Err(AgentError::NoSecrets(
                "the shell dropped the prompt".to_owned(),
            )),
            Some(Err(_)) => {
                withdrawn_tx.send(()).ok();
                Err(AgentError::NoSecrets("no answer in time".to_owned()))
            }
            None => {
                withdrawn_tx.send(()).ok();
                Err(AgentError::AgentCanceled(
                    "the request was withdrawn".to_owned(),
                ))
            }
        }
    }

    fn cancel_get_secrets(
        &self,
        connection_path: ObjectPath<'_>,
        setting_name: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_nm(&header, "CancelGetSecrets")?;
        self.shared
            .withdraw_matching(connection_path.as_str(), &setting_name);
        Ok(())
    }

    // Nothing is ever stored here, so there is nothing to delete; and nothing is saved.
    fn save_secrets(
        &self,
        _connection: BTreeMap<String, Props>,
        _connection_path: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_nm(&header, "SaveSecrets")?;
        Err(AgentError::Failed("this agent keeps no secrets".to_owned()))
    }

    fn delete_secrets(
        &self,
        _connection: BTreeMap<String, Props>,
        _connection_path: ObjectPath<'_>,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.require_nm(&header, "DeleteSecrets")
    }
}

/// Starts the model on `handle`'s runtime. The state receiver holds `None` until
/// NetworkManager is on the bus, and ends when the model does: without the system bus it never
/// leaves `None`. Commands wait in an unbounded queue and run one at a time; a send fails only
/// when the model has ended. Take the password prompts from the last receiver, or a network
/// that needs a password cannot be joined from the shell.
pub fn spawn(
    handle: &Handle,
    buses: Buses,
) -> (
    watch::Receiver<Option<NetworkState>>,
    mpsc::UnboundedSender<NetworkCommand>,
    mpsc::Receiver<PasswordPrompt>,
) {
    start(handle, buses, REPLY_TIMEOUT)
}

fn start(
    handle: &Handle,
    buses: Buses,
    reply_timeout: Duration,
) -> (
    watch::Receiver<Option<NetworkState>>,
    mpsc::UnboundedSender<NetworkCommand>,
    mpsc::Receiver<PasswordPrompt>,
) {
    let (published, rx) = watch::channel(None);
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (prompts, prompt_rx) = mpsc::channel(PROMPTS);
    let model = handle.clone();
    handle.spawn(async move {
        let connection = match buses.connection(Bus::System).await {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!(error = %err, "no system bus; the network module is hidden");
                return;
            }
        };
        run(
            &model,
            connection,
            published,
            command_rx,
            prompts,
            reply_timeout,
        )
        .await;
    });
    (rx, commands, prompt_rx)
}

/// One step of the queue.
enum Job {
    Command(NetworkCommand),
    /// `RegisterWithCapabilities`, to this unique name.
    Register(String),
}

impl Job {
    /// Whether a failure is the person's action not completing.
    fn counts(&self) -> bool {
        matches!(self, Job::Command(command) if !matches!(command, NetworkCommand::Details(..)))
    }
}

/// A job being carried out.
type Running = Pin<Box<dyn Future<Output = zbus::Result<()>> + Send>>;

/// What the profile of a path says, read once per owner of NetworkManager.
type Info = (u64, String, Option<ConnectionInfo>);

async fn run(
    handle: &Handle,
    connection: Connection,
    published: watch::Sender<Option<NetworkState>>,
    mut commands: mpsc::UnboundedReceiver<NetworkCommand>,
    prompts: mpsc::Sender<PasswordPrompt>,
    reply_timeout: Duration,
) {
    let mut nm = mirror::spawn(handle, connection.clone(), NM, Source::Managed(NM_ROOT));
    let shared = Arc::new(Shared {
        open: Mutex::new(None),
        next: AtomicU64::new(0),
        prompts,
    });
    let agent = Agent {
        nm: nm.clone(),
        shared: shared.clone(),
        timeout: reply_timeout,
    };
    // The exit guard below removes it when the model ends.
    if let Err(err) = connection.object_server().at(AGENT_PATH, agent).await {
        tracing::error!(error = %err, "cannot export the NetworkManager secret agent; joining a saved network that needs a password will fail");
    }
    let mut exit = ExitGuard {
        handle: handle.clone(),
        connection: connection.clone(),
        shared: shared.clone(),
        owner: None,
    };
    let (info_tx, mut infos) = mpsc::unbounded_channel::<Info>();
    // The settings of each saved profile, read once: they tell a saved network from a new one,
    // and name the VPNs. `asked` keeps the profiles whose read was started in this generation,
    // answered or not, so that a profile of another user, which answers PermissionDenied each
    // time, is not asked again at every change of the mirror.
    // ponytail: read once per profile; follow Settings.Connection's Updated signal if renamed
    // profiles must show their new name without a restart of NetworkManager.
    let mut connections: BTreeMap<String, ConnectionInfo> = BTreeMap::new();
    let mut asked: BTreeSet<String> = BTreeSet::new();
    let mut waiting: VecDeque<Job> = VecDeque::new();
    let mut running: Option<(bool, Running)> = None;
    let mut refused = 0;
    let mut seen = 0;
    // Whether `RegisterAgent` came, and the generation of the owner the agent was queued for.
    let mut registering = false;
    let mut registered_for: Option<u64> = None;
    loop {
        // The mirror ends only after it has emptied itself, so its last state is published too.
        let mut alive = true;
        tokio::select! {
            changed = nm.changed() => alive = changed.is_ok(),
            command = commands.recv() => match command {
                Some(NetworkCommand::RegisterAgent) => registering = true,
                Some(command) => waiting.push_back(Job::Command(command)),
                None => return,
            },
            Some((generation, path, info)) = infos.recv() => {
                if generation == seen {
                    if let Some(info) = info {
                        connections.insert(path, info);
                    }
                }
            }
            result = async {
                match running.as_mut() {
                    Some((_, call)) => call.await,
                    None => std::future::pending().await,
                }
            }, if running.is_some() => {
                let counts = running.take().is_some_and(|(counts, _)| counts);
                if let Err(err) = result {
                    tracing::warn!(error = %err, "NetworkManager refused or did not answer");
                    if counts {
                        refused += 1;
                    }
                }
            }
        }
        let (owner, generation, profiles) = {
            let snapshot = nm.borrow();
            (
                snapshot.owner.clone(),
                snapshot.generation,
                snapshot
                    .objects
                    .iter()
                    .filter(|(_, interfaces)| interfaces.contains_key(SETTINGS_CONNECTION))
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>(),
            )
        };
        exit.owner.clone_from(&owner);
        if generation != seen {
            seen = generation;
            // A new owner of NetworkManager knows nothing of the request, the agent or the
            // profiles read from the old one.
            shared.withdraw();
            connections.clear();
            asked.clear();
            waiting.retain(|job| !matches!(job, Job::Register(_)));
            registered_for = None;
        }
        if registering && registered_for != Some(seen) {
            if let Some(owner) = &owner {
                registered_for = Some(seen);
                waiting.push_back(Job::Register(owner.clone()));
            }
        }
        connections.retain(|path, _| profiles.contains(path));
        asked.retain(|path| profiles.contains(path));
        if let Some(owner) = &owner {
            for path in &profiles {
                if !connections.contains_key(path) && asked.insert(path.clone()) {
                    let (connection, owner, path, infos) = (
                        connection.clone(),
                        owner.clone(),
                        path.clone(),
                        info_tx.clone(),
                    );
                    handle.spawn(async move {
                        let info = read_info(&connection, &owner, &path).await;
                        if info.is_none() {
                            tracing::warn!(
                                path,
                                "a NetworkManager profile has unreadable settings; it is not listed"
                            );
                        }
                        // The model has ended when the send fails.
                        infos.send((generation, path, info)).ok();
                    });
                }
            }
        }
        let current = state(&nm.borrow().objects, &connections);
        if running.is_none() {
            if let Some(job) = waiting.pop_front() {
                let context = Context {
                    connection: connection.clone(),
                    owner: owner.clone(),
                    device: current
                        .as_ref()
                        .and_then(|state| state.wifi.as_ref())
                        .map(|wifi| wifi.device.clone()),
                };
                running = Some((job.counts(), Box::pin(execute(context, job))));
            }
        }
        let next = current.map(|state| NetworkState { refused, ..state });
        published.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
        if !alive {
            return;
        }
    }
}

/// However `run` ends, the agent stops answering: the request in progress is cancelled, the
/// agent is unregistered from the owner of NetworkManager it registered with, and its object
/// is removed. Spawned on the model's handle: a drop is no place to wait.
struct ExitGuard {
    handle: Handle,
    connection: Connection,
    shared: Arc<Shared>,
    owner: Option<String>,
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        self.shared.withdraw();
        let connection = self.connection.clone();
        let owner = self.owner.take();
        self.handle.spawn(async move {
            // Nothing to remove when the agent never was served.
            connection
                .object_server()
                .remove::<Agent, _>(AGENT_PATH)
                .await
                .ok();
            let Some(owner) = owner else { return };
            let call = connection.call_method(
                Some(owner.as_str()),
                AGENT_MANAGER_PATH,
                Some(AGENT_MANAGER),
                "Unregister",
                &(),
            );
            if let Err(err) = no_answer(timeout(TIMEOUT, call).await) {
                tracing::warn!(error = %err, "the NetworkManager secret agent could not be unregistered on exit");
            }
        });
    }
}

/// What a job needs of the world as it was when the job started.
struct Context {
    connection: Connection,
    owner: Option<String>,
    /// The Wi-Fi device a join goes to.
    device: Option<String>,
}

fn no_answer<T>(result: Result<zbus::Result<T>, tokio::time::error::Elapsed>) -> zbus::Result<T> {
    result.unwrap_or_else(|_| Err(zbus::Error::Failure("no answer in time".into())))
}

fn failure(why: &str) -> zbus::Error {
    zbus::Error::Failure(why.to_owned())
}

/// The settings of the profile at `path`, or `None`.
async fn read_info(connection: &Connection, owner: &str, path: &str) -> Option<ConnectionInfo> {
    let call = connection.call_method(
        Some(owner),
        path,
        Some(SETTINGS_CONNECTION),
        "GetSettings",
        &(),
    );
    let reply: Message = no_answer(timeout(TIMEOUT, call).await).ok()?;
    let settings: BTreeMap<String, Props> = reply.body().deserialize().ok()?;
    connection_info(&settings)
}

impl Context {
    async fn set_manager(&self, property: &str, on: bool) -> zbus::Result<()> {
        let args = (NM, property, Value::from(on));
        let call = async {
            Proxy::new(&self.connection, NM, NM_PATH, props::PROPERTIES)
                .await?
                .call_with_flags::<_, _, ()>("Set", MethodFlags::AllowInteractiveAuth.into(), &args)
                .await
        };
        no_answer(timeout(TIMEOUT, call).await).map(|_| ())
    }
}

async fn execute(context: Context, job: Job) -> zbus::Result<()> {
    match job {
        Job::Command(command) => command_of(&context, command).await,
        Job::Register(owner) => register(&context.connection, &owner).await,
    }
}

async fn command_of(context: &Context, command: NetworkCommand) -> zbus::Result<()> {
    match command {
        // Taken by the model's loop, which never queues it.
        NetworkCommand::RegisterAgent => Ok(()),
        NetworkCommand::Wireless(on) => context.set_manager("WirelessEnabled", on).await,
        NetworkCommand::Airplane(on) => {
            let wireless = context.set_manager("WirelessEnabled", !on).await;
            let wwan = context.set_manager("WwanEnabled", !on).await;
            wireless.and(wwan)
        }
        NetworkCommand::Join { network, password } => {
            let device = context
                .device
                .as_deref()
                .ok_or_else(|| failure("no Wi-Fi device"))?;
            let args = add_and_activate(&network, password.as_deref(), device)
                .ok_or_else(|| failure("not an object path"))?;
            let call = async {
                Proxy::new(&context.connection, NM, NM_PATH, NM)
                    .await?
                    .call_with_flags::<_, _, ()>(
                        "AddAndActivateConnection",
                        MethodFlags::AllowInteractiveAuth.into(),
                        &args,
                    )
                    .await
            };
            no_answer(timeout(INTERACTIVE_TIMEOUT, call).await).map(|_| ())
        }
        NetworkCommand::Activate {
            connection,
            device,
            specific,
        } => {
            let args = activate(&connection, &device, &specific)
                .ok_or_else(|| failure("not an object path"))?;
            let call = async {
                Proxy::new(&context.connection, NM, NM_PATH, NM)
                    .await?
                    .call_with_flags::<_, _, ()>(
                        "ActivateConnection",
                        MethodFlags::AllowInteractiveAuth.into(),
                        &args,
                    )
                    .await
            };
            no_answer(timeout(INTERACTIVE_TIMEOUT, call).await).map(|_| ())
        }
        NetworkCommand::Deactivate(active) => {
            let args = deactivate(&active).ok_or_else(|| failure("not an object path"))?;
            let call = async {
                Proxy::new(&context.connection, NM, NM_PATH, NM)
                    .await?
                    .call_with_flags::<_, _, ()>(
                        "DeactivateConnection",
                        MethodFlags::AllowInteractiveAuth.into(),
                        &args,
                    )
                    .await
            };
            no_answer(timeout(INTERACTIVE_TIMEOUT, call).await).map(|_| ())
        }
        NetworkCommand::Details(path, reply) => {
            let info = match &context.owner {
                Some(owner) => read_info(&context.connection, owner, &path).await,
                None => None,
            };
            // The asker has stopped waiting when the send fails.
            reply.send(info).ok();
            Ok(())
        }
    }
}

/// `RegisterWithCapabilities`, to this owner: no capability, as the bar's agent had none.
async fn register(connection: &Connection, owner: &str) -> zbus::Result<()> {
    let call = connection.call_method(
        Some(owner),
        AGENT_MANAGER_PATH,
        Some(AGENT_MANAGER),
        "RegisterWithCapabilities",
        &(AGENT_ID, 0u32),
    );
    no_answer(timeout(TIMEOUT, call).await).map(|_| ())
}
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::runtime::Handle;
    use zbus::message::Header;
    use zbus::zvariant::{Array, ObjectPath, OwnedObjectPath, OwnedValue, Value};
    use zbus::{Connection, Message, Proxy};

    use super::*;
    use crate::testbus::{self, TestBus};

    fn owned(value: Value<'_>) -> OwnedValue {
        value.try_to_owned().expect("owned value")
    }

    fn ops(paths: &[&str]) -> Value<'static> {
        Value::from(Array::from(
            paths
                .iter()
                .map(|path| ObjectPath::try_from(path.to_string()).expect("path"))
                .collect::<Vec<_>>(),
        ))
    }

    fn bytes(data: &[u8]) -> Value<'static> {
        Value::from(Array::from(data.to_vec()))
    }

    fn op(path: &str) -> Value<'static> {
        Value::from(ObjectPath::try_from(path.to_string()).expect("path"))
    }

    fn map(pairs: Vec<(&str, Value<'_>)>) -> Props {
        pairs
            .into_iter()
            .map(|(name, value)| (name.to_owned(), owned(value)))
            .collect()
    }

    fn add(objects: &mut Objects, path: &str, interface: &str, pairs: Vec<(&str, Value<'_>)>) {
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
                ("Ssid", bytes(ssid)),
                ("Strength", Value::from(strength)),
                ("Flags", Value::from(u32::from(rsn != 0))),
                ("WpaFlags", Value::from(0u32)),
                ("RsnFlags", Value::from(rsn)),
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
                ("WirelessEnabled", Value::from(wireless)),
                ("WwanEnabled", Value::from(false)),
                ("Devices", ops(&["/d/eth0", "/d/wlan0"])),
                ("ActiveConnections", ops(active)),
            ],
        );
        add(
            &mut objects,
            "/d/eth0",
            DEVICE,
            vec![
                ("DeviceType", Value::from(1u32)),
                ("State", Value::from(100u32)),
            ],
        );
        add(
            &mut objects,
            "/d/wlan0",
            DEVICE,
            vec![
                ("DeviceType", Value::from(2u32)),
                ("State", Value::from(30u32)),
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
            vec![("Connection", op("/s/1")), ("State", Value::from(2u32))],
        );
        let connections = BTreeMap::from([("/s/1".to_owned(), saved(b"Home"))]);
        let state = state(&objects, &connections).unwrap();
        assert_eq!(state.wired, Some(true));
        assert_eq!(state.refused, 0);
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
            vec![("Connection", op("/s/9")), ("State", Value::from(1u32))],
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
    fn a_property_of_the_wrong_type_reads_as_absent_instead_of_panicking() {
        let mut objects = world(&["/ap/1", "/ap/2"], &[], true);
        add(
            &mut objects,
            NM_PATH,
            NM,
            vec![
                ("WirelessEnabled", Value::from("yes")),
                ("Devices", Value::from(7u32)),
            ],
        );
        add(
            &mut objects,
            "/ap/1",
            ACCESS_POINT,
            vec![
                ("Ssid", Value::from("text")),
                ("Strength", Value::from("x")),
            ],
        );
        let state = state(&objects, &BTreeMap::new()).unwrap();
        assert!(state.wifi.is_none() && state.wired.is_none() && state.airplane);
        // With the devices readable again, an access point of the wrong type is skipped.
        let mut objects = world(&["/ap/1"], &[], true);
        add(
            &mut objects,
            "/ap/1",
            ACCESS_POINT,
            vec![("Ssid", Value::from("text"))],
        );
        let wifi = state_wifi(&objects);
        assert!(wifi.networks.is_empty());
    }

    fn state_wifi(objects: &Objects) -> Wifi {
        state(objects, &BTreeMap::new()).unwrap().wifi.unwrap()
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
        let (settings, device, access_point) =
            add_and_activate(&network, Some("correct horse"), "/d/wlan0").unwrap();
        assert_eq!(
            (device.as_str(), access_point.as_str()),
            ("/d/wlan0", "/ap/1")
        );
        assert_eq!(
            Vec::<u8>::try_from(settings["802-11-wireless"]["ssid"].clone()).unwrap(),
            b"Lab\xff"
        );
        assert_eq!(
            String::try_from(settings[SECURITY_SETTING]["key-mgmt"].clone()).unwrap(),
            "sae"
        );
        assert_eq!(
            String::try_from(settings[SECURITY_SETTING]["psk"].clone()).unwrap(),
            "correct horse"
        );
        network.security = Security::Open;
        let (open, _, _) = add_and_activate(&network, None, "/d/wlan0").unwrap();
        assert!(!open.contains_key(SECURITY_SETTING));
        assert_eq!(open.len(), 2, "connection and 802-11-wireless only");
        assert!(activate("/s/1", "/", "/").is_some());
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

    fn request(key_mgmt: &str, setting: &str, flags: u32) -> SecretsRequest {
        let settings = BTreeMap::from([
            (
                "connection".to_owned(),
                map(vec![
                    ("id", Value::from("Home")),
                    ("type", Value::from("802-11-wireless")),
                ]),
            ),
            (
                "802-11-wireless".to_owned(),
                map(vec![("ssid", bytes(b"Home"))]),
            ),
            (
                SECURITY_SETTING.to_owned(),
                map(vec![("key-mgmt", Value::from(key_mgmt))]),
            ),
        ]);
        secrets_request(&settings, "/s/1", setting, flags)
    }

    #[test]
    fn a_secrets_request_prompts_only_for_a_personal_network_with_interaction() {
        let asked = request("wpa-psk", SECURITY_SETTING, 0x3);
        assert_eq!(asked.connection, "/s/1");
        assert_eq!(
            asked.prompt(),
            Some(PasswordPage {
                label: "Home".into(),
                security: Security::Personal { sae: false },
                retry: true
            })
        );
        assert_eq!(
            request("sae", SECURITY_SETTING, 0x1)
                .prompt()
                .unwrap()
                .security,
            Security::Personal { sae: true }
        );
        assert_eq!(
            request("wpa-psk", SECURITY_SETTING, 0x0).prompt(),
            None,
            "no interaction"
        );
        assert_eq!(request("wpa-eap", SECURITY_SETTING, 0x1).prompt(), None);
        assert_eq!(request("wpa-psk", "vpn", 0x1).prompt(), None);
    }

    #[test]
    fn a_label_comes_from_the_id_when_the_ssid_is_of_the_wrong_type() {
        let settings = BTreeMap::from([
            (
                "connection".to_owned(),
                map(vec![("id", Value::from("Home"))]),
            ),
            (
                "802-11-wireless".to_owned(),
                map(vec![("ssid", Value::from("Home"))]),
            ),
        ]);
        assert_eq!(
            secrets_request(&settings, "/s/1", SECURITY_SETTING, 1).label,
            "Home"
        );
    }

    #[test]
    fn the_reply_carries_the_psk_and_nothing_else() {
        let reply = secrets_reply("hunter2hunter2").unwrap();
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[SECURITY_SETTING].len(), 1);
        assert_eq!(
            props::get_str(&reply[SECURITY_SETTING], "psk"),
            Some("hunter2hunter2")
        );
    }

    // ---- the model, against a NetworkManager on a private bus ----

    const ETH: &str = "/org/freedesktop/NetworkManager/Devices/1";
    const WLAN: &str = "/org/freedesktop/NetworkManager/Devices/2";
    const AP_HOME: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";
    const AP_LAB: &str = "/org/freedesktop/NetworkManager/AccessPoint/2";
    const SAVED: &str = "/org/freedesktop/NetworkManager/Settings/1";
    const BROKEN: &str = "/org/freedesktop/NetworkManager/Settings/2";
    const ACTIVE_PATH: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";

    /// What the fake NetworkManager saw.
    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        /// The unique name, id and capabilities of each agent registration.
        registered: Vec<(String, String, u32)>,
        unregistered: u32,
        get_settings: HashMap<String, u32>,
        /// Ssid, key-mgmt and psk of each `AddAndActivateConnection`.
        joined: Vec<(Vec<u8>, Option<String>, Option<String>)>,
        /// `DeactivateConnection` is refused.
        refuse_deactivate: bool,
        /// Methods and property sets that came with the interactive-authorisation flag.
        interactive: Vec<String>,
        /// ... and those that came without.
        plain: Vec<String>,
    }

    type Shared = Arc<Mutex<Log>>;

    fn note(log: &Shared, line: String) {
        log.lock().unwrap().calls.push(line);
    }

    fn flag(log: &Shared, header: &Header<'_>, what: &str) {
        let interactive = header
            .primary()
            .flags()
            .contains(zbus::message::Flags::AllowInteractiveAuth);
        let mut log = log.lock().unwrap();
        if interactive {
            &mut log.interactive
        } else {
            &mut log.plain
        }
        .push(what.to_owned());
    }

    struct FakeManager {
        wireless: bool,
        wwan: bool,
        log: Shared,
    }

    #[zbus::interface(name = "org.freedesktop.NetworkManager")]
    impl FakeManager {
        fn add_and_activate_connection(
            &self,
            connection: HashMap<String, HashMap<String, OwnedValue>>,
            device: ObjectPath<'_>,
            specific: ObjectPath<'_>,
            #[zbus(header)] header: Header<'_>,
        ) -> (OwnedObjectPath, OwnedObjectPath) {
            flag(&self.log, &header, "AddAndActivateConnection");
            let wireless = connection.get("802-11-wireless");
            let security = connection.get(SECURITY_SETTING);
            let text = |props: Option<&HashMap<String, OwnedValue>>, key: &str| {
                props
                    .and_then(|props| props.get(key))
                    .and_then(|value| String::try_from(value.try_clone().ok()?).ok())
            };
            let ssid = wireless
                .and_then(|props| props.get("ssid"))
                .and_then(|value| Vec::<u8>::try_from(value.try_clone().ok()?).ok())
                .unwrap_or_default();
            let mut log = self.log.lock().unwrap();
            log.joined
                .push((ssid, text(security, "key-mgmt"), text(security, "psk")));
            log.calls
                .push(format!("AddAndActivateConnection {device} {specific}"));
            (
                OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Settings/9")
                    .expect("path"),
                OwnedObjectPath::try_from(ACTIVE_PATH).expect("path"),
            )
        }
        fn activate_connection(
            &self,
            connection: ObjectPath<'_>,
            device: ObjectPath<'_>,
            specific: ObjectPath<'_>,
            #[zbus(header)] header: Header<'_>,
        ) -> OwnedObjectPath {
            flag(&self.log, &header, "ActivateConnection");
            note(
                &self.log,
                format!("ActivateConnection {connection} {device} {specific}"),
            );
            OwnedObjectPath::try_from(ACTIVE_PATH).expect("path")
        }
        fn deactivate_connection(
            &self,
            active: ObjectPath<'_>,
            #[zbus(header)] header: Header<'_>,
        ) -> zbus::fdo::Result<()> {
            flag(&self.log, &header, "DeactivateConnection");
            note(&self.log, format!("DeactivateConnection {active}"));
            if self.log.lock().unwrap().refuse_deactivate {
                return Err(zbus::fdo::Error::Failed("refused".into()));
            }
            Ok(())
        }
        #[zbus(property)]
        fn wireless_enabled(&self) -> bool {
            self.wireless
        }
        #[zbus(property)]
        fn set_wireless_enabled(&mut self, on: bool, #[zbus(header)] header: Option<Header<'_>>) {
            if let Some(header) = header {
                flag(&self.log, &header, "WirelessEnabled");
            }
            note(&self.log, format!("WirelessEnabled {on}"));
            self.wireless = on;
        }
        #[zbus(property)]
        fn wwan_enabled(&self) -> bool {
            self.wwan
        }
        #[zbus(property)]
        fn set_wwan_enabled(&mut self, on: bool) {
            note(&self.log, format!("WwanEnabled {on}"));
            self.wwan = on;
        }
        #[zbus(property)]
        fn devices(&self) -> Vec<OwnedObjectPath> {
            [ETH, WLAN]
                .map(|path| OwnedObjectPath::try_from(path).expect("path"))
                .to_vec()
        }
        #[zbus(property)]
        fn active_connections(&self) -> Vec<OwnedObjectPath> {
            Vec::new()
        }
    }

    struct FakeDevice {
        kind: u32,
        state: u32,
    }

    #[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
    impl FakeDevice {
        #[zbus(property)]
        fn device_type(&self) -> u32 {
            self.kind
        }
        #[zbus(property)]
        fn state(&self) -> u32 {
            self.state
        }
    }

    struct FakeWireless;

    #[zbus::interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
    impl FakeWireless {
        #[zbus(property)]
        fn access_points(&self) -> Vec<OwnedObjectPath> {
            [AP_HOME, AP_LAB]
                .map(|path| OwnedObjectPath::try_from(path).expect("path"))
                .to_vec()
        }
    }

    struct FakeAccessPoint {
        ssid: &'static [u8],
        strength: u8,
    }

    #[zbus::interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
    impl FakeAccessPoint {
        #[zbus(property)]
        fn ssid(&self) -> Vec<u8> {
            self.ssid.to_vec()
        }
        #[zbus(property)]
        fn strength(&self) -> u8 {
            self.strength
        }
        #[zbus(property)]
        fn flags(&self) -> u32 {
            1
        }
        #[zbus(property)]
        fn wpa_flags(&self) -> u32 {
            0
        }
        #[zbus(property)]
        fn rsn_flags(&self) -> u32 {
            KEY_MGMT_PSK
        }
    }

    struct FakeSettings {
        path: &'static str,
        fail: bool,
        log: Shared,
    }

    #[zbus::interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
    impl FakeSettings {
        fn get_settings(&self) -> zbus::fdo::Result<HashMap<String, HashMap<String, OwnedValue>>> {
            *self
                .log
                .lock()
                .unwrap()
                .get_settings
                .entry(self.path.to_owned())
                .or_default() += 1;
            if self.fail {
                return Err(zbus::fdo::Error::AccessDenied("another user's".into()));
            }
            Ok(HashMap::from([
                (
                    "connection".to_owned(),
                    HashMap::from([
                        ("id".to_owned(), owned(Value::from("Home"))),
                        ("type".to_owned(), owned(Value::from("802-11-wireless"))),
                    ]),
                ),
                (
                    "802-11-wireless".to_owned(),
                    HashMap::from([("ssid".to_owned(), owned(bytes(b"Home")))]),
                ),
            ]))
        }
    }

    struct FakeAgentManager {
        log: Shared,
    }

    #[zbus::interface(name = "org.freedesktop.NetworkManager.AgentManager")]
    impl FakeAgentManager {
        fn register_with_capabilities(
            &self,
            identifier: String,
            capabilities: u32,
            #[zbus(header)] header: Header<'_>,
        ) {
            let sender = header.sender().map(ToString::to_string).unwrap_or_default();
            self.log
                .lock()
                .unwrap()
                .registered
                .push((sender, identifier, capabilities));
        }
        fn unregister(&self) {
            self.log.lock().unwrap().unregistered += 1;
        }
    }

    /// NetworkManager with a wired device, a Wi-Fi device with the access points `Home` (40)
    /// and `Lab` (90), a readable saved profile for `Home` and one that refuses `GetSettings`.
    async fn serve(bus: &TestBus, log: &Shared) -> Connection {
        let build = bus
            .builder()
            .serve_at("/org/freedesktop", zbus::fdo::ObjectManager)
            .expect("object manager")
            .serve_at(
                NM_PATH,
                FakeManager {
                    wireless: true,
                    wwan: true,
                    log: log.clone(),
                },
            )
            .expect("manager")
            .serve_at(AGENT_MANAGER_PATH, FakeAgentManager { log: log.clone() })
            .expect("agent manager")
            .serve_at(
                ETH,
                FakeDevice {
                    kind: 1,
                    state: 100,
                },
            )
            .expect("eth")
            .serve_at(WLAN, FakeDevice { kind: 2, state: 30 })
            .expect("wlan")
            .serve_at(WLAN, FakeWireless)
            .expect("wireless")
            .serve_at(
                AP_HOME,
                FakeAccessPoint {
                    ssid: b"Home",
                    strength: 40,
                },
            )
            .expect("home")
            .serve_at(
                AP_LAB,
                FakeAccessPoint {
                    ssid: b"Lab",
                    strength: 90,
                },
            )
            .expect("lab")
            .serve_at(
                SAVED,
                FakeSettings {
                    path: SAVED,
                    fail: false,
                    log: log.clone(),
                },
            )
            .expect("saved")
            .serve_at(
                BROKEN,
                FakeSettings {
                    path: BROKEN,
                    fail: true,
                    log: log.clone(),
                },
            )
            .expect("broken")
            .name(NM)
            .expect("name")
            .build();
        tokio::time::timeout(testbus::WAIT, build)
            .await
            .expect("NetworkManager connected in time")
            .expect("NetworkManager")
    }

    type States = watch::Receiver<Option<NetworkState>>;

    async fn wait_for(rx: &mut States, predicate: impl Fn(&NetworkState) -> bool) -> NetworkState {
        tokio::time::timeout(
            testbus::WAIT,
            rx.wait_for(|state| state.as_ref().is_some_and(&predicate)),
        )
        .await
        .expect("the model reached the expected state in time")
        .expect("the model is running")
        .clone()
        .expect("a state")
    }

    async fn until(log: &Shared, predicate: impl Fn(&Log) -> bool) {
        tokio::time::timeout(testbus::WAIT, async {
            while !predicate(&log.lock().unwrap()) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("NetworkManager saw what the test waits for");
    }

    struct Rig {
        bus: TestBus,
        nm: Connection,
        client: Connection,
        log: Shared,
        states: States,
        commands: mpsc::UnboundedSender<NetworkCommand>,
        prompts: mpsc::Receiver<PasswordPrompt>,
    }

    async fn rig(reply_timeout: Duration) -> Rig {
        let rig = started(reply_timeout, true).await;
        until(&rig.log, |log| !log.registered.is_empty()).await;
        rig
    }

    async fn started(reply_timeout: Duration, register: bool) -> Rig {
        let bus = TestBus::start();
        let log = Shared::default();
        let nm = serve(&bus, &log).await;
        let client = bus.client().await;
        let (states, commands, prompts) = start(
            &Handle::current(),
            Buses::with(client.clone()),
            reply_timeout,
        );
        if register {
            commands.send(NetworkCommand::RegisterAgent).unwrap();
        }
        let mut rig = Rig {
            bus,
            nm,
            client,
            log,
            states,
            commands,
            prompts,
        };
        wait_for(&mut rig.states, |state| {
            state
                .wifi
                .as_ref()
                .is_some_and(|wifi| !wifi.networks.is_empty())
        })
        .await;
        rig
    }

    /// A `GetSecrets` for `Home`, asking for a WPA password, as `caller` sends it to `agent`.
    fn get_secrets(
        caller: &Connection,
        agent: &Connection,
        flags: u32,
    ) -> tokio::task::JoinHandle<zbus::Result<Message>> {
        let (caller, name) = (
            caller.clone(),
            agent.unique_name().expect("unique name").to_string(),
        );
        tokio::spawn(async move {
            let proxy = Proxy::new(&caller, name, AGENT_PATH, AGENT_IFACE).await?;
            let settings: HashMap<&str, HashMap<&str, Value>> = HashMap::from([
                (
                    "connection",
                    HashMap::from([
                        ("id", Value::from("Home")),
                        ("type", Value::from("802-11-wireless")),
                    ]),
                ),
                ("802-11-wireless", HashMap::from([("ssid", bytes(b"Home"))])),
                (
                    SECURITY_SETTING,
                    HashMap::from([("key-mgmt", Value::from("wpa-psk"))]),
                ),
            ]);
            let path = ObjectPath::try_from(SAVED)?;
            proxy
                .call_method(
                    "GetSecrets",
                    &(settings, path, SECURITY_SETTING, Vec::<&str>::new(), flags),
                )
                .await
        })
    }

    /// The error name of a reply, `ok` when it is none.
    fn outcome(result: &zbus::Result<Message>) -> String {
        match result {
            Ok(_) => "ok".to_owned(),
            Err(zbus::Error::MethodError(name, _, _)) => name.to_string(),
            Err(err) => err.to_string(),
        }
    }

    /// The error name of another method of the agent, called by `caller` on `agent`.
    async fn agent_call(caller: &Connection, agent: &Connection, method: &str) -> String {
        let name = agent.unique_name().expect("unique name").to_string();
        let proxy = Proxy::new(caller, name, AGENT_PATH, AGENT_IFACE)
            .await
            .expect("proxy");
        let path = ObjectPath::try_from(SAVED).expect("path");
        let empty: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
        let result = match method {
            "CancelGetSecrets" => proxy.call_method(method, &(path, SECURITY_SETTING)).await,
            _ => proxy.call_method(method, &(empty, path)).await,
        };
        outcome(&result)
    }

    async fn next_prompt(prompts: &mut mpsc::Receiver<PasswordPrompt>) -> PasswordPrompt {
        tokio::time::timeout(testbus::WAIT, prompts.recv())
            .await
            .expect("a prompt in time")
            .expect("the model is running")
    }

    const DENIED: &str = "org.freedesktop.NetworkManager.SecretAgent.PermissionDenied";
    const NO_SECRETS: &str = "org.freedesktop.NetworkManager.SecretAgent.NoSecrets";

    #[tokio::test(flavor = "current_thread")]
    async fn one_wifi_device_and_two_access_points_publish_two_networks_sorted() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let state = wait_for(&mut rig.states, |state| {
            state
                .wifi
                .as_ref()
                .is_some_and(|wifi| wifi.networks.iter().any(|n| n.saved.is_some()))
        })
        .await;
        assert_eq!(state.wired, Some(true));
        assert!(!state.airplane && state.refused == 0);
        let wifi = state.wifi.unwrap();
        assert_eq!(wifi.device, WLAN);
        let labels: Vec<_> = wifi.networks.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Lab", "Home"], "the strongest first");
        assert_eq!(wifi.networks[1].saved.as_deref(), Some(SAVED));
        assert_eq!(wifi.networks[0].security, Security::Personal { sae: false });
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_model_that_was_not_asked_registers_no_agent() {
        let rig = started(REPLY_TIMEOUT, false).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(rig.log.lock().unwrap().registered.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_agent_registers_with_the_agent_manager_once() {
        let rig = rig(REPLY_TIMEOUT).await;
        let registered = rig.log.lock().unwrap().registered.clone();
        assert_eq!(
            registered,
            [(
                rig.client.unique_name().unwrap().to_string(),
                AGENT_ID.to_owned(),
                0
            )]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_agent_registers_again_with_a_new_owner_of_networkmanager() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        rig.nm.graceful_shutdown().await;
        wait_for(&mut rig.states, |_| true).await;
        let log = Shared::default();
        let nm = serve(&rig.bus, &log).await;
        until(&log, |log| !log.registered.is_empty()).await;
        assert_eq!(log.lock().unwrap().registered.len(), 1);
        drop(nm);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_profile_that_cannot_be_read_is_asked_once_not_in_a_loop() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        until(&rig.log, |log| log.get_settings.contains_key(BROKEN)).await;
        // Changes of the mirror that follow must not ask again.
        for on in [false, true, false] {
            rig.commands.send(NetworkCommand::Wireless(on)).unwrap();
            wait_for(&mut rig.states, |state| {
                state.wifi.as_ref().is_some_and(|wifi| wifi.enabled == on)
            })
            .await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let log = rig.log.lock().unwrap();
        assert_eq!(log.get_settings[BROKEN], 1);
        assert_eq!(log.get_settings[SAVED], 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn wireless_and_airplane_set_the_radios_and_a_refusal_is_counted() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        rig.commands.send(NetworkCommand::Wireless(false)).unwrap();
        let state = wait_for(&mut rig.states, |state| {
            state.wifi.as_ref().is_some_and(|wifi| !wifi.enabled)
        })
        .await;
        assert!(!state.airplane, "WWAN is still on");
        assert!(state.wifi.unwrap().networks.is_empty());
        rig.commands.send(NetworkCommand::Airplane(true)).unwrap();
        wait_for(&mut rig.states, |state| state.airplane).await;
        rig.log.lock().unwrap().refuse_deactivate = true;
        rig.commands
            .send(NetworkCommand::Deactivate(ACTIVE_PATH.into()))
            .unwrap();
        let state = wait_for(&mut rig.states, |state| state.refused == 1).await;
        assert!(state.airplane);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn join_activate_and_deactivate_reach_networkmanager() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let state = wait_for(&mut rig.states, |state| state.wifi.is_some()).await;
        let lab = state.wifi.unwrap().networks[0].clone();
        rig.commands
            .send(NetworkCommand::Join {
                network: lab,
                password: Some("correct horse".into()),
            })
            .unwrap();
        rig.commands
            .send(NetworkCommand::Activate {
                connection: SAVED.into(),
                device: WLAN.into(),
                specific: AP_HOME.into(),
            })
            .unwrap();
        rig.commands
            .send(NetworkCommand::Deactivate(ACTIVE_PATH.into()))
            .unwrap();
        until(&rig.log, |log| log.calls.len() >= 3).await;
        let log = rig.log.lock().unwrap();
        assert_eq!(
            log.joined,
            [(
                b"Lab".to_vec(),
                Some("wpa-psk".to_owned()),
                Some("correct horse".to_owned())
            )]
        );
        assert_eq!(
            log.calls,
            [
                format!("AddAndActivateConnection {WLAN} {AP_LAB}"),
                format!("ActivateConnection {SAVED} {WLAN} {AP_HOME}"),
                format!("DeactivateConnection {ACTIVE_PATH}"),
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn every_call_to_networkmanager_allows_interactive_authorization() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let state = wait_for(&mut rig.states, |state| state.wifi.is_some()).await;
        let lab = state.wifi.unwrap().networks[0].clone();
        rig.commands
            .send(NetworkCommand::Join {
                network: lab,
                password: None,
            })
            .unwrap();
        rig.commands
            .send(NetworkCommand::Activate {
                connection: SAVED.into(),
                device: WLAN.into(),
                specific: AP_HOME.into(),
            })
            .unwrap();
        rig.commands
            .send(NetworkCommand::Deactivate(ACTIVE_PATH.into()))
            .unwrap();
        rig.commands.send(NetworkCommand::Wireless(false)).unwrap();
        until(&rig.log, |log| log.interactive.len() + log.plain.len() >= 4).await;
        let log = rig.log.lock().unwrap();
        assert!(log.plain.is_empty(), "without the flag: {:?}", log.plain);
        assert_eq!(
            log.interactive,
            [
                "AddAndActivateConnection",
                "ActivateConnection",
                "DeactivateConnection",
                "WirelessEnabled"
            ]
        );
    }

    #[test]
    fn debug_of_a_command_never_shows_the_password() {
        let network = Network {
            ssid: b"Lab".to_vec(),
            label: "Lab".into(),
            strength: 50,
            security: Security::Personal { sae: false },
            access_point: AP_LAB.into(),
            link: Link::Idle,
            active: None,
            saved: None,
        };
        let command = NetworkCommand::Join {
            network,
            password: Some("hunter2".into()),
        };
        let shown = format!("{command:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("Lab"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_join_does_not_hold_the_radio_switch() {
        // AddAndActivateConnection returns when the activation starts, so the next command
        // runs at once; the model never waits for the connection to come up.
        let mut rig = rig(REPLY_TIMEOUT).await;
        let state = wait_for(&mut rig.states, |state| state.wifi.is_some()).await;
        let lab = state.wifi.unwrap().networks[0].clone();
        rig.commands
            .send(NetworkCommand::Join {
                network: lab,
                password: Some("correct horse".into()),
            })
            .unwrap();
        rig.commands.send(NetworkCommand::Wireless(false)).unwrap();
        wait_for(&mut rig.states, |state| {
            state.wifi.as_ref().is_some_and(|wifi| !wifi.enabled)
        })
        .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn details_read_the_settings_of_a_saved_profile() {
        let rig = rig(REPLY_TIMEOUT).await;
        let (reply, answer) = oneshot::channel();
        rig.commands
            .send(NetworkCommand::Details(SAVED.into(), reply))
            .unwrap();
        let info = tokio::time::timeout(testbus::WAIT, answer)
            .await
            .expect("in time")
            .expect("answered");
        assert_eq!(info, Some(saved(b"Home")));
        let (reply, answer) = oneshot::channel();
        rig.commands
            .send(NetworkCommand::Details(BROKEN.into(), reply))
            .unwrap();
        assert_eq!(answer.await.expect("answered"), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_call_from_a_process_that_is_not_networkmanager_is_refused_and_ends_nothing() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        // NetworkManager asks for a password: the request is open and answerable.
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        let prompt = next_prompt(&mut rig.prompts).await;
        let stranger = rig.bus.client().await;
        // Every method is refused for the stranger, and none of them touches the request.
        for method in ["SaveSecrets", "DeleteSecrets", "CancelGetSecrets"] {
            assert_eq!(
                agent_call(&stranger, &rig.client, method).await,
                DENIED,
                "{method}"
            );
        }
        let refused = get_secrets(&stranger, &rig.client, 0x1).await.unwrap();
        assert_eq!(outcome(&refused), DENIED);
        // The first request is still open: the stranger's cancel did not withdraw it.
        prompt.reply.send(Some("hunter2hunter2".into())).unwrap();
        let reply = asked.await.unwrap().unwrap();
        let answer: BTreeMap<String, Props> = reply.body().deserialize().unwrap();
        assert_eq!(
            props::get_str(&answer[SECURITY_SETTING], "psk"),
            Some("hunter2hunter2")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_getsecrets_from_a_stranger_shows_no_prompt() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let stranger = rig.bus.client().await;
        let refused = get_secrets(&stranger, &rig.client, 0x1).await.unwrap();
        assert_eq!(outcome(&refused), DENIED);
        assert!(
            rig.prompts.try_recv().is_err(),
            "a request that would otherwise show a prompt must not reach the shell"
        );
        // The same request from NetworkManager does reach it.
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        let prompt = next_prompt(&mut rig.prompts).await;
        assert_eq!(prompt.request.label, "Home");
        drop(prompt);
        assert_eq!(outcome(&asked.await.unwrap()), NO_SECRETS);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_dropped_prompt_answers_nosecrets_and_a_cancel_answers_usercanceled() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        drop(next_prompt(&mut rig.prompts).await);
        assert_eq!(outcome(&asked.await.unwrap()), NO_SECRETS);
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        next_prompt(&mut rig.prompts)
            .await
            .reply
            .send(None)
            .unwrap();
        assert_eq!(
            outcome(&asked.await.unwrap()),
            "org.freedesktop.NetworkManager.SecretAgent.UserCanceled"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unanswered_prompt_times_out_with_nosecrets_and_is_withdrawn() {
        let mut rig = rig(Duration::from_millis(200)).await;
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        let prompt = next_prompt(&mut rig.prompts).await;
        assert_eq!(outcome(&asked.await.unwrap()), NO_SECRETS);
        assert!(
            prompt.withdrawn.await.is_ok(),
            "the shell is told to close its page"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelgetsecrets_withdraws_the_prompt_and_answers_agentcanceled() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        let prompt = next_prompt(&mut rig.prompts).await;
        assert_eq!(
            agent_call(&rig.nm, &rig.client, "CancelGetSecrets").await,
            "ok"
        );
        assert_eq!(
            outcome(&asked.await.unwrap()),
            "org.freedesktop.NetworkManager.SecretAgent.AgentCanceled"
        );
        assert!(prompt.withdrawn.await.is_ok());
        // The slot is free again.
        let again = get_secrets(&rig.nm, &rig.client, 0x1);
        next_prompt(&mut rig.prompts)
            .await
            .reply
            .send(None)
            .unwrap();
        again.await.unwrap().ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_request_the_agent_cannot_answer_is_nosecrets_and_shows_nothing() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        // No interaction allowed.
        let refused = get_secrets(&rig.nm, &rig.client, 0).await.unwrap();
        assert_eq!(outcome(&refused), NO_SECRETS);
        assert!(rig.prompts.try_recv().is_err());
        assert_eq!(
            agent_call(&rig.nm, &rig.client, "SaveSecrets").await,
            "org.freedesktop.NetworkManager.SecretAgent.Failed"
        );
        assert_eq!(
            agent_call(&rig.nm, &rig.client, "DeleteSecrets").await,
            "ok"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_model_that_ends_unregisters_and_removes_the_agent() {
        let mut rig = rig(REPLY_TIMEOUT).await;
        let asked = get_secrets(&rig.nm, &rig.client, 0x1);
        let _prompt = next_prompt(&mut rig.prompts).await;
        drop(rig.commands);
        until(&rig.log, |log| log.unregistered == 1).await;
        assert_eq!(
            outcome(&asked.await.unwrap()),
            "org.freedesktop.NetworkManager.SecretAgent.AgentCanceled"
        );
        let gone = tokio::time::timeout(testbus::WAIT, async {
            loop {
                let seen = agent_call(&rig.nm, &rig.client, "DeleteSecrets").await;
                if seen != "ok" {
                    return seen;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the agent is no longer served");
        assert!(gone.contains("Unknown"), "{gone}");
    }
}
