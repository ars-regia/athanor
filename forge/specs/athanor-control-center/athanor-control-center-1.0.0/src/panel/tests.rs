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
fn fake_rfkill() -> (Rc<Rfkill>, PathBuf) {
    let path = std::env::temp_dir().join(format!("athanor-rfkill-{}", std::process::id()));
    let add = Event {
        idx: 0,
        kind: 1,
        op: rfkill::OP_ADD,
        soft: false,
        hard: false,
    };
    std::fs::write(&path, add.encode()).expect("the fake rfkill is written");
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
    let (rfkill, path) = fake_rfkill();
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
    assert!(gtk4::test_accessible_has_property(&wifi, gtk4::AccessibleProperty::Label));
    assert!(gtk4::test_accessible_has_state(&wifi, gtk4::AccessibleState::Pressed));
    wifi.emit_clicked();
    settle();
    assert!(matches!(rig.network_commands.try_recv(), Ok(NetworkCommand::Wireless(false))));
    // The panel registers a guest agent, never the network agent, and sends no other command.
    assert!(rig.network_commands.try_recv().is_err(), "no RegisterAgent for NetworkManager");
    assert!(matches!(rig.bluetooth_commands.try_recv(), Ok(BluetoothCommand::RegisterGuestAgent)));
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
    for scale in &scales {
        assert!(gtk4::test_accessible_has_property(scale, gtk4::AccessibleProperty::Label));
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
    assert!(gtk4::test_accessible_has_property(&back, gtk4::AccessibleProperty::Label));
    back.emit_clicked();
    settle();
    assert_eq!(panel.stack.visible_child_name().as_deref(), Some(PANEL));

    // The panel has a name, and a state it announces when it opens.
    assert!(gtk4::test_accessible_has_property(&panel.main.root, gtk4::AccessibleProperty::Label));

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
    for icon in ["media-skip-backward-symbolic", "media-playback-pause-symbolic", "media-skip-forward-symbolic"] {
        assert!(gtk4::test_accessible_has_property(&button(icon), gtk4::AccessibleProperty::Label));
    }
    button("media-playback-pause-symbolic").emit_clicked();
    button("media-skip-forward-symbolic").emit_clicked();
    settle();
    assert!(matches!(rig.media_commands.try_recv(), Ok(MediaCommand::PlayPause)));
    assert!(matches!(rig.media_commands.try_recv(), Ok(MediaCommand::Next)));
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
