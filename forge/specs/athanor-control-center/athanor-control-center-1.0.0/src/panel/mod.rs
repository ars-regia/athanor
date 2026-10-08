//! The panel itself (CC4) and the stack that holds it with the detail pages (CC5): the tiles,
//! the sliders, the media section and the footer, drawn from the models of athanor-services.
//! A tile or slider whose service or hardware is absent is not shown (BR3), decided by
//! `crate::tiles::present` on every state change.

mod a11y;
mod airplane;
mod footer;
mod media;
mod sliders;
mod tile;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_controls::{audio, battery, bluetooth, bridge, network, Host, Services};
use athanor_services::audio::{self as audio_model, AudioCommand, AudioState};
use athanor_services::battery::{BatteryCommand, BatteryState};
use athanor_services::bluetooth::{BluetoothCommand, BluetoothState};
use athanor_services::media::{MediaCommand, MediaState};
use athanor_services::network::{Link, NetworkCommand, NetworkState};
use gtk4::prelude::*;

pub use airplane::{open as open_rfkill, Rfkill};

use crate::i18n::{tr, tr_with};
use athanor_control_center::tiles::{present, Snapshot};
use athanor_control_center::Page;
use footer::Footer;
use media::MediaView;
use sliders::Slider;
use tile::Tile;

/// The stack child that is the panel itself.
pub const PANEL: &str = "panel";

/// What the panel asks of the program that shows it.
pub struct Hooks {
    /// The Settings button: opens cosmic-settings and closes the panel.
    pub open_settings: Box<dyn Fn()>,
    /// The person turned dark mode on or off.
    pub set_dark: Box<dyn Fn(bool) -> std::io::Result<()>>,
}

/// The last state of each model, which every widget is drawn from.
#[derive(Default)]
struct States {
    network: Option<NetworkState>,
    bluetooth: Option<BluetoothState>,
    audio: AudioState,
    battery: BatteryState,
    media: MediaState,
}

/// The panel's widgets.
struct Main {
    root: gtk4::Box,
    tiles_grid: gtk4::Grid,
    wifi: Tile,
    bluetooth: Tile,
    airplane: Option<(Tile, Rc<Rfkill>)>,
    dark: Tile,
    profile: Tile,
    output: Slider,
    input: Slider,
    brightness: Slider,
    media: Rc<MediaView>,
    footer: Footer,
    dark_now: Cell<bool>,
    laid_out: Cell<Option<u32>>,
    states: RefCell<States>,
}

pub struct Panel {
    stack: gtk4::Stack,
    main: Main,
    services: Option<Services>,
    /// The pages built so far, kept as long as their widgets: a dropped `Page` leaves a
    /// widget that stops updating.
    pages: RefCell<Vec<Box<dyn Any>>>,
    /// The panel is on screen: where a page asks to be shown.
    shown: Cell<bool>,
    /// Closes the panel: set by the surface that shows it.
    closer: RefCell<Option<Box<dyn Fn()>>>,
    me: Weak<Panel>,
}

impl Panel {
    /// `services` is `None` without the runtime of the models; the tiles that need one are
    /// then absent. `rfkill` is `None` where there is no `/dev/rfkill`.
    pub fn new(services: Option<Services>, rfkill: Option<Rc<Rfkill>>, hooks: Hooks) -> Rc<Panel> {
        let panel = Rc::new_cyclic(|me: &Weak<Panel>| Panel {
            stack: gtk4::Stack::new(),
            main: Main::new(me, services.as_ref(), rfkill, hooks),
            services: services.clone(),
            pages: RefCell::default(),
            shown: Cell::new(false),
            closer: RefCell::default(),
            me: me.clone(),
        });
        panel.stack.add_named(&panel.main.root, Some(PANEL));
        for page in Page::ALL {
            panel.stack.add_named(&panel.frame(page), Some(page.id()));
        }
        if let Some((_, device)) = &panel.main.airplane {
            let weak = panel.me.clone();
            device.watch(move || {
                if let Some(panel) = weak.upgrade() {
                    panel.refresh();
                }
            });
        }
        panel.follow(services);
        panel.refresh();
        panel
    }

    pub fn set_closer(&self, closer: impl Fn() + 'static) {
        self.closer.replace(Some(Box::new(closer)));
    }

    fn close(&self) {
        if let Some(closer) = &*self.closer.borrow() {
            closer();
        }
    }

    pub fn widget(&self) -> gtk4::Widget {
        self.stack.clone().upcast()
    }

    /// Shows the panel, or the page `page` names.
    pub fn show(&self, page: Option<Page>) {
        self.stack.set_visible_child_name(page.map_or(PANEL, Page::id));
    }

    /// The panel is on screen, or was hidden; dark mode is read again from where the compositor
    /// keeps it.
    pub fn set_shown(&self, shown: bool) {
        self.shown.set(shown);
        self.main.media.set_shown(shown);
        if shown {
            // A screen reader hears that the panel opened and what it is called.
            self.stack
                .announce(&tr("Control center"), gtk4::AccessibleAnnouncementPriority::Medium);
        }
    }

    /// The theme says whether the dark tile is on.
    pub fn set_dark(&self, dark: bool) {
        self.main.dark_now.set(dark);
        self.main.dark.set(dark, &on_off(dark), "weather-clear-night-symbolic");
    }

    /// The first control, for the keyboard: where Tab starts when the panel opens.
    pub fn focus_first(&self) {
        self.stack.set_visible_child_name(PANEL);
        for tile in self.main.tiles() {
            if tile.root.is_visible() {
                tile.button.grab_focus();
                return;
            }
        }
    }

    /// The page's frame: a header with the back button, and the page, built when first shown.
    fn frame(&self, page: Page) -> gtk4::Box {
        let back = gtk4::Button::from_icon_name("go-previous-symbolic");
        a11y::name(&back, &tr("Back"));
        let weak = self.me.clone();
        back.connect_clicked(move |_| {
            if let Some(panel) = weak.upgrade() {
                panel.show(None);
            }
        });
        let title = gtk4::Label::new(Some(&page_title(page)));
        title.add_css_class("title-4");
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        header.append(&back);
        header.append(&title);
        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        frame.append(&header);
        // The page is built when it is first shown, not at start: a page follows its models
        // for as long as it lives.
        let weak = self.me.clone();
        let built = Cell::new(false);
        frame.connect_map(move |frame| {
            if built.replace(true) {
                return;
            }
            if let Some(panel) = weak.upgrade() {
                frame.append(&panel.page_widget(page));
            }
        });
        frame
    }

    fn host(&self, page: Page) -> Host {
        let (open, usable) = (self.me.clone(), self.me.clone());
        Host::new(
            move || {
                if let Some(panel) = open.upgrade().filter(|panel| panel.shown.get()) {
                    panel.show(Some(page));
                }
            },
            // The page is the panel's own stack child: the person leaves it with Back.
            || {},
            move || usable.upgrade().is_some_and(|panel| panel.shown.get()),
        )
    }

    fn page_widget(&self, page: Page) -> gtk4::Widget {
        let Some(services) = self.services.as_ref() else {
            return placeholder(page);
        };
        let (widget, keep): (gtk4::Widget, Box<dyn Any>) = match page {
            Page::Network => {
                let page = network::Page::without_airplane(services, self.host(Page::Network));
                (page.widget(), Box::new(page))
            }
            Page::Bluetooth => {
                let page = bluetooth::Page::without_pairing(services, self.host(Page::Bluetooth));
                (page.widget(), Box::new(page))
            }
            Page::Audio => {
                let page = audio::Page::new(services);
                (page.widget(), Box::new(page))
            }
            Page::Battery => {
                let page = battery::Page::new(services);
                (page.widget(), Box::new(page))
            }
        };
        self.pages.borrow_mut().push(keep);
        widget
    }

    /// Feeds each model's state to the panel until the model ends.
    fn follow(&self, services: Option<Services>) {
        let Some(services) = services else { return };
        macro_rules! feed {
            ($channel:expr, $store:expr) => {{
                let weak = self.me.clone();
                let store = $store;
                bridge::follow($channel.0.clone(), move |state| {
                    if let Some(panel) = weak.upgrade() {
                        store(&mut panel.main.states.borrow_mut(), state.clone());
                        panel.refresh();
                    }
                });
            }};
        }
        feed!(services.network, |s: &mut States, v| s.network = v);
        feed!(services.bluetooth, |s: &mut States, v| s.bluetooth = v);
        feed!(services.audio, |s: &mut States, v| s.audio = v);
        feed!(services.battery, |s: &mut States, v| s.battery = v);
        feed!(services.media, |s: &mut States, v| s.media = v);
    }

    /// Draws every widget from the states, and shows the ones whose service is there.
    fn refresh(&self) {
        let states = self.main.states.borrow();
        self.main.draw(&states);
    }
}

fn on_off(on: bool) -> String {
    if on { tr("On") } else { tr("Off") }
}

fn page_title(page: Page) -> String {
    match page {
        Page::Network => tr("Network"),
        Page::Bluetooth => tr("Bluetooth"),
        Page::Audio => tr("Sound"),
        Page::Battery => tr("Power"),
    }
}

/// A page that a later task fills, or whose service is away.
fn placeholder(page: Page) -> gtk4::Widget {
    let label = gtk4::Label::new(Some(&tr_with(
        "The {name} page is not available yet.",
        "name",
        &page_title(page),
    )));
    label.set_wrap(true);
    label.upcast()
}

/// The profile after `active` in the order the service offers them.
fn next_profile(offered: &[&'static str], active: &str) -> Option<&'static str> {
    let at = offered.iter().position(|profile| *profile == active);
    offered.get(at.map_or(0, |at| (at + 1) % offered.len())).copied()
}

fn profile_label(profile: &str) -> String {
    match profile {
        "power-saver" => tr("Power saver"),
        "performance" => tr("Performance"),
        _ => tr("Balanced"),
    }
}

impl Main {
    fn new(
        me: &Weak<Panel>,
        services: Option<&Services>,
        rfkill: Option<Rc<Rfkill>>,
        hooks: Hooks,
    ) -> Main {
        // Without models no control is built: `present` finds nothing for any of them.
        let network_tx = services.map(|s| s.network.1.clone());
        let bluetooth_tx = services.map(|s| s.bluetooth.1.clone());
        let audio_tx = services.map(|s| s.audio.1.clone());
        let battery_tx = services.map(|s| s.battery.1.clone());
        let media_tx = services.map(|s| s.media.1.clone());
        macro_rules! send {
            ($tx:expr, $command:expr) => {
                if let Some(tx) = &$tx {
                    if tx.send($command).is_err() {
                        tracing::debug!("a model ended; the command is dropped");
                    }
                }
            };
        }
        let open = |page: Page| -> Option<Box<dyn Fn()>> {
            let weak = me.clone();
            Some(Box::new(move || {
                if let Some(panel) = weak.upgrade() {
                    panel.show(Some(page));
                }
            }))
        };

        let tx = network_tx;
        let wifi = Tile::new(
            &tr("Wi-Fi"),
            "network-wireless-symbolic",
            move |on| send!(tx, NetworkCommand::Wireless(on)),
            open(Page::Network),
        );
        let tx = bluetooth_tx;
        let bluetooth = Tile::new(
            &tr("Bluetooth"),
            "bluetooth-symbolic",
            move |on| send!(tx, BluetoothCommand::Power(on)),
            open(Page::Bluetooth),
        );
        // A device that reports no radio has nothing to block: no tile.
        let airplane = rfkill.filter(|device| device.has_radios()).map(|device| {
            let held = device.clone();
            let weak = me.clone();
            let tile = Tile::new(
                &tr("Airplane mode"),
                "airplane-mode-symbolic",
                move |on| {
                    // The tile follows the kernel's report, not the press: after a refused write
                    // it goes back to what the radios are.
                    if let Err(err) = held.set_blocked(on) {
                        tracing::warn!(error = %err, "the radios were not blocked");
                    }
                    if let Some(panel) = weak.upgrade() {
                        panel.refresh();
                    }
                },
                None,
            );
            (tile, device)
        });
        let set_dark = hooks.set_dark;
        let weak = me.clone();
        let dark = Tile::new(
            &tr("Dark mode"),
            "weather-clear-night-symbolic",
            move |on| {
                match set_dark(on) {
                    // The compositor's theme watch reports the new mode back.
                    Ok(()) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, "the theme mode was not written");
                        if let Some(panel) = weak.upgrade() {
                            panel.set_dark(panel.main.dark_now.get());
                        }
                    }
                }
            },
            None,
        );
        let tx = battery_tx.clone();
        let weak = me.clone();
        let profile = Tile::new(
            &tr("Power mode"),
            "power-profile-balanced-symbolic",
            move |_| {
                let Some(panel) = weak.upgrade() else { return };
                let next = panel
                    .main
                    .states
                    .borrow()
                    .battery
                    .profiles
                    .as_ref()
                    .and_then(|p| next_profile(&p.offered, &p.active));
                if let Some(next) = next {
                    send!(tx, BatteryCommand::SetProfile(next.to_owned()));
                }
                // The tile shows the profile the service reports, so it goes back until then.
                panel.refresh();
            },
            open(Page::Battery),
        );

        let (tx, weak) = (audio_tx.clone(), me.clone());
        let (mute_tx, mute_weak) = (audio_tx.clone(), me.clone());
        let sound = tr("Sound");
        let output = Slider::new(
            &tr("Output volume"),
            "audio-volume-medium-symbolic",
            move |percent| {
                if let Some(name) = weak.upgrade().and_then(|p| p.main.chosen(true)) {
                    send!(tx, AudioCommand::Volume { sink: true, name, percent });
                }
            },
            Some((
                &tr("Mute output"),
                Box::new(move |on| {
                    if let Some(name) = mute_weak.upgrade().and_then(|p| p.main.chosen(true)) {
                        send!(mute_tx, AudioCommand::Mute { sink: true, name, on });
                    }
                }),
            )),
            open(Page::Audio).map(|open| (sound.as_str(), open)),
        );
        let (tx, weak) = (audio_tx.clone(), me.clone());
        let (mute_tx, mute_weak) = (audio_tx, me.clone());
        let input = Slider::new(
            &tr("Input volume"),
            "audio-input-microphone-symbolic",
            move |percent| {
                if let Some(name) = weak.upgrade().and_then(|p| p.main.chosen(false)) {
                    send!(tx, AudioCommand::Volume { sink: false, name, percent });
                }
            },
            Some((
                &tr("Mute microphone"),
                Box::new(move |on| {
                    if let Some(name) = mute_weak.upgrade().and_then(|p| p.main.chosen(false)) {
                        send!(mute_tx, AudioCommand::Mute { sink: false, name, on });
                    }
                }),
            )),
            open(Page::Audio).map(|open| (sound.as_str(), open)),
        );
        let (tx, weak) = (battery_tx, me.clone());
        let brightness = Slider::new(
            &tr("Brightness"),
            "display-brightness-symbolic",
            move |percent| {
                let backlight = weak
                    .upgrade()
                    .and_then(|p| p.main.states.borrow().battery.backlight.clone());
                if let Some(backlight) = backlight {
                    send!(
                        tx,
                        BatteryCommand::SetBrightness {
                            raw: backlight.raw(f64::from(percent)),
                            device: backlight.name,
                        }
                    );
                }
            },
            None,
            None,
        );
        let media = MediaView::new(move |command: MediaCommand| send!(media_tx, command));
        let (open_settings, weak) = (hooks.open_settings, me.clone());
        let footer = Footer::new(move || {
            open_settings();
            if let Some(panel) = weak.upgrade() {
                panel.close();
            }
        });

        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        root.add_css_class("control-center-panel");
        // A group carries a label; a plain box does not.
        root.set_accessible_role(gtk4::AccessibleRole::Group);
        a11y::label(&root, &tr("Control center"));
        let tiles_grid = gtk4::Grid::new();
        tiles_grid.set_row_spacing(8);
        tiles_grid.set_column_spacing(8);
        tiles_grid.set_column_homogeneous(true);
        root.append(&tiles_grid);
        root.append(&output.row);
        root.append(&input.row);
        root.append(&brightness.row);
        root.append(&media.root);
        root.append(&footer.root);
        Main {
            root,
            tiles_grid,
            wifi,
            bluetooth,
            airplane,
            dark,
            profile,
            output,
            input,
            brightness,
            media,
            footer,
            dark_now: Cell::new(false),
            laid_out: Cell::new(None),
            states: RefCell::default(),
        }
    }

    fn tiles(&self) -> Vec<&Tile> {
        let mut tiles = vec![&self.wifi, &self.bluetooth];
        tiles.extend(self.airplane.as_ref().map(|(tile, _)| tile));
        tiles.push(&self.dark);
        tiles.push(&self.profile);
        tiles
    }

    /// The name of the device the slider acts on: the default one, else the first.
    fn chosen(&self, sink: bool) -> Option<String> {
        let states = self.states.borrow();
        let (devices, default) = if sink {
            (&states.audio.outputs, states.audio.default_output.as_deref())
        } else {
            (&states.audio.inputs, states.audio.default_input.as_deref())
        };
        audio_model::chosen(devices, default).map(|at| devices[at].name.clone())
    }

    fn draw(&self, states: &States) {
        let snapshot = Snapshot {
            network: states.network.as_ref(),
            bluetooth: states.bluetooth.as_ref(),
            audio: &states.audio,
            battery: &states.battery,
            media: &states.media,
        };
        let here = present(&snapshot);
        let wifi = states.network.as_ref().and_then(|n| n.wifi.as_ref());
        self.wifi.root.set_visible(here.wifi);
        if let Some(wifi) = wifi {
            let connecting = wifi.networks.iter().any(|n| n.link == Link::Connecting);
            let text = if connecting { tr("Connecting") } else { on_off(wifi.enabled) };
            self.wifi.set(wifi.enabled, &text, "network-wireless-symbolic");
        }
        self.bluetooth.root.set_visible(here.bluetooth);
        if let Some(bluetooth) = &states.bluetooth {
            self.bluetooth
                .set(bluetooth.powered, &on_off(bluetooth.powered), "bluetooth-symbolic");
        }
        if let Some((tile, device)) = &self.airplane {
            let on = device.airplane();
            tile.set(on, &on_off(on), "airplane-mode-symbolic");
        }
        self.profile.root.set_visible(here.power_profile);
        if let Some(profiles) = &states.battery.profiles {
            self.profile.set(
                false,
                &profile_label(&profiles.active),
                profile_icon(&profiles.active),
            );
        }
        self.output.row.set_visible(here.output_volume);
        if let Some(device) = audio_model::chosen(&states.audio.outputs, states.audio.default_output.as_deref())
            .map(|at| &states.audio.outputs[at])
        {
            self.output.set(
                device.percent,
                device.muted,
                audio_model::output_icon(device.percent, device.muted),
            );
        }
        self.input.row.set_visible(here.input_volume);
        if let Some(device) = audio_model::chosen(&states.audio.inputs, states.audio.default_input.as_deref())
            .map(|at| &states.audio.inputs[at])
        {
            self.input.set(device.percent, device.muted, audio_model::input_icon(device.muted));
        }
        self.brightness.row.set_visible(here.brightness);
        if let Some(backlight) = &states.battery.backlight {
            self.brightness
                .set(backlight.percent(), false, "display-brightness-symbolic");
        }
        self.media.root.set_visible(here.media);
        if here.media {
            self.media.update(&states.media);
        }
        self.footer.set(states.battery.battery.as_ref());
        self.lay_out();
    }

    /// Two columns, in order, over the tiles that are shown: a tile that is not built leaves
    /// no hole, and the keyboard meets the tiles in the order they are drawn.
    fn lay_out(&self) {
        let shown: Vec<&Tile> = self.tiles().into_iter().filter(|t| t.root.is_visible()).collect();
        let key = self
            .tiles()
            .iter()
            .fold(0u32, |key, tile| key << 1 | u32::from(tile.root.is_visible()));
        // Moving a tile takes the keyboard's place off it: only when the set changed.
        if self.laid_out.replace(Some(key)) == Some(key) {
            return;
        }
        for tile in self.tiles() {
            if tile.root.parent().is_some() {
                self.tiles_grid.remove(&tile.root);
            }
        }
        for (at, tile) in shown.into_iter().enumerate() {
            self.tiles_grid.attach(&tile.root, (at % 2) as i32, (at / 2) as i32, 1, 1);
        }
    }
}

fn profile_icon(profile: &str) -> &'static str {
    match profile {
        "power-saver" => "power-profile-power-saver-symbolic",
        "performance" => "power-profile-performance-symbolic",
        _ => "power-profile-balanced-symbolic",
    }
}

#[cfg(test)]
mod tests;
