//! The clock (doc_bar.md, BR3); a click opens the notification center, which holds the calendar. Every tick reads the zone and the wall
//! clock again: nothing is counted, so a resume or a clock that jumps shows at once.

use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use athanor_bar::clock;
use athanor_compositor_client::clock as cosmic_clock;
use gtk4::accessible::Property;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

use super::control_center::toggle_notifications;
use super::{Bar, Changed, ModuleUi};
use crate::i18n::tr;

/// The zone identifier last read, and the zone it gave.
type Zone = Rc<RefCell<Option<(Option<String>, glib::TimeZone)>>>;

struct ClockUi {
    button: gtk4::Button,
    label: gtk4::Label,
    zone: Zone,
    /// The last tick could not read or format the time: logged once, not every second.
    failed: Cell<bool>,
    /// Whether the clock shows 24 hours (BR3): COSMIC's `military_time`, else the locale.
    hours24: Rc<Cell<bool>>,
    _watch: Option<gio::FileMonitor>,
}

/// COSMIC's `military_time` when it is set, else the locale's own 12/24-hour convention.
fn read_hours24() -> bool {
    let now = glib::DateTime::from_utc(2000, 1, 1, 13, 0, 0.0).ok();
    let format = |pattern| {
        now.as_ref()
            .and_then(|now| now.format(pattern).ok())
            .map(|text| text.to_string())
            .unwrap_or_default()
    };
    clock::twenty_four_hour(cosmic_clock::military_time(), &format("%X"), &format("%p"))
}

fn current_zone(zone: &Zone) -> glib::TimeZone {
    let link = fs::read_link("/etc/localtime").ok();
    let id = clock::zone(
        env::var("TZ").ok().as_deref(),
        link.as_deref().map(Path::new),
    );
    let mut last = zone.borrow_mut();
    match &*last {
        Some((last_id, tz)) if *last_id == id => tz.clone(),
        _ => {
            let (tz, unknown) = clock::time_zone(id.as_deref());
            if unknown {
                // Logged when the zone changes, not every second.
                tracing::warn!(
                    zone = id.as_deref().unwrap_or(""),
                    "unknown time zone; the clock shows local time"
                );
            }
            *last = Some((id, tz.clone()));
            tz
        }
    }
}

impl ModuleUi for ClockUi {
    fn widget(&self) -> gtk4::Widget {
        self.button.clone().upcast()
    }

    fn refresh(&self, _bar: &Rc<Bar>, changed: Changed) {
        if changed != Changed::Tick {
            return;
        }
        let (short_pattern, long_pattern) = if self.hours24.get() {
            (
                // TRANSLATORS: a g_date_time_format() pattern for the clock on the bar.
                tr("%a %-d %b %H:%M"),
                // TRANSLATORS: a g_date_time_format() pattern, read aloud by screen readers.
                tr("%A %-d %B %Y, %H:%M"),
            )
        } else {
            (
                // TRANSLATORS: a g_date_time_format() pattern for the clock on the bar (12-hour clock).
                tr("%a %-d %b %-I:%M %p"),
                // TRANSLATORS: a g_date_time_format() pattern, read aloud by screen readers (12-hour clock).
                tr("%A %-d %B %Y, %-I:%M %p"),
            )
        };
        let texts = glib::DateTime::now(&current_zone(&self.zone)).and_then(|now| {
            let short = now.format(&short_pattern)?;
            let long = now.format(&long_pattern)?;
            Ok((short, long))
        });
        let (short, long) = match texts {
            Ok(texts) => {
                self.failed.set(false);
                texts
            }
            Err(err) => {
                if !self.failed.replace(true) {
                    tracing::error!(error = %err, "cannot read or format the time; the clock is not updated");
                }
                return;
            }
        };
        if self.label.text() != short {
            self.label.set_text(&short);
            self.button.set_tooltip_text(Some(&long));
            self.button.update_property(&[Property::Label(&long)]);
        }
    }

    fn open(&self, _bar: &Rc<Bar>) {
        toggle_notifications();
    }
}

pub fn new(_bar: &Rc<Bar>) -> Option<Box<dyn ModuleUi>> {
    let label = gtk4::Label::new(None);
    label.add_css_class("bar-clock");
    let button = gtk4::Button::new();
    button.set_child(Some(&label));
    button.add_css_class("bar-button");
    let name = tr("Clock");
    button.set_tooltip_text(Some(&name));
    button.update_property(&[Property::Label(&name)]);
    button.connect_clicked(|_| toggle_notifications());
    let zone: Zone = Rc::new(RefCell::new(None));
    let hours24 = Rc::new(Cell::new(read_hours24()));
    let watched = hours24.clone();
    let watch = cosmic_clock::watch(move || watched.set(read_hours24()));
    Some(Box::new(ClockUi {
        button,
        label,
        zone,
        failed: Cell::new(false),
        hours24,
        _watch: watch,
    }))
}
