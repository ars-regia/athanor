//! The audio page drawn with athanor-sal (doc_bar.md, BR3): output and input volume, mute, the
//! device in use, and the media controls, as the GTK page shows them. The sound server is reached
//! by the model in athanor-services; the page only applies the state the model publishes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_sal::{Event, Image, Kind, TextStyle, Ui, WidgetId};
use athanor_services::audio::{self, AudioCommand, AudioState, Device};
use athanor_services::media::MediaCommand;
use tokio::sync::mpsc;

use super::widgets::{column, switch_row, Failure, Look};
use super::Turn;
use crate::bridge;
use crate::i18n::{tr, tr_with};
use crate::media::{Media, NowPlaying};
use crate::Services;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

/// The audio and media models of the process, which every page shows.
struct Service {
    /// The model's last state, which `bridge::follow` keeps current.
    state: RefCell<AudioState>,
    commands: mpsc::UnboundedSender<AudioCommand>,
    media: RefCell<Option<Rc<Media>>>,
    /// Counts the states of both models, so that a page shows one it has not seen.
    states: Cell<u64>,
    /// Counts the actions that did not complete.
    failures: Cell<u64>,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = SERVICE.with(|cell| cell.borrow().clone()) {
            return service;
        }
        let (states, commands) = services.audio.clone();
        let service = Rc::new(Service {
            state: RefCell::new(AudioState::default()),
            commands,
            media: RefCell::new(None),
            states: Cell::new(0),
            failures: Cell::new(0),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak: the thread-local owns the service.
        let (notified, failed) = (Rc::downgrade(&service), Rc::downgrade(&service));
        service.media.replace(Some(Media::new(
            services.media.clone(),
            move || {
                if let Some(service) = notified.upgrade() {
                    service.changed();
                }
            },
            move || {
                if let Some(service) = failed.upgrade() {
                    service.failed();
                }
            },
        )));
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state: &AudioState| {
            let Some(service) = weak.upgrade() else {
                return;
            };
            if state.refused != service.state.borrow().refused {
                service.failed();
            }
            service.state.replace(state.clone());
            service.changed();
        });
        service
    }

    fn changed(&self) {
        self.states.set(self.states.get() + 1);
    }

    /// An action did not complete: the pages show the last state again, which puts a slider
    /// or a switch back, and say so.
    fn failed(&self) {
        self.failures.set(self.failures.get() + 1);
        self.changed();
    }

    fn command(&self, command: AudioCommand) {
        if self.commands.send(command).is_err() {
            tracing::debug!("the audio model ended; the command is dropped");
        }
    }

    fn now(&self) -> Option<NowPlaying> {
        self.media.borrow().as_ref().and_then(|media| media.now())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Output,
    Input,
}

/// One direction's widgets: a volume slider, a mute switch, and the devices when there are
/// several.
struct Channel {
    direction: Direction,
    section: WidgetId,
    /// None only if athanor-sal refused the slider's range, which is a constant.
    slider: Option<WidgetId>,
    mute: WidgetId,
    /// The device rows, (name, button), and what they show, (name, label, in use).
    devices: Vec<(String, WidgetId)>,
    shown: Vec<(String, String, bool)>,
    /// The device the slider and the switch control.
    chosen: Option<String>,
}

impl Channel {
    fn build(
        ui: &mut Ui,
        parent: WidgetId,
        direction: Direction,
        state: &AudioState,
        title: impl Fn() -> TextStyle,
    ) -> Channel {
        let (heading, slider, mute) = match direction {
            Direction::Output => (tr("Output"), tr("Output volume"), tr("Mute output")),
            Direction::Input => (tr("Input"), tr("Input volume"), tr("Mute microphone")),
        };
        let section = column(ui, parent);
        let label = ui.add(section, Kind::Label(heading));
        ui.set_text_style(label, title());
        let slider = ui.slider(section, &slider, 0.0, 100.0, 1.0, 0.0);
        let mute = switch_row(ui, section, &mute, false);
        let shown = rows(direction.devices(state), direction.default(state));
        let list = column(ui, section);
        let devices = shown
            .iter()
            .map(|(name, label, in_use)| {
                // Across the page, its text centred, as GTK's button with a label.
                let row = ui.add(list, Kind::Button(row_text(label, *in_use)));
                ui.set_fill(row, true);
                (name.clone(), row)
            })
            .collect();
        Channel {
            direction,
            section,
            slider,
            mute,
            devices,
            shown,
            chosen: None,
        }
    }

    /// Shows the device in use, and a row's new label or use in place. Whether the device rows
    /// must change, which the widgets cannot: a device came, went or moved.
    fn show(&mut self, ui: &mut Ui, state: &AudioState) -> bool {
        let (devices, default) = (self.direction.devices(state), self.direction.default(state));
        let chosen = audio::chosen(devices, default).and_then(|index| devices.get(index));
        ui.set_visible(self.section, chosen.is_some());
        self.chosen = chosen.map(|device| device.name.clone());
        if let Some(device) = chosen {
            if let Some(slider) = self.slider {
                // As the sound server gives it, off the steps, as gtk_range_set_value keeps it.
                ui.set_exact_value(slider, device.percent);
            }
            if ui.is_active(self.mute) != device.muted {
                ui.set_active(self.mute, device.muted);
            }
        }
        let rows = rows(devices, default);
        if !rows.iter().map(|row| &row.0).eq(self.shown.iter().map(|row| &row.0)) {
            return true;
        }
        for ((_, button), (_, label, in_use)) in self.devices.iter().zip(&rows) {
            ui.set_text(*button, &row_text(label, *in_use));
        }
        self.shown = rows;
        false
    }

    /// The command for the person's `event` on this channel's widgets.
    fn command(&self, ui: &Ui, event: &Event) -> Option<AudioCommand> {
        let sink = self.direction == Direction::Output;
        match *event {
            Event::Changed(id) if Some(id) == self.slider => Some(AudioCommand::Volume {
                sink,
                name: self.chosen.clone()?,
                // Clamped, so the cast cannot truncate; NaN is 0.
                percent: ui.value(id)?.clamp(0.0, 100.0).round() as u32,
            }),
            Event::Toggled(id) if id == self.mute => Some(AudioCommand::Mute {
                sink,
                name: self.chosen.clone()?,
                on: ui.is_active(id),
            }),
            Event::Clicked(id) => {
                let (name, _) = self.devices.iter().find(|(_, button)| *button == id)?;
                Some(AudioCommand::Default {
                    sink,
                    name: name.clone(),
                })
            }
            _ => None,
        }
    }
}

impl Direction {
    fn devices(self, state: &AudioState) -> &[Device] {
        match self {
            Direction::Output => &state.outputs,
            Direction::Input => &state.inputs,
        }
    }

    fn default(self, state: &AudioState) -> Option<&str> {
        match self {
            Direction::Output => state.default_output.as_deref(),
            Direction::Input => state.default_input.as_deref(),
        }
    }
}

/// A device row's text: its label, and whether it is the one in use.
fn row_text(label: &str, in_use: bool) -> String {
    if in_use {
        tr_with("{device}, in use", "device", label)
    } else {
        label.to_owned()
    }
}

/// The device rows, (name, label, in use): one per device when there are several.
fn rows(devices: &[Device], default: Option<&str>) -> Vec<(String, String, bool)> {
    if devices.len() < 2 {
        return Vec::new();
    }
    let chosen = audio::chosen(devices, default);
    devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            (
                device.name.clone(),
                device.label.clone(),
                chosen == Some(index),
            )
        })
        .collect()
}

/// The icons of the media buttons, from the surface's icon theme at its scale.
pub struct MediaIcons {
    /// The side of the square each icon is drawn in, in logical pixels.
    pub size: f32,
    pub previous: Image,
    pub play: Image,
    pub pause: Image,
    pub next: Image,
}

impl MediaIcons {
    /// The icon names the surface looks up.
    pub const PREVIOUS: &'static str = "media-skip-backward-symbolic";
    pub const PLAY: &'static str = "media-playback-start-symbolic";
    pub const PAUSE: &'static str = "media-playback-pause-symbolic";
    pub const NEXT: &'static str = "media-skip-forward-symbolic";
}

struct MediaIds {
    column: WidgetId,
    title: WidgetId,
    artist: WidgetId,
    previous: WidgetId,
    play: WidgetId,
    next: WidgetId,
    /// The play and pause icons, which the play button swaps; none for text buttons.
    faces: Option<(Image, Image)>,
}

struct Ids {
    output: Channel,
    input: Channel,
    media: MediaIds,
    failure: Failure,
}

/// What the surface's button shows of the page.
pub struct Face {
    /// The icon of the device in use.
    pub icon: &'static str,
    /// Whether the media controls are final. The captures open the popover only then: a
    /// popover that grows after its first frame leaves its edge column stale in cosmic-comp at
    /// a fractional scale.
    pub settled: bool,
}

/// The audio page: one per surface, over the one model of the process. The surface adds it to
/// its widgets with [`Page::build`], calls [`Page::turn`] in each turn of its loop and hands it
/// every event with [`Page::event`].
pub struct Page {
    service: Rc<Service>,
    /// The state and the failure the page shows last.
    seen: (u64, u64),
    /// A failure's note is on screen: a page built again, while the old one is, shows it too.
    noted: bool,
    /// A failure came with a rebuild: the surface drops the old page's [`Ui`] unannounced, so
    /// the page built again announces it.
    unheard: bool,
    /// The page is on screen: built, and not closed or discarded since.
    shown: bool,
    ids: Option<Ids>,
}

impl Page {
    pub fn new(services: &Services) -> Page {
        Page {
            service: Service::get(services),
            seen: (0, 0),
            noted: false,
            unheard: false,
            shown: false,
            ids: None,
        }
    }

    /// What the surface's button shows: `None` while there is no device at all, and then the
    /// surface hides its button.
    pub fn face(&self) -> Option<Face> {
        let state = self.service.state.borrow();
        let device = |direction: Direction| {
            let devices = direction.devices(&state);
            audio::chosen(devices, direction.default(&state)).and_then(|i| devices.get(i))
        };
        let icon = match (device(Direction::Output), device(Direction::Input)) {
            (Some(output), _) => audio::output_icon(output.percent, output.muted),
            (None, Some(input)) => audio::input_icon(input.muted),
            (None, None) => return None,
        };
        let settled = self
            .service
            .media
            .borrow()
            .as_ref()
            .is_none_or(|media| media.settled());
        Some(Face { icon, settled })
    }

    /// Adds the page's widgets as a column under `parent`, showing the models' state, and
    /// returns the column. `icons` draws the media buttons; without it they show their names.
    /// The artist and a failure's note are secondary lines of `look`.
    pub fn build(
        &mut self,
        ui: &mut Ui,
        parent: WidgetId,
        icons: Option<MediaIcons>,
        look: &Look,
    ) -> WidgetId {
        let title = || TextStyle {
            weight: 600,
            ..TextStyle::default()
        };
        let page = column(ui, parent);
        let (output, input) = {
            let state = self.service.state.borrow();
            (
                Channel::build(ui, page, Direction::Output, &state, title),
                Channel::build(ui, page, Direction::Input, &state, title),
            )
        };

        let column = column(ui, page);
        let track = ui.add(column, Kind::Label(String::new()));
        ui.set_text_style(track, title());
        let artist = ui.add(column, Kind::Label(String::new()));
        look.note(ui, artist);
        // The buttons centred under the track, as the GTK box at `halign` Center: the middle of
        // a centre box, whose start is an empty place.
        let centre = ui.add(column, Kind::CenterBox);
        let start = ui.add(centre, Kind::Label(String::new()));
        ui.set_visible(start, false);
        let controls = ui.inline(centre, [0.0; 2]);
        let names = [tr("Previous track"), tr("Pause"), tr("Next track")];
        let (buttons, faces) = match icons {
            Some(icons) => {
                let size = icons.size;
                let [previous, play, next] = [
                    (&names[0], icons.previous),
                    (&names[1], icons.pause.clone()),
                    (&names[2], icons.next),
                ]
                .map(|(name, image)| ui.icon_button(controls, name, image, size, None));
                ([previous, play, next], Some((icons.play, icons.pause)))
            }
            None => (
                names
                    .each_ref()
                    .map(|name| ui.add(controls, Kind::Button(name.clone()))),
                None,
            ),
        };
        for (button, name) in buttons.iter().zip(&names) {
            ui.set_tooltip(*button, Some(name));
        }
        let [previous, play, next] = buttons;
        let failure = Failure::new(ui, page, look);
        if std::mem::take(&mut self.unheard) {
            self.noted |= failure.show(ui);
        } else if self.noted {
            failure.restore(ui);
        }
        self.ids = Some(Ids {
            output,
            input,
            media: MediaIds {
                column,
                title: track,
                artist,
                previous,
                play,
                next,
                faces,
            },
            failure,
        });
        self.shown = true;
        self.show(ui);
        page
    }

    /// Shows what changed since the last call: a new state, and a failure as the note under
    /// the page.
    pub fn turn(&mut self, ui: &mut Ui) -> Turn {
        let now = (self.service.states.get(), self.service.failures.get());
        if now == self.seen {
            return Turn::Unchanged;
        }
        let failed = now.1 != self.seen.1;
        if self.show(ui) {
            self.unheard |= failed && self.shown;
            return Turn::Rebuild;
        }
        if let (true, Some(ids)) = (failed, &self.ids) {
            self.noted |= ids.failure.show(ui);
        }
        Turn::Updated
    }

    /// The page left the screen: a note says nothing of the actions to come.
    pub fn closed(&mut self, ui: &mut Ui) {
        self.discarded();
        if let Some(ids) = &self.ids {
            ids.failure.hide(ui);
        }
    }

    /// The surface that showed the page closed: a page built again shows no note.
    pub fn discarded(&mut self) {
        self.noted = false;
        self.unheard = false;
        self.shown = false;
    }

    /// Takes `event` if it is one of the page's widgets': whether it was.
    pub fn event(&mut self, ui: &mut Ui, event: &Event) -> bool {
        let Some(ids) = &self.ids else {
            return false;
        };
        if let Some(command) = [&ids.output, &ids.input]
            .into_iter()
            .find_map(|channel| channel.command(ui, event))
        {
            self.service.command(command);
            return true;
        }
        let media = &ids.media;
        let command = match *event {
            Event::Clicked(id) if id == media.previous => MediaCommand::Previous,
            Event::Clicked(id) if id == media.play => MediaCommand::PlayPause,
            Event::Clicked(id) if id == media.next => MediaCommand::Next,
            _ => return false,
        };
        if let Some(media) = self.service.media.borrow().as_ref() {
            media.command(command);
        }
        true
    }

    /// Shows the models' state. Whether the device rows must change.
    fn show(&mut self, ui: &mut Ui) -> bool {
        self.seen = (self.service.states.get(), self.service.failures.get());
        let Some(ids) = &mut self.ids else {
            return false;
        };
        let state = self.service.state.borrow();
        let output = ids.output.show(ui, &state);
        let input = ids.input.show(ui, &state);
        let media = &ids.media;
        let now = self.service.now();
        ui.set_visible(media.column, now.is_some());
        if let Some(now) = now {
            ui.set_text(media.title, &now.track.title);
            ui.set_text(media.artist, now.track.artist.as_deref().unwrap_or(""));
            ui.set_visible(media.artist, now.track.artist.is_some());
            let name = if now.playing { tr("Pause") } else { tr("Play") };
            ui.set_text(media.play, &name);
            ui.set_tooltip(media.play, Some(&name));
            if let Some((play, pause)) = &media.faces {
                let face = if now.playing { pause } else { play };
                ui.set_image(media.play, Some(face.clone()));
            }
            ui.set_sensitive(media.previous, now.can_previous);
            ui.set_sensitive(media.next, now.can_next);
        }
        output || input
    }
}

#[cfg(test)]
mod tests {
    use athanor_sal::Theme;

    use super::*;

    fn page() -> (Page, mpsc::UnboundedReceiver<AudioCommand>) {
        let (commands, received) = mpsc::unbounded_channel();
        let service = Rc::new(Service {
            state: RefCell::new(AudioState::default()),
            commands,
            media: RefCell::new(None),
            states: Cell::new(0),
            failures: Cell::new(0),
        });
        let page = Page {
            service,
            seen: (0, 0),
            noted: false,
            unheard: false,
            shown: false,
            ids: None,
        };
        (page, received)
    }

    fn publish(page: &Page, state: AudioState) {
        let service = &page.service;
        if state.refused != service.state.borrow().refused {
            service.failed();
        }
        service.state.replace(state);
        service.changed();
    }

    fn device(name: &str, percent: f64, muted: bool) -> Device {
        Device {
            name: name.into(),
            label: name.to_uppercase(),
            percent,
            muted,
        }
    }

    fn speakers_and_headset() -> AudioState {
        AudioState {
            outputs: vec![device("speakers", 40.0, false), device("headset", 70.0, true)],
            inputs: vec![device("mic", 55.0, false)],
            default_output: Some("headset".into()),
            default_input: None,
            refused: 0,
            settled: true,
        }
    }

    fn label(ui: &Ui, id: WidgetId) -> &str {
        match &ui.widget(id).kind {
            Kind::Label(text) | Kind::Button(text) => text,
            _ => "",
        }
    }

    #[test]
    fn the_device_rows_fill_the_page_and_the_media_controls_are_centred() {
        let (mut page, _) = page();
        publish(&page, speakers_and_headset());
        let mut ui = Ui::new("Sound", Theme::default(), false);
        ui.min_width = 400.0;
        let column = page.build(&mut ui, Ui::ROOT, None, &Look::plain());
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        ui.set_visible(ids.media.column, true);
        ui.layout();
        let width = ui.widget(column).rect.w;
        for (_, row) in &ids.output.devices {
            assert_eq!(ui.widget(*row).rect.w, width, "a device row across the page");
        }
        let (first, last) = (ui.widget(ids.media.previous).rect, ui.widget(ids.media.next).rect);
        let media = ui.widget(ids.media.column).rect;
        let middle = (first.x + last.x + last.w) / 2.0;
        assert!((middle - (media.x + media.w / 2.0)).abs() < 0.5, "the controls centred");
    }

    #[test]
    fn the_page_shows_the_device_in_use_and_the_others_as_rows() {
        let (mut page, _) = page();
        assert!(page.face().is_none(), "no device: the surface hides its button");
        publish(&page, speakers_and_headset());
        let mut ui = Ui::new("Sound", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, None, &Look::plain());
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        let out = &ids.output;
        assert_eq!(out.slider.and_then(|s| ui.value(s)), Some(70.0));
        assert!(ui.is_active(out.mute));
        let rows: Vec<_> = out.devices.iter().map(|(_, id)| label(&ui, *id)).collect();
        assert_eq!(rows, ["SPEAKERS", "HEADSET, in use"]);
        assert!(ids.input.devices.is_empty(), "one input: no rows");
        assert_eq!(ids.input.slider.and_then(|s| ui.value(s)), Some(55.0));
        assert!(!ui.is_visible(ids.media.column), "no player");
        assert_eq!(page.face().map(|f| f.icon), Some("audio-volume-muted-symbolic"));

        // A volume step changes the widgets; a device that leaves changes the rows.
        let mut state = speakers_and_headset();
        state.outputs[1].percent = 80.0;
        publish(&page, state.clone());
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        state.outputs.remove(0);
        publish(&page, state);
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
    }

    /// A row whose device comes into use, or takes a new label, is set in place, which keeps
    /// the keyboard focus on the row the person just pressed; a device that comes, goes or
    /// moves builds the rows again (#1248).
    #[test]
    fn a_device_in_use_keeps_the_rows_and_a_device_that_moves_rebuilds_them() {
        let texts = |page: &Page, ui: &Ui| -> Vec<String> {
            let devices = page.ids.as_ref().map_or(&[][..], |ids| &ids.output.devices);
            devices.iter().map(|(_, id)| label(ui, *id).to_owned()).collect()
        };
        let build = |page: &mut Page| {
            let mut ui = Ui::new("Sound", Theme::default(), false);
            page.build(&mut ui, Ui::ROOT, None, &Look::plain());
            ui
        };
        let (mut page, mut received) = page();
        publish(&page, speakers_and_headset());
        let mut ui = build(&mut page);
        let headset = page.ids.as_ref().map(|ids| ids.output.devices[1].1);

        let mut state = speakers_and_headset();
        state.default_output = Some("speakers".into());
        state.outputs[1].label = "Headset".into();
        publish(&page, state.clone());
        assert_eq!(page.turn(&mut ui), Turn::Updated, "only the rows' state changed");
        assert_eq!(texts(&page, &ui), ["SPEAKERS, in use", "Headset"]);
        assert_eq!(page.ids.as_ref().map(|ids| ids.output.devices[1].1), headset);
        let event = headset.and_then(|row| ui.activate(row)).expect("a row is clicked");
        assert!(page.event(&mut ui, &event));
        assert!(matches!(
            received.try_recv(),
            Ok(AudioCommand::Default { sink: true, name }) if name == "headset"
        ));

        let mut moved = state.clone();
        moved.outputs.reverse();
        let mut added = state.clone();
        added.outputs.push(device("hdmi", 50.0, false));
        let mut removed = added.clone();
        removed.outputs.remove(1);
        for (now, why) in [
            (moved, "the same devices in another order"),
            (added, "a device came"),
            (removed, "a device left"),
        ] {
            publish(&page, now);
            assert_eq!(page.turn(&mut ui), Turn::Rebuild, "{why}");
            ui = build(&mut page);
        }
    }

    #[test]
    fn the_persons_actions_go_to_the_device_in_use() {
        let (mut page, mut received) = page();
        publish(&page, speakers_and_headset());
        let mut ui = Ui::new("Sound", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, None, &Look::plain());
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        let (slider, mute, speakers) = (ids.output.slider, ids.output.mute, ids.output.devices[0].1);
        let note = ids.failure.note;

        let event = slider.and_then(|s| ui.set_value(s, 25.0)).expect("the slider moves");
        assert!(page.event(&mut ui, &event));
        assert!(matches!(
            received.try_recv(),
            Ok(AudioCommand::Volume { sink: true, name, percent: 25 }) if name == "headset"
        ));
        let event = ui.activate(mute).expect("the switch turns");
        assert!(page.event(&mut ui, &event));
        assert!(matches!(
            received.try_recv(),
            Ok(AudioCommand::Mute { sink: true, name, on: false }) if name == "headset"
        ));
        let event = ui.activate(speakers).expect("a row is clicked");
        assert!(page.event(&mut ui, &event));
        assert!(matches!(
            received.try_recv(),
            Ok(AudioCommand::Default { sink: true, name }) if name == "speakers"
        ));

        // The server refused: the switch goes back and the note shows.
        publish(&page, AudioState { refused: 1, ..speakers_and_headset() });
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(ui.is_active(mute));
        assert!(ui.is_visible(note));
        page.closed(&mut ui);
        assert!(!ui.is_visible(note));
    }

    #[test]
    fn a_note_shown_stays_in_the_page_built_again_until_its_surface_closes() {
        let note = |page: &Page, ui: &Ui| {
            page.ids
                .as_ref()
                .is_some_and(|ids| ui.is_visible(ids.failure.note))
        };
        let both = |refused| AudioState {
            refused,
            ..speakers_and_headset()
        };
        let headset = |refused| {
            let mut state = both(refused);
            state.outputs.remove(0);
            state
        };
        let build = |page: &mut Page| {
            let mut ui = Ui::new("Sound", Theme::default(), false);
            page.build(&mut ui, Ui::ROOT, None, &Look::plain());
            ui
        };
        let (mut page, _received) = page();
        publish(&page, both(0));
        let mut ui = build(&mut page);

        // A refusal that comes with a device that left: the page built again shows it.
        publish(&page, headset(1));
        assert_eq!(page.turn(&mut ui), Turn::Rebuild);
        let mut again = build(&mut page);
        assert!(
            note(&page, &again),
            "the refusal that came with the rebuild"
        );
        assert_eq!(
            again.announcement().map(|(text, _)| text.to_owned()),
            Some(tr("The action did not complete.")),
            "announced on the Ui the surface keeps"
        );

        // A device comes while the note is on screen: the page built again keeps it.
        publish(&page, both(1));
        assert_eq!(page.turn(&mut again), Turn::Rebuild);
        let third = build(&mut page);
        assert!(note(&page, &third));
        assert_eq!(
            third.announcement(),
            None,
            "a note already announced is not again"
        );

        // The popover closed: the next one starts without the note.
        page.discarded();
        let mut next = build(&mut page);
        assert!(!note(&page, &next));

        // A refusal in place, then a rebuild: the page built again keeps the note, silent.
        publish(&page, both(2));
        assert_eq!(page.turn(&mut next), Turn::Updated);
        assert!(note(&page, &next));
        publish(&page, headset(2));
        assert_eq!(page.turn(&mut next), Turn::Rebuild);
        let mut fourth = build(&mut page);
        assert!(note(&page, &fourth));
        assert_eq!(fourth.announcement(), None);

        // A refusal with a rebuild the popover closed before: nothing shown, nothing said.
        publish(&page, both(3));
        assert_eq!(page.turn(&mut fourth), Turn::Rebuild);
        page.discarded();
        let last = build(&mut page);
        assert!(!note(&page, &last));
        assert_eq!(last.announcement(), None);
    }

    #[test]
    fn a_page_closed_with_its_ui_kept_forgets_its_note_and_says_nothing() {
        let note = |page: &Page, ui: &Ui| {
            page.ids
                .as_ref()
                .is_some_and(|ids| ui.is_visible(ids.failure.note))
        };
        let fresh = |page: &mut Page| {
            let mut ui = Ui::new("Sound", Theme::default(), false);
            page.build(&mut ui, Ui::ROOT, None, &Look::plain());
            ui
        };
        let (mut page, _received) = page();
        let mut refused = 0;
        publish(&page, speakers_and_headset());
        let mut ui = fresh(&mut page);

        // A note shown in place, then the page closes: the page built again has none.
        refused += 1;
        publish(
            &page,
            AudioState {
                refused,
                ..speakers_and_headset()
            },
        );
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        assert!(note(&page, &ui));
        page.closed(&mut ui);
        let mut next = fresh(&mut page);
        assert!(!note(&page, &next));

        // A refusal with a rebuild, then the page closes before it is built again.
        refused += 1;
        let mut headset = AudioState {
            refused,
            ..speakers_and_headset()
        };
        headset.outputs.remove(0);
        publish(&page, headset);
        assert_eq!(page.turn(&mut next), Turn::Rebuild);
        page.closed(&mut next);
        let mut later = fresh(&mut page);
        assert!(!note(&page, &later));
        assert_eq!(later.announcement(), None);

        // A refusal with a rebuild while the page is closed, as another surface's action
        // fails: the hidden page says nothing, and shows no note when it is built again.
        page.closed(&mut later);
        refused += 1;
        publish(
            &page,
            AudioState {
                refused,
                ..speakers_and_headset()
            },
        );
        assert_eq!(page.turn(&mut later), Turn::Rebuild);
        let last = fresh(&mut page);
        assert!(!note(&page, &last));
        assert_eq!(last.announcement(), None);
    }
}
