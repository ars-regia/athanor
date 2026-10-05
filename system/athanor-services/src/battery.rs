//! UPower, the power profiles and the backlight, as the battery module shows them (doc_bar.md,
//! BR3; doc_control_center.md, CC3). The backlight is read from sysfs and written through
//! logind, so the process needs no write access to sysfs and no group. The model follows
//! UPower and the profiles daemon through two mirrors and publishes a [`BatteryState`].

use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use zbus::proxy::MethodFlags;
use zbus::zvariant::Value;
use zbus::{Connection, Proxy};

use crate::mirror::{self, Snapshot, Source, TIMEOUT};
use crate::props::{self, Props, PROPERTIES};
use crate::runtime::{Bus, Buses};

pub const UPOWER: &str = "org.freedesktop.UPower";
pub const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
pub const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
/// Owned by tuned-ppd and by power-profiles-daemon 0.20 and later.
pub const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
pub const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
/// In the order the popover lists them.
pub const PROFILE_NAMES: [&str; 3] = ["power-saver", "balanced", "performance"];
pub const BACKLIGHT_ROOT: &str = "/sys/class/backlight";
const LOGIN1: &str = "org.freedesktop.login1";
/// logind resolves `auto` to the caller's session.
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const SESSION: &str = "org.freedesktop.login1.Session";

const TYPE_BATTERY: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: f64,
    pub charge: Charge,
    /// Until empty while discharging, until full while charging; `None` when UPower does
    /// not know yet.
    pub seconds: Option<u64>,
}

/// The display device's battery; `None` when it is not a present battery, and then the
/// module hides (SH1).
pub fn battery(props: &Props) -> Option<Battery> {
    if props::get_u32(props, "Type") != Some(TYPE_BATTERY)
        || props::get_bool(props, "IsPresent") != Some(true)
    {
        return None;
    }
    let percent = props::get_f64(props, "Percentage")
        .filter(|p| p.is_finite())?
        .clamp(0.0, 100.0);
    let charge = match props::get_u32(props, "State") {
        Some(1) => Charge::Charging,
        Some(2 | 3) => Charge::Discharging,
        Some(4) => Charge::Full,
        _ => Charge::Unknown,
    };
    let key = match charge {
        Charge::Charging => Some("TimeToFull"),
        Charge::Discharging => Some("TimeToEmpty"),
        Charge::Full | Charge::Unknown => None,
    };
    let seconds = key
        .and_then(|key| props::get_i64(props, key))
        .and_then(|seconds| u64::try_from(seconds).ok())
        .filter(|&seconds| seconds > 0);
    Some(Battery {
        percent,
        charge,
        seconds,
    })
}

pub fn icon(battery: &Battery) -> &'static str {
    let charging = battery.charge == Charge::Charging;
    match (battery.charge, battery.percent) {
        (Charge::Full, _) => "battery-full-charged-symbolic",
        (_, p) if p >= 80.0 => {
            if charging {
                "battery-full-charging-symbolic"
            } else {
                "battery-full-symbolic"
            }
        }
        (_, p) if p >= 50.0 => {
            if charging {
                "battery-good-charging-symbolic"
            } else {
                "battery-good-symbolic"
            }
        }
        (_, p) if p >= 20.0 => {
            if charging {
                "battery-low-charging-symbolic"
            } else {
                "battery-low-symbolic"
            }
        }
        (_, p) if p >= 5.0 => {
            if charging {
                "battery-caution-charging-symbolic"
            } else {
                "battery-caution-symbolic"
            }
        }
        _ => {
            if charging {
                "battery-empty-charging-symbolic"
            } else {
                "battery-empty-symbolic"
            }
        }
    }
}

pub fn hours_minutes(seconds: u64) -> (u64, u64) {
    let minutes = seconds / 60;
    (minutes / 60, minutes % 60)
}

/// The profiles the daemon offers, in `PROFILE_NAMES` order, and the active one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profiles {
    pub offered: Vec<&'static str>,
    pub active: String,
}

/// The `Profile` of each entry of `Profiles`, `aa{sv}`; an entry of another shape is skipped.
fn offered_profiles(props: &Props) -> Option<Vec<String>> {
    let Value::Array(entries) = &**props.get("Profiles")? else {
        return None;
    };
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                let Value::Dict(dict) = entry else {
                    return None;
                };
                match dict.get::<&str, Value>(&"Profile") {
                    Ok(Some(Value::Str(name))) => Some(name.as_str().to_owned()),
                    _ => None,
                }
            })
            .collect(),
    )
}

/// `None` when the reply names no profile the bar knows.
pub fn profiles(props: &Props) -> Option<Profiles> {
    let offered = offered_profiles(props)?;
    let known: Vec<&'static str> = PROFILE_NAMES
        .into_iter()
        .filter(|name| offered.iter().any(|offered| offered == name))
        .collect();
    let active = props::get_str(props, "ActiveProfile")
        .filter(|active| known.contains(active))?
        .to_owned();
    Some(Profiles {
        offered: known,
        active,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backlight {
    /// The directory's name under `/sys/class/backlight`, as logind's `SetBrightness` takes it.
    pub name: String,
    pub max: u32,
    pub level: u32,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn read_u32(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The panel's backlight: firmware first, then platform, then raw, as the kernel documents
/// the preference; `None` without one, and then the popover shows no brightness.
pub fn read_backlight(root: &Path) -> Option<Backlight> {
    let rank = |kind: &str| match kind {
        "firmware" => Some(0),
        "platform" => Some(1),
        "raw" => Some(2),
        _ => None,
    };
    let mut found: Vec<(u8, Backlight)> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry
                .file_name()
                .into_string()
                .ok()
                .filter(|name| valid_name(name))?;
            let dir = entry.path();
            let kind = fs::read_to_string(dir.join("type")).ok()?;
            let rank = rank(kind.trim())?;
            let max = read_u32(&dir.join("max_brightness")).filter(|&max| max > 0)?;
            let level = read_u32(&dir.join("brightness"))?.min(max);
            Some((rank, Backlight { name, max, level }))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().next().map(|(_, backlight)| backlight)
}

impl Backlight {
    pub fn percent(&self) -> f64 {
        f64::from(self.level) * 100.0 / f64::from(self.max)
    }

    /// The level for `percent`, never 0: a slider at the left end must not turn the panel
    /// black.
    pub fn raw(&self, percent: f64) -> u32 {
        let percent = if percent.is_finite() {
            percent.clamp(0.0, 100.0)
        } else {
            100.0
        };
        // At most `max`, so the cast cannot truncate.
        ((percent * f64::from(self.max) / 100.0).round() as u32).clamp(1, self.max)
    }
}

/// What the battery module shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatteryState {
    pub battery: Option<Battery>,
    pub profiles: Option<Profiles>,
    pub backlight: Option<Backlight>,
    /// Counts the commands a service refused or did not answer: the state is published again
    /// after each, so a reader that sees this change knows its last action did not complete.
    /// Beyond the plan's three fields, so the bar keeps its "action did not complete" note.
    pub refused: u32,
}

pub enum BatteryCommand {
    SetProfile(String),
    SetBrightness { device: String, raw: u32 },
}

/// Starts the model on `handle`'s runtime. `backlight_dir` is the sysfs directory the
/// backlight is read from. The receiver ends when the model does: without the system bus it
/// never leaves its empty state.
pub fn spawn(
    handle: &Handle,
    buses: Buses,
    backlight_dir: PathBuf,
) -> (
    watch::Receiver<BatteryState>,
    mpsc::UnboundedSender<BatteryCommand>,
) {
    let (state, rx) = watch::channel(BatteryState::default());
    let (commands, command_rx) = mpsc::unbounded_channel();
    let model = handle.clone();
    handle.spawn(async move {
        let connection = match buses.connection(Bus::System).await {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!(error = %err, "no system bus; the battery module is hidden");
                return;
            }
        };
        run(&model, connection, backlight_dir, state, command_rx).await;
    });
    (rx, commands)
}

async fn run(
    handle: &Handle,
    connection: Connection,
    backlight_dir: PathBuf,
    state: watch::Sender<BatteryState>,
    mut commands: mpsc::UnboundedReceiver<BatteryCommand>,
) {
    let mut upower = mirror::spawn(
        handle,
        connection.clone(),
        UPOWER,
        Source::Fixed(vec![(DISPLAY_DEVICE, DEVICE_IFACE)]),
    );
    let mut daemon = mirror::spawn(
        handle,
        connection.clone(),
        PROFILES,
        Source::Fixed(vec![(PROFILES_PATH, PROFILES)]),
    );
    let mut refused = 0;
    // Commands wait here while one runs, so the channel is read at once; a
    // new brightness replaces a waiting one, so a drag of the slider ends on its last level
    // and logind is not asked for every step.
    let mut waiting: VecDeque<BatteryCommand> = VecDeque::new();
    let mut running: Option<Running> = None;
    loop {
        // A mirror ends only after it has emptied itself, so its last state is published too.
        let mut alive = true;
        tokio::select! {
            changed = upower.changed() => alive = changed.is_ok(),
            changed = daemon.changed() => alive = changed.is_ok(),
            command = commands.recv() => match command {
                Some(command) => {
                    if matches!(command, BatteryCommand::SetBrightness { .. }) {
                        waiting.retain(|queued| !matches!(queued, BatteryCommand::SetBrightness { .. }));
                    }
                    waiting.push_back(command);
                }
                None => return,
            },
            result = async {
                match running.as_mut() {
                    Some(call) => call.await,
                    None => std::future::pending().await,
                }
            }, if running.is_some() => {
                running = None;
                if let Err(err) = result {
                    tracing::warn!(error = %err, "a system service refused or did not answer");
                    refused += 1;
                }
            }
        }
        if running.is_none() {
            running = waiting
                .pop_front()
                .map(|command| Box::pin(execute(connection.clone(), command)) as Running);
        }
        // Read again after every wake, a refused command included: the backlight is not
        // signalled, and a reader that sees `refused` change puts its controls back.
        let next = BatteryState {
            battery: current(&upower, DISPLAY_DEVICE, DEVICE_IFACE, battery),
            profiles: current(&daemon, PROFILES_PATH, PROFILES, profiles),
            backlight: read_backlight(&backlight_dir),
            refused,
        };
        state.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
        if !alive {
            return;
        }
    }
}

fn current<T>(
    mirror: &watch::Receiver<Snapshot>,
    path: &str,
    interface: &str,
    decode: impl Fn(&Props) -> Option<T>,
) -> Option<T> {
    decode(props::lookup(&mirror.borrow().objects, path, interface)?)
}

/// A command being carried out.
type Running = Pin<Box<dyn Future<Output = zbus::Result<()>> + Send>>;

/// Every call allows interactive authorisation, so polkit can ask.
async fn execute(connection: Connection, command: BatteryCommand) -> zbus::Result<()> {
    let interactive = MethodFlags::AllowInteractiveAuth.into();
    let call = async {
        match &command {
            BatteryCommand::SetProfile(profile) => {
                let proxy = Proxy::new(&connection, PROFILES, PROFILES_PATH, PROPERTIES).await?;
                proxy
                    .call_with_flags::<_, _, ()>(
                        "Set",
                        interactive,
                        &(PROFILES, "ActiveProfile", Value::from(profile.as_str())),
                    )
                    .await
            }
            BatteryCommand::SetBrightness { device, raw } => {
                let proxy = Proxy::new(&connection, LOGIN1, SESSION_PATH, SESSION).await?;
                proxy
                    .call_with_flags::<_, _, ()>(
                        "SetBrightness",
                        interactive,
                        &("backlight", device.as_str(), *raw),
                    )
                    .await
            }
        }
    };
    timeout(TIMEOUT, call)
        .await
        .map_err(|_| zbus::Error::Failure("no answer within 5 s".into()))?
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::runtime::Handle;
    use zbus::zvariant::{OwnedValue, Value};

    use super::*;
    use crate::testbus::{self, TestBus};

    fn device(entries: &[(&str, Value<'_>)]) -> Props {
        entries
            .iter()
            .map(|(name, value)| {
                (
                    (*name).to_owned(),
                    value.try_to_owned().expect("owned value"),
                )
            })
            .collect()
    }

    fn profile_list(names: &[&str]) -> Value<'static> {
        Value::new(
            names
                .iter()
                .map(|name| HashMap::from([("Profile", Value::from(name.to_string()))]))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_present_battery_is_read_and_anything_else_hides_the_module() {
        let on_battery = device(&[
            ("Type", Value::from(2u32)),
            ("IsPresent", Value::from(true)),
            ("Percentage", Value::from(72.0f64)),
            ("State", Value::from(2u32)),
            ("TimeToEmpty", Value::from(12300i64)),
            ("TimeToFull", Value::from(0i64)),
        ]);
        assert_eq!(
            battery(&on_battery),
            Some(Battery {
                percent: 72.0,
                charge: Charge::Discharging,
                seconds: Some(12300)
            })
        );
        assert_eq!(
            icon(&battery(&on_battery).unwrap()),
            "battery-good-symbolic"
        );
        assert_eq!(hours_minutes(12300), (3, 25));
        let absent = device(&[
            ("Type", Value::from(2u32)),
            ("IsPresent", Value::from(false)),
            ("Percentage", Value::from(0.0f64)),
        ]);
        assert_eq!(battery(&absent), None);
        let ups = device(&[
            ("Type", Value::from(3u32)),
            ("IsPresent", Value::from(true)),
            ("Percentage", Value::from(50.0f64)),
        ]);
        assert_eq!(battery(&ups), None);
        let charging = device(&[
            ("Type", Value::from(2u32)),
            ("IsPresent", Value::from(true)),
            ("Percentage", Value::from(130.0f64)),
            ("State", Value::from(1u32)),
            ("TimeToFull", Value::from(-5i64)),
        ]);
        let charging = battery(&charging).unwrap();
        assert_eq!((charging.percent, charging.seconds), (100.0, None));
        assert_eq!(icon(&charging), "battery-full-charging-symbolic");
    }

    #[test]
    fn a_property_of_the_wrong_type_hides_the_battery_instead_of_panicking() {
        let wrong = device(&[
            ("Type", Value::from(2u32)),
            ("IsPresent", Value::from("yes")),
            ("Percentage", Value::from(50u32)),
        ]);
        assert_eq!(battery(&wrong), None);
        assert_eq!(profiles(&device(&[("Profiles", Value::from(3u32))])), None);
    }

    #[test]
    fn profiles_keep_the_known_ones_in_order() {
        let props = device(&[
            ("ActiveProfile", Value::from("balanced")),
            (
                "Profiles",
                profile_list(&["performance", "balanced", "turbo"]),
            ),
        ]);
        assert_eq!(
            profiles(&props),
            Some(Profiles {
                offered: vec!["balanced", "performance"],
                active: "balanced".into()
            })
        );
        let unknown = device(&[
            ("ActiveProfile", Value::from("turbo")),
            ("Profiles", profile_list(&["turbo"])),
        ]);
        assert_eq!(profiles(&unknown), None);
        assert_eq!(
            profiles(&device(&[("ActiveProfile", Value::from("balanced"))])),
            None
        );
    }

    #[test]
    fn the_backlight_prefers_firmware_and_never_goes_to_zero() {
        let root =
            std::env::temp_dir().join(format!("athanor-services-backlight-{}", std::process::id()));
        let make = |name: &str, kind: &str, max: &str, level: &str| {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("type"), kind).unwrap();
            fs::write(dir.join("max_brightness"), max).unwrap();
            fs::write(dir.join("brightness"), level).unwrap();
        };
        make("intel_backlight", "raw\n", "1000\n", "600\n");
        make("acpi_video0", "firmware\n", "0\n", "0\n");
        make(".hidden", "firmware\n", "10\n", "5\n");
        make("nv_backlight", "platform\n", "100\n", "250\n");
        let backlight = read_backlight(&root).unwrap();
        assert_eq!(
            backlight,
            Backlight {
                name: "nv_backlight".into(),
                max: 100,
                level: 100
            }
        );
        assert_eq!(backlight.raw(0.0), 1);
        assert_eq!(backlight.raw(50.0), 50);
        assert_eq!(backlight.raw(f64::NAN), 100);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(read_backlight(&root), None);
    }

    /// UPower's display device, with the properties the model reads.
    struct FakeDevice;

    #[zbus::interface(name = "org.freedesktop.UPower.Device")]
    impl FakeDevice {
        #[zbus(property, name = "Type")]
        fn kind(&self) -> u32 {
            2
        }
        #[zbus(property)]
        fn is_present(&self) -> bool {
            true
        }
        #[zbus(property)]
        fn percentage(&self) -> f64 {
            55.0
        }
        #[zbus(property)]
        fn state(&self) -> u32 {
            2
        }
    }

    /// The power profiles daemon, with a writable `ActiveProfile` that refuses `turbo`.
    struct FakeProfiles {
        active: String,
    }

    #[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
    impl FakeProfiles {
        #[zbus(property)]
        fn active_profile(&self) -> String {
            self.active.clone()
        }
        #[zbus(property)]
        fn set_active_profile(&mut self, profile: String) -> zbus::fdo::Result<()> {
            if profile == "turbo" {
                return Err(zbus::fdo::Error::InvalidArgs("no such profile".into()));
            }
            self.active = profile;
            Ok(())
        }
        #[zbus(property)]
        fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
            ["power-saver", "balanced", "performance"]
                .into_iter()
                .map(|name| {
                    HashMap::from([(
                        "Profile".to_owned(),
                        Value::from(name).try_to_owned().expect("owned value"),
                    )])
                })
                .collect()
        }
    }

    async fn serve(bus: &TestBus) -> Connection {
        bus.builder()
            .serve_at(DISPLAY_DEVICE, FakeDevice)
            .expect("device")
            .serve_at(
                PROFILES_PATH,
                FakeProfiles {
                    active: "balanced".into(),
                },
            )
            .expect("profiles")
            .name(UPOWER)
            .expect("name")
            .name(PROFILES)
            .expect("profiles name")
            .build()
            .await
            .expect("server")
    }

    async fn wait_for(
        rx: &mut watch::Receiver<BatteryState>,
        predicate: impl Fn(&BatteryState) -> bool,
    ) -> BatteryState {
        tokio::time::timeout(testbus::WAIT, rx.wait_for(|state| predicate(state)))
            .await
            .expect("the model reached the expected state in time")
            .expect("the model is running")
            .clone()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_display_device_on_the_bus_publishes_its_charge() {
        let bus = TestBus::start();
        let _server = serve(&bus).await;
        let (mut rx, _commands) = spawn(
            &Handle::current(),
            Buses::with(bus.client().await),
            PathBuf::from("/nonexistent"),
        );
        let state = wait_for(&mut rx, |state| {
            state.battery.is_some() && state.profiles.is_some()
        })
        .await;
        let battery = state.battery.unwrap();
        assert_eq!(
            (battery.percent, battery.charge),
            (55.0, Charge::Discharging)
        );
        assert_eq!(state.profiles.unwrap().active, "balanced");
        assert_eq!(state.backlight, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_profile_is_set_and_a_refused_one_is_counted() {
        let bus = TestBus::start();
        let _server = serve(&bus).await;
        let (mut rx, commands) = spawn(
            &Handle::current(),
            Buses::with(bus.client().await),
            PathBuf::from("/nonexistent"),
        );
        wait_for(&mut rx, |state| state.profiles.is_some()).await;
        commands
            .send(BatteryCommand::SetProfile("performance".into()))
            .unwrap();
        let state = wait_for(&mut rx, |state| {
            state
                .profiles
                .as_ref()
                .is_some_and(|profiles| profiles.active == "performance")
        })
        .await;
        assert_eq!(state.refused, 0);
        commands
            .send(BatteryCommand::SetProfile("turbo".into()))
            .unwrap();
        let state = wait_for(&mut rx, |state| state.refused == 1).await;
        assert_eq!(state.profiles.unwrap().active, "performance");
    }

    /// logind's session, which answers the first `SetBrightness` late and records every level.
    struct FakeSession {
        levels: Arc<Mutex<Vec<u32>>>,
    }

    #[zbus::interface(name = "org.freedesktop.login1.Session")]
    impl FakeSession {
        async fn set_brightness(&self, _class: String, _name: String, level: u32) {
            let first = {
                let mut levels = self.levels.lock().unwrap();
                levels.push(level);
                levels.len() == 1
            };
            if first {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_drag_of_the_slider_ends_on_its_last_position() {
        let bus = TestBus::start();
        let levels = Arc::new(Mutex::new(Vec::new()));
        let _logind = bus
            .builder()
            .serve_at(
                SESSION_PATH,
                FakeSession {
                    levels: levels.clone(),
                },
            )
            .expect("session")
            .name(LOGIN1)
            .expect("name")
            .build()
            .await
            .expect("logind");
        let (_rx, commands) = spawn(
            &Handle::current(),
            Buses::with(bus.client().await),
            PathBuf::from("/nonexistent"),
        );
        for raw in 1..=40 {
            commands
                .send(BatteryCommand::SetBrightness {
                    device: "intel_backlight".into(),
                    raw,
                })
                .unwrap();
        }
        let last = || levels.lock().unwrap().last().copied();
        tokio::time::timeout(testbus::WAIT, async {
            while last() != Some(40) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the last position reached logind");
        let calls = levels.lock().unwrap().len();
        assert!(calls < 40, "{calls} calls for 40 positions");
    }
}
