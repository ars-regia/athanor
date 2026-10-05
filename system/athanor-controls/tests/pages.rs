//! Each page, built on a display from the state of a model and driven the way the person
//! drives it. GTK may be started on one thread only, so the pages share one test.
//!
//! Without a display the test says so and passes, unless `ATHANOR_REQUIRE_DISPLAY` is set:
//! `forge/test/shell/with-display.sh` sets it, so a gate cannot pass without having run.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use athanor_controls::{audio, battery, bluetooth, network, Host, Services};
use athanor_services::audio::{AudioCommand, AudioState, Device as Sink};
use athanor_services::battery::{
    Backlight, Battery, BatteryCommand, BatteryState, Charge, Profiles,
};
use athanor_services::bluetooth::{BluetoothCommand, BluetoothState, Device as Peer};
use athanor_services::media::{MediaCommand, MediaState};
use athanor_services::network::{Link, Network, NetworkCommand, NetworkState, Security, Wifi};
use gtk4::glib;
use gtk4::prelude::*;
use tokio::sync::{mpsc, watch};

struct Rig {
    services: Services,
    battery: watch::Sender<BatteryState>,
    battery_commands: mpsc::UnboundedReceiver<BatteryCommand>,
    network: watch::Sender<Option<NetworkState>>,
    network_commands: mpsc::UnboundedReceiver<NetworkCommand>,
    bluetooth: watch::Sender<Option<BluetoothState>>,
    bluetooth_commands: mpsc::UnboundedReceiver<BluetoothCommand>,
    audio: watch::Sender<AudioState>,
    audio_commands: mpsc::UnboundedReceiver<AudioCommand>,
    // Kept so the media model does not look ended.
    _media: (
        watch::Sender<MediaState>,
        mpsc::UnboundedReceiver<MediaCommand>,
    ),
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
        battery,
        battery_commands,
        network,
        network_commands,
        bluetooth,
        bluetooth_commands,
        audio,
        audio_commands,
        _media: (media, media_commands),
    }
}

/// Runs the main context until the pages have applied what was published.
fn settle() {
    let context = glib::MainContext::default();
    for _ in 0..50 {
        while context.iteration(false) {}
    }
}

/// Every text a person can read in `root`: labels, and the labels of buttons and switches.
fn texts(root: &gtk4::Widget) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if let Some(label) = widget.downcast_ref::<gtk4::Label>() {
            found.push(label.text().to_string());
        }
        if let Some(button) = widget.downcast_ref::<gtk4::CheckButton>() {
            found.extend(button.label().map(|label| label.to_string()));
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            stack.push(next.clone());
            child = next.next_sibling();
        }
    }
    found
}

fn has(root: &gtk4::Widget, text: &str) -> bool {
    texts(root).iter().any(|found| found == text)
}

/// The first widget of type `W` under `root` whose text satisfies `pick`.
fn find<W: IsA<gtk4::Widget>>(root: &gtk4::Widget, pick: impl Fn(&W) -> bool) -> Option<W> {
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if let Some(found) = widget.downcast_ref::<W>().filter(|found| pick(found)) {
            return Some(found.clone());
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            stack.push(next.clone());
            child = next.next_sibling();
        }
    }
    None
}

/// The row that says `text`, and that tells assistive technologies its name as well: what a
/// screen reader says does not come from the visible label.
fn row_with(root: &gtk4::Widget, text: &str) -> gtk4::Button {
    let row = find::<gtk4::Button>(root, |button| has(button.upcast_ref(), text))
        .unwrap_or_else(|| panic!("no row says {text:?}"));
    assert!(
        gtk4::test_accessible_has_property(&row, gtk4::AccessibleProperty::Label),
        "the row {text:?} has no accessible name"
    );
    row
}

fn battery_page(rig: &mut Rig) {
    let page = battery::Page::new(&rig.services);
    let shown = Rc::new(Cell::new(false));
    let seen = shown.clone();
    page.connect_shown(move |state| seen.set(state.battery.is_some()));
    assert!(!shown.get(), "no battery yet");
    rig.battery
        .send(BatteryState {
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
        })
        .unwrap();
    settle();
    let root = page.widget();
    assert!(shown.get(), "the surface was told a battery is present");
    for text in [
        "80 %",
        "2 h 0 min left",
        "Power mode",
        "Power saver",
        "Balanced",
        "Performance",
        "Screen brightness",
    ] {
        assert!(has(&root, text), "the battery page lacks {text:?}");
    }
    let performance = find::<gtk4::CheckButton>(&root, |button| {
        button.label().as_deref() == Some("Performance")
    })
    .expect("the Performance radio");
    assert!(
        gtk4::test_accessible_has_property(&performance, gtk4::AccessibleProperty::Label),
        "the Performance radio has no accessible name"
    );
    performance.set_active(true);
    match rig.battery_commands.try_recv() {
        Ok(BatteryCommand::SetProfile(profile)) => assert_eq!(profile, "performance"),
        other => panic!("choosing Performance sent {:?}", other.is_ok()),
    }
}

fn network_page(rig: &mut Rig) {
    let opened = Rc::new(Cell::new(0));
    let counted = opened.clone();
    let page = network::Page::new(
        &rig.services,
        Host::new(move || counted.set(counted.get() + 1), || (), || true),
    );
    let icon = Rc::new(RefCell::new(String::new()));
    let seen = icon.clone();
    page.connect_shown(move |state| {
        if let Some(state) = state {
            seen.replace(athanor_services::network::icon(state).to_owned());
        }
    });
    rig.network
        .send(Some(NetworkState {
            wired: Some(true),
            wifi: Some(Wifi {
                device: "/wlan".to_owned(),
                enabled: true,
                networks: vec![Network {
                    ssid: b"Cafe".to_vec(),
                    label: "Cafe".to_owned(),
                    strength: 70,
                    security: Security::Personal { sae: false },
                    access_point: "/ap".to_owned(),
                    link: Link::Idle,
                    active: None,
                    saved: None,
                }],
            }),
            vpns: Vec::new(),
            airplane: false,
            refused: 0,
        }))
        .unwrap();
    settle();
    let root = page.widget();
    assert_eq!(*icon.borrow(), "network-wired-symbolic");
    for text in ["Network", "Wired: connected", "Wi-Fi", "Airplane mode"] {
        assert!(has(&root, text), "the network page lacks {text:?}");
    }
    let airplane = find::<gtk4::Switch>(&root, |_| true).expect("a switch row");
    assert!(
        gtk4::test_accessible_has_relation(&airplane, gtk4::AccessibleRelation::LabelledBy),
        "the switch is not labelled by its row"
    );
    // A secured network without a saved profile asks for its password, on the surface.
    row_with(&root, "Cafe, secured").emit_clicked();
    assert_eq!(opened.get(), 1, "the page asked its surface to show");
    assert!(has(&root, "Password for Cafe"));
    assert!(rig.network_commands.try_recv().is_err());
}

fn bluetooth_page(rig: &mut Rig) {
    let page = bluetooth::Page::new(&rig.services, Host::new(|| (), || (), || true));
    rig.bluetooth
        .send(Some(BluetoothState {
            adapter: "/hci0".to_owned(),
            powered: true,
            discovering: false,
            pairable: false,
            discoverable: false,
            paired: vec![Peer {
                path: "/dev1".to_owned(),
                label: "Headset".to_owned(),
                icon: "audio-headset-symbolic",
                paired: true,
                connected: false,
            }],
            nearby: Vec::new(),
            refused: 0,
        }))
        .unwrap();
    settle();
    let root = page.widget();
    assert!(has(&root, "Bluetooth"));
    row_with(&root, "Headset").emit_clicked();
    match rig.bluetooth_commands.try_recv() {
        Ok(BluetoothCommand::Connect(path)) => assert_eq!(path, "/dev1"),
        other => panic!("pressing a paired device sent {:?}", other.is_ok()),
    }
}

fn audio_page(rig: &mut Rig) {
    let page = audio::Page::new(&rig.services);
    let icon = Rc::new(RefCell::new(None));
    let seen = icon.clone();
    page.connect_shown(move |face| {
        seen.replace(face.map(|face| face.icon));
    });
    assert_eq!(*icon.borrow(), None, "no device yet");
    rig.audio
        .send(AudioState {
            outputs: vec![
                Sink {
                    name: "speakers".to_owned(),
                    label: "Speakers".to_owned(),
                    percent: 40.0,
                    muted: false,
                },
                Sink {
                    name: "headphones".to_owned(),
                    label: "Headphones".to_owned(),
                    percent: 20.0,
                    muted: false,
                },
            ],
            default_output: Some("speakers".to_owned()),
            settled: true,
            ..AudioState::default()
        })
        .unwrap();
    settle();
    let root = page.widget();
    assert!(
        icon.borrow().is_some(),
        "the surface was told a device exists"
    );
    for text in ["Output", "Mute output", "Speakers, in use", "Headphones"] {
        assert!(has(&root, text), "the audio page lacks {text:?}");
    }
    row_with(&root, "Headphones").emit_clicked();
    match rig.audio_commands.try_recv() {
        Ok(AudioCommand::Default { sink, name }) => assert!(sink && name == "headphones"),
        other => panic!("choosing a device sent {:?}", other.is_ok()),
    }
}

#[test]
fn the_pages_draw_their_models_and_send_the_persons_actions() {
    if gtk4::init().is_err() {
        assert!(
            std::env::var_os("ATHANOR_REQUIRE_DISPLAY").is_none(),
            "ATHANOR_REQUIRE_DISPLAY is set and GTK found no display"
        );
        eprintln!("no display: the page tests were skipped");
        return;
    }
    let mut rig = rig();
    battery_page(&mut rig);
    network_page(&mut rig);
    bluetooth_page(&mut rig);
    audio_page(&mut rig);
}
