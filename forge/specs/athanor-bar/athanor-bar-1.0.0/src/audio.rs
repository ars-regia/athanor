//! The audio module's arithmetic (doc_bar.md, BR3): volumes between the sound server's scale
//! and a percentage, the icons and device labels. Descriptions come from other processes:
//! they are sanitised here.

use athanor_unit::text::{line, NAME_CHARS};

/// The sound server's 100 %, `PA_VOLUME_NORM`.
pub const NORMAL: u32 = 0x10000;
pub const MAX_DEVICES: usize = 16;

/// A volume as a percentage. Above 100 % the slider shows 100 %: the bar does not amplify.
pub fn percent(raw: u32) -> f64 {
    (f64::from(raw) * 100.0 / f64::from(NORMAL)).clamp(0.0, 100.0)
}

pub fn raw(percent: f64) -> u32 {
    let percent = if percent.is_finite() {
        percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    // At most NORMAL, so the cast cannot truncate.
    (percent * f64::from(NORMAL) / 100.0).round() as u32
}

pub fn output_icon(percent: f64, muted: bool) -> &'static str {
    match percent {
        _ if muted || percent <= 0.0 => "audio-volume-muted-symbolic",
        p if p < 34.0 => "audio-volume-low-symbolic",
        p if p < 67.0 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

pub fn input_icon(muted: bool) -> &'static str {
    if muted {
        "microphone-disabled-symbolic"
    } else {
        "audio-input-microphone-symbolic"
    }
}

/// The description when there is a usable one, else the device's name.
pub fn device_label(description: Option<&str>, name: &str) -> String {
    description
        .map(|text| line(text, NAME_CHARS))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| line(name, NAME_CHARS))
}

/// One output or input device.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The sound server's name, to address it.
    pub name: String,
    pub label: String,
    pub percent: f64,
    pub muted: bool,
}

/// The device the module controls: the default one, else the first.
pub fn chosen(devices: &[Device], default: Option<&str>) -> Option<usize> {
    default
        .and_then(|name| devices.iter().position(|device| device.name == name))
        .or_else(|| (!devices.is_empty()).then_some(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_round_trip_and_are_bounded() {
        assert_eq!(percent(NORMAL), 100.0);
        assert_eq!(percent(NORMAL / 2), 50.0);
        assert_eq!(percent(NORMAL * 3 / 2), 100.0, "amplified shows as 100");
        assert_eq!(raw(100.0), NORMAL);
        assert_eq!(raw(150.0), NORMAL);
        assert_eq!(raw(-3.0), 0);
        assert_eq!(raw(f64::NAN), 0);
        for p in [0.0, 1.0, 33.0, 50.0, 99.0] {
            assert!((percent(raw(p)) - p).abs() < 0.01, "{p}");
        }
    }

    #[test]
    fn icons_follow_the_level_and_the_mute() {
        assert_eq!(output_icon(80.0, true), "audio-volume-muted-symbolic");
        assert_eq!(output_icon(0.0, false), "audio-volume-muted-symbolic");
        assert_eq!(output_icon(10.0, false), "audio-volume-low-symbolic");
        assert_eq!(output_icon(50.0, false), "audio-volume-medium-symbolic");
        assert_eq!(output_icon(100.0, false), "audio-volume-high-symbolic");
        assert_eq!(input_icon(true), "microphone-disabled-symbolic");
    }

    #[test]
    fn the_default_device_is_chosen_else_the_first() {
        let device = |name: &str| Device {
            name: name.into(),
            label: name.into(),
            percent: 0.0,
            muted: false,
        };
        let devices = [device("a"), device("b")];
        assert_eq!(chosen(&devices, Some("b")), Some(1));
        assert_eq!(chosen(&devices, Some("gone")), Some(0));
        assert_eq!(chosen(&[], Some("b")), None);
        assert_eq!(
            device_label(Some("\u{202e}Speakers\n"), "sink.0"),
            "Speakers"
        );
        assert_eq!(device_label(Some(" "), "sink.0"), "sink.0");
    }
}
