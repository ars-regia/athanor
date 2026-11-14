//! The panel from the state of the models, driven the way the person drives it (ST7): the
//! widgets are Sal's, so no display is needed. Each test runs on a main context of its own,
//! where the models' bridge and the rfkill stream run.

use std::cell::{Cell, RefCell};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use athanor_control_center::rfkill::{self, Event as RfkillEvent};
use athanor_sal::{Key, Theme};
use athanor_services::audio::Device as Sink;
use athanor_services::battery::{Backlight, Battery, Charge, Profiles};
use athanor_services::media::{MediaCommand, Player, Track};
use athanor_services::network::Wifi;
use tokio::sync::{mpsc, watch};

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

/// Runs `test` with a main context of its own as the thread's default, as the tests run on
/// threads of their own.
fn on_context(test: impl FnOnce(&glib::MainContext)) {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| test(&context))
        .expect("a new context is free to own");
}

/// Lets the models' bridge and the rfkill stream run until they wait.
fn settle(context: &glib::MainContext) {
    for _ in 0..50 {
        while context.iteration(false) {}
    }
}

/// The view built in a new `Ui`, as the surface builds it.
fn view(panel: &mut Panel, icons: &mut Icons) -> Ui {
    let mut ui = Ui::new("Control center", Theme::default(), false);
    let content = ui.add(Ui::ROOT, Kind::Group("Control center".into()));
    panel.build(&mut ui, content, icons);
    ui
}

/// Every widget of `ui` that is shown, in reading order.
fn shown(ui: &Ui) -> Vec<WidgetId> {
    let mut found = Vec::new();
    let mut stack = vec![Ui::ROOT];
    while let Some(id) = stack.pop() {
        if !ui.is_visible(id) {
            continue;
        }
        found.push(id);
        stack.extend(ui.widget(id).children.iter().rev());
    }
    found
}

/// The text a widget shows or is named by.
fn text(ui: &Ui, id: WidgetId) -> Option<&str> {
    match &ui.widget(id).kind {
        Kind::Label(t) | Kind::Button(t) | Kind::Group(t) => Some(t),
        Kind::Tile(tile) => Some(&tile.title),
        Kind::IconButton { label, .. } => Some(label),
        Kind::IconToggle(toggle) => Some(&toggle.label),
        Kind::Image { description, .. } => Some(description),
        _ => None,
    }
}

fn says(ui: &Ui, wanted: &str) -> bool {
    shown(ui).into_iter().any(|id| text(ui, id) == Some(wanted))
}

/// The shown widget named `wanted`.
fn named(ui: &Ui, wanted: &str) -> WidgetId {
    shown(ui)
        .into_iter()
        .find(|&id| text(ui, id) == Some(wanted))
        .unwrap_or_else(|| panic!("nothing shown is named {wanted:?}"))
}

/// What the person's activation of `id` makes the panel do.
fn press(panel: &mut Panel, ui: &mut Ui, id: WidgetId) -> Action {
    let event = ui.activate(id).expect("the widget takes the activation");
    panel.event(ui, &event)
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
        art: Some(athanor_services::media::Art::Data(Arc::from(
            &b"not a picture"[..],
        ))),
    }
}

/// A regular file standing for /dev/rfkill: it holds the radios the kernel announced, and
/// takes what the tile writes. Each call has a file of its own, as the tests run at once.
fn fake_rfkill(radios: usize) -> (Rc<Rfkill>, PathBuf) {
    thread_local!(static NEXT: Cell<u32> = const { Cell::new(0) });
    let n = NEXT.with(|next| next.replace(next.get() + 1));
    let thread = format!("{:?}", std::thread::current().id())
        .replace(|c: char| !c.is_ascii_alphanumeric(), "");
    let name = format!("athanor-rfkill-{}-{thread}-{n}", std::process::id());
    let path = std::env::temp_dir().join(name);
    let add = |idx| RfkillEvent {
        idx,
        kind: 1,
        op: rfkill::OP_ADD,
        soft: false,
        hard: false,
    };
    let events: Vec<u8> = (0..radios as u32)
        .flat_map(|idx| add(idx).encode())
        .collect();
    std::fs::write(&path, events).expect("the fake rfkill is written");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("the fake rfkill opens");
    (Rfkill::new(file), path)
}

#[test]
fn the_tiles_follow_their_models_and_send_the_persons_presses() {
    on_context(|context| {
        let mut rig = rig();
        let (rfkill, path) = fake_rfkill(1);
        let mut icons = Icons::new(Theme::default().text);
        let mut panel = Panel::new(Some(rig.services.clone()), Some(rfkill), false);
        settle(context);
        let mut ui = view(&mut panel, &mut icons);

        // A model that has not answered builds nothing for it (BR3).
        assert!(!says(&ui, "Wi-Fi"));
        assert!(!says(&ui, "Bluetooth"));
        assert!(!says(&ui, "Power mode"));
        // Dark mode and airplane mode do not depend on a model.
        assert!(says(&ui, "Dark mode"));
        assert!(says(&ui, "Airplane mode"));

        rig.network.send(Some(wifi_state())).unwrap();
        rig.bluetooth.send(Some(bluetooth_state())).unwrap();
        rig.audio.send(audio_state()).unwrap();
        rig.battery.send(battery_state()).unwrap();
        settle(context);
        assert_eq!(
            panel.turn(&mut ui, &mut icons),
            Action::Rebuild,
            "new tiles"
        );
        let mut ui = view(&mut panel, &mut icons);
        assert!(says(&ui, "Wi-Fi") && says(&ui, "Bluetooth") && says(&ui, "Power mode"));
        assert!(says(&ui, "80%"), "the battery level is in the footer");
        assert_eq!(panel.turn(&mut ui, &mut icons), Action::Done, "nothing new");
        assert_eq!(
            panel.first(),
            Some(named(&ui, "Wi-Fi")),
            "the keyboard starts on it"
        );

        // A tile is a toggle that follows the press, and sends it.
        let wifi = named(&ui, "Wi-Fi");
        assert!(ui.is_active(wifi));
        assert_eq!(press(&mut panel, &mut ui, wifi), Action::Done);
        assert!(!ui.is_active(wifi), "the pressed state follows the press");
        assert!(matches!(
            rig.network_commands.try_recv(),
            Ok(NetworkCommand::Wireless(false))
        ));
        // The panel registers no agent, for NetworkManager or for BlueZ, and sends no other command.
        assert!(
            rig.network_commands.try_recv().is_err(),
            "no RegisterAgent for NetworkManager"
        );
        assert!(
            rig.bluetooth_commands.try_recv().is_err(),
            "no agent for BlueZ"
        );
        let bluetooth = named(&ui, "Bluetooth");
        press(&mut panel, &mut ui, bluetooth);
        assert!(matches!(
            rig.bluetooth_commands.try_recv(),
            Ok(BluetoothCommand::Power(false))
        ));

        // The power-mode tile steps to the next profile the service offers, and shows the
        // profile the service reports until it reports another.
        let profile = named(&ui, "Power mode");
        press(&mut panel, &mut ui, profile);
        assert!(
            matches!(rig.battery_commands.try_recv(), Ok(BatteryCommand::SetProfile(p)) if p == "performance")
        );
        assert!(!ui.is_active(profile));

        // Airplane mode writes struct rfkill_event of RFKILL_OP_CHANGE_ALL.
        let airplane = named(&ui, "Airplane mode");
        press(&mut panel, &mut ui, airplane);
        let mut written = Vec::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_end(&mut written)
            .unwrap();
        assert_eq!(written[8..], rfkill::block_all(true).encode());
        std::fs::remove_file(&path).ok();

        // Dark mode is the surface's to apply.
        let dark = named(&ui, "Dark mode");
        assert_eq!(press(&mut panel, &mut ui, dark), Action::SetDark(true));

        // The sliders send the person's change in whole percents, to the device in use.
        let ids = panel.ids.as_ref().expect("the panel is built");
        let (output, input, brightness) = (
            ids.output.as_ref().expect("an output slider"),
            ids.input.as_ref().expect("an input slider"),
            ids.brightness.as_ref().expect("a brightness slider"),
        );
        let (output, mute, input, brightness) = (
            output.slider,
            output.mute.expect("a mute toggle"),
            input.slider,
            brightness.slider,
        );
        assert_eq!(ui.value(output), Some(40.0), "the model's state is drawn");
        assert_eq!(ui.value(input), Some(70.0));
        assert_eq!(ui.value(brightness), Some(50.0));
        let event = ui.set_value(output, 55.0).expect("the slider moves");
        panel.event(&mut ui, &event);
        assert!(matches!(
            rig.audio_commands.try_recv(),
            Ok(AudioCommand::Volume {
                sink: true,
                percent: 55,
                ..
            })
        ));
        press(&mut panel, &mut ui, mute);
        assert!(matches!(
            rig.audio_commands.try_recv(),
            Ok(AudioCommand::Mute {
                sink: true,
                on: true,
                ..
            })
        ));
        let event = ui.set_value(brightness, 30.0).expect("the slider moves");
        panel.event(&mut ui, &event);
        assert!(matches!(
            rig.battery_commands.try_recv(),
            Ok(BatteryCommand::SetBrightness { raw: 30, .. })
        ));

        // Everything the person can use is reached with Tab.
        let usable: Vec<WidgetId> = shown(&ui)
            .into_iter()
            .filter(|&id| {
                matches!(
                    ui.widget(id).kind,
                    Kind::Tile(_) | Kind::IconButton { .. } | Kind::IconToggle(_) | Kind::Slider(_)
                )
            })
            .collect();
        let mut reached = Vec::new();
        ui.set_focus(None);
        for _ in 0..2 * usable.len() + 2 {
            ui.key(Key::Tab);
            reached.extend(ui.focus());
        }
        for id in &usable {
            assert!(
                reached.contains(id),
                "Tab does not reach {:?}",
                text(&ui, *id)
            );
        }

        // An arrow opens the tile's page; Back returns to the panel.
        let arrow = named(&ui, "Open the Wi-Fi page");
        assert_eq!(press(&mut panel, &mut ui, arrow), Action::Rebuild);
        assert_eq!(panel.view(), View::Page(Page::Network));
        let mut ui = view(&mut panel, &mut icons);
        let back = named(&ui, "Back");
        assert_eq!(panel.first(), Some(back), "the keyboard starts on Back");
        assert_eq!(press(&mut panel, &mut ui, back), Action::Rebuild);
        assert_eq!(panel.view(), View::Panel);
        let mut ui = view(&mut panel, &mut icons);
        let sound = named(&ui, "Open the Sound page");
        assert_eq!(press(&mut panel, &mut ui, sound), Action::Rebuild);
        assert_eq!(panel.view(), View::Page(Page::Audio));
        panel.show(None);
        let mut ui = view(&mut panel, &mut icons);

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
        settle(context);
        panel.turn(&mut ui, &mut icons);
        let arrow = named(&ui, "Open the Bluetooth page");
        assert_eq!(press(&mut panel, &mut ui, arrow), Action::Rebuild);
        let mut ui = view(&mut panel, &mut icons);
        panel.turn(&mut ui, &mut icons);
        assert!(!says(&ui, "Nearby devices") && !says(&ui, "Unpaired speaker"));
        assert!(!says(&ui, "Pair"), "no pairing entry");
        while let Ok(command) = rig.bluetooth_commands.try_recv() {
            assert!(!matches!(
                command,
                BluetoothCommand::Discovery(_) | BluetoothCommand::Pair(_)
            ));
        }
        panel.closed(&mut ui);

        // The footer's Settings button opens Settings, and is named for it.
        panel.show(None);
        let mut ui = view(&mut panel, &mut icons);
        let settings = named(&ui, "Settings");
        assert_eq!(ui.tooltip(settings), Some("Settings"));
        assert_eq!(press(&mut panel, &mut ui, settings), Action::OpenSettings);
    });
}

#[test]
fn the_media_section_and_the_airplane_tile_need_what_they_show() {
    on_context(|context| {
        let mut rig = rig();
        let mut icons = Icons::new(Theme::default().text);
        let mut panel = Panel::new(Some(rig.services.clone()), None, false);
        settle(context);
        let mut ui = view(&mut panel, &mut icons);
        assert!(!says(&ui, "A song"), "no player, no media section");
        assert!(
            !says(&ui, "Airplane mode"),
            "no /dev/rfkill, no airplane tile"
        );
        // A device that reports no radio is no airplane tile either.
        let (empty, empty_path) = fake_rfkill(0);
        let mut bare = Panel::new(Some(rig.services.clone()), Some(empty), false);
        settle(context);
        let bare_ui = view(&mut bare, &mut icons);
        assert!(
            !says(&bare_ui, "Airplane mode"),
            "no radio, no airplane tile"
        );
        std::fs::remove_file(&empty_path).ok();

        rig.media
            .send(MediaState {
                players: vec![player()],
                current: Some(0),
                refused: 0,
                settled: true,
            })
            .unwrap();
        settle(context);
        assert_eq!(
            panel.turn(&mut ui, &mut icons),
            Action::Done,
            "the same tiles"
        );
        assert!(says(&ui, "A song") && says(&ui, "An artist"));
        // A cover that cannot be decoded is the icon, not a hole.
        assert!(says(&ui, "Cover"));
        let previous = named(&ui, "Previous track");
        let pause = named(&ui, "Pause");
        let next = named(&ui, "Next track");
        assert_eq!(press(&mut panel, &mut ui, pause), Action::Done);
        press(&mut panel, &mut ui, next);
        press(&mut panel, &mut ui, previous);
        assert!(matches!(
            rig.media_commands.try_recv(),
            Ok(MediaCommand::PlayPause)
        ));
        assert!(matches!(
            rig.media_commands.try_recv(),
            Ok(MediaCommand::Next)
        ));
        assert!(matches!(
            rig.media_commands.try_recv(),
            Ok(MediaCommand::Previous)
        ));

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
        settle(context);
        panel.turn(&mut ui, &mut icons);
        panel.media.seek_to(1e30);
        panel.media.seek_to(-1e30);
        panel.tick(&mut ui);
    });
}

#[test]
fn without_the_models_only_dark_mode_shows_and_a_page_says_it_is_unavailable() {
    on_context(|_| {
        let mut icons = Icons::new(Theme::default().text);
        let mut panel = Panel::new(None, None, true);
        let mut ui = view(&mut panel, &mut icons);
        let tiles: Vec<&str> = shown(&ui)
            .into_iter()
            .filter(|&id| matches!(ui.widget(id).kind, Kind::Tile(_)))
            .filter_map(|id| text(&ui, id))
            .collect();
        assert_eq!(tiles, ["Dark mode"]);
        assert!(
            ui.is_active(named(&ui, "Dark mode")),
            "dark as the compositor keeps it"
        );
        panel.set_dark(Some(&mut ui), false);
        assert!(!ui.is_active(named(&ui, "Dark mode")));
        panel.show(Some(Page::Battery));
        ui = view(&mut panel, &mut icons);
        assert!(says(&ui, "The Power page is not available yet."));
    });
}
