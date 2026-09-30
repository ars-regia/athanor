//! UPower, the power profiles and the backlight, as the battery module shows them (doc_bar.md,
//! BR3). The backlight is read from sysfs and written through logind, so the bar needs no
//! write access to sysfs and no group.

use std::fs;
use std::path::Path;

use crate::props::{self, Props};

pub const UPOWER: &str = "org.freedesktop.UPower";
pub const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
pub const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
/// Owned by tuned-ppd and by power-profiles-daemon 0.20 and later.
pub const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
pub const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
/// In the order the popover lists them.
pub const PROFILE_NAMES: [&str; 3] = ["power-saver", "balanced", "performance"];
pub const BACKLIGHT_ROOT: &str = "/sys/class/backlight";

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
    if props::value::<u32>(props, "Type") != Some(TYPE_BATTERY) || props::value::<bool>(props, "IsPresent") != Some(true) {
        return None;
    }
    let percent = props::value::<f64>(props, "Percentage").filter(|p| p.is_finite())?.clamp(0.0, 100.0);
    let charge = match props::value::<u32>(props, "State") {
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
        .and_then(|key| props::value::<i64>(props, key))
        .and_then(|seconds| u64::try_from(seconds).ok())
        .filter(|&seconds| seconds > 0);
    Some(Battery { percent, charge, seconds })
}

pub fn icon(battery: &Battery) -> &'static str {
    let charging = battery.charge == Charge::Charging;
    match (battery.charge, battery.percent) {
        (Charge::Full, _) => "battery-full-charged-symbolic",
        (_, p) if p >= 80.0 => if charging { "battery-full-charging-symbolic" } else { "battery-full-symbolic" },
        (_, p) if p >= 50.0 => if charging { "battery-good-charging-symbolic" } else { "battery-good-symbolic" },
        (_, p) if p >= 20.0 => if charging { "battery-low-charging-symbolic" } else { "battery-low-symbolic" },
        (_, p) if p >= 5.0 => if charging { "battery-caution-charging-symbolic" } else { "battery-caution-symbolic" },
        _ => if charging { "battery-empty-charging-symbolic" } else { "battery-empty-symbolic" },
    }
}

pub fn hours_minutes(seconds: u64) -> (u64, u64) {
    let minutes = seconds / 60;
    (minutes / 60, minutes % 60)
}

/// The profiles the daemon offers, in `PROFILE_NAMES` order, and the active one; `None`
/// when the reply names no profile the bar knows.
pub fn profiles(props: &Props) -> Option<(Vec<&'static str>, String)> {
    let offered: Vec<String> = props::value::<Vec<Props>>(props, "Profiles")?
        .iter()
        .filter_map(|profile| props::value::<String>(profile, "Profile"))
        .collect();
    let known: Vec<&'static str> = PROFILE_NAMES
        .into_iter()
        .filter(|name| offered.iter().any(|offered| offered == name))
        .collect();
    let active = props::value::<String>(props, "ActiveProfile").filter(|active| known.contains(&active.as_str()))?;
    Some((known, active))
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
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
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
            let name = entry.file_name().into_string().ok().filter(|name| valid_name(name))?;
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
        let percent = if percent.is_finite() { percent.clamp(0.0, 100.0) } else { 100.0 };
        // At most `max`, so the cast cannot truncate.
        ((percent * f64::from(self.max) / 100.0).round() as u32).clamp(1, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::{Variant, VariantTy};

    fn device(text: &str) -> Props {
        crate::props::get_all(&Variant::parse(Some(VariantTy::new("(a{sv})").unwrap()), text).unwrap()).unwrap()
    }

    #[test]
    fn a_present_battery_is_read_and_anything_else_hides_the_module() {
        let on_battery = device("({'Type': <uint32 2>, 'IsPresent': <true>, 'Percentage': <72.0>, 'State': <uint32 2>, 'TimeToEmpty': <int64 12300>, 'TimeToFull': <int64 0>},)");
        assert_eq!(battery(&on_battery), Some(Battery { percent: 72.0, charge: Charge::Discharging, seconds: Some(12300) }));
        assert_eq!(icon(&battery(&on_battery).unwrap()), "battery-good-symbolic");
        assert_eq!(hours_minutes(12300), (3, 25));
        let absent = device("({'Type': <uint32 2>, 'IsPresent': <false>, 'Percentage': <0.0>},)");
        assert_eq!(battery(&absent), None);
        let ups = device("({'Type': <uint32 3>, 'IsPresent': <true>, 'Percentage': <50.0>},)");
        assert_eq!(battery(&ups), None);
        let charging = device("({'Type': <uint32 2>, 'IsPresent': <true>, 'Percentage': <130.0>, 'State': <uint32 1>, 'TimeToFull': <int64 -5>},)");
        let charging = battery(&charging).unwrap();
        assert_eq!((charging.percent, charging.seconds), (100.0, None));
        assert_eq!(icon(&charging), "battery-full-charging-symbolic");
    }

    #[test]
    fn profiles_keep_the_known_ones_in_order() {
        let props = device("({'ActiveProfile': <'balanced'>, 'Profiles': <[{'Profile': <'performance'>}, {'Profile': <'balanced'>}, {'Profile': <'turbo'>}]>},)");
        assert_eq!(profiles(&props), Some((vec!["balanced", "performance"], "balanced".into())));
        let unknown = device("({'ActiveProfile': <'turbo'>, 'Profiles': <[{'Profile': <'turbo'>}]>},)");
        assert_eq!(profiles(&unknown), None);
        assert_eq!(profiles(&device("({'ActiveProfile': <'balanced'>},)")), None);
    }

    #[test]
    fn the_backlight_prefers_firmware_and_never_goes_to_zero() {
        let root = std::env::temp_dir().join(format!("athanor-bar-backlight-{}", std::process::id()));
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
        assert_eq!(backlight, Backlight { name: "nv_backlight".into(), max: 100, level: 100 });
        assert_eq!(backlight.raw(0.0), 1);
        assert_eq!(backlight.raw(50.0), 50);
        assert_eq!(backlight.raw(f64::NAN), 100);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(read_backlight(&root), None);
    }
}
