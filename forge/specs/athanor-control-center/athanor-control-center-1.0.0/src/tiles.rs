//! Which tiles and sliders the panel builds (CC4, BR3): one whose service or hardware is
//! absent is not shown. Decided from a snapshot of the models' states, so it is tested
//! without a display and re-decided whenever a state changes.

use athanor_services::audio::AudioState;
use athanor_services::battery::BatteryState;
use athanor_services::bluetooth::BluetoothState;
use athanor_services::media::MediaState;
use athanor_services::network::NetworkState;

/// What the models say now. A model that has not answered, or whose service is away, holds
/// its empty state.
pub struct Snapshot<'a> {
    pub network: Option<&'a NetworkState>,
    pub bluetooth: Option<&'a BluetoothState>,
    pub audio: &'a AudioState,
    pub battery: &'a BatteryState,
    pub media: &'a MediaState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Present {
    pub wifi: bool,
    pub bluetooth: bool,
    pub output_volume: bool,
    pub input_volume: bool,
    pub brightness: bool,
    pub power_profile: bool,
    pub battery: bool,
    pub media: bool,
}

pub fn present(snapshot: &Snapshot<'_>) -> Present {
    Present {
        wifi: snapshot.network.is_some_and(|network| network.wifi.is_some()),
        bluetooth: snapshot.bluetooth.is_some(),
        output_volume: !snapshot.audio.outputs.is_empty(),
        input_volume: !snapshot.audio.inputs.is_empty(),
        brightness: snapshot.battery.backlight.is_some(),
        power_profile: snapshot.battery.profiles.is_some(),
        battery: snapshot.battery.battery.is_some(),
        media: snapshot.media.current_player().is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_services::audio::Device;
    use athanor_services::battery::{Backlight, Battery, Charge, Profiles};
    use athanor_services::media::Player;
    use athanor_services::network::Wifi;

    fn present_of(
        network: Option<&NetworkState>,
        bluetooth: Option<&BluetoothState>,
        audio: &AudioState,
        battery: &BatteryState,
        media: &MediaState,
    ) -> Present {
        present(&Snapshot {
            network,
            bluetooth,
            audio,
            battery,
            media,
        })
    }

    fn network(wifi: Option<Wifi>) -> NetworkState {
        NetworkState {
            wired: None,
            wifi,
            vpns: Vec::new(),
            airplane: false,
            refused: 0,
        }
    }

    fn device() -> Device {
        Device {
            name: "sink".into(),
            label: "Speakers".into(),
            percent: 40.0,
            muted: false,
        }
    }

    #[test]
    fn nothing_answered_nothing_is_built() {
        assert_eq!(
            present_of(
                None,
                None,
                &AudioState::default(),
                &BatteryState::default(),
                &MediaState::default()
            ),
            Present::default()
        );
    }

    #[test]
    fn a_desktop_with_a_wired_link_and_a_sink_has_no_wifi_battery_or_backlight() {
        let wired = network(None);
        let audio = AudioState {
            outputs: vec![device()],
            ..AudioState::default()
        };
        let found = present_of(
            Some(&wired),
            None,
            &audio,
            &BatteryState::default(),
            &MediaState::default(),
        );
        assert_eq!(
            found,
            Present {
                output_volume: true,
                ..Present::default()
            }
        );
    }

    #[test]
    fn a_laptop_has_every_tile_its_hardware_calls_for() {
        let wifi = network(Some(Wifi {
            device: "wlan0".into(),
            enabled: true,
            networks: Vec::new(),
        }));
        let bluetooth = BluetoothState {
            adapter: "/org/bluez/hci0".into(),
            powered: false,
            discovering: false,
            pairable: false,
            discoverable: false,
            paired: Vec::new(),
            nearby: Vec::new(),
            refused: 0,
        };
        let audio = AudioState {
            outputs: vec![device()],
            inputs: vec![device()],
            ..AudioState::default()
        };
        let battery = BatteryState {
            battery: Some(Battery {
                percent: 80.0,
                charge: Charge::Discharging,
                seconds: None,
            }),
            profiles: Some(Profiles {
                offered: vec!["balanced"],
                active: "balanced".into(),
            }),
            backlight: Some(Backlight {
                name: "intel_backlight".into(),
                max: 100,
                level: 50,
            }),
            refused: 0,
        };
        let media = MediaState {
            players: vec![Player {
                bus_name: "org.mpris.MediaPlayer2.x".into(),
                identity: "X".into(),
                track: None,
                playing: false,
                can_next: false,
                can_previous: false,
                can_seek: false,
                position_us: None,
                length_us: None,
                art: None,
            }],
            current: Some(0),
            refused: 0,
            settled: true,
        };
        assert_eq!(
            present_of(Some(&wifi), Some(&bluetooth), &audio, &battery, &media),
            Present {
                wifi: true,
                bluetooth: true,
                output_volume: true,
                input_volume: true,
                brightness: true,
                power_profile: true,
                battery: true,
                media: true,
            }
        );
    }
}
