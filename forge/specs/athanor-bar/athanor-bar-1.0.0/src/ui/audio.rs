//! The audio module (doc_bar.md, BR3): output and input volume, mute, the device in use,
//! and the media controls. The sound server is reached by the model in athanor-services, which
//! keeps libpulse on a thread of its own: nothing here calls it, and the main loop only applies
//! the state the model publishes. A lost server hides the module until it returns.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use athanor_services::audio::{self, AudioCommand, AudioState, Device};
use athanor_services::media::MediaCommand;
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;
use tokio::sync::mpsc;

use super::bridge;
use super::mpris::{Media, NowPlaying};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Output,
    Input,
}

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

/// An action the person took did not complete: the views show the last state again, which
/// puts a slider or a switch back, and the open popover says so.
fn action_failed() {
    if let Some(service) = service() {
        service.show_all();
        for view in service.views() {
            view.popup.failed();
        }
    }
}

struct Service {
    bar: Weak<Bar>,
    /// The model's last state, which `bridge::follow` keeps current.
    state: RefCell<AudioState>,
    /// How many commands the model had counted as refused when the views last showed it.
    refused: Cell<u32>,
    /// `None` without a runtime: there is no model, and the module stays hidden.
    commands: Option<mpsc::UnboundedSender<AudioCommand>>,
    media: RefCell<Option<Rc<Media>>>,
    views: RefCell<Vec<Weak<View>>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let model = bridge::runtime().map(|(handle, _)| audio::spawn(&handle));
        let (states, commands) = match model {
            Some((states, commands)) => (Some(states), Some(commands)),
            None => {
                tracing::warn!("no runtime for the models; the audio module is hidden");
                (None, None)
            }
        };
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            state: RefCell::new(AudioState::default()),
            refused: Cell::new(0),
            commands,
            media: RefCell::new(None),
            views: RefCell::new(Vec::new()),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        let weak = Rc::downgrade(&service);
        service.media.replace(Some(Media::new(
            move || {
                if let Some(service) = weak.upgrade() {
                    service.show_all();
                }
            },
            action_failed,
        )));
        if let Some(states) = states {
            // Weak, as the other modules hold the service: the thread-local owns it.
            let weak = Rc::downgrade(&service);
            bridge::follow(states, move |state| {
                let Some(service) = weak.upgrade() else {
                    return;
                };
                let failed = service.refused.replace(state.refused) != state.refused;
                if *service.state.borrow() != *state {
                    service.state.replace(state.clone());
                    service.show_all();
                }
                if failed {
                    action_failed();
                }
            });
        }
        service
    }

    fn show_all(&self) {
        let now = self.media.borrow().as_ref().and_then(|media| media.now());
        let state = self.state.borrow();
        // No device at all hides the module.
        let shown = (!state.outputs.is_empty() || !state.inputs.is_empty()).then_some(&*state);
        for view in self.views() {
            view.show(shown, now.as_ref());
        }
        if let Some(bar) = self.bar.upgrade() {
            bar.fit_groups();
        }
    }

    fn views(&self) -> Vec<Rc<View>> {
        let mut views = self.views.borrow_mut();
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect()
    }

    fn command(&self, command: AudioCommand) {
        if let Some(commands) = &self.commands {
            if commands.send(command).is_err() {
                tracing::debug!("the audio model ended; the command is dropped");
            }
        }
    }

    fn set_volume(&self, kind: Kind, name: &str, percent: f64) {
        self.command(AudioCommand::Volume {
            sink: kind == Kind::Output,
            name: name.to_owned(),
            // Clamped, so the cast cannot truncate; NaN is 0.
            percent: percent.clamp(0.0, 100.0).round() as u32,
        });
    }

    fn set_mute(&self, kind: Kind, name: &str, muted: bool) {
        self.command(AudioCommand::Mute {
            sink: kind == Kind::Output,
            name: name.to_owned(),
            on: muted,
        });
    }

    fn set_default(&self, kind: Kind, name: &str) {
        self.command(AudioCommand::Default {
            sink: kind == Kind::Output,
            name: name.to_owned(),
        });
    }

    fn media(&self, command: MediaCommand) {
        if let Some(media) = self.media.borrow().as_ref() {
            media.command(command);
        }
    }
}

/// One direction: a volume slider, a mute switch, and the devices when there are several.
struct Channel {
    kind: Kind,
    section: gtk4::Box,
    scale: gtk4::Scale,
    mute: gtk4::Switch,
    devices: gtk4::Box,
    chosen: RefCell<Option<String>>,
    /// What the device rows show, (name, label, in use): they are rebuilt only when it
    /// changes, not on every volume step, which would take the keyboard focus off them.
    shown: RefCell<Vec<(String, String, bool)>>,
}

impl Channel {
    fn new(kind: Kind, heading: &str, slider: &str, mute: &str) -> Channel {
        let title = gtk4::Label::new(Some(heading));
        title.add_css_class("bar-popover-title");
        title.set_xalign(0.0);
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.update_property(&[Property::Label(slider)]);
        let (mute_row, mute) = switch_row(mute);
        let devices = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let section = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        section.append(&title);
        section.append(&scale);
        section.append(&mute_row);
        section.append(&devices);
        Channel {
            kind,
            section,
            scale,
            mute,
            devices,
            chosen: RefCell::new(None),
            shown: RefCell::new(Vec::new()),
        }
    }

    /// Shows the device in use; returns it.
    fn show(&self, devices: &[Device], default: Option<&str>) -> Option<Device> {
        let chosen = audio::chosen(devices, default)
            .and_then(|index| devices.get(index))
            .cloned();
        self.section.set_visible(chosen.is_some());
        self.chosen
            .replace(chosen.as_ref().map(|device| device.name.clone()));
        if let Some(device) = &chosen {
            self.scale.set_value(device.percent);
            self.mute.set_active(device.muted);
        }
        let rows: Vec<(String, String, bool)> = if devices.len() > 1 {
            devices
                .iter()
                .map(|device| {
                    let in_use = chosen
                        .as_ref()
                        .is_some_and(|chosen| chosen.name == device.name);
                    (device.name.clone(), device.label.clone(), in_use)
                })
                .collect()
        } else {
            Vec::new()
        };
        if *self.shown.borrow() == rows {
            return chosen;
        }
        while let Some(child) = self.devices.first_child() {
            self.devices.remove(&child);
        }
        for (target, label, in_use) in &rows {
            let name = if *in_use {
                tr_with("{device}, in use", "device", label)
            } else {
                label.clone()
            };
            let button = gtk4::Button::with_label(&name);
            button.add_css_class("bar-row");
            button.update_property(&[Property::Label(&name)]);
            let (kind, target) = (self.kind, target.clone());
            button.connect_clicked(move |_| {
                if let Some(service) = service() {
                    service.set_default(kind, &target);
                }
            });
            self.devices.append(&button);
        }
        self.shown.replace(rows);
        chosen
    }
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    output: Channel,
    input: Channel,
    media: gtk4::Box,
    title: gtk4::Label,
    artist: gtk4::Label,
    previous: gtk4::Button,
    play: gtk4::Button,
    next: gtk4::Button,
    /// Widgets are being set from the sound server, not by the person.
    updating: Cell<bool>,
    /// `ATHANOR_BAR_OPEN=audio` came before the popover's content was complete.
    pending_open: Cell<bool>,
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("audio-volume-medium-symbolic");
        let popup = Popup::new(bar, &icon, &tr("Sound"));
        popup.button.set_visible(false);
        let output = Channel::new(
            Kind::Output,
            &tr("Output"),
            &tr("Output volume"),
            &tr("Mute output"),
        );
        let input = Channel::new(
            Kind::Input,
            &tr("Input"),
            &tr("Input volume"),
            &tr("Mute microphone"),
        );

        let title = gtk4::Label::new(None);
        title.add_css_class("bar-popover-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let artist = gtk4::Label::new(None);
        artist.add_css_class("bar-popover-note");
        artist.set_xalign(0.0);
        artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let previous = media_button("media-skip-backward-symbolic", &tr("Previous track"));
        let play = media_button("media-playback-pause-symbolic", &tr("Pause"));
        let next = media_button("media-skip-forward-symbolic", &tr("Next track"));
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        controls.set_halign(gtk4::Align::Center);
        controls.append(&previous);
        controls.append(&play);
        controls.append(&next);
        let media = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        media.append(&title);
        media.append(&artist);
        media.append(&controls);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        content.append(&output.section);
        content.append(&input.section);
        content.append(&media);
        popup.set_content(&content);

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            output,
            input,
            media,
            title,
            artist,
            previous,
            play,
            next,
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers();
        view
    }

    fn channel(&self, kind: Kind) -> &Channel {
        match kind {
            Kind::Output => &self.output,
            Kind::Input => &self.input,
        }
    }

    fn connect_handlers(self: &Rc<Self>) {
        for kind in [Kind::Output, Kind::Input] {
            let weak = Rc::downgrade(self);
            self.channel(kind)
                .scale
                .connect_value_changed(move |scale| {
                    let Some(view) = weak.upgrade() else { return };
                    let chosen = view.channel(kind).chosen.borrow().clone();
                    if let (false, Some(name), Some(service)) =
                        (view.updating.get(), chosen, service())
                    {
                        service.set_volume(kind, &name, scale.value());
                    }
                });
            let weak = Rc::downgrade(self);
            self.channel(kind).mute.connect_state_set(move |_, muted| {
                if let Some(view) = weak.upgrade() {
                    let chosen = view.channel(kind).chosen.borrow().clone();
                    if let (false, Some(name), Some(service)) =
                        (view.updating.get(), chosen, service())
                    {
                        service.set_mute(kind, &name, muted);
                    }
                }
                glib::Propagation::Proceed
            });
        }
        for (button, command) in [
            (&self.previous, MediaCommand::Previous),
            (&self.play, MediaCommand::PlayPause),
            (&self.next, MediaCommand::Next),
        ] {
            button.connect_clicked(move |_| {
                if let Some(service) = service() {
                    service.media(command.clone());
                }
            });
        }
    }

    fn show(self: &Rc<Self>, snapshot: Option<&AudioState>, now: Option<&NowPlaying>) {
        let Some(snapshot) = snapshot else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.updating.set(true);
        let output = self
            .output
            .show(&snapshot.outputs, snapshot.default_output.as_deref());
        let input = self
            .input
            .show(&snapshot.inputs, snapshot.default_input.as_deref());
        self.updating.set(false);
        let icon = match (&output, &input) {
            (Some(output), _) => audio::output_icon(output.percent, output.muted),
            (None, Some(input)) => audio::input_icon(input.muted),
            (None, None) => "audio-volume-muted-symbolic",
        };
        self.icon.set_icon_name(Some(icon));
        self.media.set_visible(now.is_some());
        if let Some(now) = now {
            self.title.set_text(&now.track.title);
            self.artist
                .set_text(now.track.artist.as_deref().unwrap_or(""));
            self.artist.set_visible(now.track.artist.is_some());
            let (icon, name) = if now.playing {
                ("media-playback-pause-symbolic", tr("Pause"))
            } else {
                ("media-playback-start-symbolic", tr("Play"))
            };
            self.play.set_icon_name(icon);
            self.play.set_tooltip_text(Some(&name));
            self.play.update_property(&[Property::Label(&name)]);
            self.previous.set_sensitive(now.can_previous);
            self.next.set_sensitive(now.can_next);
        }
        self.popup.button.set_visible(true);
        if self.pending_open.get() && media_settled() {
            self.pending_open.set(false);
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }
}

/// Whether the media controls are final. The captures open the popover only then: a
/// popover that grows after its first frame leaves its edge column stale in cosmic-comp at
/// a fractional scale.
fn media_settled() -> bool {
    service()
        .and_then(|service| service.media.borrow().as_ref().map(|media| media.settled()))
        .unwrap_or(true)
}

fn media_button(icon: &str, name: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("bar-row");
    button.set_tooltip_text(Some(name));
    button.update_property(&[Property::Label(name)]);
    button
}

struct AudioUi {
    view: Rc<View>,
}

impl ModuleUi for AudioUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() && media_settled() {
            self.view.popup.open(bar);
        } else {
            self.view.pending_open.set(true);
        }
    }
}

pub fn new(bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let service = Service::get(bar);
    let view = View::new(bar);
    service.views.borrow_mut().push(Rc::downgrade(&view));
    service.show_all();
    Some(Box::new(AudioUi { view }))
}
