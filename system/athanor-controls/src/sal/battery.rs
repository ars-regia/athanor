//! The battery page drawn with athanor-sal (doc_bar.md, BR3): the charge and the time left from
//! UPower's display device, the power profile from the power-profiles interface, and the
//! screen brightness through logind's `SetBrightness`, as the GTK page shows them.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use athanor_sal::{Event, Kind, TextStyle, Ui, WidgetId};
use athanor_services::battery::{self, BatteryCommand, BatteryState, Profiles};
use tokio::sync::mpsc;

use super::widgets::{column, Failure, Look};
use super::Turn;
use crate::bridge;
use crate::i18n::tr;
use crate::labels::profile_label;
pub use crate::labels::{note, percent_text};
use crate::Services;

thread_local! {
    static SERVICE: RefCell<Option<Rc<Service>>> = const { RefCell::new(None) };
}

/// The battery model of the process, which every page shows: the bar's popover on each output
/// and the control center's page.
struct Service {
    commands: mpsc::UnboundedSender<BatteryCommand>,
    /// The model's last state, which `bridge::follow` keeps current.
    state: RefCell<BatteryState>,
    /// Counts the states, so that a page shows one it has not seen.
    states: Cell<u64>,
    /// Counts the actions that did not complete: the model refused them or had ended.
    failures: Cell<u64>,
    backlight_root: PathBuf,
}

impl Service {
    fn get(services: &Services) -> Rc<Service> {
        if let Some(service) = SERVICE.with(|cell| cell.borrow().clone()) {
            return service;
        }
        let (states, commands) = services.battery.clone();
        let service = Rc::new(Service {
            commands,
            state: RefCell::new(BatteryState::default()),
            states: Cell::new(0),
            failures: Cell::new(0),
            backlight_root: services.backlight_root.clone(),
        });
        SERVICE.with(|cell| cell.replace(Some(service.clone())));
        // Weak: the thread-local owns the service.
        let weak = Rc::downgrade(&service);
        bridge::follow(states, move |state: &BatteryState| {
            let Some(service) = weak.upgrade() else {
                return;
            };
            if state.refused != service.state.borrow().refused {
                service.failed();
            }
            service.state.replace(state.clone());
            service.states.set(service.states.get() + 1);
        });
        service
    }

    fn failed(&self) {
        self.failures.set(self.failures.get() + 1);
    }

    fn send(&self, command: BatteryCommand) {
        if let Err(err) = self.commands.send(command) {
            tracing::warn!(error = %err, "the battery model did not take a command");
            // The channel is unbounded and the model coalesces what it reads, so a send
            // fails only when the model has ended: the action did not complete, as for a
            // refusal, and the page shows the state again.
            self.failed();
            self.states.set(self.states.get() + 1);
        }
    }
}

/// The widgets of the page in the surface's [`Ui`].
struct Ids {
    charge: WidgetId,
    note: WidgetId,
    profiles_section: WidgetId,
    profiles: Vec<(&'static str, WidgetId)>,
    brightness_section: WidgetId,
    /// None only if athanor-sal refused the slider's range, which is a constant.
    brightness: Option<WidgetId>,
    failure: Failure,
}

/// The battery page: one per surface, over the one model of the process. The surface adds it
/// to its widgets with [`Page::build`], calls [`Page::turn`] in each turn of its loop and hands
/// it every event with [`Page::event`].
pub struct Page {
    service: Rc<Service>,
    /// The state and the failure the page shows last.
    seen: (u64, u64),
    /// A failure's note is on screen: a page built again, while the old one is, shows it too.
    noted: bool,
    ids: Option<Ids>,
}

impl Page {
    pub fn new(services: &Services) -> Page {
        Page {
            service: Service::get(services),
            seen: (0, 0),
            noted: false,
            ids: None,
        }
    }

    /// The state the page shows: `battery` is `None` without a present battery, and then the
    /// surface hides what opens the page.
    pub fn state(&self) -> BatteryState {
        self.service.state.borrow().clone()
    }

    /// Adds the page's widgets as a column under `parent`, showing the model's state, and
    /// returns the column, the time left and a failure's note as secondary lines of `look`. A
    /// page built again, in a new [`Ui`], leaves the old one's widgets.
    pub fn build(&mut self, ui: &mut Ui, parent: WidgetId, look: &Look) -> WidgetId {
        let title = || TextStyle {
            weight: 600,
            ..TextStyle::default()
        };
        let page = column(ui, parent);
        let summary = column(ui, page);
        let charge = ui.add(summary, Kind::Label(String::new()));
        ui.set_text_style(charge, title());
        let note = ui.add(summary, Kind::Label(String::new()));
        look.note(ui, note);

        // Columns, not groups: the GTK page's boxes give assistive technologies no group.
        let profiles_section = column(ui, page);
        ui.set_spacing(profiles_section, look.radio_gap);
        let heading = ui.add(profiles_section, Kind::Label(tr("Power mode")));
        ui.set_text_style(heading, title());
        let profiles = battery::PROFILE_NAMES
            .iter()
            .map(|&name| {
                (
                    name,
                    ui.radio(profiles_section, &profile_label(name), false),
                )
            })
            .collect();

        let brightness_section = column(ui, page);
        let heading = ui.add(brightness_section, Kind::Label(tr("Screen brightness")));
        ui.set_text_style(heading, title());
        let brightness = ui
            .slider(
                brightness_section,
                &tr("Screen brightness"),
                1.0,
                100.0,
                1.0,
                50.0,
            );
        let failure = Failure::new(ui, page, look);
        if self.noted {
            failure.restore(ui);
        }
        self.ids = Some(Ids {
            charge,
            note,
            profiles_section,
            profiles,
            brightness_section,
            brightness,
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
        self.show(ui);
        if let (true, Some(ids)) = (failed, &self.ids) {
            self.noted |= ids.failure.show(ui);
        }
        Turn::Updated
    }

    /// The page came on screen: the brightness keys change the backlight without a signal, so
    /// it is read again.
    pub fn opened(&mut self, ui: &mut Ui) {
        let backlight = battery::read_backlight(&self.service.backlight_root);
        self.service.state.borrow_mut().backlight = backlight;
        self.show(ui);
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
    }

    /// Takes `event` if it is one of the page's widgets': whether it was.
    pub fn event(&mut self, ui: &mut Ui, event: &Event) -> bool {
        let Some(ids) = &self.ids else {
            return false;
        };
        match *event {
            Event::Selected(id) => {
                let Some(&(name, _)) = ids.profiles.iter().find(|(_, w)| *w == id) else {
                    return false;
                };
                self.service
                    .send(BatteryCommand::SetProfile(name.to_owned()));
                true
            }
            Event::Changed(id) if Some(id) == ids.brightness => {
                let backlight = self.service.state.borrow().backlight.clone();
                if let (Some(backlight), Some(percent)) = (backlight, ui.value(id)) {
                    self.service.send(BatteryCommand::SetBrightness {
                        raw: backlight.raw(percent),
                        device: backlight.name,
                    });
                }
                true
            }
            _ => false,
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        self.seen = (self.service.states.get(), self.service.failures.get());
        let (Some(ids), status) = (&self.ids, self.service.state.borrow()) else {
            return;
        };
        let Some(battery) = status.battery else {
            return;
        };
        ui.set_text(ids.charge, &percent_text(battery.percent));
        let note = note(&battery);
        ui.set_text(ids.note, note.as_deref().unwrap_or(""));
        ui.set_visible(ids.note, note.is_some());

        ui.set_visible(ids.profiles_section, status.profiles.is_some());
        if let Some(Profiles { offered, active }) = &status.profiles {
            for &(name, radio) in &ids.profiles {
                ui.set_visible(radio, offered.contains(&name));
                if name == active.as_str() {
                    ui.set_active(radio, true);
                }
            }
        }
        ui.set_visible(ids.brightness_section, status.backlight.is_some());
        if let Some(backlight) = &status.backlight {
            if let Some(slider) = ids.brightness {
                ui.set_value(slider, backlight.percent());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use athanor_sal::Theme;
    use athanor_services::battery::{Backlight, Battery, Charge};

    use super::*;

    fn page() -> (Page, mpsc::UnboundedReceiver<BatteryCommand>) {
        let (commands, received) = mpsc::unbounded_channel();
        let service = Rc::new(Service {
            commands,
            state: RefCell::new(BatteryState::default()),
            states: Cell::new(0),
            failures: Cell::new(0),
            backlight_root: PathBuf::from("/nonexistent"),
        });
        let page = Page {
            service,
            seen: (0, 0),
            noted: false,
            ids: None,
        };
        (page, received)
    }

    fn publish(page: &Page, state: BatteryState) {
        let service = &page.service;
        if state.refused != service.state.borrow().refused {
            service.failed();
        }
        service.state.replace(state);
        service.states.set(service.states.get() + 1);
    }

    fn laptop() -> BatteryState {
        BatteryState {
            battery: Some(Battery {
                percent: 72.4,
                charge: Charge::Discharging,
                seconds: Some(3 * 3600 + 20 * 60),
            }),
            profiles: Some(Profiles {
                offered: vec!["power-saver", "balanced"],
                active: "balanced".into(),
            }),
            backlight: Some(Backlight {
                name: "intel_backlight".into(),
                max: 1000,
                level: 600,
            }),
            refused: 0,
        }
    }

    #[test]
    fn the_page_shows_the_charge_the_profiles_offered_and_the_brightness() {
        let (mut page, _) = page();
        let mut ui = Ui::new("Battery", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &Look::plain());
        assert_eq!(page.turn(&mut ui), Turn::Unchanged, "nothing new");
        publish(&page, laptop());
        assert_eq!(page.turn(&mut ui), Turn::Updated);
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        assert!(matches!(&ui.widget(ids.charge).kind, Kind::Label(t) if t == "72 %"));
        assert!(matches!(&ui.widget(ids.note).kind, Kind::Label(t) if t == "3 h 20 min left"));
        assert_eq!(ui.widget(ids.note).text_style(), &Look::plain().caption);
        let shown: Vec<_> = ids
            .profiles
            .iter()
            .map(|&(name, id)| (name, ui.is_visible(id), ui.is_active(id)))
            .collect();
        assert_eq!(
            shown,
            [
                ("power-saver", true, false),
                ("balanced", true, true),
                ("performance", false, false)
            ]
        );
        assert_eq!(ids.brightness.and_then(|s| ui.value(s)), Some(60.0));

        // A desktop: no profiles daemon, no backlight.
        publish(
            &page,
            BatteryState {
                profiles: None,
                backlight: None,
                ..laptop()
            },
        );
        page.turn(&mut ui);
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        assert!(!ui.is_visible(ids.profiles_section));
        assert!(!ui.is_visible(ids.brightness_section));
    }

    #[test]
    fn the_persons_choices_go_to_the_model_and_a_refusal_shows_the_note() {
        let (mut page, mut received) = page();
        let mut ui = Ui::new("Battery", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &Look::plain());
        publish(&page, laptop());
        page.turn(&mut ui);
        let Some(ids) = &page.ids else {
            unreachable!("built");
        };
        let (saver, slider, note) = (ids.profiles[0].1, ids.brightness, ids.failure.note);

        let event = ui.activate(saver).expect("a radio is chosen");
        assert!(page.event(&mut ui, &event));
        assert!(matches!(
            received.try_recv(),
            Ok(BatteryCommand::SetProfile(p)) if p == "power-saver"
        ));
        let slider = slider.expect("a slider");
        let event = ui.set_value(slider, 30.0).expect("the slider moves");
        assert!(page.event(&mut ui, &event));
        assert!(matches!(
            received.try_recv(),
            Ok(BatteryCommand::SetBrightness { device, raw: 300 }) if device == "intel_backlight"
        ));
        assert!(!page.event(&mut ui, &Event::Clicked(Ui::ROOT)), "not the page's");

        // The model refused: the state comes back with the active profile, and the note shows.
        assert!(!ui.is_visible(note));
        publish(&page, BatteryState { refused: 1, ..laptop() });
        page.turn(&mut ui);
        assert!(ui.is_visible(note));
        assert!(ui.is_active(page.ids.as_ref().map_or(saver, |ids| ids.profiles[1].1)));
        page.closed(&mut ui);
        assert!(!ui.is_visible(note));
    }

    #[test]
    fn a_note_shown_stays_in_the_page_built_again_until_its_surface_closes() {
        let note = |page: &Page| page.ids.as_ref().map(|ids| ids.failure.note);
        let (mut page, _) = page();
        let mut ui = Ui::new("Battery", Theme::default(), false);
        page.build(&mut ui, Ui::ROOT, &Look::plain());
        publish(&page, laptop());
        page.turn(&mut ui);
        publish(&page, BatteryState { refused: 1, ..laptop() });
        page.turn(&mut ui);

        // The note grew the popover, which the bar builds again at its new size.
        let mut again = Ui::new("Battery", Theme::default(), false);
        page.build(&mut again, Ui::ROOT, &Look::plain());
        assert_eq!(note(&page).map(|id| again.is_visible(id)), Some(true));

        // The popover closed: the next one starts without the note.
        page.discarded();
        let mut next = Ui::new("Battery", Theme::default(), false);
        page.build(&mut next, Ui::ROOT, &Look::plain());
        assert_eq!(note(&page).map(|id| next.is_visible(id)), Some(false));
    }
}
