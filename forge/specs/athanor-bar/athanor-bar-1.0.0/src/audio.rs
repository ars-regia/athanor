//! The audio module's arithmetic (doc_bar.md, BR3): volumes between the sound server's scale
//! and a percentage, the icons, device labels, and the MPRIS metadata of the media controls.
//! Descriptions and track titles come from other processes: they are sanitised here.

use athanor_unit::text::{line, NAME_CHARS};

use crate::props::{self, Props};

/// The sound server's 100 %, `PA_VOLUME_NORM`.
pub const NORMAL: u32 = 0x10000;
pub const MAX_DEVICES: usize = 16;
pub const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
pub const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
pub const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: Option<String>,
}

/// The track from a player's `Metadata`; `None` without a title.
pub fn track(player: &Props) -> Option<Track> {
    let metadata = props::value::<Props>(player, "Metadata")?;
    let title = props::value::<String>(&metadata, "xesam:title")
        .map(|title| line(&title, NAME_CHARS))
        .filter(|title| !title.trim().is_empty())?;
    let artist = props::value::<Vec<String>>(&metadata, "xesam:artist")
        .map(|artists| line(&artists.join(", "), NAME_CHARS))
        .filter(|artist| !artist.trim().is_empty());
    Some(Track { title, artist })
}

/// A well-known name of an MPRIS player.
pub fn is_player(name: &str) -> bool {
    name.strip_prefix(MPRIS_PREFIX)
        .is_some_and(|rest| !rest.is_empty())
}

pub fn playing(player: &Props) -> bool {
    props::value::<String>(player, "PlaybackStatus").as_deref() == Some("Playing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::prelude::*;
    use glib::{Variant, VariantTy};

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

    #[test]
    fn the_track_is_sanitised_and_needs_a_title() {
        let player = |metadata: &str| -> Props {
            let v = Variant::parse(Some(VariantTy::new("a{sv}").unwrap()), metadata).unwrap();
            Props::from([
                ("Metadata".into(), v),
                ("PlaybackStatus".into(), "Playing".to_variant()),
            ])
        };
        let full =
            player("{'xesam:title': <'Night\u{202e} Drive'>, 'xesam:artist': <['Calmo', 'Duo']>}");
        assert_eq!(
            track(&full),
            Some(Track {
                title: "Night Drive".into(),
                artist: Some("Calmo, Duo".into())
            })
        );
        assert!(playing(&full));
        assert_eq!(track(&player("{'xesam:artist': <['Calmo']>}")), None);
        assert_eq!(track(&player("{'xesam:title': <42>}")), None);
        assert!(is_player("org.mpris.MediaPlayer2.athanor"));
        assert!(!is_player("org.mpris.MediaPlayer2."));
        assert!(!is_player("org.example.Player"));
    }
}
