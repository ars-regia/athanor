//! UPower, the power profiles, the backlight and the keyboard backlight, as the battery module
//! shows them (doc_bar.md, BR3; doc_control_center.md, CC3) and the on-screen display steps
//! them (doc_osd.md, OD6). Both backlights are read from sysfs and written through logind, so
//! the process needs no write access to sysfs and no group. The model follows UPower and the
//! profiles daemon through two mirrors and publishes a [`BatteryState`]; it reads the backlight
//! again when the kernel signals it, and the keyboard backlight when UPower does.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::future::Future;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::unix::AsyncFd;
use tokio::io::Interest;
use tokio::runtime::Handle;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use zbus::proxy::{MethodFlags, SignalStream};
use zbus::Variant;
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
pub const LEDS_ROOT: &str = "/sys/class/leds";
/// UPower's keyboard backlight, whose signal names the source of a change (doc_osd.md OD6).
const KBD_BACKLIGHT_PATH: &str = "/org/freedesktop/UPower/KbdBacklight";
const KBD_BACKLIGHT: &str = "org.freedesktop.UPower.KbdBacklight";
/// How long after its command ended a level this model wrote stands for the backlight's
/// notification of that write, which may come before or after logind's reply.
const COMMANDED_FOR: Duration = Duration::from_secs(1);
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
    let Variant::Array(entries) = &**props.get("Profiles")? else {
        return None;
    };
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                let Variant::Dict(dict) = entry else {
                    return None;
                };
                match dict.get::<&str, Variant>(&"Profile") {
                    Ok(Some(Variant::Str(name))) => Some(name.as_str().to_owned()),
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
    /// The directory's name under its class, `/sys/class/backlight` or `/sys/class/leds`, as
    /// logind's `SetBrightness` takes it.
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
            Some((rank, read_device(&dir, name)?))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().next().map(|(_, backlight)| backlight)
}

/// The keyboard backlight: the first `*::kbd_backlight` device under `root`, by name, read like
/// the screen's (doc_osd.md OD6); `None` without one.
pub fn read_keyboard_backlight(root: &Path) -> Option<Backlight> {
    let mut found: Vec<Backlight> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry
                .file_name()
                .into_string()
                .ok()
                .filter(|name| valid_name(name) && name.ends_with("::kbd_backlight"))?;
            read_device(&entry.path(), name)
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.into_iter().next()
}

/// A device of the backlight or LED class, with a maximum above 0.
fn read_device(dir: &Path, name: String) -> Option<Backlight> {
    let max = read_u32(&dir.join("max_brightness")).filter(|&max| max > 0)?;
    let level = read_u32(&dir.join("brightness"))?.min(max);
    Some(Backlight { name, max, level })
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
    /// The keyboard backlight; its level runs from 0, off, to `max`.
    pub keyboard: Option<Backlight>,
    /// Counts the backlight's changes this model did not command, each a `POLLPRI` of its
    /// `actual_brightness` with no write of the model's to account for it: the firmware's, and
    /// those of any other writer (doc_osd.md OD5, OD6). One that left the level where it was,
    /// as a hotkey at the end of the range, counts too.
    pub backlight_changes: u32,
    /// Counts the keyboard backlight's changes that UPower reports with the source `external`,
    /// the embedded controller's (doc_osd.md OD6).
    pub keyboard_changes: u32,
    /// Counts the commands a service refused or did not answer: the state is published again
    /// after each, so a reader that sees this change knows its last action did not complete.
    /// Beyond the plan's three fields, so the bar keeps its "action did not complete" note.
    pub refused: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatteryCommand {
    SetProfile(String),
    SetBrightness {
        device: String,
        raw: u32,
    },
    /// A level of the keyboard backlight, from 0 to its `max`.
    SetKeyboardBrightness {
        device: String,
        level: u32,
    },
}

/// Starts the model on `handle`'s runtime. `backlight_dir` and `leds_dir` are the sysfs
/// directories the backlight and the keyboard backlight are read from. The receiver ends when
/// the model does: without the system bus it never leaves its empty state.
pub fn spawn(
    handle: &Handle,
    buses: Buses,
    backlight_dir: PathBuf,
    leds_dir: PathBuf,
) -> (
    watch::Receiver<BatteryState>,
    mpsc::UnboundedSender<BatteryCommand>,
) {
    start(handle, buses, backlight_dir, leds_dir, None)
}

/// [`spawn`], with the backlight's notifications from `notices` instead of its
/// `actual_brightness` when they are given, as a test gives them.
fn start(
    handle: &Handle,
    buses: Buses,
    backlight_dir: PathBuf,
    leds_dir: PathBuf,
    notices: Option<mpsc::Receiver<()>>,
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
        let dirs = Dirs {
            backlight: backlight_dir,
            leds: leds_dir,
        };
        run(&model, connection, dirs, notices, state, command_rx).await;
    });
    (rx, commands)
}

/// The sysfs directories of the two backlights.
struct Dirs {
    backlight: PathBuf,
    leds: PathBuf,
}

/// Sends a notice on each `POLLPRI` of `path`, the backlight's `actual_brightness`, which the
/// kernel raises on every sysfs write and every hotkey change (doc_osd.md OD6). A file that
/// cannot be polled, as one outside sysfs, is logged once and sends nothing.
fn watch_backlight(handle: &Handle, path: PathBuf, notices: mpsc::Sender<()>) {
    handle.spawn(async move {
        let watched = File::open(&path).and_then(|file| {
            rearm(&file)?;
            register(file)
        });
        let file = match watched {
            Ok(file) => file,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "the backlight's changes are not signalled; a change by the firmware shows no overlay");
                return;
            }
        };
        loop {
            let mut ready = match file.ready(Interest::PRIORITY).await {
                Ok(ready) => ready,
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "the backlight's signal failed; its changes are no longer followed");
                    return;
                }
            };
            if let Err(err) = rearm(ready.get_inner()) {
                tracing::warn!(path = %path.display(), error = %err, "the backlight cannot be read; its changes are no longer followed");
                return;
            }
            ready.clear_ready();
            // A full queue already holds a notice, and one read of the level serves both.
            if let Err(TrySendError::Closed(())) = notices.try_send(()) {
                return;
            }
        }
    });
}

/// Registers `file` with the reactor for `POLLPRI`, the readiness sysfs raises on a change.
#[allow(unsafe_code)]
fn register(file: File) -> io::Result<AsyncFd<File>> {
    // SAFETY: the `AsyncFd` owns the `File`, which owns its descriptor and closes it only
    // when dropped, so the descriptor stays open and the same for the `AsyncFd`'s lifetime.
    let registered = unsafe { AsyncFd::register_with_interest(file, Interest::PRIORITY) };
    Ok(registered?)
}

/// sysfs raises `POLLPRI` again only once the attribute has been read from its start.
fn rearm(file: &File) -> io::Result<()> {
    let mut buffer = [0; 16];
    file.read_at(&mut buffer, 0).map(|_| ())
}

/// The levels this model wrote to the backlight, each until the backlight's notification of
/// it arrives, or [`COMMANDED_FOR`] after its command ended.
#[derive(Debug, Default)]
struct Commanded {
    /// The level and when its command ended; `None` while it runs.
    levels: VecDeque<(u32, Option<Instant>)>,
}

impl Commanded {
    fn start(&mut self, level: u32) {
        self.levels.push_back((level, None));
    }

    /// The running command ended, carried out or refused.
    fn finish(&mut self, now: Instant) {
        if let Some(entry) = self.levels.iter_mut().find(|(_, ended)| ended.is_none()) {
            entry.1 = Some(now);
        }
    }

    /// Whether a notification that found `level` is the one of a write of this model's. It
    /// takes that write, and the earlier ones, whose notifications the kernel folded into it.
    fn take(&mut self, level: u32, now: Instant) -> bool {
        self.levels.retain(|(_, ended)| {
            ended.is_none_or(|ended| now.duration_since(ended) < COMMANDED_FOR)
        });
        match self
            .levels
            .iter()
            .position(|(written, _)| *written == level)
        {
            Some(at) => {
                self.levels.drain(..=at);
                true
            }
            None => false,
        }
    }
}

/// UPower's `BrightnessChangedWithSource` of the keyboard backlight, from its owner only;
/// `None`, with one journal line, when it cannot be followed.
async fn keyboard_signals(connection: &Connection) -> Option<SignalStream<'static>> {
    let subscribed = async {
        let proxy = Proxy::new(connection, UPOWER, KBD_BACKLIGHT_PATH, KBD_BACKLIGHT).await?;
        proxy.receive_signal("BrightnessChangedWithSource").await
    };
    match subscribed.await {
        Ok(signals) => Some(signals),
        Err(err) => {
            tracing::warn!(error = %err, "UPower's keyboard backlight is not followed; a change by the embedded controller shows no overlay");
            None
        }
    }
}

async fn run(
    handle: &Handle,
    connection: Connection,
    dirs: Dirs,
    notices: Option<mpsc::Receiver<()>>,
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
    let mut backlight = read_backlight(&dirs.backlight);
    let mut keyboard = read_keyboard_backlight(&dirs.leds);
    // The backlight's notifications, or none without a backlight.
    let mut notices = notices.or_else(|| {
        let backlight = backlight.as_ref()?;
        let (notify, notices) = mpsc::channel(1);
        let path = dirs
            .backlight
            .join(&backlight.name)
            .join("actual_brightness");
        watch_backlight(handle, path, notify);
        Some(notices)
    });
    let mut keyboard_signals = match keyboard {
        Some(_) => keyboard_signals(&connection).await,
        None => None,
    };
    let mut commanded = Commanded::default();
    let mut backlight_changes: u32 = 0;
    let mut keyboard_changes: u32 = 0;
    let mut refused: u32 = 0;
    // Commands wait here while one runs, so the channel is read at once; a new level of a
    // backlight replaces a waiting one of the same backlight, so a drag of the slider ends on
    // its last level and logind is not asked for every step.
    let mut waiting: VecDeque<BatteryCommand> = VecDeque::new();
    let mut running: Option<Running> = None;
    let mut alive = true;
    loop {
        // Both backlights are read at start, and then only when something signals that they
        // changed, so the idle model wakes for no backlight (doc_osd.md OD13).
        let next = BatteryState {
            battery: current(&upower, DISPLAY_DEVICE, DEVICE_IFACE, battery),
            profiles: current(&daemon, PROFILES_PATH, PROFILES, profiles),
            backlight: backlight.clone(),
            keyboard: keyboard.clone(),
            backlight_changes,
            keyboard_changes,
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
        // A mirror ends only after it has emptied itself, so its last state is published too.
        tokio::select! {
            changed = upower.changed() => alive = changed.is_ok(),
            changed = daemon.changed() => alive = changed.is_ok(),
            command = commands.recv() => match command {
                Some(command) => {
                    if matches!(
                        command,
                        BatteryCommand::SetBrightness { .. } | BatteryCommand::SetKeyboardBrightness { .. }
                    ) {
                        let kind = std::mem::discriminant(&command);
                        waiting.retain(|queued| std::mem::discriminant(queued) != kind);
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
                commanded.finish(Instant::now());
                if let Err(err) = result {
                    tracing::warn!(error = %err, "a system service refused or did not answer");
                    refused = refused.wrapping_add(1);
                }
                // A refused write is read too: a reader that sees `refused` change puts its
                // controls back to the level that stands.
                backlight = read_backlight(&dirs.backlight);
                keyboard = read_keyboard_backlight(&dirs.leds);
            }
            notice = async {
                match notices.as_mut() {
                    Some(notices) => notices.recv().await,
                    None => std::future::pending().await,
                }
            }, if notices.is_some() => match notice {
                Some(()) => {
                    backlight = read_backlight(&dirs.backlight);
                    let level = backlight.as_ref().map(|backlight| backlight.level);
                    if !level.is_some_and(|level| commanded.take(level, Instant::now())) {
                        backlight_changes = backlight_changes.wrapping_add(1);
                    }
                }
                None => notices = None,
            },
            signal = async {
                match keyboard_signals.as_mut() {
                    Some(signals) => signals.next().await,
                    None => std::future::pending().await,
                }
            }, if keyboard_signals.is_some() => match signal {
                Some(message) => {
                    keyboard = read_keyboard_backlight(&dirs.leds);
                    match message.body().deserialize::<(i32, &str)>() {
                        Ok((_, "external")) => keyboard_changes = keyboard_changes.wrapping_add(1),
                        Ok(_) => {}
                        Err(err) => tracing::warn!(error = %err, "UPower's keyboard backlight signal had an unexpected type"),
                    }
                }
                None => keyboard_signals = None,
            },
        }
        if running.is_none() {
            running = waiting.pop_front().map(|command| {
                if let BatteryCommand::SetBrightness { raw, .. } = &command {
                    commanded.start(*raw);
                }
                Box::pin(execute(connection.clone(), command)) as Running
            });
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
    let interactive = MethodFlags::ALLOW_INTERACTIVE_AUTH;
    let call = async {
        let (class, device, level) = match &command {
            BatteryCommand::SetProfile(profile) => {
                let proxy = Proxy::new(&connection, PROFILES, PROFILES_PATH, PROPERTIES).await?;
                return proxy
                    .call_with_flags::<_, _, ()>(
                        "Set",
                        interactive,
                        &(PROFILES, "ActiveProfile", Variant::from(profile.as_str())),
                    )
                    .await;
            }
            BatteryCommand::SetBrightness { device, raw } => ("backlight", device, *raw),
            BatteryCommand::SetKeyboardBrightness { device, level } => ("leds", device, *level),
        };
        let proxy = Proxy::new(&connection, LOGIN1, SESSION_PATH, SESSION).await?;
        proxy
            .call_with_flags::<_, _, ()>(
                "SetBrightness",
                interactive,
                &(class, device.as_str(), level),
            )
            .await
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
    use zbus::{OwnedVariant, Variant};

    use super::*;
    use crate::testbus::{self, TestBus};

    fn device(entries: &[(&str, Variant<'_>)]) -> Props {
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

    fn profile_list(names: &[&str]) -> Variant<'static> {
        Variant::new(
            names
                .iter()
                .map(|name| HashMap::from([("Profile", Variant::from(name.to_string()))]))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_present_battery_is_read_and_anything_else_hides_the_module() {
        let on_battery = device(&[
            ("Type", Variant::from(2u32)),
            ("IsPresent", Variant::from(true)),
            ("Percentage", Variant::from(72.0f64)),
            ("State", Variant::from(2u32)),
            ("TimeToEmpty", Variant::from(12300i64)),
            ("TimeToFull", Variant::from(0i64)),
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
            ("Type", Variant::from(2u32)),
            ("IsPresent", Variant::from(false)),
            ("Percentage", Variant::from(0.0f64)),
        ]);
        assert_eq!(battery(&absent), None);
        let ups = device(&[
            ("Type", Variant::from(3u32)),
            ("IsPresent", Variant::from(true)),
            ("Percentage", Variant::from(50.0f64)),
        ]);
        assert_eq!(battery(&ups), None);
        let charging = device(&[
            ("Type", Variant::from(2u32)),
            ("IsPresent", Variant::from(true)),
            ("Percentage", Variant::from(130.0f64)),
            ("State", Variant::from(1u32)),
            ("TimeToFull", Variant::from(-5i64)),
        ]);
        let charging = battery(&charging).unwrap();
        assert_eq!((charging.percent, charging.seconds), (100.0, None));
        assert_eq!(icon(&charging), "battery-full-charging-symbolic");
    }

    #[test]
    fn a_property_of_the_wrong_type_hides_the_battery_instead_of_panicking() {
        let wrong = device(&[
            ("Type", Variant::from(2u32)),
            ("IsPresent", Variant::from("yes")),
            ("Percentage", Variant::from(50u32)),
        ]);
        assert_eq!(battery(&wrong), None);
        assert_eq!(profiles(&device(&[("Profiles", Variant::from(3u32))])), None);
    }

    #[test]
    fn profiles_keep_the_known_ones_in_order() {
        let props = device(&[
            ("ActiveProfile", Variant::from("balanced")),
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
            ("ActiveProfile", Variant::from("turbo")),
            ("Profiles", profile_list(&["turbo"])),
        ]);
        assert_eq!(profiles(&unknown), None);
        assert_eq!(
            profiles(&device(&[("ActiveProfile", Variant::from("balanced"))])),
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
        fn profiles(&self) -> Vec<HashMap<String, OwnedVariant>> {
            ["power-saver", "balanced", "performance"]
                .into_iter()
                .map(|name| {
                    HashMap::from([(
                        "Profile".to_owned(),
                        Variant::from(name).try_to_owned().expect("owned value"),
                    )])
                })
                .collect()
        }
    }

    async fn serve(bus: &TestBus) -> Connection {
        bus.builder()
            .serve_at(DISPLAY_DEVICE, FakeDevice)
            .serve_at(
                PROFILES_PATH,
                FakeProfiles {
                    active: "balanced".into(),
                },
            )
            .name(UPOWER)
            .name(PROFILES)
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
    /// With `sysfs`, it also writes the level to `<sysfs>/<class>/<name>/brightness`, as logind
    /// writes the device.
    struct FakeSession {
        levels: Arc<Mutex<Vec<u32>>>,
        sysfs: Option<PathBuf>,
    }

    #[zbus::interface(name = "org.freedesktop.login1.Session")]
    impl FakeSession {
        async fn set_brightness(&self, class: String, name: String, level: u32) {
            if let Some(sysfs) = &self.sysfs {
                fs::write(
                    sysfs.join(class).join(name).join("brightness"),
                    level.to_string(),
                )
                .unwrap();
            }
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
                    sysfs: None,
                },
            )
            .name(LOGIN1)
            .build()
            .await
            .expect("logind");
        let (_rx, commands) = spawn(
            &Handle::current(),
            Buses::with(bus.client().await),
            PathBuf::from("/nonexistent"),
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

    #[test]
    fn the_keyboard_backlight_is_the_first_kbd_backlight_led() {
        let root =
            std::env::temp_dir().join(format!("athanor-services-leds-{}", std::process::id()));
        let make = |name: &str, max: &str, level: &str| {
            let dir = root.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("max_brightness"), max).unwrap();
            fs::write(dir.join("brightness"), level).unwrap();
        };
        make("input3::capslock", "1\n", "0\n");
        make("tpacpi::kbd_backlight", "2\n", "1\n");
        make("dell::kbd_backlight", "0\n", "0\n");
        make("platform::kbd_backlight", "3\n", "5\n");
        assert_eq!(
            read_keyboard_backlight(&root),
            Some(Backlight {
                name: "platform::kbd_backlight".into(),
                max: 3,
                level: 3
            })
        );
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(read_keyboard_backlight(&root), None);
    }

    #[test]
    fn a_notification_is_the_models_own_only_for_a_level_it_wrote() {
        let now = Instant::now();
        let mut commanded = Commanded::default();
        commanded.start(40);
        assert!(
            commanded.take(40, now),
            "the notification came before logind's reply"
        );
        assert!(!commanded.take(40, now), "a write is taken once");
        commanded.start(30);
        commanded.finish(now);
        assert!(!commanded.take(70, now), "another level is not the model's");
        assert!(commanded.take(30, now + Duration::from_millis(500)));
        commanded.start(10);
        commanded.finish(now);
        commanded.start(20);
        commanded.finish(now);
        assert!(commanded.take(20, now));
        assert!(
            !commanded.take(10, now),
            "the kernel folded the earlier write's notification into the later one"
        );
        commanded.start(20);
        commanded.finish(now);
        assert!(
            !commanded.take(20, now + COMMANDED_FOR),
            "a write stands for no notification after COMMANDED_FOR"
        );
    }

    /// A sysfs of one backlight at 50 of 100 and one keyboard backlight at 0 of 2.
    fn sysfs(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "athanor-services-sysfs-{tag}-{}",
            std::process::id()
        ));
        let backlight = root.join("backlight/intel_backlight");
        fs::create_dir_all(&backlight).unwrap();
        fs::write(backlight.join("type"), "raw\n").unwrap();
        fs::write(backlight.join("max_brightness"), "100\n").unwrap();
        fs::write(backlight.join("brightness"), "50\n").unwrap();
        let keyboard = root.join("leds/tpacpi::kbd_backlight");
        fs::create_dir_all(&keyboard).unwrap();
        fs::write(keyboard.join("max_brightness"), "2\n").unwrap();
        fs::write(keyboard.join("brightness"), "0\n").unwrap();
        root
    }

    async fn logind(bus: &TestBus, sysfs: &Path) -> Connection {
        bus.builder()
            .serve_at(
                SESSION_PATH,
                FakeSession {
                    levels: Arc::default(),
                    sysfs: Some(sysfs.to_owned()),
                },
            )
            .name(LOGIN1)
            .build()
            .await
            .expect("logind")
    }

    /// Hands the model one notification of the backlight and returns once the model has taken
    /// it: the slot frees as the model receives it, and on this one thread the test runs again
    /// only after the model has read the level and waits once more.
    async fn notify(notify: &mpsc::Sender<()>) {
        notify.send(()).await.unwrap();
        drop(notify.reserve().await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_backlight_change_the_model_did_not_write_is_counted() {
        let bus = TestBus::start();
        let root = sysfs("backlight");
        let _logind = logind(&bus, &root).await;
        let (notices_tx, notices) = mpsc::channel(1);
        let (mut rx, commands) = start(
            &Handle::current(),
            Buses::with(bus.client().await),
            root.join("backlight"),
            root.join("leds"),
            Some(notices),
        );
        let level = |state: &BatteryState| state.backlight.as_ref().map(|b| b.level);
        wait_for(&mut rx, |state| level(state) == Some(50)).await;
        commands
            .send(BatteryCommand::SetBrightness {
                device: "intel_backlight".into(),
                raw: 40,
            })
            .unwrap();
        wait_for(&mut rx, |state| level(state) == Some(40)).await;
        // The kernel's notification of the model's own write.
        notify(&notices_tx).await;
        // The firmware's change.
        fs::write(root.join("backlight/intel_backlight/brightness"), "70").unwrap();
        notify(&notices_tx).await;
        let state = wait_for(&mut rx, |state| level(state) == Some(70)).await;
        assert_eq!(
            state.backlight_changes, 1,
            "the model's own write is not counted"
        );
        // A hotkey at the end of the range leaves the level where it is, and counts.
        notify(&notices_tx).await;
        wait_for(&mut rx, |state| state.backlight_changes == 2).await;
        fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_keyboard_backlight_counts_the_embedded_controllers_changes() {
        let bus = TestBus::start();
        let root = sysfs("keyboard");
        let _logind = logind(&bus, &root).await;
        let upower = serve(&bus).await;
        let (_notices_tx, notices) = mpsc::channel(1);
        let (mut rx, commands) = start(
            &Handle::current(),
            Buses::with(bus.client().await),
            root.join("backlight"),
            root.join("leds"),
            Some(notices),
        );
        let level = |state: &BatteryState| state.keyboard.as_ref().map(|k| k.level);
        wait_for(&mut rx, |state| level(state) == Some(0)).await;
        commands
            .send(BatteryCommand::SetKeyboardBrightness {
                device: "tpacpi::kbd_backlight".into(),
                level: 2,
            })
            .unwrap();
        let state = wait_for(&mut rx, |state| level(state) == Some(2)).await;
        assert_eq!(state.keyboard_changes, 0);
        let changed = |level: i32, source: &'static str| {
            fs::write(
                root.join("leds/tpacpi::kbd_backlight/brightness"),
                level.to_string(),
            )
            .unwrap();
            let upower = &upower;
            async move {
                upower
                    .emit_signal(
                        None::<zbus::names::BusName<'_>>,
                        KBD_BACKLIGHT_PATH,
                        KBD_BACKLIGHT,
                        "BrightnessChangedWithSource",
                        &(level, source),
                    )
                    .await
            }
        };
        changed(1, "external").await.unwrap();
        let state = wait_for(&mut rx, |state| level(state) == Some(1)).await;
        assert_eq!(state.keyboard_changes, 1);
        // A change through UPower's own method, another program's, is read and not counted.
        changed(0, "internal").await.unwrap();
        let state = wait_for(&mut rx, |state| level(state) == Some(0)).await;
        assert_eq!(state.keyboard_changes, 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
