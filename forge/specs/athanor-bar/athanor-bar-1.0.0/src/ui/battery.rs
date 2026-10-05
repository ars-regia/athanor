//! The battery module (doc_bar.md, BR3): the charge and the time left from UPower's display
//! device, the power profile from the power-profiles interface (tuned-ppd on Fedora), and
//! the screen brightness through logind's `SetBrightness`. Nothing goes through COSMIC's
//! settings daemon. Without a present battery the module hides (SH1).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use athanor_services::battery::{
    self, BatteryCommand, BatteryState, Charge, Profiles, BACKLIGHT_ROOT,
};
use gtk4::accessible::{Property, Relation};
use gtk4::prelude::*;
use tokio::sync::mpsc;

use super::bridge;
use super::popup::{expose_choose_action, Popup};
use super::{Bar, Changed, ModuleUi};
use crate::i18n::{tr, tr_with};

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

fn service() -> Option<Rc<Service>> {
    SERVICE.with(|cell| cell.borrow().clone())
}

struct Service {
    bar: Weak<Bar>,
    /// `None` without the runtime of the models; the module is then hidden.
    commands: Option<mpsc::UnboundedSender<BatteryCommand>>,
    /// The model's last state, which `bridge::follow` keeps current.
    state: RefCell<BatteryState>,
    /// How many commands the model had counted as refused when the views last showed it.
    refused: Cell<u32>,
    backlight_root: PathBuf,
    views: RefCell<Vec<Weak<View>>>,
}

impl Service {
    fn get(bar: &Rc<Bar>) -> Rc<Service> {
        if let Some(service) = service() {
            return service;
        }
        // The rig points the bar at a fake sysfs directory; the level still goes through
        // logind, which checks the device itself.
        let backlight_root = std::env::var_os("ATHANOR_BAR_BACKLIGHT_DIR")
            .map_or_else(|| PathBuf::from(BACKLIGHT_ROOT), PathBuf::from);
        let model = bridge::runtime()
            .map(|(handle, buses)| battery::spawn(&handle, buses, backlight_root.clone()));
        let (states, commands) = match model {
            Some((states, commands)) => (Some(states), Some(commands)),
            None => {
                tracing::warn!("no runtime for the models; the battery module is hidden");
                (None, None)
            }
        };
        let service = Rc::new(Service {
            bar: Rc::downgrade(bar),
            commands,
            state: RefCell::new(BatteryState::default()),
            refused: Cell::new(0),
            backlight_root,
            views: RefCell::new(Vec::new()),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        if let Some(states) = states {
            // Weak, as the other modules hold the service: the thread-local owns it.
            let weak = Rc::downgrade(&service);
            bridge::follow(states, move |state| {
                let Some(service) = weak.upgrade() else {
                    return;
                };
                let failed = service.refused.replace(state.refused) != state.refused;
                service.state.replace(state.clone());
                // The views show the state again, which puts a radio or the slider back, and
                // the open popover says the action did not complete.
                service.show_all();
                if failed {
                    for view in service.views() {
                        view.popup.failed();
                    }
                }
            });
        }
        service
    }

    /// The state with the backlight read again: the brightness keys change it without a
    /// signal, so a popover reads it when it opens.
    fn fresh_status(&self) -> BatteryState {
        BatteryState {
            backlight: battery::read_backlight(&self.backlight_root),
            ..self.state.borrow().clone()
        }
    }

    fn show_all(&self) {
        let status = self.state.borrow().clone();
        for view in self.views() {
            view.show(&status);
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

    fn send(&self, command: BatteryCommand) {
        let Some(commands) = &self.commands else {
            return;
        };
        if let Err(err) = commands.send(command) {
            tracing::warn!(error = %err, "the battery model did not take a command");
            // The channel is unbounded and the model coalesces what it reads, so a send
            // fails only when the model has ended: say the action did not complete, as for
            // a refusal.
            self.show_all();
            for view in self.views() {
                view.popup.failed();
            }
        }
    }

    fn set_profile(&self, profile: &'static str) {
        self.send(BatteryCommand::SetProfile(profile.to_owned()));
    }

    fn set_brightness(&self, percent: f64) {
        let Some(backlight) = self.state.borrow().backlight.clone() else {
            return;
        };
        self.send(BatteryCommand::SetBrightness {
            raw: backlight.raw(percent),
            device: backlight.name,
        });
    }
}

fn percent_text(percent: f64) -> String {
    // At most 100 after `battery::battery`, so the cast cannot truncate.
    tr_with(
        "{percent} %",
        "percent",
        &(percent.round() as u32).to_string(),
    )
}

/// A translated `text` with `{hours}` and `{minutes}` filled in. The callers pass `tr(...)`
/// with the literal message id, so xgettext finds it.
fn duration(text: &str, seconds: u64) -> String {
    let (hours, minutes) = battery::hours_minutes(seconds);
    text.replace("{hours}", &hours.to_string())
        .replace("{minutes}", &minutes.to_string())
}

/// The line under the charge: the time left, the time until full, or the state.
fn note(battery: &battery::Battery) -> Option<String> {
    match (battery.charge, battery.seconds) {
        (Charge::Discharging, Some(seconds)) => {
            Some(duration(&tr("{hours} h {minutes} min left"), seconds))
        }
        (Charge::Charging, Some(seconds)) => {
            Some(duration(&tr("{hours} h {minutes} min until full"), seconds))
        }
        (Charge::Charging, None) => Some(tr("Charging")),
        (Charge::Full, _) => Some(tr("Fully charged")),
        (Charge::Discharging | Charge::Unknown, _) => None,
    }
}

fn profile_label(profile: &str) -> String {
    match profile {
        "power-saver" => tr("Power saver"),
        "performance" => tr("Performance"),
        _ => tr("Balanced"),
    }
}

struct View {
    bar: Weak<Bar>,
    popup: Popup,
    icon: gtk4::Image,
    level: gtk4::Label,
    charge: gtk4::Label,
    note: gtk4::Label,
    profiles_section: gtk4::Box,
    profiles: Vec<(&'static str, gtk4::CheckButton)>,
    brightness_section: gtk4::Box,
    brightness: gtk4::Scale,
    /// Widgets are being set from the services, not by the person.
    updating: Cell<bool>,
    pending_open: Cell<bool>,
}

impl View {
    fn new(bar: &Rc<Bar>) -> Rc<View> {
        let icon = gtk4::Image::from_icon_name("battery-good-symbolic");
        let level = gtk4::Label::new(None);
        let face = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        face.append(&icon);
        face.append(&level);
        let popup = Popup::new(bar, &face, &tr("Battery"));
        popup.button.set_visible(false);

        let charge = gtk4::Label::new(None);
        charge.add_css_class("bar-popover-title");
        charge.set_xalign(0.0);
        let note = gtk4::Label::new(None);
        note.add_css_class("bar-popover-note");
        note.set_xalign(0.0);

        let profiles_title = gtk4::Label::new(Some(&tr("Power mode")));
        profiles_title.add_css_class("bar-popover-title");
        profiles_title.set_xalign(0.0);
        let profiles_section = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        profiles_section.append(&profiles_title);
        let mut profiles: Vec<(&'static str, gtk4::CheckButton)> = Vec::new();
        for name in battery::PROFILE_NAMES {
            let label = profile_label(name);
            let button = gtk4::CheckButton::with_label(&label);
            button.update_property(&[Property::Label(&label)]);
            if let Some((_, first)) = profiles.first() {
                button.set_group(Some(first));
            }
            expose_choose_action(&button);
            profiles_section.append(&button);
            profiles.push((name, button));
        }

        let brightness_title = gtk4::Label::new(Some(&tr("Screen brightness")));
        brightness_title.add_css_class("bar-popover-title");
        brightness_title.set_xalign(0.0);
        let brightness = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 1.0, 100.0, 1.0);
        brightness.set_draw_value(false);
        brightness.set_hexpand(true);
        brightness.update_relation(&[Relation::LabelledBy(&[brightness_title.upcast_ref()])]);
        let brightness_section = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        brightness_section.append(&brightness_title);
        brightness_section.append(&brightness);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        let summary = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        summary.append(&charge);
        summary.append(&note);
        content.append(&summary);
        content.append(&profiles_section);
        content.append(&brightness_section);
        popup.set_content(&content);

        let view = Rc::new(View {
            bar: Rc::downgrade(bar),
            popup,
            icon,
            level,
            charge,
            note,
            profiles_section,
            profiles,
            brightness_section,
            brightness,
            updating: Cell::new(false),
            pending_open: Cell::new(false),
        });
        view.connect_handlers();
        view
    }

    fn connect_handlers(self: &Rc<Self>) {
        for (name, button) in &self.profiles {
            let (weak, name) = (Rc::downgrade(self), *name);
            button.connect_toggled(move |button| {
                let Some(view) = weak.upgrade() else { return };
                if button.is_active() && !view.updating.get() {
                    if let Some(service) = service() {
                        service.set_profile(name);
                    }
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.brightness.connect_value_changed(move |scale| {
            let Some(view) = weak.upgrade() else { return };
            if let (false, Some(service)) = (view.updating.get(), service()) {
                service.set_brightness(scale.value());
            }
        });
        // The backlight is read again on every opening: the keys change it too.
        let weak = Rc::downgrade(self);
        self.popup.popover.connect_show(move |_| {
            if let (Some(view), Some(service)) = (weak.upgrade(), service()) {
                view.show(&service.fresh_status());
            }
        });
    }

    fn show(self: &Rc<Self>, status: &BatteryState) {
        let Some(battery) = status.battery else {
            self.popup.popover.popdown();
            self.popup.button.set_visible(false);
            return;
        };
        self.updating.set(true);
        let charge = percent_text(battery.percent);
        let note = note(&battery);
        self.icon.set_icon_name(Some(battery::icon(&battery)));
        self.level.set_text(&charge);
        self.charge.set_text(&charge);
        self.note.set_text(note.as_deref().unwrap_or(""));
        self.note.set_visible(note.is_some());
        let description = match &note {
            Some(note) => format!("{charge}, {note}"),
            None => charge.clone(),
        };
        self.popup.button.set_tooltip_text(Some(&description));
        self.popup
            .button
            .update_property(&[Property::Description(&description)]);

        self.profiles_section.set_visible(status.profiles.is_some());
        if let Some(Profiles { offered, active }) = &status.profiles {
            for (name, button) in &self.profiles {
                button.set_visible(offered.contains(name));
                if *name == active.as_str() {
                    button.set_active(true);
                }
            }
        }
        self.brightness_section
            .set_visible(status.backlight.is_some());
        if let Some(backlight) = &status.backlight {
            self.brightness.set_value(backlight.percent());
        }
        self.updating.set(false);

        self.popup.button.set_visible(true);
        if self.pending_open.take() {
            if let Some(bar) = self.bar.upgrade() {
                self.popup.open(&bar);
            }
        }
    }
}

struct BatteryUi {
    view: Rc<View>,
}

impl ModuleUi for BatteryUi {
    fn widget(&self) -> gtk4::Widget {
        self.view.popup.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, _changed: Changed) {}

    fn open(&self, bar: &Rc<Bar>) {
        if self.view.popup.button.get_visible() {
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
    let status = service.state.borrow().clone();
    view.show(&status);
    Some(Box::new(BatteryUi { view }))
}
