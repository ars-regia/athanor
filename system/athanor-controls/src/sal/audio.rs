//! The audio page drawn with athanor-sal (doc_bar.md, BR3): output and input volume, mute, the
//! device in use, and the media controls, as the GTK page shows them. The sound server is reached
//! by the model in athanor-services; the page only applies the state the model publishes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_sal::{Event, Image, Kind, TextStyle, Ui, WidgetId};
use athanor_services::audio::{self, AudioCommand, AudioState, Device};
use athanor_services::media::MediaCommand;
use tokio::sync::mpsc;

use super::widgets::{column, switch_row, Failure};
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
                let text = if *in_use {
                    tr_with("{device}, in use", "device", label)
                } else {
                    label.clone()
                };
                (name.clone(), ui.add(list, Kind::Button(text)))
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

    /// Shows the device in use. Whether the device rows must change, which the widgets cannot.
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
        rows(devices, default) != self.shown
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
    ids: Option<Ids>,
}

impl Page {
    pub fn new(services: &Services) -> Page {
        Page {
            service: Service::get(services),
            seen: (0, 0),
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
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, icons: Option<MediaIcons>) -> WidgetId {
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
        let controls = ui.add(column, Kind::Row);
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
        let failure = Failure::new(ui, page);
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
            return Turn::Rebuild;
        }
        if let (true, Some(ids)) = (failed, &self.ids) {
            ids.failure.show(ui);
        }
        Turn::Updated
    }

    /// The page left the screen: a note says nothing of the actions to come.
    pub fn closed(&mut self, ui: &mut Ui) {
        if let Some(ids) = &self.ids {
            ids.failure.hide(ui);
        }
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
    fn the_page_shows_the_device_in_use_and_the_others_as_rows() {
        let (mut page, _) = page();
        assert!(page.face().is_none(), "no device: the surface hides its button");
        publish(&page, speakers_and_headset());
        let mut ui = Ui::new("Sound", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, None);
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

    #[test]
    fn the_persons_actions_go_to_the_device_in_use() {
        let (mut page, mut received) = page();
        publish(&page, speakers_and_headset());
        let mut ui = Ui::new("Sound", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, None);
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
}
