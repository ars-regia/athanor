//! The audio module (doc_bar.md, BR3): output and input volume, mute, the device in use,
//! and the media controls. The sound server is reached through libpulse on the bar's GLib
//! main loop; `pipewire-pulse` serves it. A lost server hides the module, and the bar
//! reconnects with a backoff from 500 ms to 8 s.
//!
//! Two rules keep libpulse from aborting the bar:
//! - a libpulse callback only stages data and schedules an idle callback: `connect()` calls
//!   the state callback while the context is borrowed, and the other callbacks run inside
//!   libpulse's dispatch;
//! - every call on the context goes through `with_ready`: libpulse-binding asserts on the
//!   null operation a context in any other state returns.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

use athanor_bar::audio::{self, Device};
use gtk4::accessible::Property;
use gtk4::glib;
use gtk4::prelude::*;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::proplist::{properties, Proplist};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use libpulse_glib_binding::Mainloop;

use super::mpris::{Media, NowPlaying};
use super::popup::{switch_row, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

const FIRST_RETRY_MS: u64 = 500;
const LAST_RETRY_MS: u64 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Output,
    Input,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Snapshot {
    outputs: Vec<Device>,
    inputs: Vec<Device>,
    default_output: Option<String>,
    default_input: Option<String>,
}

/// What the four introspection calls of one refresh gather.
#[derive(Default)]
struct Gathering {
    snapshot: Snapshot,
    volumes: HashMap<(Kind, String), ChannelVolumes>,
    remaining: u8,
}

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

struct Service {
    bar: Weak<Bar>,
    mainloop: Option<Mainloop>,
    context: RefCell<Option<Context>>,
    retry_ms: Cell<u64>,
    gathering: RefCell<Option<Gathering>>,
    /// An event arrived during a refresh: refresh again once it ends.
    stale: Cell<bool>,
    snapshot: RefCell<Option<Snapshot>>,
    volumes: RefCell<HashMap<(Kind, String), ChannelVolumes>>,
    media: RefCell<Option<Rc<Media>>>,
    views: RefCell<Vec<Weak<View>>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        let mainloop = Mainloop::new(None);
        if mainloop.is_none() {
            tracing::error!("libpulse has no GLib main loop; the audio module is hidden");
        }
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            mainloop,
            context: RefCell::new(None),
            retry_ms: Cell::new(FIRST_RETRY_MS),
            gathering: RefCell::new(None),
            stale: Cell::new(false),
            snapshot: RefCell::new(None),
            volumes: RefCell::new(HashMap::new()),
            media: RefCell::new(None),
            views: RefCell::new(Vec::new()),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        let weak = Rc::downgrade(&service);
        service.media.replace(Some(Media::new(move || {
            if let Some(service) = weak.upgrade() {
                service.show_all();
            }
        })));
        service.connect();
        service
    }

    fn connect(self: &Rc<Self>) {
        let Some(mainloop) = self.mainloop.as_ref() else {
            return;
        };
        let Some(mut proplist) = Proplist::new() else {
            self.retry();
            return;
        };
        if proplist
            .set_str(properties::APPLICATION_NAME, "athanor-bar")
            .is_err()
            || proplist
                .set_str(properties::APPLICATION_ID, "os.athanor.Bar")
                .is_err()
        {
            tracing::error!("libpulse refused the bar's properties");
        }
        let Some(mut context) = Context::new_with_proplist(mainloop, "athanor-bar", &proplist)
        else {
            self.retry();
            return;
        };
        let weak = Rc::downgrade(self);
        context.set_state_callback(Some(Box::new(move || {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(service) = weak.upgrade() {
                    service.state_changed();
                }
            });
        })));
        let weak = Rc::downgrade(self);
        context.set_subscribe_callback(Some(Box::new(move |facility, _, _| {
            if matches!(
                facility,
                Some(Facility::Sink | Facility::Source | Facility::Server)
            ) {
                let weak = weak.clone();
                glib::idle_add_local_once(move || {
                    if let Some(service) = weak.upgrade() {
                        service.refresh();
                    }
                });
            }
        })));
        // No autospawn: the session starts the sound server, never the bar.
        if let Err(err) = context.connect(None, FlagSet::NOAUTOSPAWN, None) {
            tracing::info!(error = %err, "no sound server yet");
            self.retry();
            return;
        }
        self.context.replace(Some(context));
    }

    fn retry(self: &Rc<Self>) {
        let delay = self.retry_ms.get();
        self.retry_ms.set((delay * 2).min(LAST_RETRY_MS));
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            if let Some(service) = weak.upgrade() {
                service.connect();
            }
        });
    }

    fn state_changed(self: &Rc<Self>) {
        let state = self.context.borrow().as_ref().map(Context::get_state);
        match state {
            Some(State::Ready) => {
                self.retry_ms.set(FIRST_RETRY_MS);
                self.with_ready(|context| {
                    context.subscribe(
                        InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
                        |_| {},
                    );
                });
                self.refresh();
            }
            Some(State::Failed | State::Terminated) => {
                tracing::info!(
                    "the sound server went away; the audio module is hidden until it returns"
                );
                if let Some(mut context) = self.context.take() {
                    context.set_state_callback(None);
                    context.set_subscribe_callback(None);
                    context.disconnect();
                }
                self.gathering.replace(None);
                self.stale.set(false);
                self.volumes.borrow_mut().clear();
                self.publish(None);
                self.retry();
            }
            _ => {}
        }
    }

    /// Runs `action` on a ready context, and never on one in any other state.
    fn with_ready(&self, action: impl FnOnce(&mut Context)) {
        let mut context = self.context.borrow_mut();
        if let Some(context) = context
            .as_mut()
            .filter(|context| context.get_state() == State::Ready)
        {
            action(context);
        }
    }

    /// Reads the default sink and source, the sinks and the sources; events during a
    /// refresh coalesce into one more. The defaults come through the special names, whose
    /// callbacks turn an error or a timeout into `ListResult::Error` (rule 3).
    fn refresh(self: &Rc<Self>) {
        if self.gathering.borrow().is_some() {
            self.stale.set(true);
            return;
        }
        self.gathering.replace(Some(Gathering {
            remaining: 4,
            ..Gathering::default()
        }));
        let mut started = false;
        self.with_ready(|context| {
            let introspect = context.introspect();
            let weak = Rc::downgrade(self);
            introspect.get_sink_info_by_name("@DEFAULT_SINK@", move |result| match result {
                ListResult::Item(info) => {
                    let output = info.name.as_deref().map(str::to_owned);
                    stage(&weak, |gathering| {
                        gathering.snapshot.default_output = output
                    });
                }
                ListResult::End | ListResult::Error => finish(&weak),
            });
            let weak = Rc::downgrade(self);
            introspect.get_source_info_by_name("@DEFAULT_SOURCE@", move |result| match result {
                ListResult::Item(info) => {
                    let input = info.name.as_deref().map(str::to_owned);
                    stage(&weak, |gathering| gathering.snapshot.default_input = input);
                }
                ListResult::End | ListResult::Error => finish(&weak),
            });
            let weak = Rc::downgrade(self);
            introspect.get_sink_info_list(move |result| match result {
                ListResult::Item(info) => {
                    if let Some(name) = info.name.as_deref() {
                        let device =
                            device(name, info.description.as_deref(), &info.volume, info.mute);
                        let volume = info.volume;
                        stage(&weak, |gathering| {
                            if gathering.snapshot.outputs.len() < audio::MAX_DEVICES {
                                gathering
                                    .volumes
                                    .insert((Kind::Output, device.name.clone()), volume);
                                gathering.snapshot.outputs.push(device);
                            }
                        });
                    }
                }
                ListResult::End | ListResult::Error => finish(&weak),
            });
            let weak = Rc::downgrade(self);
            introspect.get_source_info_list(move |result| match result {
                // A monitor is a sink's loopback, not a microphone.
                ListResult::Item(info) if info.monitor_of_sink.is_none() => {
                    if let Some(name) = info.name.as_deref() {
                        let device =
                            device(name, info.description.as_deref(), &info.volume, info.mute);
                        let volume = info.volume;
                        stage(&weak, |gathering| {
                            if gathering.snapshot.inputs.len() < audio::MAX_DEVICES {
                                gathering
                                    .volumes
                                    .insert((Kind::Input, device.name.clone()), volume);
                                gathering.snapshot.inputs.push(device);
                            }
                        });
                    }
                }
                ListResult::Item(_) => {}
                ListResult::End | ListResult::Error => finish(&weak),
            });
            started = true;
        });
        if !started {
            self.gathering.replace(None);
        }
    }

    /// One of the four calls ended: the last one publishes, on an idle callback.
    fn finished_one(self: &Rc<Self>) {
        let done = match self.gathering.borrow_mut().as_mut() {
            Some(gathering) => {
                gathering.remaining = gathering.remaining.saturating_sub(1);
                gathering.remaining == 0
            }
            None => false,
        };
        if !done {
            return;
        }
        let Some(gathering) = self.gathering.take() else {
            return;
        };
        self.volumes.replace(gathering.volumes);
        self.publish(Some(gathering.snapshot));
        if self.stale.replace(false) {
            self.refresh();
        }
    }

    fn publish(&self, snapshot: Option<Snapshot>) {
        let snapshot =
            snapshot.filter(|snapshot| !snapshot.outputs.is_empty() || !snapshot.inputs.is_empty());
        if *self.snapshot.borrow() == snapshot {
            return;
        }
        self.snapshot.replace(snapshot);
        self.show_all();
    }

    fn show_all(&self) {
        let now = self.media.borrow().as_ref().and_then(|media| media.now());
        for view in self.views() {
            view.show(self.snapshot.borrow().as_ref(), now.as_ref());
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

    fn set_volume(&self, kind: Kind, name: &str, percent: f64) {
        let Some(mut volume) = self.volumes.borrow().get(&(kind, name.to_owned())).copied() else {
            return;
        };
        // `scale` keeps the balance between channels; an invalid volume would make libpulse
        // return a null operation.
        if !volume.is_valid() || volume.scale(Volume(audio::raw(percent))).is_none() {
            return;
        }
        self.with_ready(|context| {
            let mut introspect = context.introspect();
            match kind {
                Kind::Output => introspect.set_sink_volume_by_name(name, &volume, None),
                Kind::Input => introspect.set_source_volume_by_name(name, &volume, None),
            };
        });
    }

    fn set_mute(&self, kind: Kind, name: &str, muted: bool) {
        self.with_ready(|context| {
            let mut introspect = context.introspect();
            match kind {
                Kind::Output => introspect.set_sink_mute_by_name(name, muted, None),
                Kind::Input => introspect.set_source_mute_by_name(name, muted, None),
            };
        });
    }

    fn set_default(&self, kind: Kind, name: &str) {
        self.with_ready(|context| {
            match kind {
                Kind::Output => context.set_default_sink(name, |_| {}),
                Kind::Input => context.set_default_source(name, |_| {}),
            };
        });
    }

    fn media(&self, method: &'static str) {
        if let Some(media) = self.media.borrow().as_ref() {
            media.command(method);
        }
    }
}

fn device(name: &str, description: Option<&str>, volume: &ChannelVolumes, muted: bool) -> Device {
    Device {
        name: name.to_owned(),
        label: audio::device_label(description, name),
        percent: audio::percent(volume.max().0),
        muted,
    }
}

/// Inside a libpulse callback: writes into the gathering, which no other code borrows then.
fn stage(weak: &Weak<Service>, write: impl FnOnce(&mut Gathering)) {
    if let Some(service) = weak.upgrade() {
        if let Some(gathering) = service.gathering.borrow_mut().as_mut() {
            write(gathering);
        }
    }
}

/// Inside a libpulse callback: the rest runs on the main loop.
fn finish(weak: &Weak<Service>) {
    let weak = weak.clone();
    glib::idle_add_local_once(move || {
        if let Some(service) = weak.upgrade() {
            service.finished_one();
        }
    });
}

/// One direction: a volume slider, a mute switch, and the devices when there are several.
struct Channel {
    kind: Kind,
    section: gtk4::Box,
    scale: gtk4::Scale,
    mute: gtk4::Switch,
    devices: gtk4::Box,
    chosen: RefCell<Option<String>>,
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
        while let Some(child) = self.devices.first_child() {
            self.devices.remove(&child);
        }
        if devices.len() > 1 {
            for device in devices {
                let in_use = chosen
                    .as_ref()
                    .is_some_and(|chosen| chosen.name == device.name);
                let name = if in_use {
                    tr_with("{device}, in use", "device", &device.label)
                } else {
                    device.label.clone()
                };
                let button = gtk4::Button::with_label(&name);
                button.add_css_class("bar-row");
                button.update_property(&[Property::Label(&name)]);
                let (kind, target) = (self.kind, device.name.clone());
                button.connect_clicked(move |_| {
                    if let Some(service) = service() {
                        service.set_default(kind, &target);
                    }
                });
                self.devices.append(&button);
            }
        }
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
        popup.popover.set_child(Some(&content));

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
        for (button, method) in [
            (&self.previous, "Previous"),
            (&self.play, "PlayPause"),
            (&self.next, "Next"),
        ] {
            button.connect_clicked(move |_| {
                if let Some(service) = service() {
                    service.media(method);
                }
            });
        }
    }

    fn show(self: &Rc<Self>, snapshot: Option<&Snapshot>, now: Option<&NowPlaying>) {
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
    let now = service
        .media
        .borrow()
        .as_ref()
        .and_then(|media| media.now());
    view.show(service.snapshot.borrow().as_ref(), now.as_ref());
    Some(Box::new(AudioUi { view }))
}
