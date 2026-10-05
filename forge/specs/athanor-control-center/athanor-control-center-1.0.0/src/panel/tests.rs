//! The panel on a display, from the state of the models, driven the way the person drives it
//! (ST7). GTK may be started on one thread only, so the cases share one test.
//!
//! Without a display the test says so and passes, unless `ATHANOR_REQUIRE_DISPLAY` is set:
//! `forge/test/shell/with-display.sh` sets it, so a gate cannot pass without having run.

use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use athanor_control_center::rfkill::{self, Event};
use athanor_services::audio::{AudioCommand, AudioState, Device as Sink};
use athanor_services::battery::{Backlight, Battery, BatteryCommand, BatteryState, Charge, Profiles};
use athanor_services::bluetooth::{BluetoothCommand, BluetoothState};
use athanor_services::media::{MediaCommand, MediaState, Player, Track};
use athanor_services::network::{NetworkCommand, NetworkState, Wifi};
use gtk4::glib;
use tokio::sync::{mpsc, watch};

use super::airplane::Rfkill;
use super::*;

struct Rig {
    services: Services,
    network: watch::Sender<Option<NetworkState>>,
    network_commands: mpsc::UnboundedReceiver<NetworkCommand>,
    bluetooth: watch::Sender<Option<BluetoothState>>,
    bluetooth_commands: mpsc::UnboundedReceiver<BluetoothCommand>,
    audio: watch::Sender<AudioState>,
    audio_commands: mpsc::UnboundedReceiver<AudioCommand>,
    battery: watch::Sender<BatteryState>,
    battery_commands: mpsc::UnboundedReceiver<BatteryCommand>,
    media: watch::Sender<MediaState>,
    media_commands: mpsc::UnboundedReceiver<MediaCommand>,
}

fn rig() -> Rig {
    let (battery, battery_rx) = watch::channel(BatteryState::default());
    let (battery_tx, battery_commands) = mpsc::unbounded_channel();
    let (network, network_rx) = watch::channel(None);
    let (network_tx, network_commands) = mpsc::unbounded_channel();
    let (bluetooth, bluetooth_rx) = watch::channel(None);
    let (bluetooth_tx, bluetooth_commands) = mpsc::unbounded_channel();
    let (audio, audio_rx) = watch::channel(AudioState::default());
    let (audio_tx, audio_commands) = mpsc::unbounded_channel();
    let (media, media_rx) = watch::channel(MediaState::default());
    let (media_tx, media_commands) = mpsc::unbounded_channel();
    let (_prompts, prompts_rx) = mpsc::channel(1);
    let (_requests, requests_rx) = mpsc::channel(1);
    Rig {
        services: Services {
            battery: (battery_rx, battery_tx),
            network: (network_rx, network_tx),
            bluetooth: (bluetooth_rx, bluetooth_tx),
            audio: (audio_rx, audio_tx),
            media: (media_rx, media_tx),
            backlight_root: PathBuf::from("/nonexistent"),
            password_prompts: Rc::new(RefCell::new(Some(prompts_rx))),
            pairing_requests: Rc::new(RefCell::new(Some(requests_rx))),
        },
        network,
        network_commands,
        bluetooth,
        bluetooth_commands,
        audio,
        audio_commands,
        battery,
        battery_commands,
        media,
        media_commands,
    }
}

fn settle() {
    let context = glib::MainContext::default();
    for _ in 0..50 {
        while context.iteration(false) {}
    }
}

/// Every widget under `root`, in document order, that is on screen: a hidden tile and a stack page
/// that is not the visible one are not mapped.
fn shown(root: &gtk4::Widget) -> Vec<gtk4::Widget> {
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if !widget.is_mapped() {
            continue;
        }
        found.push(widget.clone());
        let mut child = widget.last_child();
        while let Some(previous) = child {
            stack.push(previous.clone());
            child = previous.prev_sibling();
        }
    }
    found
}

fn says(root: &gtk4::Widget, text: &str) -> bool {
    shown(root)
        .iter()
        .filter_map(|widget| widget.downcast_ref::<gtk4::Label>())
        .any(|label| label.text() == text)
}

/// The tile's toggle: the toggle button that has `text` on it.
fn tile(root: &gtk4::Widget, text: &str) -> gtk4::ToggleButton {
    shown(root)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::ToggleButton>().ok())
        .find(|button| says(button.upcast_ref(), text))
        .unwrap_or_else(|| panic!("no tile says {text:?}"))
}

fn arrows(root: &gtk4::Widget) -> Vec<gtk4::Button> {
    shown(root)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Button>().ok())
        .filter(|button| button.icon_name().as_deref() == Some("go-next-symbolic"))
        .collect()
}

fn wifi_state() -> NetworkState {
    NetworkState {
        wired: None,
        wifi: Some(Wifi {
            device: "/wlan".to_owned(),
            enabled: true,
            networks: Vec::new(),
        }),
        vpns: Vec::new(),
        airplane: false,
        refused: 0,
    }
}

fn bluetooth_state() -> BluetoothState {
    BluetoothState {
        adapter: "/hci0".to_owned(),
        powered: true,
        discovering: false,
        pairable: false,
        discoverable: false,
        paired: Vec::new(),
        nearby: Vec::new(),
        refused: 0,
    }
}

fn sink(name: &str, percent: f64) -> Sink {
    Sink {
        name: name.to_owned(),
        label: name.to_owned(),
        percent,
        muted: false,
    }
}

fn audio_state() -> AudioState {
    AudioState {
        outputs: vec![sink("speakers", 40.0)],
        inputs: vec![sink("mic", 70.0)],
        default_output: Some("speakers".to_owned()),
        default_input: Some("mic".to_owned()),
        ..AudioState::default()
    }
}

fn battery_state() -> BatteryState {
    BatteryState {
        battery: Some(Battery {
            percent: 80.0,
            charge: Charge::Discharging,
            seconds: Some(7200),
        }),
        profiles: Some(Profiles {
            offered: vec!["power-saver", "balanced", "performance"],
            active: "balanced".to_owned(),
        }),
        backlight: Some(Backlight {
            name: "intel".to_owned(),
            max: 100,
            level: 50,
        }),
        refused: 0,
    }
}

fn player() -> Player {
    Player {
        bus_name: "org.mpris.MediaPlayer2.one".to_owned(),
        identity: "One".to_owned(),
        track: Some(Track {
            title: "A song".to_owned(),
            artist: Some("An artist".to_owned()),
        }),
        playing: true,
        can_next: true,
        can_previous: true,
        can_seek: true,
        position_us: Some(10_000_000),
        length_us: Some(200_000_000),
        // Not an image: the cover cannot be decoded and the player's icon stands in.
        art: Some(athanor_services::media::Art::Data(Arc::from(&b"not a picture"[..]))),
    }
}

/// A regular file standing for /dev/rfkill: it holds one radio the kernel announced, and
/// takes what the tile writes.
fn fake_rfkill(radios: usize) -> (Rc<Rfkill>, PathBuf) {
    let path = std::env::temp_dir().join(format!("athanor-rfkill-{}", std::process::id()));
    let add = |idx| Event {
        idx,
        kind: 1,
        op: rfkill::OP_ADD,
        soft: false,
        hard: false,
    };
    let events: Vec<u8> = (0..radios as u32).flat_map(|idx| add(idx).encode()).collect();
    std::fs::write(&path, events).expect("the fake rfkill is written");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("the fake rfkill opens");
    (Rfkill::new(file), path)
}

fn hooks() -> Hooks {
    Hooks {
        open_settings: Box::new(|| {}),
        set_dark: Box::new(|_| Ok(())),
    }
}

/// Every control the person can use, in the order Tab reaches them: the buttons, the toggles,
/// the scales and the drop-downs that are shown.
fn tab_order(window: &gtk4::Window) -> Vec<gtk4::Widget> {
    GtkWindowExt::set_focus(window, None::<&gtk4::Widget>);
    let mut order = Vec::new();
    for _ in 0..60 {
        if !window.child_focus(gtk4::DirectionType::TabForward) {
            break;
        }
        let Some(focus) = GtkWindowExt::focus(window) else { break };
        if order.contains(&focus) {
            break;
        }
        order.push(focus);
    }
    order
}

fn interactive(root: &gtk4::Widget) -> Vec<gtk4::Widget> {
    shown(root)
        .into_iter()
        .filter(|widget| {
            widget.is::<gtk4::Button>()
                || widget.is::<gtk4::ToggleButton>()
                || widget.is::<gtk4::Scale>()
                || widget.is::<gtk4::DropDown>()
        })
        .collect()
}

fn the_tiles(mut rig: Rig) -> Rig {
    let (rfkill, path) = fake_rfkill(1);
    let panel = Panel::new(Some(rig.services.clone()), Some(rfkill), hooks());
    let window = gtk4::Window::new();
    window.set_child(Some(&panel.widget()));
    window.present();
    panel.set_shown(true);
    let root = panel.widget();
    settle();

    // A model that has not answered builds nothing for it (BR3).
    assert!(!says(&root, "Wi-Fi"));
    assert!(!says(&root, "Bluetooth"));
    assert!(!says(&root, "Power mode"));
    // Dark mode and airplane mode do not depend on a model.
    assert!(says(&root, "Dark mode"));
    assert!(says(&root, "Airplane mode"));

    rig.network.send(Some(wifi_state())).unwrap();
    rig.bluetooth.send(Some(bluetooth_state())).unwrap();
    rig.audio.send(audio_state()).unwrap();
    rig.battery.send(battery_state()).unwrap();
    settle();
    assert!(says(&root, "Wi-Fi") && says(&root, "Bluetooth") && says(&root, "Power mode"));
    assert!(says(&root, "80%"), "the battery level is in the footer");

    // A tile is a toggle with an accessible name and the pressed state.
    let wifi = tile(&root, "Wi-Fi");
    assert!(wifi.is_active());
    assert!(a11y::is_named(&wifi, "Wi-Fi"));
    assert!(!a11y::is_named(&wifi, "Bluetooth"), "a wrong name is not accepted");
    assert!(gtk4::test_accessible_has_state(&wifi, gtk4::AccessibleState::Pressed));
    wifi.emit_clicked();
    settle();
    assert!(!wifi.is_active(), "the pressed state follows the press");
    assert!(gtk4::test_accessible_has_state(&wifi, gtk4::AccessibleState::Pressed));
    assert!(matches!(rig.network_commands.try_recv(), Ok(NetworkCommand::Wireless(false))));
    // The panel registers no agent, for NetworkManager or for BlueZ, and sends no other command.
    assert!(rig.network_commands.try_recv().is_err(), "no RegisterAgent for NetworkManager");
    assert!(rig.bluetooth_commands.try_recv().is_err(), "no agent for BlueZ");
    tile(&root, "Bluetooth").emit_clicked();
    settle();
    assert!(matches!(rig.bluetooth_commands.try_recv(), Ok(BluetoothCommand::Power(false))));

    // The power-mode tile steps to the next profile the service offers.
    tile(&root, "Power mode").emit_clicked();
    settle();
    assert!(matches!(rig.battery_commands.try_recv(), Ok(BatteryCommand::SetProfile(p)) if p == "performance"));

    // Airplane mode writes struct rfkill_event of RFKILL_OP_CHANGE_ALL.
    tile(&root, "Airplane mode").emit_clicked();
    settle();
    let mut written = Vec::new();
    std::fs::File::open(&path).unwrap().read_to_end(&mut written).unwrap();
    assert_eq!(written[8..], rfkill::block_all(true).encode());
    std::fs::remove_file(&path).ok();

    // The sliders send the person's change in whole percents, to the device in use.
    let scales: Vec<gtk4::Scale> = shown(&root)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Scale>().ok())
        .collect();
    assert_eq!(scales.len(), 3, "output, input and brightness");
    for (scale, name) in scales.iter().zip(["Output volume", "Input volume", "Brightness"]) {
        assert!(a11y::is_named(scale, name), "the scale of {name}");
    }
    assert_eq!(scales[0].value(), 40.0, "the model's state is drawn");
    scales[0].set_value(55.0);
    settle();
    assert!(matches!(rig.audio_commands.try_recv(), Ok(AudioCommand::Volume { sink: true, percent: 55, .. })));
    scales[2].set_value(30.0);
    settle();
    assert!(matches!(rig.battery_commands.try_recv(), Ok(BatteryCommand::SetBrightness { raw: 30, .. })));

    // An arrow opens the tile's page; Back returns to the panel.
    let arrows_before = arrows(&root).len();
    assert!(arrows_before >= 4, "Wi-Fi, Bluetooth, power mode and sound have a page");
    arrows(&root)[0].emit_clicked();
    settle();
    assert_eq!(panel.stack.visible_child_name().as_deref(), Some("network"));
    let back = shown(&root)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Button>().ok())
        .find(|button| button.icon_name().as_deref() == Some("go-previous-symbolic"))
        .expect("a page has a Back button");
    assert!(a11y::is_named(&back, "Back"));
    back.emit_clicked();
    settle();
    assert_eq!(panel.stack.visible_child_name().as_deref(), Some(PANEL));

    // The Bluetooth page does not pair yet: with a device nearby and a discovery running, it
    // offers no "Nearby devices", asks BlueZ for no discovery, and so has nothing to pair.
    rig.bluetooth
        .send(Some(BluetoothState {
            powered: true,
            discovering: true,
            nearby: vec![athanor_services::bluetooth::Device {
                path: "/dev9".to_owned(),
                label: "Unpaired speaker".to_owned(),
                icon: "audio-headset-symbolic",
                paired: false,
                connected: false,
            }],
            ..bluetooth_state()
        }))
        .unwrap();
    settle();
    arrows(&root)[1].emit_clicked();
    settle();
    assert_eq!(panel.stack.visible_child_name().as_deref(), Some("bluetooth"));
    assert!(!says(&root, "Nearby devices") && !says(&root, "Unpaired speaker"));
    assert!(!says(&root, "Pair"), "no pairing entry");
    while let Ok(command) = rig.bluetooth_commands.try_recv() {
        assert!(!matches!(command, BluetoothCommand::Discovery(_) | BluetoothCommand::Pair(_)));
    }
    panel.show(None);
    settle();

    // The panel has a name, and a state it announces when it opens.
    assert_eq!(panel.main.root.accessible_role(), gtk4::AccessibleRole::Group);
    assert!(a11y::is_labelled(&panel.main.root), "a container has a label and no tooltip");

    // Everything the person can use is reached with Tab.
    let order = tab_order(&window);
    for widget in interactive(&root) {
        assert!(order.contains(&widget), "Tab does not reach {widget:?}");
    }
    window.destroy();
    rig
}

fn the_media(mut rig: Rig) -> Rig {
    let panel = Panel::new(Some(rig.services.clone()), None, hooks());
    let window = gtk4::Window::new();
    window.set_child(Some(&panel.widget()));
    window.present();
    panel.set_shown(true);
    let root = panel.widget();
    settle();
    assert!(!says(&root, "A song"), "no player, no media section");
    assert!(!says(&root, "Airplane mode"), "no /dev/rfkill, no airplane tile");
    // A device that reports no radio is no airplane tile either.
    let (empty, empty_path) = fake_rfkill(0);
    let bare = Panel::new(Some(rig.services.clone()), Some(empty), hooks());
    let bare_window = gtk4::Window::new();
    bare_window.set_child(Some(&bare.widget()));
    bare_window.present();
    settle();
    assert!(!says(&bare.widget(), "Airplane mode"), "no radio, no airplane tile");
    bare_window.destroy();
    std::fs::remove_file(&empty_path).ok();

    rig.media
        .send(MediaState {
            players: vec![player()],
            current: Some(0),
            refused: 0,
            settled: true,
        })
        .unwrap();
    settle();
    assert!(says(&root, "A song") && says(&root, "An artist"));
    let button = |icon: &str| {
        shown(&root)
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk4::Button>().ok())
            .find(|button| button.icon_name().as_deref() == Some(icon))
            .unwrap_or_else(|| panic!("no {icon} button"))
    };
    for (icon, name) in [
        ("media-skip-backward-symbolic", "Previous track"),
        ("media-playback-pause-symbolic", "Pause"),
        ("media-skip-forward-symbolic", "Next track"),
    ] {
        assert!(a11y::is_named(&button(icon), name), "{icon} is {name}");
    }
    button("media-playback-pause-symbolic").emit_clicked();
    button("media-skip-forward-symbolic").emit_clicked();
    settle();
    assert!(matches!(rig.media_commands.try_recv(), Ok(MediaCommand::PlayPause)));
    assert!(matches!(rig.media_commands.try_recv(), Ok(MediaCommand::Next)));
    // What a player controls cannot panic the panel: a negative length, a position at the end
    // of the integers, a seek to the far ends.
    let mut hostile = player();
    hostile.length_us = Some(-5);
    hostile.position_us = Some(i64::MAX);
    rig.media
        .send(MediaState {
            players: vec![hostile],
            current: Some(0),
            refused: 0,
            settled: true,
        })
        .unwrap();
    settle();
    panel.main.media.seek_to(1e30);
    panel.main.media.seek_to(-1e30);
    panel.main.media.set_shown(true);
    settle();
    // A cover that cannot be decoded is the icon, not a hole.
    assert!(panel.main.media.art.paintable().is_some());
    window.destroy();
    rig
}

#[test]
fn the_panel_draws_its_models_and_sends_the_persons_actions() {
    if gtk4::init().is_err() {
        assert!(
            std::env::var_os("ATHANOR_REQUIRE_DISPLAY").is_none(),
            "ATHANOR_REQUIRE_DISPLAY is set and GTK found no display"
        );
        eprintln!("no display: the panel tests were skipped");
        return;
    }
    let rig = the_tiles(rig());
    the_media(rig);
}
