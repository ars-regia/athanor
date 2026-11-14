//! The panel itself (CC4) and its detail pages (CC5): the tiles, the sliders, the media section
//! and the footer, drawn from the models of athanor-services. A tile or slider whose service or
//! hardware is absent is not shown (BR3), decided by `tiles::present` on every state change.
//! The panel and each page are one view: the surface shows one at a time, built when it comes on
//! screen, as the GTK stack built a page when it was first shown.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_control_center::tiles::{present, Present, Snapshot};
use athanor_control_center::Page;
use athanor_controls::sal::audio::{self, MediaIcons};
use athanor_controls::sal::{battery, bluetooth, network, Turn};
use athanor_controls::{bridge, Services};
use athanor_sal::{Event, Kind, TextStyle, Ui, WidgetId};
use athanor_services::audio::{self as audio_model, AudioCommand, AudioState};
use athanor_services::battery::{self as battery_model, BatteryCommand, BatteryState};
use athanor_services::bluetooth::{BluetoothCommand, BluetoothState};
use athanor_services::media::MediaState;
use athanor_services::network::{Link, NetworkCommand, NetworkState};
use athanor_style::sal;

use crate::airplane::Rfkill;
use crate::i18n::{tr, tr_with};
use crate::icons::Icons;
use crate::media::Media;

/// The width of the panel, as GTK's `width-request` on it (CC4).
pub const WIDTH: f32 = 360.0;

/// The last state of each model, which every widget is drawn from.
#[derive(Default)]
struct States {
    network: Option<NetworkState>,
    bluetooth: Option<BluetoothState>,
    audio: AudioState,
    battery: BatteryState,
    media: MediaState,
}

/// The tiles, in the order they are drawn (CC4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TileKind {
    Wifi,
    Bluetooth,
    Airplane,
    Dark,
    Profile,
}

impl TileKind {
    const ALL: [TileKind; 5] = [
        TileKind::Wifi,
        TileKind::Bluetooth,
        TileKind::Airplane,
        TileKind::Dark,
        TileKind::Profile,
    ];

    fn title(self) -> String {
        match self {
            TileKind::Wifi => tr("Wi-Fi"),
            TileKind::Bluetooth => tr("Bluetooth"),
            TileKind::Airplane => tr("Airplane mode"),
            TileKind::Dark => tr("Dark mode"),
            TileKind::Profile => tr("Power mode"),
        }
    }

    /// The page the tile's arrow opens.
    fn page(self) -> Option<Page> {
        match self {
            TileKind::Wifi => Some(Page::Network),
            TileKind::Bluetooth => Some(Page::Bluetooth),
            TileKind::Profile => Some(Page::Battery),
            TileKind::Airplane | TileKind::Dark => None,
        }
    }
}

/// A slider row: the volume of an output or an input, or the brightness.
struct SliderRow {
    row: WidgetId,
    /// The mute toggle, which shows the icon; else the icon alone.
    mute: Option<WidgetId>,
    icon: Option<WidgetId>,
    slider: WidgetId,
    arrow: Option<WidgetId>,
}

/// The widgets of the panel's view.
struct Ids {
    first: Option<WidgetId>,
    tiles: Vec<(TileKind, WidgetId, Option<WidgetId>)>,
    output: Option<SliderRow>,
    input: Option<SliderRow>,
    brightness: Option<SliderRow>,
    battery: WidgetId,
    battery_icon: WidgetId,
    battery_level: WidgetId,
    settings: WidgetId,
}

/// The view the surface shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Panel,
    Page(Page),
}

/// What the surface does after an event or a turn.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// The widgets are as the surface draws them.
    Done,
    /// The view's shape changed: the surface builds it again.
    Rebuild,
    /// The panel closes.
    Close,
    /// The Settings button: Settings opens, and the panel closes.
    OpenSettings,
    /// The person turned dark mode on or off.
    SetDark(bool),
}

/// The pages built so far, kept as long as the panel: a page follows its model while it lives.
#[derive(Default)]
struct Pages {
    network: Option<network::Page>,
    bluetooth: Option<bluetooth::Page>,
    audio: Option<audio::Page>,
    battery: Option<battery::Page>,
}

pub struct Panel {
    services: Option<Services>,
    states: Rc<RefCell<States>>,
    /// A model reported a state, or the radios changed, since the last turn.
    changed: Rc<Cell<bool>>,
    rfkill: Option<Rc<Rfkill>>,
    /// Whether the theme is dark, as the compositor keeps it.
    dark: bool,
    view: View,
    ids: Option<Ids>,
    /// The tiles the panel's view was built with.
    built: Vec<TileKind>,
    media: Media,
    pages: Pages,
    back: Option<WidgetId>,
}

fn on_off(on: bool) -> String {
    if on {
        tr("On")
    } else {
        tr("Off")
    }
}

pub fn page_title(page: Page) -> String {
    match page {
        Page::Network => tr("Network"),
        Page::Bluetooth => tr("Bluetooth"),
        Page::Audio => tr("Sound"),
        Page::Battery => tr("Power"),
    }
}

/// The profile after `active` in the order the service offers them.
fn next_profile(offered: &[&'static str], active: &str) -> Option<&'static str> {
    let at = offered.iter().position(|profile| *profile == active);
    offered
        .get(at.map_or(0, |at| (at + 1) % offered.len()))
        .copied()
}

fn profile_label(profile: &str) -> String {
    match profile {
        "power-saver" => tr("Power saver"),
        "performance" => tr("Performance"),
        _ => tr("Balanced"),
    }
}

fn profile_icon(profile: &str) -> &'static str {
    match profile {
        "power-saver" => "power-profile-power-saver-symbolic",
        "performance" => "power-profile-performance-symbolic",
        _ => "power-profile-balanced-symbolic",
    }
}

fn tile_icon(kind: TileKind) -> &'static str {
    match kind {
        TileKind::Wifi => "network-wireless-symbolic",
        TileKind::Bluetooth => "bluetooth-symbolic",
        TileKind::Airplane => "airplane-mode-symbolic",
        TileKind::Dark => "weather-clear-night-symbolic",
        TileKind::Profile => "power-profile-balanced-symbolic",
    }
}

fn style(step: sal::Step) -> TextStyle {
    TextStyle {
        scale: step.scale,
        weight: step.weight,
        features: Vec::new(),
    }
}

fn send<T>(tx: Option<&tokio::sync::mpsc::UnboundedSender<T>>, command: T) {
    if let Some(tx) = tx {
        if tx.send(command).is_err() {
            tracing::debug!("a model ended; the command is dropped");
        }
    }
}

impl Panel {
    /// `services` is `None` without the runtime of the models; the tiles that need one are then
    /// absent. `rfkill` is `None` where there is no `/dev/rfkill`.
    pub fn new(services: Option<Services>, rfkill: Option<Rc<Rfkill>>, dark: bool) -> Panel {
        let states: Rc<RefCell<States>> = Rc::default();
        let changed = Rc::new(Cell::new(true));
        if let Some(services) = &services {
            macro_rules! feed {
                ($channel:expr, $store:expr) => {{
                    let (states, changed) = (states.clone(), changed.clone());
                    let store = $store;
                    bridge::follow($channel.0.clone(), move |state| {
                        store(&mut states.borrow_mut(), state.clone());
                        changed.set(true);
                    });
                }};
            }
            feed!(services.network, |s: &mut States, v| s.network = v);
            feed!(services.bluetooth, |s: &mut States, v| s.bluetooth = v);
            feed!(services.audio, |s: &mut States, v| s.audio = v);
            feed!(services.battery, |s: &mut States, v| s.battery = v);
            feed!(services.media, |s: &mut States, v| s.media = v);
        }
        // A device that reports no radio has nothing to block: no tile.
        let rfkill = rfkill.filter(|device| device.has_radios());
        if let Some(device) = &rfkill {
            let changed = changed.clone();
            device.watch(move || changed.set(true));
        }
        Panel {
            services,
            states,
            changed,
            rfkill,
            dark,
            view: View::Panel,
            ids: None,
            built: Vec::new(),
            media: Media::default(),
            pages: Pages::default(),
            back: None,
        }
    }

    pub fn view(&self) -> View {
        self.view
    }

    /// Shows the panel, or the page `page` names, from the next build on.
    pub fn show(&mut self, page: Option<Page>) {
        self.view = page.map_or(View::Panel, View::Page);
    }

    /// The theme says whether the dark tile is on.
    pub fn set_dark(&mut self, ui: Option<&mut Ui>, dark: bool) {
        self.dark = dark;
        let tile = self.tile(TileKind::Dark);
        if let (Some(ui), Some(tile)) = (ui, tile) {
            ui.set_active(tile, dark);
            ui.set_tile_subtitle(tile, &on_off(dark));
        }
    }

    fn tile(&self, kind: TileKind) -> Option<WidgetId> {
        let ids = self.ids.as_ref()?;
        ids.tiles.iter().find(|t| t.0 == kind).map(|t| t.1)
    }

    /// The control the keyboard starts on when the panel opens: its first tile.
    pub fn first(&self) -> Option<WidgetId> {
        match self.view {
            View::Panel => self.ids.as_ref().and_then(|ids| ids.first),
            View::Page(_) => self.back,
        }
    }

    fn present(&self) -> Present {
        let states = self.states.borrow();
        present(&Snapshot {
            network: states.network.as_ref(),
            bluetooth: states.bluetooth.as_ref(),
            audio: &states.audio,
            battery: &states.battery,
            media: &states.media,
        })
    }

    fn shown_tiles(&self, here: &Present) -> Vec<TileKind> {
        TileKind::ALL
            .into_iter()
            .filter(|kind| match kind {
                TileKind::Wifi => here.wifi,
                TileKind::Bluetooth => here.bluetooth,
                TileKind::Airplane => self.rfkill.is_some(),
                TileKind::Dark => true,
                TileKind::Profile => here.power_profile,
            })
            .collect()
    }

    /// Adds the view's widgets under `parent`, drawn from the models' states.
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, icons: &mut Icons) {
        self.ids = None;
        self.back = None;
        match self.view {
            View::Panel => self.build_panel(ui, parent, icons),
            View::Page(page) => self.build_page(ui, parent, page, icons),
        }
        self.changed.set(false);
        self.draw(ui, icons);
    }

    fn build_panel(&mut self, ui: &mut Ui, parent: WidgetId, icons: &mut Icons) {
        let here = self.present();
        self.built = self.shown_tiles(&here);
        // Two equal columns, in order, over the tiles shown: a tile that is not built leaves no
        // hole, and the keyboard meets the tiles in the order they are drawn.
        let grid = ui.add(parent, Kind::Column);
        let mut tiles = Vec::new();
        let mut row = None;
        for (at, kind) in self.built.clone().into_iter().enumerate() {
            if at % 2 == 0 {
                let new = ui.add(grid, Kind::Row);
                ui.set_slots(new, 2);
                row = Some(new);
            }
            let Some(row) = row else { continue };
            let cell = ui.inline(row, [0.0; 2]);
            let Some(image) = icons.get(tile_icon(kind), Icons::GLYPH) else {
                continue;
            };
            let title = kind.title();
            let tile = ui.tile(cell, &title, "", image, false);
            ui.set_fill(tile, true);
            let arrow = kind.page().and_then(|_| {
                let image = icons.get("go-next-symbolic", Icons::BUTTON)?;
                let name = tr_with("Open the {name} page", "name", &title);
                Some(ui.icon_button(cell, &name, image, Icons::BUTTON, None))
            });
            tiles.push((kind, tile, arrow));
        }
        let output = self.slider_row(
            ui,
            parent,
            icons,
            &tr("Output volume"),
            Some(&tr("Mute output")),
            true,
        );
        let input = self.slider_row(
            ui,
            parent,
            icons,
            &tr("Input volume"),
            Some(&tr("Mute microphone")),
            true,
        );
        let brightness = self.slider_row(ui, parent, icons, &tr("Brightness"), None, false);
        if self.media.build(ui, parent, icons).is_none() {
            tracing::warn!("the media section's icons are unavailable; it is not shown");
        }
        let footer = ui.inline(parent, [0.0; 2]);
        let battery = ui.add(footer, Kind::Group(String::new()));
        ui.set_fill(battery, true);
        let level = ui.inline(battery, [0.0; 2]);
        let battery_icon = match icons.get("battery-missing-symbolic", Icons::BUTTON) {
            Some(image) => ui.image(level, image, Icons::BUTTON, ""),
            None => ui.add(level, Kind::Label(String::new())),
        };
        let battery_level = ui.add(level, Kind::Label(String::new()));
        let settings_name = tr("Settings");
        let settings = match icons.get("preferences-system-symbolic", Icons::BUTTON) {
            Some(image) => ui.icon_button(footer, &settings_name, image, Icons::BUTTON, None),
            None => ui.add(footer, Kind::Button(settings_name.clone())),
        };
        ui.set_tooltip(settings, Some(&settings_name));
        self.ids = Some(Ids {
            first: tiles.first().map(|t| t.1),
            tiles,
            output,
            input,
            brightness,
            battery,
            battery_icon,
            battery_level,
            settings,
        });
    }

    fn slider_row(
        &self,
        ui: &mut Ui,
        parent: WidgetId,
        icons: &mut Icons,
        name: &str,
        mute: Option<&str>,
        page: bool,
    ) -> Option<SliderRow> {
        let row = ui.inline(parent, [0.0; 2]);
        let icon_name = if page {
            "audio-volume-medium-symbolic"
        } else {
            "display-brightness-symbolic"
        };
        let image = icons.get(icon_name, Icons::BUTTON)?;
        let (mute, icon) = match mute {
            Some(label) => (
                Some(ui.icon_toggle(row, label, image, Icons::BUTTON, false)),
                None,
            ),
            None => (None, Some(ui.image(row, image, Icons::BUTTON, ""))),
        };
        let slider = ui.slider(row, name, 0.0, 100.0, 1.0, 0.0)?;
        ui.set_fill(slider, true);
        let arrow = if page {
            let image = icons.get("go-next-symbolic", Icons::BUTTON)?;
            let name = tr_with("Open the {name} page", "name", &tr("Sound"));
            Some(ui.icon_button(row, &name, image, Icons::BUTTON, None))
        } else {
            None
        };
        Some(SliderRow {
            row,
            mute,
            icon,
            slider,
            arrow,
        })
    }

    fn build_page(&mut self, ui: &mut Ui, parent: WidgetId, page: Page, icons: &mut Icons) {
        let header = ui.inline(parent, [0.0; 2]);
        let back_name = tr("Back");
        let back = match icons.get("go-previous-symbolic", Icons::BUTTON) {
            Some(image) => ui.icon_button(header, &back_name, image, Icons::BUTTON, None),
            None => ui.add(header, Kind::Button(back_name)),
        };
        self.back = Some(back);
        let title = ui.add(header, Kind::Label(page_title(page)));
        ui.set_text_style(title, style(sal::size::TITLE_4));
        let Some(services) = self.services.as_ref() else {
            let note = tr_with(
                "The {name} page is not available yet.",
                "name",
                &page_title(page),
            );
            ui.add(parent, Kind::Label(note));
            return;
        };
        let pages = &mut self.pages;
        match page {
            Page::Network => {
                let page = pages
                    .network
                    .get_or_insert_with(|| network::Page::without_airplane(services));
                page.build(ui, parent);
            }
            Page::Bluetooth => {
                let page = pages
                    .bluetooth
                    .get_or_insert_with(|| bluetooth::Page::new(services));
                page.build(ui, parent);
            }
            Page::Audio => {
                let media = media_icons(icons);
                let page = pages
                    .audio
                    .get_or_insert_with(|| audio::Page::new(services));
                page.build(ui, parent, media);
            }
            Page::Battery => {
                let page = pages
                    .battery
                    .get_or_insert_with(|| battery::Page::new(services));
                page.build(ui, parent);
                page.opened(ui);
            }
        }
    }

    /// The view left the screen: a page's note says nothing of the actions to come.
    pub fn closed(&mut self, ui: &mut Ui) {
        let View::Page(page) = self.view else {
            return;
        };
        let pages = &mut self.pages;
        match page {
            Page::Network => pages.network.as_mut().map(|p| p.closed(ui)),
            Page::Bluetooth => pages.bluetooth.as_mut().map(|p| p.closed(ui)),
            Page::Audio => pages.audio.as_mut().map(|p| p.closed(ui)),
            Page::Battery => pages.battery.as_mut().map(|p| p.closed(ui)),
        };
    }

    /// A page that asks to be shown, as the network page does for a password the network
    /// needs.
    pub fn wanted(&mut self) -> Option<Page> {
        let network = self.pages.network.as_mut()?;
        network.wants_open().then_some(Page::Network)
    }

    /// Applies what the models reported since the last turn.
    pub fn turn(&mut self, ui: &mut Ui, icons: &mut Icons) -> Action {
        let turn = match self.view {
            View::Panel => Turn::Unchanged,
            View::Page(page) => {
                let pages = &mut self.pages;
                let turn = match page {
                    Page::Network => pages.network.as_mut().map(|p| p.turn(ui)),
                    Page::Bluetooth => pages.bluetooth.as_mut().map(|p| p.turn(ui)),
                    Page::Audio => pages.audio.as_mut().map(|p| p.turn(ui)),
                    Page::Battery => pages.battery.as_mut().map(|p| p.turn(ui)),
                };
                turn.unwrap_or(Turn::Unchanged)
            }
        };
        if turn == Turn::Rebuild {
            return Action::Rebuild;
        }
        if self.changed.replace(false) && self.view == View::Panel {
            let here = self.present();
            if self.shown_tiles(&here) != self.built {
                return Action::Rebuild;
            }
            self.draw(ui, icons);
        }
        Action::Done
    }

    /// Draws the position of the track that plays; the surface calls it every second.
    pub fn tick(&self, ui: &mut Ui) {
        if self.view == View::Panel && self.media.ticking(ui) {
            self.media.tick(ui);
        }
    }

    /// Draws every widget of the panel's view from the states, and shows the ones whose
    /// service is there.
    fn draw(&mut self, ui: &mut Ui, icons: &mut Icons) {
        let Some(ids) = &self.ids else {
            return;
        };
        let here = self.present();
        let states = self.states.borrow();
        for &(kind, tile, _) in &ids.tiles {
            let (active, subtitle, icon) = match kind {
                TileKind::Wifi => {
                    let Some(wifi) = states.network.as_ref().and_then(|n| n.wifi.as_ref()) else {
                        continue;
                    };
                    let connecting = wifi.networks.iter().any(|n| n.link == Link::Connecting);
                    let text = if connecting {
                        tr("Connecting")
                    } else {
                        on_off(wifi.enabled)
                    };
                    (wifi.enabled, text, tile_icon(kind))
                }
                TileKind::Bluetooth => {
                    let Some(bluetooth) = &states.bluetooth else {
                        continue;
                    };
                    (
                        bluetooth.powered,
                        on_off(bluetooth.powered),
                        tile_icon(kind),
                    )
                }
                TileKind::Airplane => {
                    let on = self.rfkill.as_ref().is_some_and(|device| device.airplane());
                    (on, on_off(on), tile_icon(kind))
                }
                TileKind::Dark => (self.dark, on_off(self.dark), tile_icon(kind)),
                TileKind::Profile => {
                    let Some(profiles) = &states.battery.profiles else {
                        continue;
                    };
                    let active = &profiles.active;
                    (false, profile_label(active), profile_icon(active))
                }
            };
            ui.set_active(tile, active);
            ui.set_tile_subtitle(tile, &subtitle);
            if let Some(image) = icons.get(icon, Icons::GLYPH) {
                ui.set_image(tile, Some(image));
            }
        }
        let audio = &states.audio;
        let output = audio_model::chosen(&audio.outputs, audio.default_output.as_deref())
            .map(|at| &audio.outputs[at]);
        draw_slider(
            ui,
            icons,
            ids.output.as_ref(),
            here.output_volume,
            output.map(|d| {
                (
                    d.percent,
                    d.muted,
                    audio_model::output_icon(d.percent, d.muted),
                )
            }),
        );
        let input = audio_model::chosen(&audio.inputs, audio.default_input.as_deref())
            .map(|at| &audio.inputs[at]);
        draw_slider(
            ui,
            icons,
            ids.input.as_ref(),
            here.input_volume,
            input.map(|d| (d.percent, d.muted, audio_model::input_icon(d.muted))),
        );
        let backlight = states.battery.backlight.as_ref();
        draw_slider(
            ui,
            icons,
            ids.brightness.as_ref(),
            here.brightness,
            backlight.map(|b| (b.percent(), false, "display-brightness-symbolic")),
        );
        self.media.update(ui, &states.media, here.media);
        let battery = states.battery.battery.as_ref();
        ui.set_visible(ids.battery, battery.is_some());
        if let Some(battery) = battery {
            let text = tr_with("{percent}%", "percent", &format!("{:.0}", battery.percent));
            ui.set_text(ids.battery_level, &text);
            ui.set_text(ids.battery, &tr_with("Battery {level}", "level", &text));
            if let Some(image) = icons.get(battery_model::icon(battery), Icons::BUTTON) {
                ui.set_image(ids.battery_icon, Some(image));
            }
        }
        ui.layout();
    }

    /// Takes an event of the view's widgets.
    pub fn event(&mut self, ui: &mut Ui, event: &Event) -> Action {
        if let (Event::Clicked(id), Some(back)) = (event, self.back) {
            if *id == back {
                return self.open(None);
            }
        }
        if let View::Page(page) = self.view {
            let pages = &mut self.pages;
            let taken = match page {
                Page::Network => pages.network.as_mut().map(|p| p.event(ui, event)),
                Page::Bluetooth => pages.bluetooth.as_mut().map(|p| p.event(ui, event)),
                Page::Audio => pages.audio.as_mut().map(|p| p.event(ui, event)),
                Page::Battery => pages.battery.as_mut().map(|p| p.event(ui, event)),
            };
            if taken == Some(true) {
                ui.layout();
            }
            return Action::Done;
        }
        if let Some(command) = self.media.event(ui, event) {
            send(self.services.as_ref().map(|s| &s.media.1), command);
            return Action::Done;
        }
        let Some(ids) = &self.ids else {
            return Action::Done;
        };
        let services = self.services.as_ref();
        match *event {
            Event::Toggled(id) => {
                if let Some(&(kind, tile, _)) = ids.tiles.iter().find(|t| t.1 == id) {
                    let on = ui.is_active(tile);
                    return self.pressed(ui, kind, tile, on);
                }
                for (row, sink) in [(&ids.output, true), (&ids.input, false)] {
                    if row.as_ref().and_then(|r| r.mute) == Some(id) {
                        let on = ui.is_active(id);
                        if let Some(name) = self.chosen(sink) {
                            let command = AudioCommand::Mute { sink, name, on };
                            send(services.map(|s| &s.audio.1), command);
                        }
                    }
                }
            }
            Event::Changed(id) => {
                let Some(percent) = ui.value(id).map(|v| v.round() as u32) else {
                    return Action::Done;
                };
                for (row, sink) in [(&ids.output, true), (&ids.input, false)] {
                    if row.as_ref().map(|r| r.slider) == Some(id) {
                        if let Some(name) = self.chosen(sink) {
                            let command = AudioCommand::Volume {
                                sink,
                                name,
                                percent,
                            };
                            send(services.map(|s| &s.audio.1), command);
                        }
                    }
                }
                if ids.brightness.as_ref().map(|r| r.slider) == Some(id) {
                    let backlight = self.states.borrow().battery.backlight.clone();
                    if let Some(backlight) = backlight {
                        let command = BatteryCommand::SetBrightness {
                            raw: backlight.raw(f64::from(percent)),
                            device: backlight.name,
                        };
                        send(services.map(|s| &s.battery.1), command);
                    }
                }
            }
            Event::Clicked(id) if id == ids.settings => return Action::OpenSettings,
            Event::Clicked(id) => {
                let page = ids
                    .tiles
                    .iter()
                    .find(|t| t.2 == Some(id))
                    .and_then(|t| t.0.page());
                let sound = [&ids.output, &ids.input]
                    .into_iter()
                    .any(|row| row.as_ref().and_then(|r| r.arrow) == Some(id));
                if let Some(page) = page.or(sound.then_some(Page::Audio)) {
                    return self.open(Some(page));
                }
            }
            _ => {}
        }
        Action::Done
    }

    /// Shows `page`, or the panel: the surface builds the new view.
    fn open(&mut self, page: Option<Page>) -> Action {
        self.show(page);
        Action::Rebuild
    }

    /// A tile was pressed, asking for `on`.
    fn pressed(&mut self, ui: &mut Ui, kind: TileKind, tile: WidgetId, on: bool) -> Action {
        let services = self.services.as_ref();
        match kind {
            TileKind::Wifi => send(services.map(|s| &s.network.1), NetworkCommand::Wireless(on)),
            TileKind::Bluetooth => send(
                services.map(|s| &s.bluetooth.1),
                BluetoothCommand::Power(on),
            ),
            TileKind::Airplane => {
                // The tile follows the kernel's report, not the press: after a refused write it
                // goes back to what the radios are.
                if let Some(device) = &self.rfkill {
                    if let Err(err) = device.set_blocked(on) {
                        tracing::warn!(error = %err, "the radios were not blocked");
                    }
                }
                self.changed.set(true);
            }
            TileKind::Dark => return Action::SetDark(on),
            TileKind::Profile => {
                let next = self
                    .states
                    .borrow()
                    .battery
                    .profiles
                    .as_ref()
                    .and_then(|p| next_profile(&p.offered, &p.active));
                if let Some(next) = next {
                    let command = BatteryCommand::SetProfile(next.to_owned());
                    send(services.map(|s| &s.battery.1), command);
                }
                // The tile shows the profile the service reports, so it goes back until then.
                ui.set_active(tile, false);
            }
        }
        Action::Done
    }

    /// The name of the device the slider acts on: the default one, else the first.
    fn chosen(&self, sink: bool) -> Option<String> {
        let states = self.states.borrow();
        let (devices, default) = if sink {
            (
                &states.audio.outputs,
                states.audio.default_output.as_deref(),
            )
        } else {
            (&states.audio.inputs, states.audio.default_input.as_deref())
        };
        audio_model::chosen(devices, default).map(|at| devices[at].name.clone())
    }
}

fn draw_slider(
    ui: &mut Ui,
    icons: &mut Icons,
    row: Option<&SliderRow>,
    shown: bool,
    state: Option<(f64, bool, &'static str)>,
) {
    let Some(row) = row else {
        return;
    };
    ui.set_visible(row.row, shown);
    let Some((percent, muted, icon)) = state else {
        return;
    };
    // A value set from the model is not the person's: its event is dropped. It is kept as the
    // sound server gives it, off the slider's steps, as gtk_range_set_value keeps it.
    let _ = ui.set_exact_value(row.slider, percent);
    if let Some(image) = icons.get(icon, Icons::BUTTON) {
        if let Some(id) = row.mute.or(row.icon) {
            ui.set_image(id, Some(image));
        }
    }
    if let Some(mute) = row.mute {
        let _ = ui.set_active(mute, muted);
    }
}

/// The icons of the sound page's media controls.
fn media_icons(icons: &mut Icons) -> Option<MediaIcons> {
    let size = Icons::BUTTON;
    Some(MediaIcons {
        size,
        previous: icons.get(MediaIcons::PREVIOUS, size)?,
        play: icons.get(MediaIcons::PLAY, size)?,
        pause: icons.get(MediaIcons::PAUSE, size)?,
        next: icons.get(MediaIcons::NEXT, size)?,
    })
}

#[cfg(test)]
mod tests;
